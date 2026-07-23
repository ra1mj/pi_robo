#![allow(dead_code)]

use pi_cli::{
    CliRequest, MemoryTarget, ModelServiceFactory, OutputTargets, RootCancellation, run_cli,
};
use pi_model::{ModelService, ModelServiceError};
use pi_protocol::{
    AssistantMessage, ContentBlock, Model, Settings, StopReason, TextBlock, ToolCallBlock, Usage,
};
use pi_store::ResolvedCredential;
use pi_test_support::{FakeClock, FauxModelService, FauxResponse};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(name: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pi-rs-{name}-{}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).expect("temporary directory must be created");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone)]
pub struct InjectedFactory {
    service: Arc<dyn ModelService>,
}

impl InjectedFactory {
    pub fn faux(responses: Vec<FauxResponse>) -> Self {
        let service = Arc::new(FauxModelService::new(Arc::new(FakeClock::new(100, 10)), 64));
        service
            .set_responses(responses)
            .expect("Faux responses must configure");
        Self { service }
    }
}

impl ModelServiceFactory for InjectedFactory {
    fn requires_credential(&self) -> bool {
        false
    }

    fn create(
        &self,
        _model: &Model,
        _credential: Option<&ResolvedCredential>,
        _settings: &Settings,
    ) -> Result<Arc<dyn ModelService>, ModelServiceError> {
        Ok(self.service.clone())
    }
}

pub fn response(text: &str) -> FauxResponse {
    FauxResponse::Message(Box::new(AssistantMessage::new(
        vec![ContentBlock::Text(TextBlock::new(text))],
        "ignored",
        "ignored",
        "ignored",
        Usage::default(),
        StopReason::Stop,
        0,
    )))
}

pub fn tool_response(name: &str, arguments: Value) -> FauxResponse {
    FauxResponse::Message(Box::new(AssistantMessage::new(
        vec![ContentBlock::ToolCall(ToolCallBlock::new(
            "call-1", name, arguments,
        ))],
        "ignored",
        "ignored",
        "ignored",
        Usage::default(),
        StopReason::ToolUse,
        0,
    )))
}

pub struct RunResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub async fn run(
    root: &TempDir,
    cwd: &Path,
    arguments: &[&str],
    stdin: Option<&str>,
    stdin_is_terminal: bool,
    factory: &dyn ModelServiceFactory,
    cancellation: &RootCancellation,
) -> RunResult {
    let home = root.path().join("home");
    let agent = home.join(".pi").join("agent");
    std::fs::create_dir_all(&agent).expect("agent directory");
    std::fs::create_dir_all(cwd).expect("cwd directory");
    let (targets, stdout, stderr): (OutputTargets, MemoryTarget, MemoryTarget) =
        OutputTargets::memory();
    let exit = run_cli(
        CliRequest {
            arguments: arguments.iter().map(ToString::to_string).collect(),
            environment: BTreeMap::from([
                ("HOME".to_owned(), home.display().to_string()),
                (
                    "PI_CODING_AGENT_DIR".to_owned(),
                    agent.display().to_string(),
                ),
            ]),
            cwd: cwd.to_path_buf(),
            home,
            stdin: stdin.map(str::to_owned),
            stdin_is_terminal,
        },
        factory,
        targets,
        cancellation,
    )
    .await;
    RunResult {
        code: exit.code,
        stdout: stdout.text().expect("stdout UTF-8"),
        stderr: stderr.text().expect("stderr UTF-8"),
    }
}
