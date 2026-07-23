use clap::{Arg, ArgAction, Command, error::ErrorKind};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

const SUPPORTED_TOOLS: [&str; 4] = ["read", "bash", "edit", "write"];

const DEFERRED_OPTIONS: [(&str, &str); 18] = [
    ("--resume", "use --session <path|id>"),
    ("-r", "use --session <path|id>"),
    ("--fork", "start a new session without --fork"),
    ("--models", "use --provider and --model"),
    ("--no-builtin-tools", "use --no-tools or --tools"),
    ("-nbt", "use --no-tools or --tools"),
    ("--extension", "external integrations are deferred"),
    ("-e", "external integrations are deferred"),
    (
        "--no-extensions",
        "executable extensions are not loaded by pi-rs",
    ),
    ("-ne", "executable extensions are not loaded by pi-rs"),
    ("--prompt-template", "use --system-prompt"),
    (
        "--no-prompt-templates",
        "prompt templates are not loaded by pi-rs",
    ),
    ("-np", "prompt templates are not loaded by pi-rs"),
    ("--theme", "pi-rs has no interactive theme"),
    ("--no-themes", "pi-rs has no interactive theme"),
    (
        "--export",
        "use the TypeScript pi command to export sessions",
    ),
    (
        "--tree",
        "use the TypeScript pi command to inspect session trees",
    ),
    ("--verbose", "diagnostics are always written to stderr"),
];

const DEFERRED_COMMANDS: [&str; 6] = ["install", "remove", "uninstall", "update", "list", "config"];

const VALUE_OPTIONS: [&str; 15] = [
    "--mode",
    "--provider",
    "--model",
    "--api-key",
    "--thinking",
    "--system-prompt",
    "--append-system-prompt",
    "--skill",
    "--session",
    "--session-id",
    "--session-dir",
    "--name",
    "-n",
    "--tools",
    "-t",
];

const OPTIONAL_VALUE_OPTIONS: [&str; 1] = ["--list-models"];

