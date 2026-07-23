use crate::{
    CliArgs, ModelServiceFactory, OutputMode, OutputTargets, PreparedPrompt, RootCancellation,
    RuntimeOutputSink, ThinkingLevelArg, command, open_session, parse_args, prepare_prompts,
    resolve_output_mode,
};
use futures_util::StreamExt;
use pi_agent::{Clock, SystemClock, Tool};
use pi_model::{
    CacheRetention, Cancellation, ModelRequest, ModelRequestOptions, ModelService,
    ModelServiceError, ThinkingLevel,
};
use pi_protocol::{
    AssistantMessageEvent, ContentBlock, Message, MessageContent, Model, ModelInput, Settings,
    TextBlock, UserMessage,
};
use pi_resources::{
    SkillDiscoveryRequest, SystemPromptInput, assemble_system_prompt, discover_context,
    discover_skills, format_context_files, format_skills,
};
use pi_runtime::{
    CompactionFuture, CompactionOutput, CompactionPolicy, CompactionRequest, Compactor,
    RetryPolicy, Runtime, RuntimeBoundaryError, RuntimePolicies, RuntimeRequest, RuntimeStatus,
    TokioSleeper,
};
use pi_store::{
    AuthDocument, AuthRecord, CredentialRequest, ModelSourceSnapshot, StoreDiagnostic, StoreError,
    StorePaths, TokioProcessRunner, TrustDocument, TrustRequest, load_model_sources, load_settings,
    resolve_credential, resolve_trust,
};
use pi_tools::{
    BashTool, BashToolConfig, EditTool, ImagePolicy, MutationCoordinator, ReadTool, WriteTool,
};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_SYSTEM_PROMPT: &str =
    "You are an expert coding assistant operating inside pi, a coding agent harness.";

#[derive(Clone, Debug)]
pub struct CliRequest {
    pub arguments: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub stdin: Option<String>,
    pub stdin_is_terminal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CliExit {
    pub code: i32,
}

pub async fn run_cli(
    request: CliRequest,
    factory: &dyn ModelServiceFactory,
    targets: OutputTargets,
    cancellation: &RootCancellation,
) -> CliExit {
    let result = run_cli_inner(&request, factory, &targets, cancellation).await;
    let mut code = match result {
        Ok(code) => code,
        Err(error) => {
            let _ = targets.stderr_line(&format!("Error: {error}"));
            1
        }
    };
    if let Some(signal_code) = cancellation.signal_exit_code() {
        code = i32::from(signal_code);
    }
    if let Err(error) = targets.flush()
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        let _ = targets.stderr_line(&format!("Error: output flush failed: {error}"));
        code = 1;
    }
    CliExit { code }
}

