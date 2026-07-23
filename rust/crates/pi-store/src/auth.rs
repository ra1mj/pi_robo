use crate::{StoreDiagnostic, StoreError, StoreErrorCategory, StorePaths};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub type ProcessFuture<'a> =
    Pin<Box<dyn Future<Output = Result<CommandResult, StoreError>> + Send + 'a>>;
pub type CancellationFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

#[derive(Clone, Eq, PartialEq)]
pub struct CommandRequest {
    pub command: String,
    pub cwd: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
}

impl std::fmt::Debug for CommandRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandRequest")
            .field("command", &"[REDACTED]")
            .field("cwd", &self.cwd)
            .field("environment_keys", &self.environment.keys())
            .field("timeout", &self.timeout)
            .field("max_stdout_bytes", &self.max_stdout_bytes)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandResult {
    pub stdout: String,
    pub success: bool,
}

pub trait CommandCancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
    fn cancelled(&self) -> CancellationFuture<'_>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NeverCancelled;

impl CommandCancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }

    fn cancelled(&self) -> CancellationFuture<'_> {
        Box::pin(std::future::pending())
    }
}

pub trait ProcessRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        request: CommandRequest,
        cancellation: &'a dyn CommandCancellation,
    ) -> ProcessFuture<'a>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TokioProcessRunner;

impl ProcessRunner for TokioProcessRunner {
    fn run<'a>(
        &'a self,
        request: CommandRequest,
        cancellation: &'a dyn CommandCancellation,
    ) -> ProcessFuture<'a> {
        Box::pin(async move {
            if cancellation.is_cancelled() {
                return Err(StoreError::new(
                    StoreErrorCategory::Cancelled,
                    "credential command was cancelled",
                ));
            }
            let timeout = request.timeout;
            tokio::select! {
                () = cancellation.cancelled() => Err(StoreError::new(
                    StoreErrorCategory::Cancelled,
                    "credential command was cancelled",
                )),
                result = tokio::time::timeout(timeout, run_command(request)) => {
                    result.map_err(|_| StoreError::new(
                        StoreErrorCategory::Timeout,
                        "credential command timed out",
                    ))?
                }
            }
        })
    }
}

async fn run_command(request: CommandRequest) -> Result<CommandResult, StoreError> {
    #[cfg(windows)]
    let mut command = {
        let mut value = Command::new("cmd");
        value.arg("/C").arg(&request.command);
        value
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut value = Command::new("sh");
        value.arg("-c").arg(&request.command);
        value
    };
    command
        .current_dir(&request.cwd)
        .env_clear()
        .envs(&request.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        StoreError::new(
            StoreErrorCategory::CommandFailed,
            format!("could not start credential command: {error}"),
        )
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        StoreError::new(
            StoreErrorCategory::CommandFailed,
            "credential command stdout was unavailable",
        )
    })?;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut exit_status = None;

    loop {
        let read = if exit_status.is_some() {
            stdout
                .read(&mut buffer)
                .await
                .map_err(|error| command_io_error("read", error))?
        } else {
            tokio::select! {
                result = stdout.read(&mut buffer) => {
                    result.map_err(|error| command_io_error("read", error))?
                }
                result = child.wait() => {
                    exit_status = Some(result.map_err(|error| command_io_error("wait for", error))?);
                    continue;
                }
            }
        };
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) > request.max_stdout_bytes {
            let _ = child.kill().await;
            return Err(StoreError::new(
                StoreErrorCategory::OutputLimit,
                "credential command exceeded the stdout limit",
            ));
        }
        output.extend_from_slice(&buffer[..read]);
    }

    let status = match exit_status {
        Some(value) => value,
        None => child
            .wait()
            .await
            .map_err(|error| command_io_error("wait for", error))?,
    };
    let stdout = String::from_utf8(output).map_err(|_| {
        StoreError::new(
            StoreErrorCategory::CommandFailed,
            "credential command output was not valid UTF-8",
        )
    })?;
    Ok(CommandResult {
        stdout,
        success: status.success(),
    })
}

fn command_io_error(operation: &str, error: std::io::Error) -> StoreError {
    StoreError::new(
        StoreErrorCategory::CommandFailed,
        format!("could not {operation} credential command: {error}"),
    )
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretString(String);

impl SecretString {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Clone, PartialEq)]
pub enum AuthRecord {
    ApiKey {
        key: String,
        environment: BTreeMap<String, String>,
        raw: Value,
    },
    Oauth {
        raw: Value,
    },
    Unsupported {
        kind: Option<String>,
        raw: Value,
    },
}

impl std::fmt::Debug for AuthRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey { environment, .. } => formatter
                .debug_struct("ApiKey")
                .field("key", &"[REDACTED]")
                .field("environment_keys", &environment.keys().collect::<Vec<_>>())
                .finish(),
            Self::Oauth { .. } => formatter
                .debug_struct("Oauth")
                .field("credentials", &"[REDACTED]")
                .finish(),
            Self::Unsupported { kind, .. } => formatter
                .debug_struct("Unsupported")
                .field("kind", kind)
                .field("record", &"[REDACTED]")
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct AuthDocument {
    pub raw: Value,
    pub records: BTreeMap<String, AuthRecord>,
    pub diagnostics: Vec<StoreDiagnostic>,
}

