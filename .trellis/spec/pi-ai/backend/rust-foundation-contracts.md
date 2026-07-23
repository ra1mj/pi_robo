# Rust Foundation Contracts

## 1. Scope / Trigger

Use this contract whenever a task changes `rust/**`, the Rust model catalog exporter, persisted session DTOs, or a TypeScript/Rust compatibility fixture. It prevents provider and runtime crates from redefining shared JSON shapes or silently dropping future fields.

## 2. Signatures

- Toolchain: Rust `1.97.0`, edition `2024`, resolver `3`.
- Catalog commands:
  - Generate: `npm run generate:rust-model-catalog`
  - Verify: `npm run check:rust-model-catalog`
- Persisted record API: `PersistedSessionRecord::parse(&str) -> Result<PersistedSessionRecord, ContractError>`.
- Model boundary: `ModelService::stream<'a>(&'a self, ModelRequest, &'a dyn Cancellation) -> ModelFuture<'a>`.
- Compatibility validation: `validate_compatibility_catalog(&Path) -> Result<CompatibilityCatalog, Vec<String>>`.

## 3. Contracts

- `pi-protocol` owns shared JSON DTOs; downstream crates import them rather than defining provider-local copies.
- Known extensible objects use `#[serde(flatten)]` extension maps. Session persistence keeps both the raw JSON value and an optional typed record. Unknown session record kinds must round-trip unchanged.
- `rust/assets/models.json` is derived offline from `packages/ai/src/models.generated.ts`. Never edit either generated artifact by hand; update the owning generator and regenerate.
- Production crate direction is enforced by `workspace_policy.rs`. No production crate may depend on `pi-test-support`.
- Direct third-party crates are exact-pinned in `Cargo.toml`, locked in `Cargo.lock`, and recorded in `rust/DEPENDENCIES.md`.
- Foundation tests require no credentials or outbound provider access. Local HTTP tests bind only to `127.0.0.1`.

## 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Invalid JSON | `ContractErrorCategory::InvalidJson` |
| Non-object or missing `type` | `InvalidShape` with a JSON path |
| Recognized session type with missing/invalid fields | `InvalidShape`; do not reinterpret as an unknown record |
| Unknown session type | Preserve raw value and return `known() == None` |
| Stale model artifact | `check:rust-model-catalog` fails |
| Duplicate compatibility ID, invalid owner, missing evidence, or root normalizer | Compatibility validation fails closed |
| Wildcard, unapproved license, git source, or registry | `cargo deny check` fails |
| Forbidden internal dependency edge | `workspace_policy` fails |

## 5. Good / Base / Bad Cases

- Good: a future field on a known session record is available in the typed extension map and survives serialization.
- Base: current v3 records decode to typed variants and reserialize to equivalent JSON.
- Bad: decoding a session entry into a closed struct with default Serde field dropping.
- Bad: fetching live catalog data from a Rust build or test.

## 6. Tests Required

- Protocol fixtures assert message, event, settings, model-catalog, and session round trips.
- Session tests assert malformed JSON, missing discriminants, malformed recognized records, unknown fields, and unknown record kinds.
- Test-support tests assert cancellation, bounded sinks, deterministic time/IDs/sleeps, fixture confinement, credential scanning, local-only HTTP, and normalization allowlists.
- Compatibility tests assert valid evidence and fail-closed invalid catalogs.
- Required gates: format, locked Clippy, locked tests, rustdoc warnings, `cargo deny check`, and `npm run check`.

## 7. Wrong vs Correct

Wrong:

```rust
#[derive(serde::Deserialize)]
struct SessionEntry {
    id: String,
}
```

This silently discards unknown persisted fields.

Correct:

```rust
#[derive(serde::Deserialize, serde::Serialize)]
struct SessionEntry {
    id: String,
    #[serde(default, flatten)]
    extensions: pi_protocol::Extensions,
}
```

For discriminated session records, also retain the original `serde_json::Value` through `PersistedSessionRecord`.

## Scenario: Provider Protocol Adapters

### 1. Scope / Trigger

Use this scenario when adding or changing a Rust provider adapter, provider request option, streamed assistant event, transport timeout, or provider error mapping. The provider layer is a protocol boundary: all adapters must expose one provider-independent service contract even when upstream wire formats differ.

### 2. Signatures

