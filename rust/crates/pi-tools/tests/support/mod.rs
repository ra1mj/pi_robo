#![allow(dead_code)]

use pi_agent::{ToolUpdateFuture, ToolUpdateSink};
use pi_protocol::{ContentBlock, ToolCallBlock};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct TempRoot {
    path: PathBuf,
}

impl TempRoot {
    pub fn new(label: &str) -> Self {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pi-tools-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temporary root");
        Self {
            path: std::fs::canonicalize(path).expect("canonical temporary root"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Default)]
pub struct RecordingUpdates {
    values: Mutex<Vec<Value>>,
}

impl RecordingUpdates {
    pub fn snapshot(&self) -> Vec<Value> {
        self.values.lock().expect("updates lock").clone()
    }
}

impl ToolUpdateSink for RecordingUpdates {
    fn send<'a>(&'a self, partial_result: Value) -> ToolUpdateFuture<'a> {
        Box::pin(async move {
            self.values
                .lock()
                .map_err(|_| pi_agent::ToolError::execution("updates lock poisoned"))?
                .push(partial_result);
            Ok(())
        })
    }
}

pub fn call(name: &str, arguments: Value) -> ToolCallBlock {
    ToolCallBlock::new("call-1", name, arguments)
}

pub fn output_text(output: &pi_agent::ToolOutput) -> &str {
    output
        .content
        .iter()
        .find_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .expect("text output")
}