impl std::fmt::Debug for AuthDocument {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthDocument")
            .field("providers", &self.records.keys().collect::<Vec<_>>())
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

impl AuthDocument {
    pub fn load(paths: &StorePaths) -> Result<Self, StoreError> {
        let path = paths.auth_file();
        let content = match std::fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    raw: Value::Object(serde_json::Map::new()),
                    records: BTreeMap::new(),
                    diagnostics: Vec::new(),
                });
            }
            Err(error) => return Err(StoreError::io(error, &path)),
        };
        let raw: Value =
            serde_json::from_str(&content).map_err(|error| StoreError::json(error, &path))?;
        let object = raw.as_object().ok_or_else(|| {
            StoreError::new(
                StoreErrorCategory::InvalidShape,
                "auth.json must be a JSON object",
            )
            .with_path(&path)
        })?;
        let mut records = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (provider, value) in object {
            let Some(record) = value.as_object() else {
                diagnostics.push(
                    StoreDiagnostic::warning(format!(
                        "auth record for {provider:?} must be an object"
                    ))
                    .with_path(&path),
                );
                continue;
            };
            let kind = record.get("type").and_then(Value::as_str);
            let parsed = match kind {
                Some("api_key") => match record.get("key").and_then(Value::as_str) {
                    Some(key) => AuthRecord::ApiKey {
                        key: key.to_owned(),
                        environment: string_map(record.get("env")),
                        raw: value.clone(),
                    },
                    None => {
                        diagnostics.push(
                            StoreDiagnostic::warning(format!(
                                "API-key auth record for {provider:?} has no string key"
                            ))
                            .with_path(&path),
                        );
                        AuthRecord::Unsupported {
                            kind: kind.map(str::to_owned),
                            raw: value.clone(),
                        }
                    }
                },
                Some("oauth") => AuthRecord::Oauth { raw: value.clone() },
                _ => AuthRecord::Unsupported {
                    kind: kind.map(str::to_owned),
                    raw: value.clone(),
                },
            };
            records.insert(provider.clone(), parsed);
        }
        Ok(Self {
            raw,
            records,
            diagnostics,
        })
    }
}

fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
pub struct ConfigValueSource<'a> {
    pub environment: &'a BTreeMap<String, String>,
    pub cwd: &'a Path,
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
}

impl std::fmt::Debug for ConfigValueSource<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigValueSource")
            .field("environment_keys", &self.environment.keys())
            .field("cwd", &self.cwd)
            .field("timeout", &self.timeout)
            .field("max_stdout_bytes", &self.max_stdout_bytes)
            .finish()
    }
}

impl<'a> ConfigValueSource<'a> {
    #[must_use]
    pub fn new(environment: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self {
            environment,
            cwd,
            timeout: Duration::from_secs(10),
            max_stdout_bytes: 64 * 1024,
        }
    }
}

pub async fn resolve_config_value(
    value: &str,
    source: ConfigValueSource<'_>,
    runner: &dyn ProcessRunner,
    cancellation: &dyn CommandCancellation,
) -> Result<Option<SecretString>, StoreError> {
    if let Some(command) = value.strip_prefix('!') {
        if command.trim().is_empty() {
            return Err(StoreError::new(
                StoreErrorCategory::CommandFailed,
                "credential command must not be empty",
            ));
        }
        let result = runner
            .run(
                CommandRequest {
                    command: command.to_owned(),
                    cwd: source.cwd.to_path_buf(),
                    environment: source.environment.clone(),
                    timeout: source.timeout,
                    max_stdout_bytes: source.max_stdout_bytes,
                },
                cancellation,
            )
            .await?;
        if !result.success {
            return Err(StoreError::new(
                StoreErrorCategory::CommandFailed,
                "credential command failed",
            ));
        }
        let output = result.stdout.trim();
        return Ok((!output.is_empty()).then(|| SecretString::new(output)));
    }

    interpolate_environment(value, source.environment).map(|value| value.map(SecretString::new))
}