- Service: `ModelService::stream<'a>(&'a self, ModelRequest, &'a dyn Cancellation) -> ModelFuture<'a>`.
- Cancellation: `Cancellation::cancelled(&self) -> CancellationFuture<'_>` must wake a pending waiter.
- Stream: `ModelEventStream<'a>` yields `Result<AssistantMessageEvent, ModelServiceError>`.
- Configuration: `ProviderAdapterConfig::new(base_url, ProviderTimeouts)` returns an immutable resolved configuration; credentials and proxy values use `SecretString`.
- Supported adapters: `OpenAiChatAdapter`, `OpenAiResponsesAdapter`, `AnthropicMessagesAdapter`, and `GoogleGenerativeLanguageAdapter`.

### 3. Contracts

- A `ModelRequest` carries the selected model, system prompt, ordered message history, tools, and validated `ModelRequestOptions`.
- Request options remain provider-independent: temperature, maximum tokens, reasoning level, optional thinking budgets, cache retention, session ID, and tool choice. Adapters map only supported options and fail closed on invalid values.
- A successful stream emits exactly one `Start`, preserves provider delta order, and ends with exactly one `Done`. The `Done` completion reason must match the assembled message stop reason.
- Pre-stream failures are returned by `ModelFuture`; established-stream failures are `Err(ModelServiceError)` items. Adapters must not emit `AssistantMessageEvent::Error`.
- Abrupt EOF before the provider terminal marker is a retryable `Protocol` error. Partial or malformed tool arguments must never be exposed as a successful tool call.
- Provider response IDs, response model IDs, reasoning signatures, thought signatures, redacted thinking blocks, tool-call IDs, and cache-token usage are retained when the upstream protocol supplies them.
- Connect, response-header, and body-idle timeouts are independent. Cancellation is observed before dispatch, while waiting for headers, and while consuming the body.
- Error messages are bounded to 4,096 characters and redact configured authorization/proxy secrets.

### 4. Validation & Error Matrix

| Condition | Canonical result | Retryable |
| --- | --- | --- |
| Missing/invalid configuration | `Configuration` | No |
| HTTP 401 / invalid API key | `Authentication` | No |
| HTTP 403 / permission denied | `Permission` | No |
| Context limit exceeded | `ContextOverflow` | No |
| Billing quota exhausted | `QuotaExceeded` | No |
| HTTP 429 / throttling | `RateLimit` | Yes |
| Connect/header/body timeout | `Timeout` | Yes |
| Network failure | `Network` | Yes |
| HTTP 502–504 / unavailable | `Unavailable` | Yes |
| Provider/server overload | `Server` | Yes |
| User cancellation | `Cancelled` | No |
| Malformed SSE, invalid UTF-8, invalid event order, or abrupt EOF | `Protocol` | Depends on whether retry can safely replay |

Provider codes override ambiguous HTTP status classification. Hard non-retryable categories remain non-retryable even if an upstream payload claims otherwise. Preserve `http_status`, `provider_code`, and `retry_after_ms` when available.

### 5. Good / Base / Bad Cases

- Good: OpenAI parallel tool-call fragments are coalesced by stable call index while text and reasoning deltas remain ordered.
- Good: Anthropic signed thinking and Google thought signatures survive history replay and the streamed response.
- Base: a text-only response produces `Start`, ordered text deltas, usage, and a matching `Done`.
- Bad: treating socket EOF as successful completion because some text was already received.
- Bad: retrying authentication, quota, context-overflow, or cancellation failures.
- Bad: making adapter contract tests depend on live credentials or paid provider calls.

### 6. Tests Required

- Each adapter needs request-mapping, mixed-content streaming, usage, terminal-marker, abrupt-EOF, malformed-tool, and representative HTTP/provider-error assertions where supported.
- Cross-protocol tests must run the same canonical text-stream contract against every adapter plus the faux provider.
- Transport tests must distinguish connect/header/body-idle timeouts and prove cancellation wakes pending header/body work.
- SSE tests must cover arbitrary byte splits, UTF-8 splits, multiline data, comments, line-ending variants, invalid UTF-8, and bounded event size.
- Fixture rows move to `verified` only when their `runner` points to the actual passing contract test.
- Tests use the faux provider or a one-shot `127.0.0.1` server; no real provider endpoint, token, or billable request is allowed.