async fn run_cli_inner(
    request: &CliRequest,
    factory: &dyn ModelServiceFactory,
    targets: &OutputTargets,
    cancellation: &RootCancellation,
) -> Result<i32, CliAppError> {
    let args = parse_args(&request.arguments).map_err(CliAppError::message)?;
    let output_mode =
        resolve_output_mode(&args, request.stdin_is_terminal).map_err(CliAppError::message)?;
    if args.version {
        targets
            .stdout_text(crate::VERSION)
            .and_then(|()| targets.stdout_text("\n"))?;
        return Ok(0);
    }
    if args.help {
        let help = command().render_long_help().to_string();
        targets.stdout_text(&help)?;
        return Ok(0);
    }

    let startup_paths = StorePaths::new(
        agent_home(&request.environment, &request.home),
        &request.cwd,
        &request.home,
    )?;
    if let Some(search) = &args.list_models {
        let sources = load_model_sources(&startup_paths)?;
        report_store_diagnostics(targets, &sources.diagnostics)?;
        write_model_list(targets, &sources, search.as_deref())?;
        return Ok(0);
    }

    let bootstrap_settings = load_settings(&startup_paths, false)?;
    report_store_diagnostics(targets, &bootstrap_settings.diagnostics)?;
    let session_directory =
        resolve_session_directory(&args, request, &startup_paths, &bootstrap_settings.merged)?;
    let mut session = open_session(&args, &startup_paths, &session_directory)?;
    let paths = StorePaths::new(&startup_paths.agent_home, &session.cwd, &startup_paths.home)?;

    let trust_document = TrustDocument::load(&paths)?;
    let protected_resources_present =
        paths.project_settings_file().is_file() || paths.project_skills_dir().is_dir();
    let (trust, _) = resolve_trust(
        TrustRequest {
            cwd: &paths.cwd,
            explicit: args.trust_override,
            default: bootstrap_settings.default_project_trust(),
            protected_resources_present,
            context_disabled: args.no_context_files,
        },
        &trust_document,
    )?;
    let settings = load_settings(&paths, trust.trusted)?;
    report_store_diagnostics(targets, &settings.diagnostics)?;
    let model_sources = load_model_sources(&paths)?;
    report_store_diagnostics(targets, &model_sources.diagnostics)?;
    let auth = AuthDocument::load(&paths)?;
    report_store_diagnostics(targets, &auth.diagnostics)?;
    let context = discover_context(&paths, args.no_context_files)?;
    report_store_diagnostics(targets, &context.diagnostics)?;
    let explicit_skill_paths = args
        .skill_paths
        .iter()
        .map(|path| paths.resolve_user_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let skills = discover_skills(SkillDiscoveryRequest {
        paths: &paths,
        settings: &settings.merged,
        explicit_paths: &explicit_skill_paths,
        project_trusted: trust.trusted,
        include_defaults: !args.no_skills,
    })?;
    for diagnostic in &skills.diagnostics {
        let path = diagnostic
            .path
            .as_ref()
            .map_or_else(String::new, |path| format!(" ({})", path.display()));
        targets.stderr_line(&format!("Warning: {}{path}", diagnostic.message))?;
    }

    let (model, thinking) = select_model(
        &args,
        &session,
        &settings.merged,
        &model_sources,
        &auth,
        &request.environment,
    )?;
    let credential = if factory.requires_credential() {
        let environment_keys = provider_environment_keys(&model.provider);
        Some(
            resolve_credential(
                CredentialRequest {
                    provider: &model.provider,
                    cli_override: args.api_key.as_deref(),
                    provider_environment_keys: &environment_keys,
                    environment: &request.environment,
                    models_json_key: model_sources.configured_api_key(&model.provider),
                    cwd: &paths.cwd,
                },
                &auth,
                &TokioProcessRunner,
                cancellation,
            )
            .await?,
        )
    } else {
        None
    };
    let model_service = factory.create(&model, credential.as_ref(), &settings.merged)?;
    let selected_tool_names = args.tools.enabled.clone();
    let tools = build_tools(
        &paths.cwd,
        &request.environment,
        &settings.merged,
        &selected_tool_names,
    );
    let system_prompt = build_system_prompt(
        &args,
        &paths,
        &settings.merged,
        &context.files,
        &skills.skills,
        &selected_tool_names,
    )?;
    let image_policy = image_policy(&settings.merged);
    let prompts = prepare_prompts(
        &args,
        &paths,
        request.stdin.as_deref(),
        image_policy,
        cancellation,
    )
    .await?;

    if let Some(name) = &args.name {
        session.append_name(name)?;
    }
    session.append_model_and_thinking(
        &model.provider,
        &model.id,
        thinking.map(ThinkingLevelArg::as_str),
    )?;
    if output_mode == OutputMode::Json {
        targets.write_header(&session.header)?;
    }

    let event_sink = RuntimeOutputSink::new(output_mode, targets.clone());
    let session_sink = session.sink();
    let sleeper = TokioSleeper;
    let clock = SystemClock;
    let compactor = ProviderCompactor {
        service: model_service.as_ref(),
        model: model.clone(),
        clock: &clock,
    };
    let runtime = Runtime::new(
        model_service.as_ref(),
        &event_sink,
        &session_sink,
        &compactor,
        &sleeper,
        &clock,
        runtime_policies(&settings.merged),
    );
    let mut history = session.messages.clone();
    let mut terminal_status = RuntimeStatus::Completed;
    let mut last_result = None;
    for prompt in prompts {
        let user_message = prompt_message(prompt, now_ms());
        let result = runtime
            .run(
                RuntimeRequest {
                    model_request: ModelRequest {
                        model: model.clone(),
                        system_prompt: system_prompt.clone(),
                        messages: history,
                        tools: Vec::new(),
                        options: ModelRequestOptions {
                            reasoning: thinking.and_then(to_runtime_thinking),
                            cache_retention: CacheRetention::Short,
                            session_id: Some(session.header.id.clone()),
                            max_tokens: Some(model.max_tokens),
                            ..ModelRequestOptions::default()
                        },
                    },
                    prompt: Some(user_message),
                    tools: tools.clone(),
                    parallel_tools: true,
                },
                cancellation,
            )
            .await?;
        terminal_status = result.status.clone();
        history = result.messages.clone();
        let should_stop = terminal_status != RuntimeStatus::Completed;
        last_result = Some(result);
        if should_stop {
            break;
        }
    }
    let result = last_result.ok_or_else(|| CliAppError::message("no prompt was executed"))?;
    if output_mode == OutputMode::Text {
        crate::output::write_text_result(targets, &result)?;
    } else if terminal_status != RuntimeStatus::Completed
        && let Some(Message::Assistant(assistant)) = result.messages.last()
    {
        targets.stderr_line(
            assistant
                .error_message
                .as_deref()
                .unwrap_or("request did not complete"),
        )?;
    }
    Ok(if terminal_status == RuntimeStatus::Completed {
        0
    } else {
        1
    })
}

fn agent_home(environment: &BTreeMap<String, String>, home: &Path) -> PathBuf {
    environment
        .get("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".pi").join("agent"))
}

fn resolve_session_directory(
    args: &CliArgs,
    request: &CliRequest,
    paths: &StorePaths,
    settings: &Settings,
) -> Result<PathBuf, StoreError> {
    if let Some(path) = &args.session_dir {
        return paths.resolve_user_path(path);
    }
    if let Some(path) = request.environment.get("PI_CODING_AGENT_SESSION_DIR") {
        return paths.resolve_user_path(path);
    }
    if let Some(path) = &settings.session_dir {
        return paths.resolve_user_path(path);
    }
    Ok(paths.default_session_dir())
}

fn select_model(
    args: &CliArgs,
    session: &crate::CliSession,
    settings: &Settings,
    sources: &ModelSourceSnapshot,
    auth: &AuthDocument,
    environment: &BTreeMap<String, String>,
) -> Result<(Model, Option<ThinkingLevelArg>), CliAppError> {
    let mut thinking = args.thinking;
    let mut provider = args.provider.clone();
    let mut pattern = args.model.clone();
    if provider.is_none()
        && let Some(model_pattern) = &pattern
        && let Some((prefix, suffix)) = model_pattern.split_once('/')
        && sources.catalog.providers.contains_key(prefix)
    {
        provider = Some(prefix.to_owned());
        pattern = Some(suffix.to_owned());
    }
    if thinking.is_none()
        && let Some(model_pattern) = &pattern
        && let Some((prefix, suffix)) = model_pattern.rsplit_once(':')
        && let Ok(parsed) = parse_thinking(suffix)
    {
        thinking = Some(parsed);
        pattern = Some(prefix.to_owned());
    }

    if args.api_key.is_some()
        && provider.is_none()
        && pattern.is_none()
        && session.saved_provider.is_none()
        && settings.default_provider.is_none()
    {
        return Err(CliAppError::message(
            "--api-key requires a resolvable provider and model",
        ));
    }

    let selected_provider = provider
        .or_else(|| session.saved_provider.clone())
        .or_else(|| settings.default_provider.clone());
    let selected_pattern = pattern
        .or_else(|| {
            session
                .saved_provider
                .as_ref()
                .zip(session.saved_model.as_ref())
                .filter(|(saved, _)| selected_provider.as_ref() == Some(*saved))
                .map(|(_, model)| model.clone())
        })
        .or_else(|| {
            settings
                .default_provider
                .as_ref()
                .zip(settings.default_model.as_ref())
                .filter(|(saved, _)| selected_provider.as_ref() == Some(*saved))
                .map(|(_, model)| model.clone())
        });

    if let Some(provider) = selected_provider {
        let provider_catalog = sources.catalog.providers.get(&provider).ok_or_else(|| {
            CliAppError::message(format!(
                "unknown provider {provider:?}; use --list-models to inspect supported models"
            ))
        })?;
        let base = if let Some(pattern) = selected_pattern {
            match_model(provider_catalog.models.values(), &pattern).or_else(|| {
                provider_catalog
                    .models
                    .values()
                    .next()
                    .cloned()
                    .map(|mut model| {
                        model.id.clone_from(&pattern);
                        model.name = pattern;
                        model
                    })
            })
        } else {
            default_model(&provider, provider_catalog.models.values())
        };
        let model = base.ok_or_else(|| {
            CliAppError::message(format!("provider {provider:?} has no milestone-1 models"))
        })?;
        if sources
            .supported_model(&model.provider, &model.id)
            .is_none()
            && !sources.custom_providers.contains(&model.provider)
            && !matches!(model.provider.as_str(), "openai" | "anthropic" | "google")
        {
            return Err(CliAppError::message(format!(
                "provider {:?} is unsupported in pi-rs milestone 1",
                model.provider
            )));
        }
        return Ok((
            model,
            thinking.or_else(|| parse_saved_thinking(session, settings)),
        ));
    }

    let supported = supported_models(sources);
    if let Some(pattern) = selected_pattern {
        let matches: Vec<Model> = supported
            .iter()
            .filter(|model| model.id.eq_ignore_ascii_case(&pattern))
            .cloned()
            .collect();
        if matches.len() == 1 {
            return Ok((
                matches[0].clone(),
                thinking.or_else(|| parse_saved_thinking(session, settings)),
            ));
        }
        if matches.len() > 1 {
            return Err(CliAppError::message(format!(
                "model {pattern:?} is ambiguous; specify --provider"
            )));
        }
        return Err(CliAppError::message(format!(
            "model {pattern:?} was not found; use --list-models"
        )));
    }
    for provider in ["anthropic", "openai", "google"]
        .into_iter()
        .chain(sources.custom_providers.iter().map(String::as_str))
    {
        if !credential_is_configured(provider, auth, sources, environment) {
            continue;
        }
        let Some(catalog) = sources.catalog.providers.get(provider) else {
            continue;
        };
        if let Some(model) = default_model(provider, catalog.models.values()) {
            return Ok((
                model,
                thinking.or_else(|| parse_saved_thinking(session, settings)),
            ));
        }
    }
    Err(CliAppError::message(
        "no model is configured; specify --provider and --model",
    ))
}

fn credential_is_configured(
    provider: &str,
    auth: &AuthDocument,
    sources: &ModelSourceSnapshot,
    environment: &BTreeMap<String, String>,
) -> bool {
    matches!(auth.records.get(provider), Some(AuthRecord::ApiKey { .. }))
        || provider_environment_keys(provider)
            .iter()
            .any(|key| environment.get(key).is_some_and(|value| !value.is_empty()))
        || sources.configured_api_key(provider).is_some()
}

fn supported_models(sources: &ModelSourceSnapshot) -> Vec<Model> {
    sources
        .catalog
        .providers
        .iter()
        .flat_map(|(provider, catalog)| {
            catalog
                .models
                .values()
                .filter_map(|model| sources.supported_model(provider, &model.id).cloned())
        })
        .collect()
}

fn match_model<'a>(models: impl Iterator<Item = &'a Model>, pattern: &str) -> Option<Model> {
    let models: Vec<&Model> = models.collect();
    models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(pattern))
        .or_else(|| {
            let mut matches: Vec<&&Model> = models
                .iter()
                .filter(|model| {
                    model.id.to_lowercase().contains(&pattern.to_lowercase())
                        || model.name.to_lowercase().contains(&pattern.to_lowercase())
                })
                .collect();
            matches.sort_by(|left, right| right.id.cmp(&left.id));
            matches.first().copied()
        })
        .map(|model| (*model).clone())
}

