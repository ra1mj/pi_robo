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