### 7. Wrong vs Correct

Wrong:

```rust
// EOF is not proof that a provider completed the response.
if upstream.next().await.is_none() {
    return Ok(done_from_partial_message());
}
```

Correct:

```rust
if upstream.next().await.is_none() && !saw_provider_terminal_marker {
    return Err(ModelServiceError::protocol(
        "provider stream ended before its terminal marker",
        true,
    ));
}
```

Keep protocol-specific parsing inside `pi-provider`; downstream runtime crates consume only `ModelService`, `ModelEventStream`, canonical events, and canonical errors.

## Scenario: Agent Runtime and Core Tools

### 1. Scope / Trigger

Use this scenario when changing the Rust agent loop, tool contract, `read`/`bash`/`edit`/`write`, retry scheduling, compaction, or in-memory session integration. These layers share ordering, cancellation, and persistence contracts that must not drift independently.

### 2. Signatures

- Low-level run: `Agent::run(AgentRunRequest, &dyn Cancellation) -> Result<AgentRunResult, AgentRunError>`.
- Tool: `Tool::execute(&ToolCallBlock, &dyn Cancellation, &dyn ToolUpdateSink) -> ToolFuture<'_>`; `definition()` returns the canonical JSON Schema and `execution_mode()` defaults to parallel.
- Events: `EventSink::emit(AgentEvent) -> EventFuture<'_>`; `event_channel(capacity)` is bounded and backpressured.
- High-level run: `Runtime::run(RuntimeRequest, &dyn Cancellation) -> Result<RuntimeRunResult, RuntimeError>`.
- Persistence: `SessionSink::record_run(AgentRunResult)` and `record_compaction(CompactionRecord)`.
- Compaction: `Compactor::compact(CompactionRequest, &dyn Cancellation) -> CompactionFuture<'_>`.

### 3. Contracts

- `pi-agent` owns provider-neutral stream accumulation and event ordering. It emits `agent_start`, complete turn/message/tool events, then awaits `agent_end` last.
- Parallel tool completion/update events use actual completion order. Persisted `ToolResultMessage` values remain in assistant source order. A sequential tool override serializes the whole batch.
- Unknown tools, invalid arguments, and tool failures become error tool results. Tool calls from `length` responses are rejected and never executed.
- Canonical history retains image blocks. Requests to text-only models omit those blocks and add the explicit non-vision omission note.
- `pi-tools` resolves relative paths from the supplied absolute cwd. Existing mutation targets use canonical keys; missing targets use normalized absolute keys. Mutation leases cover reads and writes.
- Text/bash output uses the shared 2,000-line/50-KiB contract. Bash keeps a UTF-8-safe tail, writes full truncated output to a mode-`0600` temporary file, and kills the Linux process group on timeout/cancellation.
- Image work runs on a blocking worker with decoder allocation/pixel limits. Supported input is JPEG, non-animated PNG, GIF, WebP, and validated BMP; BMP is normalized to PNG.
- Retry belongs to `pi-runtime`: three retries by default, 2/4/8-second backoff, with canonical `retry_after_ms` overriding the calculated delay.
- Threshold compaction runs after a completed response without starting another turn. Context overflow compacts and retries at most once. Run attempts and compaction records go through `SessionSink`.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Unknown tool / malformed tool arguments | Source-ordered `ToolResultMessage` with `is_error = true` |
| Tool call in a `length` response | Error tool result; do not invoke the tool |
| Closed event receiver | `AgentRunError::EventSink`; never drop the event silently |
| Cancelled model/tool/retry/compaction | Typed cancelled/aborted status; settle announced events and process cleanup |
| Missing/unreadable path | `ToolErrorCategory::Execution` with the path and OS error |
| Empty, missing, duplicate, no-op, or overlapping edit | Deterministic invalid/execution error; file remains unchanged |
| Oversized image allocation/pixel count | Omission text; no image block and no panic |
| Bash timeout/cancellation | Terminate the process group, drain late output, then return timeout/cancelled error |
| Retryable provider error below bound | Remove the terminal error from active context, persist the attempt, sleep, retry |
| Non-retryable/exhausted provider error | Preserve the terminal attempt and return failed status |
| Second context overflow | No second compaction; return failed status and an observable compaction error |

### 5. Good / Base / Bad Cases