fn default_model<'a>(provider: &str, models: impl Iterator<Item = &'a Model>) -> Option<Model> {
    let preferred = match provider {
        "openai" => Some("gpt-5.5"),
        "anthropic" => Some("claude-opus-4-8"),
        "google" => Some("gemini-3.1-pro-preview"),
        _ => None,
    };
    let models: Vec<&Model> = models.collect();
    preferred
        .and_then(|id| models.iter().find(|model| model.id == id).copied())
        .or_else(|| models.first().copied())
        .cloned()
}

fn parse_saved_thinking(
    session: &crate::CliSession,
    settings: &Settings,
) -> Option<ThinkingLevelArg> {
    session
        .saved_thinking
        .as_deref()
        .and_then(|value| parse_thinking(value).ok())
        .or_else(|| {
            settings
                .default_thinking_level
                .as_deref()
                .and_then(|value| parse_thinking(value).ok())
        })
        .or(Some(ThinkingLevelArg::Medium))
}

fn parse_thinking(value: &str) -> Result<ThinkingLevelArg, ()> {
    match value {
        "off" => Ok(ThinkingLevelArg::Off),
        "minimal" => Ok(ThinkingLevelArg::Minimal),
        "low" => Ok(ThinkingLevelArg::Low),
        "medium" => Ok(ThinkingLevelArg::Medium),
        "high" => Ok(ThinkingLevelArg::High),
        "xhigh" => Ok(ThinkingLevelArg::XHigh),
        "max" => Ok(ThinkingLevelArg::Max),
        _ => Err(()),
    }
}

