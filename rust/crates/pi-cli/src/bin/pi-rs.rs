use pi_cli::{
    CliRequest, OutputTargets, ProductionModelServiceFactory, RootCancellation, StreamTarget,
    install_signal_handlers, run_cli,
};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let environment: BTreeMap<String, String> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("Error: could not determine current working directory: {error}");
            std::process::exit(1);
        }
    };
    let home = environment
        .get("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.clone());
    let stdin_is_terminal = std::io::stdin().is_terminal();
    let stdin = if stdin_is_terminal || is_metadata_invocation(&arguments) {
        None
    } else {
        let mut input = String::new();
        match std::io::stdin().read_to_string(&mut input) {
            Ok(_) => Some(input),
            Err(error) => {
                eprintln!("Error: could not read stdin: {error}");
                std::process::exit(1);
            }
        }
    };
    let targets = OutputTargets {
        stdout: Arc::new(StreamTarget::new(std::io::stdout())),
        stderr: Arc::new(StreamTarget::new(std::io::stderr())),
    };
    let cancellation = RootCancellation::default();
    let signal_task = install_signal_handlers(cancellation.clone());
    let exit = run_cli(
        CliRequest {
            arguments,
            environment,
            cwd,
            home,
            stdin,
            stdin_is_terminal,
        },
        &ProductionModelServiceFactory,
        targets,
        &cancellation,
    )
    .await;
    signal_task.abort();
    if exit.code != 0 {
        std::process::exit(exit.code);
    }
}

fn is_metadata_invocation(arguments: &[String]) -> bool {
    arguments.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--help" | "-h" | "--version" | "-v" | "--list-models"
        ) || argument.starts_with("--list-models=")
    })
}
