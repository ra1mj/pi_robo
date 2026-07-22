use crate::truncate::{
    DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, TruncatedBy, TruncationResult, truncate_tail,
};
use pi_agent::{Cancellation, Tool, ToolError, ToolFuture, ToolOutput, ToolUpdateSink};
use pi_protocol::{Extensions, ToolCallBlock, ToolDefinition};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

const MAX_TIMEOUT_SECONDS: f64 = 2_147_483_647.0 / 1_000.0;
const LATE_OUTPUT_GRACE: Duration = Duration::from_millis(100);
const TERMINATION_GRACE: Duration = Duration::from_millis(250);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct BashToolConfig {
    pub cwd: PathBuf,
    pub shell_path: Option<PathBuf>,
    pub command_prefix: Option<String>,
    pub env: BTreeMap<String, String>,
    pub temp_dir: PathBuf,
}

impl BashToolConfig {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            shell_path: None,
            command_prefix: None,
            env: BTreeMap::new(),
            temp_dir: std::env::temp_dir(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BashTool {
    config: BashToolConfig,
}

impl BashTool {
    #[must_use]
    pub const fn new(config: BashToolConfig) -> Self {
        Self { config }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BashArgs {
    command: String,
    timeout: Option<f64>,
}

impl Tool for BashTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash".to_owned(),
            description: format!(
                "Execute a shell command. Output is limited to the last {DEFAULT_MAX_LINES} lines or {}KB.",
                DEFAULT_MAX_BYTES / 1_024
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "timeout": { "type": "number" }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
            extensions: Extensions::new(),
        }
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCallBlock,
        cancellation: &'a dyn Cancellation,
        updates: &'a dyn ToolUpdateSink,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: BashArgs =
                serde_json::from_value(call.arguments.clone()).map_err(|error| {
                    ToolError::invalid_arguments(format!("invalid bash arguments: {error}"))
                })?;
            let timeout = validate_timeout(args.timeout)?;
            if cancellation.is_cancelled() {
                return Err(ToolError::cancelled());
            }
            if !self.config.cwd.is_dir() {
                return Err(ToolError::execution(format!(
                    "Working directory does not exist: {}\nCannot execute bash commands.",
                    self.config.cwd.display()
                )));
            }
            let shell = resolve_shell(self.config.shell_path.as_deref())?;
            let command_text = self.config.command_prefix.as_ref().map_or_else(
                || args.command.clone(),
                |prefix| format!("{prefix}\n{}", args.command),
            );
            run_command(
                &shell,
                &command_text,
                &self.config,
                timeout,
                args.timeout,
                cancellation,
                updates,
            )
            .await
        })
    }
}

