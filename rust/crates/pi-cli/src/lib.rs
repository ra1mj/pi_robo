//! Headless `pi-rs` command parsing and runtime composition.

mod app;
mod args;
mod cancellation;
mod input;
mod output;
mod production;
mod session;

pub use app::{CliExit, CliRequest, run_cli};
pub use args::{
    CliArgs, CliMode, CliParseError, CliParseErrorKind, OutputMode, ThinkingLevelArg,
    ToolSelection, command, parse_args, resolve_output_mode,
};
pub use cancellation::{RootCancellation, install_signal_handlers};
pub use input::{PreparedPrompt, prepare_prompts};
pub use output::{MemoryTarget, OutputTarget, OutputTargets, RuntimeOutputSink, StreamTarget};
pub use production::{ModelServiceFactory, ProductionModelServiceFactory};
pub use session::{CliSession, SystemIdentity, open_session};

/// Native CLI version. The Rust workspace is versioned independently while `pi-rs`
/// remains a side-by-side milestone artifact.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
