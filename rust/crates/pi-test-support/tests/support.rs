use pi_model::{ModelRequest, ModelService};
use pi_protocol::{Model, ModelCost};
use pi_test_support::{
    DeterministicIds, FakeCancellation, FakeClock, FakeSleeper, InMemoryEventSink, LocalHttpServer,
    NormalizationRule, ScriptedModelService, fixture_path, normalize_json, scan_fixture_tree,
};
use serde_json::json;
use std::error::Error;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

#[test]
fn deterministic_clock_and_ids_are_reproducible() {
    let clock = FakeClock::new(100, 5);
    assert_eq!(clock.now_ms(), 100);
    assert_eq!(clock.now_ms(), 105);

    let ids = DeterministicIds::new("event");
    assert_eq!(ids.next(), "event-00000001");
    assert_eq!(ids.next(), "event-00000002");
}

#[test]
fn fake_sleeper_records_without_waiting() -> Result<(), Box<dyn Error>> {
    let sleeper = FakeSleeper::default();
    sleeper.sleep(Duration::from_millis(20))?;
    sleeper.sleep(Duration::from_millis(40))?;
    assert_eq!(
        sleeper.snapshot()?,
        [Duration::from_millis(20), Duration::from_millis(40)]
    );
    Ok(())
}

#[test]
fn event_sink_enforces_capacity() -> Result<(), Box<dyn Error>> {
    let sink = InMemoryEventSink::new(1);
    sink.emit(json!({ "type": "first" }))?;
    assert!(sink.emit(json!({ "type": "second" })).is_err());
    assert_eq!(sink.snapshot()?.len(), 1);
    Ok(())
}

#[test]
fn scripted_model_honors_cancellation_before_consuming_events() {
    let service = ScriptedModelService::new(Vec::new());
    let cancellation = FakeCancellation::default();
    cancellation.cancel();
    let request = ModelRequest {
        model: Model {
            id: "example".to_owned(),
            name: "Example".to_owned(),
            api: "test".to_owned(),
            provider: "test".to_owned(),
            base_url: "http://127.0.0.1".to_owned(),
            reasoning: false,
            input: Vec::new(),
            cost: ModelCost {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
            context_window: 1,
            max_tokens: 1,
            headers: None,
            compat: None,
            thinking_level_map: None,
            extensions: Default::default(),
        },
        system_prompt: String::new(),
        messages: Vec::new(),
        tools: Vec::new(),
    };
    let mut future = service.stream(request, &cancellation);
    let mut context = Context::from_waker(Waker::noop());
    let result = future.as_mut().poll(&mut context);
    assert!(matches!(result, Poll::Ready(Err(error)) if error.category == "cancelled"));
}

#[test]
fn normalizer_changes_only_explicit_leaf_paths() -> Result<(), Box<dyn Error>> {
    let mut value = json!({ "id": "random", "payload": { "timestamp": 123, "keep": true } });
    normalize_json(
        &mut value,
        &[
            NormalizationRule {
                pointer: "/id".to_owned(),
                replacement: json!("<id>"),
            },
            NormalizationRule {
                pointer: "/payload/timestamp".to_owned(),
                replacement: json!("<timestamp>"),
            },
        ],
    )?;
    assert_eq!(
        value,
        json!({ "id": "<id>", "payload": { "timestamp": "<timestamp>", "keep": true } })
    );
    assert!(
        normalize_json(
            &mut value,
            &[NormalizationRule {
                pointer: String::new(),
                replacement: json!(null),
            }]
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn fixture_paths_cannot_escape_the_repository() -> Result<(), Box<dyn Error>> {
    assert!(fixture_path("rust/fixtures/protocol/messages.json")?.is_file());
    assert!(fixture_path("../outside").is_err());
    Ok(())
}

#[test]
fn committed_fixtures_have_no_credentials_or_machine_paths() -> Result<(), Box<dyn Error>> {
    let root = fixture_path("rust/fixtures")?;
    assert_eq!(scan_fixture_tree(&root)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn local_http_server_is_loopback_and_one_shot() -> Result<(), Box<dyn Error>> {
    let server = LocalHttpServer::start(200, "fixture")?;
    assert!(server.address().ip().is_loopback());
    let mut stream = TcpStream::connect(server.address())?;
    stream.write_all(b"GET /contract HTTP/1.1\r\nHost: localhost\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    assert!(response.ends_with("fixture"));
    assert!(
        server
            .receive_request(Duration::from_secs(1))?
            .starts_with("GET /contract")
    );
    Ok(())
}