fn interpolate_environment(
    value: &str,
    environment: &BTreeMap<String, String>,
) -> Result<Option<String>, StoreError> {
    const ESCAPED_DOLLAR: char = '\u{e000}';
    const ESCAPED_BANG: char = '\u{e001}';
    let protected = value
        .replace("$$", &ESCAPED_DOLLAR.to_string())
        .replace("$!", &ESCAPED_BANG.to_string());
    let characters: Vec<char> = protected.chars().collect();
    let mut result = String::with_capacity(protected.len());
    let mut index = 0;
    while index < characters.len() {
        if characters[index] != '$' {
            result.push(characters[index]);
            index += 1;
            continue;
        }
        let (name, next) = if characters.get(index + 1) == Some(&'{') {
            let Some(end) = characters[index + 2..]
                .iter()
                .position(|character| *character == '}')
                .map(|offset| index + 2 + offset)
            else {
                result.push('$');
                index += 1;
                continue;
            };
            let name = characters[index + 2..end].iter().collect::<String>();
            if !is_environment_name(&name) {
                result.extend(characters[index..=end].iter().copied());
                index = end + 1;
                continue;
            }
            (name, end + 1)
        } else {
            let start = index + 1;
            let mut end = start;
            if !characters
                .get(start)
                .is_some_and(|character| *character == '_' || character.is_ascii_alphabetic())
            {
                result.push('$');
                index += 1;
                continue;
            }
            while end < characters.len() && is_environment_character(characters[end]) {
                end += 1;
            }
            (characters[start..end].iter().collect::<String>(), end)
        };
        let Some(replacement) = environment.get(&name) else {
            return Ok(None);
        };
        result.push_str(replacement);
        index = next;
    }
    Ok(Some(
        result
            .replace(ESCAPED_DOLLAR, "$")
            .replace(ESCAPED_BANG, "!"),
    ))
}

fn is_environment_name(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && value.chars().all(is_environment_character)
}

fn is_environment_character(character: char) -> bool {
    character == '_' || character.is_ascii_alphanumeric()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialSource {
    CliOverride,
    AuthJson,
    ProviderEnvironment,
    ModelsJson,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedCredential {
    pub secret: SecretString,
    pub source: CredentialSource,
    pub environment: BTreeMap<String, String>,
}

impl std::fmt::Debug for ResolvedCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedCredential")
            .field("secret", &self.secret)
            .field("source", &self.source)
            .field("environment_keys", &self.environment.keys())
            .finish()
    }
}

#[derive(Clone, Copy)]
pub struct CredentialRequest<'a> {
    pub provider: &'a str,
    pub cli_override: Option<&'a str>,
    pub provider_environment_keys: &'a [String],
    pub environment: &'a BTreeMap<String, String>,
    pub models_json_key: Option<&'a str>,
    pub cwd: &'a Path,
}

impl std::fmt::Debug for CredentialRequest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialRequest")
            .field("provider", &self.provider)
            .field("cli_override", &self.cli_override.map(|_| "[REDACTED]"))
            .field("provider_environment_keys", &self.provider_environment_keys)
            .field("environment_keys", &self.environment.keys())
            .field(
                "models_json_key",
                &self.models_json_key.map(|_| "[REDACTED]"),
            )
            .field("cwd", &self.cwd)
            .finish()
    }
}

pub async fn resolve_credential(
    request: CredentialRequest<'_>,
    auth: &AuthDocument,
    runner: &dyn ProcessRunner,
    cancellation: &dyn CommandCancellation,
) -> Result<ResolvedCredential, StoreError> {
    if let Some(value) = request.cli_override {
        return Ok(ResolvedCredential {
            secret: SecretString::new(value),
            source: CredentialSource::CliOverride,
            environment: BTreeMap::new(),
        });
    }

    let mut oauth_present = false;
    if let Some(record) = auth.records.get(request.provider) {
        match record {
            AuthRecord::ApiKey {
                key, environment, ..
            } => {
                let mut combined_environment = request.environment.clone();
                combined_environment.extend(environment.clone());
                if let Some(secret) = resolve_config_value(
                    key,
                    ConfigValueSource::new(&combined_environment, request.cwd),
                    runner,
                    cancellation,
                )
                .await?
                {
                    return Ok(ResolvedCredential {
                        secret,
                        source: CredentialSource::AuthJson,
                        environment: environment.clone(),
                    });
                }
            }
            AuthRecord::Oauth { .. } => oauth_present = true,
            AuthRecord::Unsupported { .. } => {}
        }
    }

    for key in request.provider_environment_keys {
        if let Some(value) = request
            .environment
            .get(key)
            .filter(|value| !value.is_empty())
        {
            return Ok(ResolvedCredential {
                secret: SecretString::new(value),
                source: CredentialSource::ProviderEnvironment,
                environment: BTreeMap::new(),
            });
        }
    }

    if let Some(value) = request.models_json_key
        && let Some(secret) = resolve_config_value(
            value,
            ConfigValueSource::new(request.environment, request.cwd),
            runner,
            cancellation,
        )
        .await?
    {
        return Ok(ResolvedCredential {
            secret,
            source: CredentialSource::ModelsJson,
            environment: BTreeMap::new(),
        });
    }

    if oauth_present {
        return Err(StoreError::new(
            StoreErrorCategory::UnsupportedOauth,
            format!(
                "provider {:?} has OAuth credentials, but Rust milestone 1 supports API keys only",
                request.provider
            ),
        ));
    }
    Err(StoreError::new(
        StoreErrorCategory::Authentication,
        format!("no API key was found for provider {:?}", request.provider),
    ))
}
