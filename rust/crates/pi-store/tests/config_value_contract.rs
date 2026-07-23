use pi_store::{
    CommandCancellation, CommandRequest, CommandResult, ConfigValueSource, NeverCancelled,
    ProcessFuture, ProcessRunner, StoreError, StoreErrorCategory, TokioProcessRunner,
    resolve_config_value,
};
use std::collections::BTreeMap;
use std::future;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug)]
struct RecordingRunner {
    result: Result<CommandResult, StoreError>,
    requests: Mutex<Vec<CommandRequest>>,
}

impl ProcessRunner for RecordingRunner {
    fn run<'a>(
        &'a self,
        request: CommandRequest,
        _cancellation: &'a dyn CommandCancellation,
    ) -> ProcessFuture<'a> {
        self.requests
            .lock()
            .expect("request lock must work")
            .push(request);
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

#[tokio::test]
async fn literals_environment_interpolation_and_escapes_match_typescript() {
    let environment = BTreeMap::from([
        ("LEFT".to_owned(), "left".to_owned()),
        ("RIGHT".to_owned(), "right".to_owned()),
    ]);
    let runner = RecordingRunner {
        result: Ok(CommandResult {
            stdout: String::new(),
            success: true,
        }),
        requests: Mutex::new(Vec::new()),
    };
    let source = ConfigValueSource::new(&environment, Path::new("/tmp"));

    let literal = resolve_config_value("UPPERCASE_LITERAL", source, &runner, &NeverCancelled)
        .await
        .expect("literal resolves")
        .expect("literal is present");
    assert_eq!(literal.expose_secret(), "UPPERCASE_LITERAL");
    let interpolated = resolve_config_value("${LEFT}_$RIGHT", source, &runner, &NeverCancelled)
        .await
        .expect("interpolation resolves")
        .expect("interpolation is present");
    assert_eq!(interpolated.expose_secret(), "left_right");
    let escaped = resolve_config_value(
        "$$LEFT-$!literal-${RIGHT}",
        source,
        &runner,
        &NeverCancelled,
    )
    .await
    .expect("escapes resolve")
    .expect("escaped value is present");
    assert_eq!(escaped.expose_secret(), "$LEFT-!literal-right");
    assert!(
        resolve_config_value("$MISSING", source, &runner, &NeverCancelled)
            .await
            .expect("missing environment is not an error")
            .is_none()
    );
    for literal in ["$9", "${bad-name}", "${UNTERMINATED"] {
        let resolved = resolve_config_value(literal, source, &runner, &NeverCancelled)
            .await
            .expect("invalid environment syntax remains literal")
            .expect("literal remains present");
        assert_eq!(resolved.expose_secret(), literal);
    }
}

#[tokio::test]
async fn command_values_are_trimmed_bounded_and_redacted() {
    let environment = BTreeMap::from([("PATH".to_owned(), "/synthetic/bin".to_owned())]);
    let runner = RecordingRunner {
        result: Ok(CommandResult {
            stdout: "  secret-from-command  \n".to_owned(),
            success: true,
        }),
        requests: Mutex::new(Vec::new()),
    };
    let mut source = ConfigValueSource::new(&environment, Path::new("/synthetic/project"));
    source.max_stdout_bytes = 32;

    let secret = resolve_config_value("!credential-helper", source, &runner, &NeverCancelled)
        .await
        .expect("command resolves")
        .expect("command returns a value");

    assert_eq!(secret.expose_secret(), "secret-from-command");
    assert!(!format!("{secret:?}").contains("secret-from-command"));
    let requests = runner.requests.lock().expect("request lock must work");
    assert_eq!(requests[0].max_stdout_bytes, 32);
    assert_eq!(requests[0].environment, environment);
}

#[tokio::test]
async fn command_failures_do_not_include_command_or_secret_output() {
    let runner = RecordingRunner {
        result: Err(StoreError::new(
            StoreErrorCategory::CommandFailed,
            "credential command failed",
        )),
        requests: Mutex::new(Vec::new()),
    };
    let error = resolve_config_value(
        "!print-super-secret",
        ConfigValueSource::new(&BTreeMap::new(), Path::new("/tmp")),
        &runner,
        &NeverCancelled,
    )
    .await
    .expect_err("command failure must surface");

    assert_eq!(error.category, StoreErrorCategory::CommandFailed);
    assert!(!error.to_string().contains("print-super-secret"));
}

#[tokio::test]
async fn tokio_runner_enforces_output_timeout_and_cancellation_limits() {
    let environment = BTreeMap::from([("PATH".to_owned(), "/usr/bin:/bin".to_owned())]);
    let mut output_source = ConfigValueSource::new(&environment, Path::new("/tmp"));
    output_source.max_stdout_bytes = 4;
    let output_error = resolve_config_value(
        "!printf 123456789",
        output_source,
        &TokioProcessRunner,
        &NeverCancelled,
    )
    .await
    .expect_err("oversized output must be rejected");
    assert_eq!(output_error.category, StoreErrorCategory::OutputLimit);

    let mut timeout_source = ConfigValueSource::new(&environment, Path::new("/tmp"));
    timeout_source.timeout = Duration::from_millis(20);
    let timeout_error = resolve_config_value(
        "!sleep 5",
        timeout_source,
        &TokioProcessRunner,
        &NeverCancelled,
    )
    .await
    .expect_err("slow command must time out");
    assert_eq!(timeout_error.category, StoreErrorCategory::Timeout);

    let cancelled_error = resolve_config_value(
        "!printf should-not-run",
        ConfigValueSource::new(&environment, Path::new("/tmp")),
        &TokioProcessRunner,
        &AlreadyCancelled,
    )
    .await
    .expect_err("cancelled command must not start");
    assert_eq!(cancelled_error.category, StoreErrorCategory::Cancelled);
}

#[derive(Debug)]
struct AlreadyCancelled;

impl CommandCancellation for AlreadyCancelled {
    fn is_cancelled(&self) -> bool {
        true
    }

    fn cancelled(&self) -> pi_store::CancellationFuture<'_> {
        Box::pin(future::ready(()))
    }
}