const fn to_runtime_thinking(value: ThinkingLevelArg) -> Option<ThinkingLevel> {
    match value {
        ThinkingLevelArg::Off => None,
        ThinkingLevelArg::Minimal => Some(ThinkingLevel::Minimal),
        ThinkingLevelArg::Low => Some(ThinkingLevel::Low),
        ThinkingLevelArg::Medium => Some(ThinkingLevel::Medium),
        ThinkingLevelArg::High => Some(ThinkingLevel::High),
        ThinkingLevelArg::XHigh => Some(ThinkingLevel::XHigh),
        ThinkingLevelArg::Max => Some(ThinkingLevel::Max),
    }
}

fn provider_environment_keys(provider: &str) -> Vec<String> {
    match provider {
        "openai" => vec!["OPENAI_API_KEY".to_owned()],
        "anthropic" => vec!["ANTHROPIC_API_KEY".to_owned()],
        "google" => vec!["GEMINI_API_KEY".to_owned()],
        _ => Vec::new(),
    }
}

fn build_tools(
    cwd: &Path,
    environment: &BTreeMap<String, String>,
    settings: &Settings,
    enabled: &[String],
) -> Vec<Arc<dyn Tool>> {
    let mutations = MutationCoordinator::default();
    let image_policy = image_policy(settings);
    let mut bash_config = BashToolConfig::new(cwd);
    bash_config.env.clone_from(environment);
    bash_config.shell_path = settings.shell_path.as_ref().map(PathBuf::from);
    bash_config.command_prefix = settings.shell_command_prefix.clone();
    let mut registry: BTreeMap<&str, Arc<dyn Tool>> = BTreeMap::from([
        (
            "read",
            Arc::new(ReadTool::new(cwd, image_policy)) as Arc<dyn Tool>,
        ),
        (
            "bash",
            Arc::new(BashTool::new(bash_config)) as Arc<dyn Tool>,
        ),
        (
            "edit",
            Arc::new(EditTool::new(cwd, mutations.clone())) as Arc<dyn Tool>,
        ),
        (
            "write",
            Arc::new(WriteTool::new(cwd, mutations)) as Arc<dyn Tool>,
        ),
    ]);
    enabled
        .iter()
        .filter_map(|name| registry.remove(name.as_str()))
        .collect()
}

