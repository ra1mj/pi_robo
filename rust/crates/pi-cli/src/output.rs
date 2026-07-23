use crate::OutputMode;
use pi_protocol::{AgentEvent, ContentBlock, Message, SessionHeader};
use pi_runtime::{
    CompactionReason, CompactionRecord, RuntimeBoundaryError, RuntimeEvent, RuntimeEventFuture,
    RuntimeEventSink, RuntimeRunResult, RuntimeStatus,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

pub trait OutputTarget: Send + Sync {
    fn write(&self, bytes: &[u8]) -> io::Result<()>;
    fn flush(&self) -> io::Result<()>;
}

#[derive(Clone, Debug, Default)]
pub struct MemoryTarget {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl MemoryTarget {
    pub fn text(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(
            self.bytes
                .lock()
                .map_or_else(|_| Vec::new(), |bytes| bytes.clone()),
        )
    }
}

impl OutputTarget for MemoryTarget {
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        self.bytes
            .lock()
            .map_err(|_| io::Error::other("memory output lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }
}

pub struct StreamTarget {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl StreamTarget {
    #[must_use]
    pub fn new(writer: impl Write + Send + 'static) -> Self {
        Self {
            writer: Mutex::new(Box::new(writer)),
        }
    }
}

impl OutputTarget for StreamTarget {
    fn write(&self, bytes: &[u8]) -> io::Result<()> {
        self.writer
            .lock()
            .map_err(|_| io::Error::other("output lock poisoned"))?
            .write_all(bytes)
    }

    fn flush(&self) -> io::Result<()> {
        self.writer
            .lock()
            .map_err(|_| io::Error::other("output lock poisoned"))?
            .flush()
    }
}

#[derive(Clone)]
pub struct OutputTargets {
    pub stdout: Arc<dyn OutputTarget>,
    pub stderr: Arc<dyn OutputTarget>,
}

impl OutputTargets {
    #[must_use]
    pub fn memory() -> (Self, MemoryTarget, MemoryTarget) {
        let stdout = MemoryTarget::default();
        let stderr = MemoryTarget::default();
        (
            Self {
                stdout: Arc::new(stdout.clone()),
                stderr: Arc::new(stderr.clone()),
            },
            stdout,
            stderr,
        )
    }

    pub fn stdout_text(&self, text: &str) -> io::Result<()> {
        self.stdout.write(text.as_bytes())
    }

    pub fn stderr_line(&self, text: &str) -> io::Result<()> {
        self.stderr.write(text.as_bytes())?;
        self.stderr.write(b"\n")
    }

    pub fn write_json<T: Serialize>(&self, value: &T) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        bytes.push(b'\n');
        self.stdout.write(&bytes)
    }

    pub fn write_header(&self, header: &SessionHeader) -> io::Result<()> {
        self.write_json(header)
    }

    pub fn flush(&self) -> io::Result<()> {
        self.stdout.flush()?;
        self.stderr.flush()
    }
}

pub struct RuntimeOutputSink {
    mode: OutputMode,
    targets: OutputTargets,
}

impl RuntimeOutputSink {
    #[must_use]
    pub const fn new(mode: OutputMode, targets: OutputTargets) -> Self {
        Self { mode, targets }
    }
}

impl RuntimeEventSink for RuntimeOutputSink {
    fn emit<'a>(&'a self, event: RuntimeEvent) -> RuntimeEventFuture<'a> {
        Box::pin(async move {
            if self.mode != OutputMode::Json {
                return Ok(());
            }
            let value = runtime_event_value(&event);
            self.targets.write_json(&value).map_err(output_boundary)
        })
    }
}

pub fn write_text_result(
    targets: &OutputTargets,
    result: &RuntimeRunResult,
) -> Result<(), RuntimeBoundaryError> {
    let Some(Message::Assistant(assistant)) = result.messages.last() else {
        return Ok(());
    };
    match result.status {
        RuntimeStatus::Completed => {
            for block in &assistant.content {
                if let ContentBlock::Text(text) = block {
                    targets
                        .stdout_text(&text.text)
                        .and_then(|()| targets.stdout_text("\n"))
                        .map_err(output_boundary)?;
                }
            }
        }
        RuntimeStatus::Failed | RuntimeStatus::Cancelled => {
            targets
                .stderr_line(
                    assistant
                        .error_message
                        .as_deref()
                        .unwrap_or("request did not complete"),
                )
                .map_err(output_boundary)?;
        }
    }
    Ok(())
}

fn runtime_event_value(event: &RuntimeEvent) -> Value {
    match event {
        RuntimeEvent::Agent(event) => agent_event_value(event),
        RuntimeEvent::AutoRetryStart {
            attempt,
            max_attempts,
            delay,
            error_message,
        } => json!({
            "type": "auto_retry_start",
            "attempt": attempt,
            "maxAttempts": max_attempts,
            "delayMs": u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            "errorMessage": error_message,
        }),
        RuntimeEvent::AutoRetryEnd {
            success,
            attempt,
            final_error,
        } => json!({
            "type": "auto_retry_end",
            "success": success,
            "attempt": attempt,
            "finalError": final_error,
        }),
        RuntimeEvent::CompactionStart { reason } => json!({
            "type": "compaction_start",
            "reason": compaction_reason(*reason),
        }),
        RuntimeEvent::CompactionEnd {
            reason,
            result,
            aborted,
            will_retry,
            error_message,
        } => json!({
            "type": "compaction_end",
            "reason": compaction_reason(*reason),
            "result": result.as_ref().map(compaction_value),
            "aborted": aborted,
            "willRetry": will_retry,
            "errorMessage": error_message,
        }),
    }
}

fn agent_event_value(event: &AgentEvent) -> Value {
    serde_json::to_value(event).unwrap_or_else(|error| {
        json!({
            "type": "serialization_error",
            "message": format!("could not serialize agent event: {error}"),
        })
    })
}

fn compaction_value(record: &CompactionRecord) -> Value {
    json!({
        "summary": record.summary,
        "tokensBefore": record.tokens_before,
        "estimatedTokensAfter": record.estimated_tokens_after,
        "usage": {
            "input": record.usage.input,
            "output": record.usage.output,
            "cacheRead": record.usage.cache_read,
            "cacheWrite": record.usage.cache_write,
            "totalTokens": record.usage.total_tokens,
            "cost": {
                "input": record.usage.cost.input,
                "output": record.usage.cost.output,
                "cacheRead": record.usage.cost.cache_read,
                "cacheWrite": record.usage.cost.cache_write,
                "total": record.usage.cost.total,
            }
        }
    })
}

const fn compaction_reason(reason: CompactionReason) -> &'static str {
    match reason {
        CompactionReason::Threshold => "threshold",
        CompactionReason::Overflow => "overflow",
    }
}

fn output_boundary(error: io::Error) -> RuntimeBoundaryError {
    RuntimeBoundaryError::new(format!("output failed: {error}"))
}