async fn run_command(
    shell: &Path,
    command_text: &str,
    config: &BashToolConfig,
    timeout: Option<Duration>,
    timeout_seconds: Option<f64>,
    cancellation: &dyn Cancellation,
    updates: &dyn ToolUpdateSink,
) -> Result<ToolOutput, ToolError> {
    let mut command = Command::new(shell);
    command
        .arg("-c")
        .arg(command_text)
        .current_dir(&config.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .envs(&config.env);
    let mut child = command.spawn().map_err(|error| {
        ToolError::execution(format!(
            "failed to start shell {}: {error}",
            shell.display()
        ))
    })?;
    let pid = child
        .id()
        .ok_or_else(|| ToolError::execution("spawned shell has no process id"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::execution("shell stdout pipe was not created"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::execution("shell stderr pipe was not created"))?;
    let (sender, mut receiver) = mpsc::channel(32);
    let stdout_task = tokio::spawn(read_output(stdout, sender.clone()));
    let stderr_task = tokio::spawn(read_output(stderr, sender));
    let mut wait_task = tokio::spawn(async move { child.wait().await });
    let timeout_future: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
        match timeout {
            Some(duration) => Box::pin(tokio::time::sleep(duration)),
            None => Box::pin(std::future::pending()),
        };
    tokio::pin!(timeout_future);
    let mut accumulator = OutputAccumulator::new(config.temp_dir.clone());
    updates.send(json!({ "content": [] })).await?;
    let mut output_open = true;
    let termination = loop {
        tokio::select! {
            result = &mut wait_task => {
                let status = join_status(result)?;
                break Termination::Completed(status);
            }
            message = receiver.recv(), if output_open => {
                match message {
                    Some(Ok(bytes)) => {
                        let decoded = accumulator.decode(&bytes);
                        if !decoded.is_empty() {
                            accumulator.push(&decoded)?;
                            if let Err(error) = updates.send(json!({
                                "content": [{ "type": "text", "text": accumulator.preview() }]
                            })).await {
                                terminate_process_group(pid, &mut wait_task).await;
                                stdout_task.abort();
                                stderr_task.abort();
                                return Err(error);
                            }
                        }
                    }
                    Some(Err(error)) => {
                        terminate_process_group(pid, &mut wait_task).await;
                        stdout_task.abort();
                        stderr_task.abort();
                        return Err(ToolError::execution(format!("failed to read shell output: {error}")));
                    }
                    None => output_open = false,
                }
            }
            () = cancellation.cancelled() => break Termination::Cancelled,
            () = &mut timeout_future => break Termination::TimedOut,
        }
    };

    let status = match termination {
        Termination::Completed(status) => Some(status),
        Termination::Cancelled | Termination::TimedOut => {
            terminate_process_group(pid, &mut wait_task).await
        }
    };
    drain_late_output(&mut receiver, &mut accumulator).await?;
    stdout_task.abort();
    stderr_task.abort();
    accumulator.finish()?;
    let snapshot = accumulator.snapshot();
    let (output, details) = format_snapshot(&snapshot);

    match termination {
        Termination::Cancelled => Err(ToolError::cancelled_with(append_status(
            &output,
            "Command aborted",
        ))),
        Termination::TimedOut => Err(ToolError::execution(append_status(
            &output,
            &format!(
                "Command timed out after {} seconds",
                timeout_seconds.unwrap_or_default()
            ),
        ))),
        Termination::Completed(_) => {
            if status.and_then(|value| value.code()).unwrap_or(-1) != 0 {
                return Err(ToolError::execution(append_status(
                    &output,
                    &format!(
                        "Command exited with code {}",
                        status.and_then(|value| value.code()).unwrap_or(-1)
                    ),
                )));
            }
            Ok(ToolOutput {
                content: vec![pi_protocol::ContentBlock::Text(
                    pi_protocol::TextBlock::new(if output.is_empty() {
                        "(no output)".to_owned()
                    } else {
                        output
                    }),
                )],
                details,
            })
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Termination {
    Completed(ExitStatus),
    Cancelled,
    TimedOut,
}

async fn read_output<R>(mut reader: R, sender: mpsc::Sender<Result<Vec<u8>, String>>)
where
    R: AsyncRead + Unpin,
{
    let mut buffer = vec![0_u8; 8 * 1_024];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(count) => {
                if sender.send(Ok(buffer[..count].to_vec())).await.is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = sender.send(Err(error.to_string())).await;
                break;
            }
        }
    }
}

fn join_status(
    result: Result<std::io::Result<ExitStatus>, tokio::task::JoinError>,
) -> Result<ExitStatus, ToolError> {
    result
        .map_err(|error| ToolError::execution(format!("shell wait task failed: {error}")))?
        .map_err(|error| ToolError::execution(format!("failed to wait for shell: {error}")))
}

async fn terminate_process_group(
    pid: u32,
    wait_task: &mut JoinHandle<std::io::Result<ExitStatus>>,
) -> Option<ExitStatus> {
    send_group_signal(pid, "TERM").await;
    match tokio::time::timeout(TERMINATION_GRACE, &mut *wait_task).await {
        Ok(result) => join_status(result).ok(),
        Err(_) => {
            send_group_signal(pid, "KILL").await;
            join_status(wait_task.await).ok()
        }
    }
}

async fn send_group_signal(pid: u32, signal: &str) {
    let kill = if Path::new("/bin/kill").is_file() {
        "/bin/kill"
    } else {
        "kill"
    };
    let _ = Command::new(kill)
        .arg(format!("-{signal}"))
        .arg("--")
        .arg(format!("-{pid}"))
        .status()
        .await;
}

async fn drain_late_output(
    receiver: &mut mpsc::Receiver<Result<Vec<u8>, String>>,
    accumulator: &mut OutputAccumulator,
) -> Result<(), ToolError> {
    loop {
        match tokio::time::timeout(LATE_OUTPUT_GRACE, receiver.recv()).await {
            Ok(Some(Ok(bytes))) => {
                let decoded = accumulator.decode(&bytes);
                accumulator.push(&decoded)?;
            }
            Ok(Some(Err(error))) => {
                return Err(ToolError::execution(format!(
                    "failed to read late shell output: {error}"
                )));
            }
            Ok(None) | Err(_) => return Ok(()),
        }
    }
}

fn validate_timeout(timeout: Option<f64>) -> Result<Option<Duration>, ToolError> {
    let Some(seconds) = timeout else {
        return Ok(None);
    };
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(ToolError::invalid_arguments(
            "Invalid timeout: must be a finite number of seconds",
        ));
    }
    if seconds > MAX_TIMEOUT_SECONDS {
        return Err(ToolError::invalid_arguments(format!(
            "Invalid timeout: maximum is {MAX_TIMEOUT_SECONDS} seconds"
        )));
    }
    Ok(Some(Duration::from_secs_f64(seconds)))
}

fn resolve_shell(configured: Option<&Path>) -> Result<PathBuf, ToolError> {
    if let Some(path) = configured {
        if is_executable(path) {
            return Ok(path.to_path_buf());
        }
        return Err(ToolError::execution(format!(
            "configured shell is not executable: {}",
            path.display()
        )));
    }
    let bash = Path::new("/bin/bash");
    if is_executable(bash) {
        return Ok(bash.to_path_buf());
    }
    if let Some(path) = find_on_path("bash") {
        return Ok(path);
    }
    let sh = Path::new("/bin/sh");
    if is_executable(sh) {
        return Ok(sh.to_path_buf());
    }
    find_on_path("sh").ok_or_else(|| ToolError::execution("no compatible shell was found"))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| is_executable(path))
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[derive(Debug)]
struct OutputAccumulator {
    tail: String,
    total_bytes: usize,
    newline_count: usize,
    has_content: bool,
    ends_with_newline: bool,
    decoder: Utf8Decoder,
    full_output: Option<(File, PathBuf)>,
    temp_dir: PathBuf,
}

impl OutputAccumulator {
    fn new(temp_dir: PathBuf) -> Self {
        Self {
            tail: String::new(),
            total_bytes: 0,
            newline_count: 0,
            has_content: false,
            ends_with_newline: false,
            decoder: Utf8Decoder::default(),
            full_output: None,
            temp_dir,
        }
    }

    fn decode(&mut self, bytes: &[u8]) -> String {
        self.decoder.decode(bytes)
    }

    fn push(&mut self, value: &str) -> Result<(), ToolError> {
        if value.is_empty() {
            return Ok(());
        }
        if let Some((file, _)) = &mut self.full_output {
            file.write_all(value.as_bytes()).map_err(temp_error)?;
        }
        self.tail.push_str(value);
        self.total_bytes = self.total_bytes.saturating_add(value.len());
        self.newline_count = self
            .newline_count
            .saturating_add(value.bytes().filter(|byte| *byte == b'\n').count());
        self.has_content = true;
        self.ends_with_newline = value.ends_with('\n');
        if self.full_output.is_none()
            && (self.total_bytes > DEFAULT_MAX_BYTES || self.total_lines() > DEFAULT_MAX_LINES)
        {
            let (mut file, path) = create_temp_file(&self.temp_dir)?;
            file.write_all(self.tail.as_bytes()).map_err(temp_error)?;
            self.full_output = Some((file, path));
        }
        if self.tail.len() > DEFAULT_MAX_BYTES * 2
            || self.tail.bytes().filter(|byte| *byte == b'\n').count() > DEFAULT_MAX_LINES * 2
        {
            self.tail =
                truncate_tail(&self.tail, DEFAULT_MAX_LINES * 2, DEFAULT_MAX_BYTES * 2).content;
        }
        Ok(())
    }

    fn preview(&self) -> String {
        truncate_tail(&self.tail, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES).content
    }

    fn finish(&mut self) -> Result<(), ToolError> {
        let trailing = self.decoder.finish();
        self.push(&trailing)?;
        if let Some((file, _)) = &mut self.full_output {
            file.flush().map_err(temp_error)?;
        }
        Ok(())
    }

    fn snapshot(&self) -> OutputSnapshot {
        let mut truncation = truncate_tail(&self.tail, DEFAULT_MAX_LINES, DEFAULT_MAX_BYTES);
        truncation.total_bytes = self.total_bytes;
        truncation.total_lines = self.total_lines();
        if self.full_output.is_some() {
            truncation.truncated = true;
            if truncation.truncated_by.is_none() {
                truncation.truncated_by = Some(if self.total_lines() > DEFAULT_MAX_LINES {
                    TruncatedBy::Lines
                } else {
                    TruncatedBy::Bytes
                });
            }
        }
        OutputSnapshot {
            content: truncation.content.clone(),
            truncation,
            full_output_path: self
                .full_output
                .as_ref()
                .map(|(_, path)| path.to_string_lossy().into_owned()),
        }
    }

    fn total_lines(&self) -> usize {
        if self.has_content {
            self.newline_count + usize::from(!self.ends_with_newline)
        } else {
            0
        }
    }
}

#[derive(Debug)]
struct OutputSnapshot {
    content: String,
    truncation: TruncationResult,
    full_output_path: Option<String>,
}

#[derive(Debug, Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    fn decode(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(value) => {
                    output.push_str(value);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        output.push_str(std::str::from_utf8(&self.pending[..valid]).unwrap_or(""));
                        self.pending.drain(..valid);
                    }
                    if let Some(length) = error.error_len() {
                        output.push('\u{fffd}');
                        let drain = length.min(self.pending.len());
                        self.pending.drain(..drain);
                    } else {
                        break;
                    }
                }
            }
        }
        output
    }

    fn finish(&mut self) -> String {
        let output = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        output
    }
}

