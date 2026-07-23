mod support;

use pi_cli::{ProductionModelServiceFactory, RootCancellation};
use pi_test_support::AsyncLocalHttpServer;
use support::{TempDir, run};

struct ProtocolCase {
    api: &'static str,
    body: &'static str,
    request_path: &'static str,
    credential_header: &'static str,
}

const PROTOCOLS: [ProtocolCase; 4] = [
    ProtocolCase {
        api: "openai-completions",
        body: concat!(
            "data: {\"id\":\"chatcmpl-1\",\"model\":\"local-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"local ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        ),
        request_path: "/v1/chat/completions",
        credential_header: "authorization: bearer synthetic-token",
    },
    ProtocolCase {
        api: "openai-responses",
        body: concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"local-model\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"local ok\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"local ok\",\"annotations\":[]}]}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"local-model\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        ),
        request_path: "/v1/responses",
        credential_header: "authorization: bearer synthetic-token",
    },
    ProtocolCase {
        api: "anthropic-messages",
        body: concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"local-model\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"local ok\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
        ),
        request_path: "/v1/messages",
        credential_header: "x-api-key: synthetic-token",
    },
    ProtocolCase {
        api: "google-generative-ai",
        body: "data: {\"responseId\":\"resp_1\",\"modelVersion\":\"local-model\",\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"local ok\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":1,\"candidatesTokenCount\":2,\"totalTokenCount\":3}}\n\n",
        request_path: "/v1/models/local-model:streamGenerateContent?alt=sse",
        credential_header: "x-goog-api-key: synthetic-token",
    },
];

#[tokio::test]
async fn production_cli_completes_all_supported_local_protocol_runs() {
    for case in &PROTOCOLS {
        let server = AsyncLocalHttpServer::start(
            "200 OK",
            &[("Content-Type", "text/event-stream")],
            case.body,
        )
        .await
        .expect("local server");
        let root = TempDir::new(case.api);
        let cwd = root.path().join("project");
        let agent = root.path().join("home").join(".pi").join("agent");
        std::fs::create_dir_all(&agent).expect("agent");
        std::fs::write(
            agent.join("models.json"),
            serde_json::json!({
                "providers": {
                    "local": {
                        "api": case.api,
                        "baseUrl": server.base_url(),
                        "models": [{
                            "id": "local-model",
                            "name": "Local Model",
                            "reasoning": false,
                            "input": ["text"],
                            "cost": {
                                "input": 0,
                                "output": 0,
                                "cacheRead": 0,
                                "cacheWrite": 0
                            },
                            "contextWindow": 128000,
                            "maxTokens": 4096
                        }]
                    }
                }
            })
            .to_string(),
        )
        .expect("models fixture");
        let result = run(
            &root,
            &cwd,
            &[
                "--no-session",
                "--no-context-files",
                "--no-skills",
                "--no-tools",
                "--provider",
                "local",
                "--model",
                "local-model",
                "--api-key",
                "synthetic-token",
                "-p",
                "say local ok",
            ],
            None,
            true,
            &ProductionModelServiceFactory,
            &RootCancellation::default(),
        )
        .await;
        assert_eq!(result.code, 0, "{}: {}", case.api, result.stderr);
        assert_eq!(result.stdout, "local ok\n", "{}", case.api);
        let request = server.finish().await.expect("captured request");
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        let request_line = format!("post {} http/1.1", case.request_path.to_ascii_lowercase());
        assert!(
            request.starts_with(&request_line),
            "{}: {request}",
            case.api
        );
        assert!(
            request.contains(case.credential_header),
            "{}: credential header missing",
            case.api
        );
    }
}