fn image_policy(settings: &Settings) -> ImagePolicy {
    let mut policy = ImagePolicy::default();
    if let Some(images) = &settings.images {
        policy.block_images = images.block_images.unwrap_or(false);
        policy.auto_resize = images.auto_resize.unwrap_or(true);
    }
    policy
}

fn build_system_prompt(
    args: &CliArgs,
    paths: &StorePaths,
    _settings: &Settings,
    context_files: &[pi_resources::ContextFile],
    skills: &[pi_resources::Skill],
    tool_names: &[String],
) -> Result<String, StoreError> {
    let appended = args
        .append_system_prompt
        .iter()
        .map(|value| {
            let candidate = paths.resolve_user_path(value)?;
            if candidate.is_file() {
                std::fs::read_to_string(&candidate)
                    .map_err(|error| StoreError::io(error, candidate))
            } else {
                Ok(value.clone())
            }
        })
        .collect::<Result<Vec<_>, StoreError>>()?
        .join("\n\n");
    if let Some(system) = &args.system_prompt {
        let mut sections = vec![system.clone()];
        let context = format_context_files(context_files);
        if !context.is_empty() {
            sections.push(context);
        }
        let skills = format_skills(skills);
        if !skills.is_empty() && tool_names.iter().any(|tool| tool == "read") {
            sections.push(skills);
        }
        if !appended.trim().is_empty() {
            sections.push(appended);
        }
        return Ok(sections.join("\n\n"));
    }
    let snippets = BTreeMap::from([
        ("bash".to_owned(), "execute shell commands".to_owned()),
        (
            "edit".to_owned(),
            "apply exact text replacements".to_owned(),
        ),
        ("read".to_owned(), "read text and image files".to_owned()),
        ("write".to_owned(), "write complete files".to_owned()),
    ]);
    Ok(format!(
        "{DEFAULT_SYSTEM_PROMPT}\n\n{}",
        assemble_system_prompt(&SystemPromptInput {
            selected_tools: Some(tool_names.to_vec()),
            tool_snippets: snippets,
            appended_prompt: (!appended.trim().is_empty()).then_some(appended),
            context_files: context_files.to_vec(),
            skills: skills.to_vec(),
            cwd: paths.cwd.clone(),
            ..SystemPromptInput::default()
        })
    ))
}

