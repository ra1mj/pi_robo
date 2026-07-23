mod support;

use pi_test_support::AsyncLocalHttpServer;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use support::TempDir;
use tokio::process::Command;

const RESPONSE_BODY: &str = concat!(
    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"local-model\"}}\n\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}\n\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"local ok\"}\n\n",
    "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"local ok\",\"annotations\":[]}]}}\n\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"local-model\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
);

#[tokio::test(flavor = "multi_thread")]
async fn production_binary_completes_local_text_and_json_runs() {
    for mode in ["text", "json"] {
        let server = AsyncLocalHttpServer::start(
            "200 OK",
            &[("Content-Type", "text/event-stream")],
            RESPONSE_BODY,
        )
        .await
        .expect("local server");
        let root = TempDir::new(&format!("binary-{mode}"));
        let cwd = root.path().join("project");
        let home = root.path().join("home");
        let agent = home.join(".pi").join("agent");
        std::fs::create_dir_all(&cwd).expect("project directory");
        std::fs::create_dir_all(&agent).expect("agent directory");
        std::fs::write(
            agent.join("models.json"),
            serde_json::json!({
                "providers": {
                    "local": {
                        "api": "openai-responses",
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

        let output = Command::new(smoke_binary())
            .args([
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
                "--mode",
                mode,
                "say local ok",
            ])
            .env_clear()
            .env("HOME", &home)
            .env("PI_CODING_AGENT_DIR", &agent)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .output()
            .await
            .expect("pi-rs process");
        assert!(
            output.status.success(),
            "mode={mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "mode={mode}");
        let stdout = String::from_utf8(output.stdout).expect("stdout UTF-8");
        if mode == "text" {
            assert_eq!(stdout, "local ok\n");
        } else {
            let records = stdout
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record"))
                .collect::<Vec<_>>();
            assert_eq!(records[0]["type"], "session");
            assert!(stdout.contains("local ok"));
        }

        let request = server.finish().await.expect("captured request");
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        assert!(request.starts_with("post /v1/responses http/1.1"));
        assert!(request.contains("authorization: bearer synthetic-token"));
    }
}

fn smoke_binary() -> PathBuf {
    std::env::var_os("PI_RS_SMOKE_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_pi-rs")))
}
