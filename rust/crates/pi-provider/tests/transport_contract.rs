use pi_model::ModelServiceErrorCategory;
use pi_provider::{ProviderAdapterConfig, ProviderHttpClient, ProviderTimeouts, SecretString};
use pi_test_support::FakeCancellation;
use serde_json::json;
use std::collections::BTreeMap;
use std::error::Error;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

fn timeouts() -> ProviderTimeouts {
    ProviderTimeouts::new(
        Duration::from_secs(1),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
}

async fn spawn_static_server(
    response: &'static [u8],
) -> Result<(String, JoinHandle<Vec<u8>>), Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept request");
        let request = read_http_request(&mut socket).await.expect("read request");
        socket.write_all(response).await.expect("write response");
        request
    });
    Ok((format!("http://{address}"), worker))
}

async fn read_http_request(
    socket: &mut TcpStream,
) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1_024];
    loop {
        let count = socket.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_end = header_end + 4;
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if request.len() >= header_end + content_length {
            break;
        }
    }
    Ok(request)
}

#[tokio::test]
async fn posts_json_with_resolved_headers_and_reads_a_bounded_body() -> Result<(), Box<dyn Error>> {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nX-Test: yes\r\nConnection: close\r\n\r\n0123456789";
    let (base_url, worker) = spawn_static_server(response).await?;
    let config = ProviderAdapterConfig::new(base_url, timeouts())
        .with_authorization(SecretString::new("Bearer synthetic-token"))
        .with_headers(BTreeMap::from([(
            "x-client".to_owned(),
            "pi-test".to_owned(),
        )]));
    let client = ProviderHttpClient::new(&config)?;
    let cancellation = FakeCancellation::default();
    let response = client
        .post_json("v1/test", &json!({ "hello": "world" }), &cancellation)
        .await?;

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("x-test").map(String::as_str),
        Some("yes")
    );
    let body = response.read_body_bounded(4, &cancellation).await?;
    assert_eq!(body.bytes, b"0123");
    assert!(body.truncated);

    let request = String::from_utf8(worker.await?)?;
    assert!(request.starts_with("POST /v1/test HTTP/1.1"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer synthetic-token")
    );
    assert!(request.to_ascii_lowercase().contains("x-client: pi-test"));
    assert!(request.ends_with("{\"hello\":\"world\"}"));
    Ok(())
}

#[tokio::test]
async fn response_header_wait_is_cancellable() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept request");
        let _ = read_http_request(&mut socket).await.expect("read request");
        std::future::pending::<()>().await;
    });
    let config = ProviderAdapterConfig::new(format!("http://{address}"), timeouts());
    let client = ProviderHttpClient::new(&config)?;
    let cancellation = FakeCancellation::default();

    let body = json!({});
    let request = client.post_json("wait", &body, &cancellation);
    let cancel = async {
        tokio::task::yield_now().await;
        cancellation.cancel();
    };
    let (result, ()) = tokio::join!(request, cancel);
    assert!(matches!(
        result,
        Err(error) if error.category == ModelServiceErrorCategory::Cancelled && !error.retryable
    ));
    worker.abort();
    Ok(())
}

#[tokio::test]
async fn response_header_and_body_idle_timeouts_are_distinct() -> Result<(), Box<dyn Error>> {
    let short_timeouts = ProviderTimeouts::new(
        Duration::from_secs(1),
        Duration::from_millis(20),
        Duration::from_millis(20),
    );
    let header_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let header_address = header_listener.local_addr()?;
    let header_worker = tokio::spawn(async move {
        let (mut socket, _) = header_listener.accept().await.expect("accept request");
        let _ = read_http_request(&mut socket).await.expect("read request");
        std::future::pending::<()>().await;
    });
    let client = ProviderHttpClient::new(&ProviderAdapterConfig::new(
        format!("http://{header_address}"),
        short_timeouts,
    ))?;
    let cancellation = FakeCancellation::default();
    let header_error = match client.post_json("wait", &json!({}), &cancellation).await {
        Ok(_) => panic!("header wait must time out"),
        Err(error) => error,
    };
    assert_eq!(header_error.category, ModelServiceErrorCategory::Timeout);
    assert!(header_error.message.contains("response-header"));
    header_worker.abort();

    let body_listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let body_address = body_listener.local_addr()?;
    let body_worker = tokio::spawn(async move {
        let (mut socket, _) = body_listener.accept().await.expect("accept request");
        let _ = read_http_request(&mut socket).await.expect("read request");
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .expect("write headers");
        std::future::pending::<()>().await;
    });
    let client = ProviderHttpClient::new(&ProviderAdapterConfig::new(
        format!("http://{body_address}"),
        short_timeouts,
    ))?;
    let mut response = client.post_json("wait", &json!({}), &cancellation).await?;
    let body_error = response
        .next_chunk(&cancellation)
        .await
        .expect_err("body wait must time out");
    assert_eq!(body_error.category, ModelServiceErrorCategory::Timeout);
    assert!(body_error.message.contains("body"));
    body_worker.abort();
    Ok(())
}

#[tokio::test]
async fn transport_disables_reqwest_protocol_retries() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let worker = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.expect("accept first request");
        let _ = read_http_request(&mut first)
            .await
            .expect("read first request");
        drop(first);
        let second = tokio::time::timeout(Duration::from_millis(50), listener.accept()).await;
        usize::from(second.is_ok()) + 1
    });
    let client = ProviderHttpClient::new(&ProviderAdapterConfig::new(
        format!("http://{address}"),
        timeouts(),
    ))?;
    let cancellation = FakeCancellation::default();
    let error = match client.post_json("close", &json!({}), &cancellation).await {
        Ok(_) => panic!("closed connection must fail"),
        Err(error) => error,
    };

    assert_eq!(error.category, ModelServiceErrorCategory::Network);
    assert_eq!(worker.await?, 1);
    Ok(())
}