fn runtime_policies(settings: &Settings) -> RuntimePolicies {
    let retry = settings.retry.as_ref();
    let compaction = settings.compaction.as_ref();
    RuntimePolicies {
        retry: RetryPolicy {
            enabled: retry.and_then(|value| value.enabled).unwrap_or(true),
            max_retries: retry.and_then(|value| value.max_retries).unwrap_or(3),
            base_delay: Duration::from_millis(
                retry.and_then(|value| value.base_delay_ms).unwrap_or(2_000),
            ),
        },
        compaction: CompactionPolicy {
            enabled: compaction.and_then(|value| value.enabled).unwrap_or(true),
            reserve_tokens: compaction
                .and_then(|value| value.reserve_tokens)
                .unwrap_or(16_384),
            keep_recent_tokens: compaction
                .and_then(|value| value.keep_recent_tokens)
                .unwrap_or(20_000),
        },
    }
}

fn prompt_message(prompt: PreparedPrompt, timestamp: u64) -> UserMessage {
    if prompt.images.is_empty() {
        return UserMessage::new(MessageContent::Text(prompt.text), timestamp);
    }
    let mut blocks = Vec::with_capacity(prompt.images.len() + 1);
    if !prompt.text.is_empty() {
        blocks.push(ContentBlock::Text(TextBlock::new(prompt.text)));
    }
    blocks.extend(prompt.images.into_iter().map(ContentBlock::Image));
    UserMessage::new(MessageContent::Blocks(blocks), timestamp)
}

fn report_store_diagnostics(
    targets: &OutputTargets,
    diagnostics: &[StoreDiagnostic],
) -> Result<(), io::Error> {
    for diagnostic in diagnostics {
        let path = diagnostic
            .path
            .as_ref()
            .map_or_else(String::new, |path| format!(" ({path})"));
        targets.stderr_line(&format!("Warning: {}{path}", diagnostic.message))?;
    }
    Ok(())
}

