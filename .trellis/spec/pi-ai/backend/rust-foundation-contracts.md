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