fn create_temp_file(directory: &Path) -> Result<(File, PathBuf), ToolError> {
    std::fs::create_dir_all(directory).map_err(temp_error)?;
    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("pi-bash-{}-{sequence}.log", std::process::id()));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(temp_error(error)),
        }
    }
    Err(ToolError::execution(
        "could not allocate a unique full-output file",
    ))
}

fn temp_error(error: std::io::Error) -> ToolError {
    ToolError::execution(format!("failed to persist full shell output: {error}"))
}

fn format_snapshot(snapshot: &OutputSnapshot) -> (String, Option<Value>) {
    let mut output = snapshot.content.clone();
    if snapshot.truncation.truncated {
        let path = snapshot
            .full_output_path
            .as_deref()
            .unwrap_or("<unavailable>");
        let first = snapshot
            .truncation
            .total_lines
            .saturating_sub(snapshot.truncation.output_lines)
            + 1;
        let last = snapshot.truncation.total_lines;
        if snapshot.truncation.last_line_partial {
            output.push_str(&format!(
                "\n\n[Showing last {} bytes of line {last}. Full output: {path}]",
                snapshot.truncation.output_bytes
            ));
        } else if snapshot.truncation.truncated_by == Some(TruncatedBy::Lines) {
            output.push_str(&format!(
                "\n\n[Showing lines {first}-{last} of {}. Full output: {path}]",
                snapshot.truncation.total_lines
            ));
        } else {
            output.push_str(&format!(
                "\n\n[Showing lines {first}-{last} of {} ({DEFAULT_MAX_BYTES} byte limit). Full output: {path}]",
                snapshot.truncation.total_lines
            ));
        }
        let details = json!({
            "truncation": truncation_json(&snapshot.truncation),
            "fullOutputPath": snapshot.full_output_path
        });
        (output, Some(details))
    } else {
        (output, None)
    }
}

fn truncation_json(value: &TruncationResult) -> Value {
    json!({
        "truncated": value.truncated,
        "truncatedBy": match value.truncated_by { Some(TruncatedBy::Lines) => "lines", Some(TruncatedBy::Bytes) => "bytes", None => "none" },
        "totalLines": value.total_lines,
        "totalBytes": value.total_bytes,
        "outputLines": value.output_lines,
        "outputBytes": value.output_bytes,
        "lastLinePartial": value.last_line_partial,
        "firstLineExceedsLimit": value.first_line_exceeds_limit,
        "maxLines": value.max_lines,
        "maxBytes": value.max_bytes
    })
}

fn append_status(output: &str, status: &str) -> String {
    if output.is_empty() {
        status.to_owned()
    } else {
        format!("{output}\n\n{status}")
    }
}