fn write_model_list(
    targets: &OutputTargets,
    sources: &ModelSourceSnapshot,
    search: Option<&str>,
) -> Result<(), io::Error> {
    let search = search.map(str::to_lowercase);
    let mut models = supported_models(sources);
    models.retain(|model| {
        search.as_ref().is_none_or(|search| {
            format!("{} {} {}", model.provider, model.id, model.name)
                .to_lowercase()
                .contains(search)
        })
    });
    models.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.id.cmp(&right.id))
    });
    if models.is_empty() {
        targets.stdout_text("No matching milestone-1 models\n")?;
        return Ok(());
    }
    targets.stdout_text("provider  model  context  max-out  thinking  images\n")?;
    for model in models {
        targets.stdout_text(&format!(
            "{}  {}  {}  {}  {}  {}\n",
            model.provider,
            model.id,
            format_tokens(model.context_window),
            format_tokens(model.max_tokens),
            if model.reasoning { "yes" } else { "no" },
            if model.input.contains(&ModelInput::Image) {
                "yes"
            } else {
                "no"
            },
        ))?;
    }
    Ok(())
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 && tokens.is_multiple_of(1_000_000) {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}K", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

struct ProviderCompactor<'a> {
    service: &'a dyn ModelService,
    model: Model,
    clock: &'a dyn Clock,
}

impl Compactor for ProviderCompactor<'_> {
    fn compact<'a>(
        &'a self,
        request: CompactionRequest,
        cancellation: &'a dyn Cancellation,
    ) -> CompactionFuture<'a> {
        Box::pin(async move {
            let mut messages = request.messages;
            messages.push(Message::User(UserMessage::new(
                MessageContent::Text(
                    "Summarize the conversation for continued coding work. Preserve decisions, files, commands, failures, and pending tasks."
                        .to_owned(),
                ),
                self.clock.now_ms(),
            )));
            let mut stream = self
                .service
                .stream(
                    ModelRequest {
                        model: self.model.clone(),
                        system_prompt: "Produce a concise continuation summary only.".to_owned(),
                        messages,
                        tools: Vec::new(),
                        options: ModelRequestOptions {
                            max_tokens: Some(self.model.max_tokens.min(4_096)),
                            tool_choice: Some(pi_model::ToolChoice::None),
                            ..ModelRequestOptions::default()
                        },
                    },
                    cancellation,
                )
                .await
                .map_err(model_boundary)?;
            while let Some(event) = stream.next().await {
                match event.map_err(model_boundary)? {
                    AssistantMessageEvent::Done { message, .. } => {
                        let summary = message
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                ContentBlock::Text(text) => Some(text.text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        if summary.trim().is_empty() {
                            return Err(RuntimeBoundaryError::new(
                                "compaction model returned no summary text",
                            ));
                        }
                        return Ok(CompactionOutput {
                            summary,
                            usage: message.usage,
                        });
                    }
                    AssistantMessageEvent::Error { error, .. } => {
                        return Err(RuntimeBoundaryError::new(
                            error
                                .error_message
                                .unwrap_or_else(|| "compaction model failed".to_owned()),
                        ));
                    }
                    _ => {}
                }
            }
            Err(RuntimeBoundaryError::new(
                "compaction stream ended without a terminal message",
            ))
        })
    }
}

fn model_boundary(error: ModelServiceError) -> RuntimeBoundaryError {
    RuntimeBoundaryError::new(error.message)
}

#[derive(Debug)]
struct CliAppError(String);

impl CliAppError {
    fn message(message: impl ToString) -> Self {
        Self(message.to_string())
    }
}

impl std::fmt::Display for CliAppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CliAppError {}

impl From<StoreError> for CliAppError {
    fn from(error: StoreError) -> Self {
        Self(error.to_string())
    }
}

impl From<ModelServiceError> for CliAppError {
    fn from(error: ModelServiceError) -> Self {
        Self(error.message)
    }
}

impl From<RuntimeBoundaryError> for CliAppError {
    fn from(error: RuntimeBoundaryError) -> Self {
        Self(error.message)
    }
}

impl From<pi_runtime::RuntimeError> for CliAppError {
    fn from(error: pi_runtime::RuntimeError) -> Self {
        Self(error.to_string())
    }
}

impl From<io::Error> for CliAppError {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}