const FLAG_OPTIONS: [&str; 22] = [
    "--print",
    "-p",
    "--no-skills",
    "-ns",
    "--no-context-files",
    "-nc",
    "--continue",
    "-c",
    "--no-session",
    "--exclude-tools",
    "-xt",
    "--no-tools",
    "-nt",
    "--approve",
    "-a",
    "--no-approve",
    "-na",
    "--offline",
    "--help",
    "-h",
    "--version",
    "-v",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliMode {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode {
    Metadata,
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThinkingLevelArg {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ThinkingLevelArg {
    fn parse(value: &str) -> Result<Self, CliParseError> {
        match value {
            "off" => Ok(Self::Off),
            "minimal" => Ok(Self::Minimal),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::XHigh),
            "max" => Ok(Self::Max),
            _ => Err(CliParseError::input(format!(
                "invalid thinking level {value:?}; expected off, minimal, low, medium, high, xhigh, or max"
            ))),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolSelection {
    pub enabled: Vec<String>,
}

impl ToolSelection {
    fn resolve(
        allow: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        no_tools: bool,
    ) -> Result<Self, CliParseError> {
        let mut enabled = if let Some(allow) = allow {
            validate_tools(allow)?
        } else if no_tools {
            Vec::new()
        } else {
            SUPPORTED_TOOLS.iter().map(ToString::to_string).collect()
        };
        let excluded = exclude.map(validate_tools).transpose()?.unwrap_or_default();
        enabled.retain(|tool| !excluded.contains(tool));
        Ok(Self { enabled })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliArgs {
    pub mode: Option<CliMode>,
    pub print: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub thinking: Option<ThinkingLevelArg>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Vec<String>,
    pub skill_paths: Vec<PathBuf>,
    pub no_skills: bool,
    pub no_context_files: bool,
    pub continue_session: bool,
    pub session: Option<String>,
    pub session_id: Option<String>,
    pub session_dir: Option<PathBuf>,
    pub no_session: bool,
    pub name: Option<String>,
    pub tools: ToolSelection,
    pub trust_override: Option<bool>,
    pub list_models: Option<Option<String>>,
    pub offline: bool,
    pub help: bool,
    pub version: bool,
    pub messages: Vec<String>,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliParseErrorKind {
    Input,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliParseError {
    pub kind: CliParseErrorKind,
    pub message: String,
}

impl CliParseError {
    fn input(message: impl Into<String>) -> Self {
        Self {
            kind: CliParseErrorKind::Input,
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            kind: CliParseErrorKind::Unsupported,
            message: message.into(),
        }
    }

    fn unknown(message: impl Into<String>) -> Self {
        Self {
            kind: CliParseErrorKind::Unknown,
            message: message.into(),
        }
    }
}

impl Display for CliParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CliParseError {}

#[must_use]
pub fn command() -> Command {
    Command::new("pi-rs")
        .version(crate::VERSION)
        .about("Native headless pi coding agent")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .args_override_self(true)
        .arg(flag("help", 'h', "Show help"))
        .arg(flag("version", 'v', "Show version"))
        .arg(flag("print", 'p', "Run in headless text mode"))
        .arg(value("mode", "text|json", "Select output mode"))
        .arg(value("provider", "name", "Select provider"))
        .arg(value("model", "pattern", "Select model"))
        .arg(value("api-key", "key", "Use an in-memory API key"))
        .arg(value("thinking", "level", "Set reasoning level"))
        .arg(value("system-prompt", "text", "Replace the system prompt"))
        .arg(
            value(
                "append-system-prompt",
                "text",
                "Append to the system prompt",
            )
            .action(ArgAction::Append),
        )
        .arg(value("skill", "path", "Load a skill file or directory").action(ArgAction::Append))
        .arg(bool_arg("no-skills", "Disable skill discovery"))
        .arg(bool_arg(
            "no-context-files",
            "Disable context-file discovery",
        ))
        .arg(
            bool_arg("continue", "Continue the most recent session")
                .short('c')
                .conflicts_with_all(["session", "session-id", "no-session"]),
        )
        .arg(
            value("session", "path|id", "Open a session").conflicts_with_all([
                "continue",
                "session-id",
                "no-session",
            ]),
        )
        .arg(
            value("session-id", "id", "Open or create an exact session ID").conflicts_with_all([
                "continue",
                "session",
                "no-session",
            ]),
        )
        .arg(value("session-dir", "dir", "Override session storage"))
        .arg(
            bool_arg("no-session", "Do not persist a session").conflicts_with_all([
                "continue",
                "session",
                "session-id",
            ]),
        )
        .arg(value("name", "name", "Set session display name").short('n'))
        .arg(value("tools", "names", "Comma-separated tool allowlist").short('t'))
        .arg(value(
            "exclude-tools",
            "names",
            "Comma-separated tool denylist",
        ))
        .arg(bool_arg("no-tools", "Disable all tools"))
        .arg(
            bool_arg("approve", "Trust project resources for this run")
                .short('a')
                .conflicts_with("no-approve"),
        )
        .arg(
            bool_arg("no-approve", "Skip project resources for this run").conflicts_with("approve"),
        )
        .arg(
            Arg::new("list-models")
                .long("list-models")
                .value_name("search")
                .num_args(0..=1)
                .default_missing_value("")
                .require_equals(false)
                .help("List available models"),
        )
        .arg(bool_arg("offline", "Disable implicit network startup work"))
        .arg(
            Arg::new("inputs")
                .num_args(0..)
                .action(ArgAction::Append)
                .allow_hyphen_values(true)
                .value_name("@files/messages"),
        )
        .after_help(
            "Bare interactive startup is not supported. Use -p or --mode text.\n\
             Deferred TypeScript-only options fail explicitly.",
        )
}

fn flag(id: &'static str, short: char, help: &'static str) -> Arg {
    bool_arg(id, help).short(short)
}

fn bool_arg(id: &'static str, help: &'static str) -> Arg {
    Arg::new(id).long(id).action(ArgAction::SetTrue).help(help)
}

fn value(id: &'static str, value_name: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(id)
        .value_name(value_name)
        .num_args(1)
        .help(help)
}

pub fn parse_args(arguments: &[String]) -> Result<CliArgs, CliParseError> {
    reject_deferred_and_unknown(arguments)?;
    let normalized_arguments = normalize_aliases(arguments);
    let matches = command()
        .try_get_matches_from(std::iter::once("pi-rs".to_owned()).chain(normalized_arguments))
        .map_err(map_clap_error)?;

    let mode = matches
        .get_one::<String>("mode")
        .map(|value| match value.as_str() {
            "text" => Ok(CliMode::Text),
            "json" => Ok(CliMode::Json),
            "rpc" => Err(CliParseError::unsupported(
                "--mode rpc is unsupported in pi-rs milestone 1; use the TypeScript pi command",
            )),
            _ => Err(CliParseError::input(format!(
                "invalid mode {value:?}; expected text or json"
            ))),
        })
        .transpose()?;
    let thinking = matches
        .get_one::<String>("thinking")
        .map(|value| ThinkingLevelArg::parse(value))
        .transpose()?;
    let name = matches
        .get_one::<String>("name")
        .map(|value| value.trim().to_owned());
    let name = match name {
        Some(value) if value.is_empty() => {
            Err(CliParseError::input("--name requires a non-empty value"))
        }
        value => Ok(value),
    }?;
    let session_id = matches.get_one::<String>("session-id").cloned();
    if let Some(id) = &session_id {
        validate_session_id(id)?;
    }
    let allow = matches
        .get_one::<String>("tools")
        .map(|value| parse_tool_list("--tools", value))
        .transpose()?;
    let exclude = matches
        .get_one::<String>("exclude-tools")
        .map(|value| parse_tool_list("--exclude-tools", value))
        .transpose()?;
    let tools = ToolSelection::resolve(allow, exclude, matches.get_flag("no-tools"))?;
    let trust_override = if matches.get_flag("approve") {
        Some(true)
    } else if matches.get_flag("no-approve") {
        Some(false)
    } else {
        None
    };
    let list_models = matches
        .get_one::<String>("list-models")
        .map(|value| (!value.is_empty()).then(|| value.clone()));
    let mut messages = Vec::new();
    let mut files = Vec::new();
    for input in matches.get_many::<String>("inputs").into_iter().flatten() {
        if let Some(path) = input.strip_prefix('@') {
            if path.is_empty() {
                return Err(CliParseError::input("@ requires a file path"));
            }
            files.push(PathBuf::from(path));
        } else {
            messages.push(input.clone());
        }
    }

    Ok(CliArgs {
        mode,
        print: matches.get_flag("print"),
        provider: matches.get_one::<String>("provider").cloned(),
        model: matches.get_one::<String>("model").cloned(),
        api_key: matches.get_one::<String>("api-key").cloned(),
        thinking,
        system_prompt: matches.get_one::<String>("system-prompt").cloned(),
        append_system_prompt: matches
            .get_many::<String>("append-system-prompt")
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
        skill_paths: matches
            .get_many::<String>("skill")
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .collect(),
        no_skills: matches.get_flag("no-skills"),
        no_context_files: matches.get_flag("no-context-files"),
        continue_session: matches.get_flag("continue"),
        session: matches.get_one::<String>("session").cloned(),
        session_id,
        session_dir: matches.get_one::<String>("session-dir").map(PathBuf::from),
        no_session: matches.get_flag("no-session"),
        name,
        tools,
        trust_override,
        list_models,
        offline: matches.get_flag("offline"),
        help: matches.get_flag("help"),
        version: matches.get_flag("version"),
        messages,
        files,
    })
}

pub fn resolve_output_mode(
    args: &CliArgs,
    stdin_is_terminal: bool,
) -> Result<OutputMode, CliParseError> {
    if args.help || args.version || args.list_models.is_some() {
        return Ok(OutputMode::Metadata);
    }
    match args.mode {
        Some(CliMode::Json) => return Ok(OutputMode::Json),
        Some(CliMode::Text) => return Ok(OutputMode::Text),
        None => {}
    }
    if args.print || !stdin_is_terminal {
        return Ok(OutputMode::Text);
    }
    Err(CliParseError::unsupported(
        "interactive mode is unsupported in pi-rs milestone 1; add -p or --mode text",
    ))
}

fn reject_deferred_and_unknown(arguments: &[String]) -> Result<(), CliParseError> {
    if let Some(command) = arguments.first()
        && DEFERRED_COMMANDS.contains(&command.as_str())
    {
        return Err(CliParseError::unsupported(format!(
            "{command:?} is unsupported in pi-rs milestone 1; use the TypeScript pi command"
        )));
    }

    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        let option = argument
            .split_once('=')
            .map_or(argument.as_str(), |pair| pair.0);
        if let Some((_, alternative)) = DEFERRED_OPTIONS
            .iter()
            .find(|(deferred, _)| *deferred == option)
        {
            return Err(CliParseError::unsupported(format!(
                "{option} is unsupported in pi-rs milestone 1; {alternative}"
            )));
        }
        if option == "--mode"
            && argument
                .split_once('=')
                .map(|pair| pair.1)
                .or_else(|| arguments.get(index + 1).map(String::as_str))
                == Some("rpc")
        {
            return Err(CliParseError::unsupported(
                "--mode rpc is unsupported in pi-rs milestone 1; use the TypeScript pi command",
            ));
        }
        if VALUE_OPTIONS.contains(&option) {
            if !argument.contains('=') {
                index = index.saturating_add(1);
            }
        } else if OPTIONAL_VALUE_OPTIONS.contains(&option) {
            if !argument.contains('=')
                && arguments
                    .get(index + 1)
                    .is_some_and(|next| !next.starts_with('-') && !next.starts_with('@'))
            {
                index = index.saturating_add(1);
            }
        } else if FLAG_OPTIONS.contains(&option)
            || argument.starts_with('@')
            || !argument.starts_with('-')
        {
        } else {
            return Err(CliParseError::unknown(format!(
                "unknown option {argument:?}; use --help for supported pi-rs options"
            )));
        }
        index = index.saturating_add(1);
    }
    Ok(())
}

fn normalize_aliases(arguments: &[String]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| {
            match argument.as_str() {
                "-ns" => "--no-skills",
                "-nc" => "--no-context-files",
                "-xt" => "--exclude-tools",
                "-nt" => "--no-tools",
                "-na" => "--no-approve",
                value => value,
            }
            .to_owned()
        })
        .collect()
}

fn map_clap_error(error: clap::Error) -> CliParseError {
    match error.kind() {
        ErrorKind::UnknownArgument => CliParseError::unknown(error.to_string()),
        _ => CliParseError::input(error.to_string()),
    }
}

fn parse_tool_list(option: &str, value: &str) -> Result<Vec<String>, CliParseError> {
    let tools: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    if tools.is_empty() {
        return Err(CliParseError::input(format!(
            "{option} requires a non-empty comma-separated list"
        )));
    }
    Ok(tools)
}

fn validate_tools(tools: Vec<String>) -> Result<Vec<String>, CliParseError> {
    for tool in &tools {
        if !SUPPORTED_TOOLS.contains(&tool.as_str()) {
            return Err(CliParseError::input(format!(
                "unsupported tool {tool:?}; pi-rs milestone 1 supports read, bash, edit, and write"
            )));
        }
    }
    let mut unique = Vec::new();
    for tool in tools {
        if !unique.contains(&tool) {
            unique.push(tool);
        }
    }
    Ok(unique)
}

fn validate_session_id(id: &str) -> Result<(), CliParseError> {
    let valid_character =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-');
    if id.is_empty()
        || !id.chars().all(valid_character)
        || !id
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        || !id
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        return Err(CliParseError::input(
            "session id must contain only alphanumeric characters, '-', '_', and '.', and start and end with an alphanumeric character",
        ));
    }
    Ok(())
}