- Good: two parallel tools finish second-first, emit completion second-first, and append results first-second.
- Good: a BMP read returns explanatory text plus a canonical PNG image while respecting decode limits.
- Base: a Faux text response emits one complete agent turn and persists through `InMemorySessionSink` without CLI or disk resources.
- Bad: executing a partially streamed tool call after an output-limit stop.
- Bad: releasing a mutation lease when cancellation wins while an in-flight write can still complete.
- Bad: implementing provider retry inside both an adapter and `pi-runtime`.

### 6. Tests Required

- `pi-agent/tests/event_trace.rs`: single/multi-turn traces, parallel/source order, sequential override, invalid/unknown/length calls, images, non-vision filtering, cancellation, and delayed sink settlement.
- `pi-tools` contract tests: path normalization, symlink mutation aliases, truncation, every supported image format, decoded-pixel bounds, read ranges/errors, bash late output/process trees, edit normalization/BOM/line endings/diffs, write permissions/symlink behavior, and captured TypeScript schemas.
- `pi-runtime` contract tests: bounded retry/backoff/retry-after/cancellation/exhaustion, threshold timing, summary/accounting, continued use, overflow retry-once, and in-memory persistence.
- Fixture catalog rows may be `verified` only when their named runner passes. Required gates remain locked format/Clippy/tests, `cargo deny check`, and `npm run check`.

### 7. Wrong vs Correct

Wrong:

```rust
for completed in run_tools_in_parallel(calls).await {
    history.push(completed.tool_result);
}
```

This makes persisted history nondeterministic and can mismatch the assistant tool-call order.

Correct:

```rust
let completed = run_tools_in_parallel(calls).await;
emit_completion_events_in_finish_order(&completed).await?;
history.extend(sort_results_by_source_index(completed));
```

Keep provider wire behavior in `pi-provider`, low-level turn ordering in `pi-agent`, filesystem/process behavior in `pi-tools`, and retry/compaction/session coordination in `pi-runtime`.

## Scenario: Rust Data, Resources, and Trust

### 1. Scope / Trigger

Use this scenario when changing Rust paths, settings, `models.json`, API-key auth, trust, session-v3 persistence, context discovery, skill discovery, or system-prompt inputs. These boundaries share read-only configuration and append-only compatibility requirements with `packages/coding-agent`.

### 2. Signatures

- Paths: `StorePaths::new(agent_home, cwd, home) -> Result<StorePaths, StoreError>`.
- Settings: `load_settings(&StorePaths, project_trusted) -> Result<SettingsSnapshot, StoreError>`.
- Models: `load_model_sources(&StorePaths) -> Result<ModelSourceSnapshot, StoreError>`.
- Auth: `resolve_credential(CredentialRequest, &AuthDocument, &dyn ProcessRunner, &dyn CommandCancellation) -> Result<ResolvedCredential, StoreError>`.
- Trust: `resolve_trust(TrustRequest, &TrustDocument) -> Result<(TrustDecision, Vec<ResourceAccess>), StoreError>`.
- Session read/write: `SessionFileSnapshot::read(path)` and `SessionWriter::{create,open,append}`.
- Resource loading: `discover_context(&StorePaths, disabled)` and `discover_skills(SkillDiscoveryRequest)`.
- Prompt projection: `assemble_system_prompt(&SystemPromptInput) -> String`.

### 3. Contracts

- Settings, models, auth, and trust are read-only in milestone 1. Reads never create config directories, lock files, or normalized rewrites.
- Trusted project settings merge over global settings at the top level; when both values are objects, merge exactly one nested level. Arrays and primitives replace.
- Credential precedence is caller CLI override, `auth.json` API-key record, provider environment, then `models.json` key. OAuth records remain raw but cannot execute or refresh.
- Config values support literals, `$VAR`, `${VAR}`, `$$`, `$!`, and `!command`. Invalid environment-reference syntax remains literal. Command execution uses injected environment/cwd/cancellation with 10-second and 64-KiB defaults.
- Debug output for credentials, command strings, environment values, and model keys is redacted.
- Direct milestone-1 built-in brands are `openai`, `anthropic`, and `google`; user-defined providers are allowed only with a selected supported protocol. Other embedded brands are not returned by `supported_model`.
- The nearest canonical saved trust ancestor wins. Explicit caller decisions win over saved/default decisions. Headless `ask` skips protected project settings and skills.
- Context files are not protected by project trust. They load global-first, then filesystem-root-to-cwd, unless context loading itself is disabled.
- Session writes require v3, append one compact JSON object plus newline, preserve every prior byte, and reject detectable external metadata/length changes. This is not a cross-process lock or a concurrent TypeScript/Rust writer guarantee.
- Session reads retain raw lines, malformed-line diagnostics, unknown fields, and unknown record kinds. v1/v2 are inspection-only and never migrated by Rust.
- Skill collisions keep the first source, exact canonical duplicates are silent, root `SKILL.md` stops recursion, ignore files apply, and `disable-model-invocation` omits a skill from prompt XML.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Invalid settings JSON | Empty typed view plus structured diagnostic; bytes unchanged |
| Invalid models/auth/trust JSON or shape | Structured `StoreError` with path/line where available |
| OAuth without an API-key fallback | `UnsupportedOauth` guidance; record unchanged |
| Missing credential | `Authentication`; never include secret values |
| Command timeout/cancellation/output overflow | `Timeout` / `Cancelled` / `OutputLimit` |
| Unsupported custom model protocol | Diagnostic; model is not advertised |
| Saved denial or headless `ask` | Context loads; project settings/skills skip |
| Session first non-empty line is not a valid header | `InvalidShape`; file unchanged |
| Session version 1/2 append | `UnsupportedVersion`; no migration or write |
| Detectable external session append/change | `StaleSession`; caller must reload |
| Same-session simultaneous TypeScript/Rust writers | Unsupported; no safety claim |

### 5. Good / Base / Bad Cases

- Good: TypeScript reads a Rust-appended v3 record, then Rust reads a later TypeScript append while every earlier line remains byte-identical.
- Good: saved project denial still loads ancestor `AGENTS.md` but omits `.pi/settings.json` and `.pi/skills`.
- Base: missing config files produce empty read-only snapshots without creating `~/.pi/agent`.
- Bad: treating project context as trust-protected.
- Bad: logging a `CredentialRequest`, configured command, or environment map with raw values.
- Bad: rewriting a session to normalize unknown records or to migrate v1/v2.

### 6. Tests Required

- `pi-store`: `paths_contract`, `settings_contract`, `models_contract`, `auth_contract`, `config_value_contract`, `trust_matrix`, `session_v3_contract`, and `typescript_interop`.
- `pi-resources`: `context_contract`, `skills_contract`, `system_prompt_contract`, and `trust_matrix`.
- Assertion points include unchanged config bytes, no lock creation, precedence/provenance, command limits/redaction, context-versus-protected-resource trust behavior, unknown session preservation, prior-line byte prefixes, legacy refusal, stale-writer refusal, and bidirectional TypeScript/Rust reads.
- Compatibility rows become `verified` only after their named runner passes.
- Required gates: locked format/Clippy/tests, `cargo deny check`, and `npm run check`.

### 7. Wrong vs Correct

Wrong:

```rust
if !trust.trusted {
    context_files.clear();
}
```

This changes current headless behavior and conflates instruction loading with sandboxing.

Correct:

```rust
let context = discover_context(&paths, no_context_files)?;
let project_skills = discover_skills(SkillDiscoveryRequest {
    project_trusted: trust.trusted,
    ..request
})?;
```

Keep context loading independent, and gate only protected project settings/skills. Do not describe trust as tool containment or a security sandbox.

## Scenario: Headless CLI Composition and Linux Artifact

### 1. Scope / Trigger

Use this scenario when changing `pi-cli`, the `pi-rs` process boundary, headless argument/output behavior, production model-service registration, signal cleanup, or the Linux x64 Actions artifact. The CLI must compose completed Rust layers without reimplementing provider, agent, tool, store, or resource semantics.

### 2. Signatures

- Parse: `parse_args(&[String]) -> Result<CliArgs, CliParseError>`.
- Mode: `resolve_output_mode(&CliArgs, stdin_is_terminal) -> Result<OutputMode, CliParseError>`.
- Library entry: `run_cli(CliRequest, &dyn ModelServiceFactory, OutputTargets, &RootCancellation) -> CliExit`.
- Production binary: `pi-rs`; Faux is injectable only through `ModelServiceFactory` in tests.
- Artifact command: `rust/scripts/package-pi-rs.sh x86_64-unknown-linux-gnu <new-output-directory> <commit>`.

### 3. Contracts

- Initialization order is parse/metadata, global bootstrap, session selection, authoritative session cwd, trust, settings/models/auth/resources, model/runtime, then ordered prompts. Startup-project settings or skills must not leak into a resumed session from another cwd.
- Production registers exactly `openai-completions`, `openai-responses`, `anthropic-messages`, and `google-generative-ai`. It has no Faux flag, provider, environment key, endpoint, or hidden binary route.
- `HOME` and `PI_CODING_AGENT_DIR` select store roots. Built-in credentials use `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY`; `--api-key` is the in-memory highest-precedence override.
- Text stdout is final assistant text only. JSON stdout starts with one v3 session header and then compact compatible events. Diagnostics and redacted failures use stderr.
- `--approve` gates protected project settings/skills; it is not a tool sandbox. `--offline` disables implicit startup networking; it does not block the explicitly selected provider call.
- The CI artifact job depends on the complete Rust check job and never changes the tag release workflow. It uploads `pi-rs-linux-x64-<commit>.tar.gz`, `SHA256SUMS`, and `build-info.json`.
- Build info schema 1 contains `sourceCommit`, `workspaceVersion`, GNU x64 `target`, `release` profile, exact `rustc`/`cargo` versions, and the `Cargo.lock` SHA-256 digest.

### 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Bare TTY or positional TTY invocation | Explicit interactive-mode guidance; do not silently select text |
| Deferred current option or `--mode rpc` | `Unsupported` diagnostic with milestone guidance |
| Unknown option | Distinct `Unknown` diagnostic |
| Invalid/conflicting value, missing input file, or unsupported tool | Input failure on stderr; exit 1 |
| Missing stored session cwd, legacy mutation, or stale writer | Actionable store error before a prompt |
| Terminal failed/cancelled run | Terminal JSON event where applicable, stderr failure, exit 1 |
| SIGHUP / SIGINT / SIGTERM | Cancel provider/tools, settle output/session work, exit 129 / 130 / 143 |
| Existing artifact output directory or non-GNU-x64 target | Packaging fails closed without deleting the directory |
| Artifact checksum/provenance mismatch | CI fails before upload |

### 5. Good / Base / Bad Cases

- Good: an extracted release binary runs text and JSON requests against a loopback custom provider from outside the checkout with no Node.js or Bun in `PATH`.
- Good: resuming a session loads resources and executes tools relative to its stored cwd, not the startup cwd.
- Base: `--help`, `--version`, and `--offline --list-models` require neither a writable session nor provider request.
- Bad: adding a production `--faux` switch to make subprocess tests deterministic.
- Bad: writing progress or Rust debug representations to stdout in text or JSON mode.
- Bad: adding `pi-rs` to the tag publication workflow before the separate release decision.

### 6. Tests Required

- Parser/mode: `args_contract`, `mode_contract`; assert every supported alias, conflict, deferred option, unknown option, and precedence fixture.
- Composition: `initialization_contract`, `input_contract`, `model_tool_contract`, `session_contract`, `output_contract`, and `signal_cleanup`.
- End to end: `headless_smoke` injects Faux; `protocol_smoke` covers all four network protocols on `127.0.0.1`; `binary_protocol_smoke` executes the production binary in text/JSON; `no_node_runtime` clears `PATH`.
- CI must format, run locked workspace Clippy/tests/Rustdoc/dependency audit, package release GNU x64, unpack outside the checkout, verify checksums/build info, and run the packaged binary protocol test.
- Required repository gate remains `npm run check`; no live provider, credential, npm release, tag workflow, or public GitHub Release is part of this scenario.

### 7. Wrong vs Correct

Wrong:

```rust
// A hidden production backdoor makes tests pass but changes the shipped surface.
if args.provider.as_deref() == Some("faux") {
    return run_with_faux(args).await;
}
```

Correct:

```rust
// Tests inject the service boundary; the production binary supplies only the
// four network protocol adapters.
let exit = run_cli(request, injected_factory, targets, &cancellation).await;
```

Keep presentation and process behavior in `pi-cli`. Reuse canonical model, runtime, store, resource, and tool boundaries instead of duplicating their policies at the command layer.
