# Rust Provider Protocol Adapters Design

## Status and Dependency Gate

Planning was reviewed and accepted on 2026-07-21, and authorization to modify code was explicitly reconfirmed on 2026-07-22. `../archive/2026-07/07-20-rust-foundation-contracts` is verified `completed`. This child remains in `planning` only because the current workflow is planning-only; once implementation is enabled, it may start after `trellis-before-dev` loads the implementation context without another approval round.

## Evidence and Compatibility Oracle

The canonical Rust DTOs and `ModelService` contract come from the foundation child. Observable protocol behavior is captured from:

- `packages/ai/src/types.ts`;
- `packages/ai/src/api/openai-completions.ts`;
- `packages/ai/src/api/openai-responses.ts` and `openai-responses-shared.ts`;
- `packages/ai/src/api/anthropic-messages.ts`;
- `packages/ai/src/api/google-generative-ai.ts` and `google-shared.ts`;
- `packages/ai/src/providers/faux.ts`;
- existing focused provider tests under `packages/ai/test/`.

These files are behavioral oracles, not code-copy sources. No provider SDK or grok-build implementation is reused.

## Boundary

`pi-provider` owns HTTP/SSE transport and wire-protocol mapping. It receives a resolved model, endpoint, headers, API key, request options, canonical context, and cancellation handle. It does not choose a provider/model, search credentials, read user files, run tools, retry an agent turn, persist sessions, or format CLI output.

Faux is implemented as a scripted `ModelService` in `pi-test-support`, injected by tests. It is not registered as a hidden production provider or public CLI flag.

## Shared Transport

One injected asynchronous HTTP client provides:

- streamed request/response bodies with bounded buffers;
- incremental SSE parsing across arbitrary byte/chunk boundaries;
- compatible proxy environment/settings handling;
- reviewed platform-root TLS behavior and certificate validation;
- response decompression required by selected providers;
- explicit connect/header/body-idle timeouts;
- root cancellation propagation;
- bounded error-body capture and credential/header redaction;
- test-only base URL, clock, and sleeper injection.

The SSE decoder handles CRLF/LF, comments/keepalives, multiple `data:` lines, blank-event delimiters, UTF-8 splits, trailing partial events, protocol sentinels, and malformed JSON. It never interprets provider semantics itself.

No adapter retries a request or started stream in milestone 1. Automatic client retries are disabled. Adapters normalize HTTP status, retry headers, and provider error payloads into retry metadata consumed by `pi-runtime`, which alone owns retry budgets and scheduling.

## Adapter Interface and State Machine

Each adapter has three stages:

1. Validate canonical input and build a provider request.
2. Decode ordered wire events into canonical assistant events while accumulating the final assistant message.
3. Emit exactly one terminal outcome: complete, length-limited, error, or aborted.

Once a stream begins, failures use the typed terminal failure path defined below; the consumer maps that path one-to-one into the canonical assistant `error` event. Configuration failures before sending return a normal typed error. Unknown events are accepted only when the protocol documents them as ignorable metadata; unknown content-bearing or terminal events fail explicitly with redacted context.

Tool-call JSON is accumulated incrementally. Malformed or truncated arguments remain a failed tool call and are never converted into an apparently executable call. Output-limit responses cannot expose a truncated tool call as valid.

## Foundation Interface Refinements

The completed foundation deliberately exposed only the smallest object-safe boundary. Before transport work, this child extends that boundary without adding provider behavior to `pi-model`. These are direct refinements of the unpublished Rust contract; no backward-compatibility layer is retained.

### Request Semantics

The common TypeScript surface is defined in `packages/ai/src/types.ts:113` and its simple-mode projection in `packages/ai/src/api/simple-options.ts:21`; protocol-specific option extensions remain in their owning adapter modules.

`ModelRequest` gains a `ModelRequestOptions` field with only protocol-neutral milestone-1 semantics:

| Field | Rust shape | Rule |
| --- | --- | --- |
| `temperature` | `Option<f64>` | Omit when absent; reject non-finite values before dispatch |
| `max_tokens` | `Option<u64>` | Adapter uses the model maximum when absent; later runtime context clamping remains outside the adapter |
| `reasoning` | `Option<ThinkingLevel>` | Closed `minimal/low/medium/high/xhigh/max` enum; absent means no explicit thinking request |
| `thinking_budgets` | `Option<ThinkingBudgets>` with `minimal/low/medium/high` fields | `xhigh` and `max` use the selected adapter's high/dynamic mapping rather than inventing extra token-budget fields |
| `cache_retention` | `CacheRetention::{None, Short, Long}` | Defaults to `Short`; adapters downgrade only when model compatibility says long retention is unsupported |
| `session_id` | `Option<String>` | Used only by compatibility rules that explicitly enable cache/session affinity |
| `tool_choice` | optional `Auto/None/Required/Named(String)` enum | Maps `Required` to Anthropic/Google `any`; absent preserves the provider default |

Authentication, base URL, headers, proxy, and connect/header/body-idle timeouts belong to immutable `ProviderAdapterConfig`. Provider environment lookup and credential resolution occur before construction. Retry scheduling and retry-delay caps belong to `pi-runtime`; adapters only classify failures and report the provider's bounded retry hint. Payload/response callbacks, WebSocket transports, injected vendor SDK clients, service tiers, reasoning-summary controls, and provider-specific display/beta toggles are published npm SDK surface rather than milestone-1 CLI requirements and are not added to the canonical Rust request.

### Wakeable Cancellation

`Cancellation` keeps `is_cancelled()` for the pre-dispatch fast path and adds an object-safe `cancelled(&self) -> CancellationFuture<'_>`. The future resolves immediately when already cancelled and wakes every registered waiter exactly once when cancellation occurs. Connect, response-header, body-read, and timeout waits race this future; no network operation relies on polling. `pi-model` remains executor-neutral, while `pi-test-support` proves pending-future wakeup without wall-clock sleeps.

### Typed Failures and Stream Termination

The current TypeScript runtime infers retryability from display strings (`packages/ai/src/utils/retry.ts:7`); Rust replaces that compatibility fallback with structured classification at the adapter boundary.

`ModelServiceError.category` becomes the closed enum `Configuration`, `Authentication`, `Permission`, `InvalidRequest`, `NotFound`, `ContextOverflow`, `RateLimit`, `QuotaExceeded`, `Timeout`, `Network`, `Unavailable`, `Server`, `Protocol`, `Cancelled`, or `Unknown`. The error also carries redacted `message`, authoritative `retryable`, and optional `http_status`, `provider_code`, and `retry_after_ms` fields. `QuotaExceeded` is non-retryable even when the HTTP status is 429; timeout, network, transient unavailable/server, and ordinary rate-limit failures may be retryable. Cancellation maps to canonical stop reason `aborted`; context overflow stays distinct so the later runtime can compact before retrying.

`ModelFuture` returns outer errors only before a stream is established. An established stream emits `start`, zero or more ordered partial events, then exactly one of `done` or a terminal `Err(ModelServiceError)` item. The consumer retains the latest partial assistant message and maps terminal `Err` exactly once to the public `AssistantMessageEvent::Error`; an adapter never emits both an error event and an error item. EOF without a terminal item is a retry-classified `Protocol` failure. This preserves structured internal retry data without serializing Rust-only state into JSON output.

### Provider-Blocking DTO Parity

Before adapter fixtures, `pi-protocol` adds the TypeScript fields already required by the selected providers (`packages/ai/src/types.ts:394`, `packages/ai/src/types.ts:395`, `packages/ai/src/types.ts:408`, `packages/ai/src/types.ts:414`, and `packages/ai/src/types.ts:474`): `AssistantMessage.response_model`, `AssistantMessage.response_id`, `ToolResultMessage.details`, and `ToolResultMessage.added_tool_names`. `ToolCallEnd.tool_call` becomes `ToolCallBlock` instead of raw JSON. Successful terminal reasons become a closed `Stop/Length/ToolUse` enum and failure reasons a closed `Error/Aborted` enum. Round-trip fixtures prove the JSON spellings remain unchanged and unknown persisted fields remain lossless.

## Dependency Decision

The first implementation pass uses these exact direct dependencies after lockfile and license review:

| Crate | Version/features | Purpose and review |
| --- | --- | --- |
| `reqwest` | `=0.13.4`, default features off; `rustls`, `http2`, `system-proxy`, `stream`, `json`, `gzip`, `brotli`, `deflate` | Shared streaming HTTP client, decompression, explicit/system proxy support, platform-root verification; excludes native-tls/OpenSSL |
| `tokio` | `=1.53.1`; `macros`, `rt-multi-thread`, `sync`, `time`, `net`, `io-util` | Cancellation-aware network/test runtime and timeout orchestration |
| `futures-util` | `=0.3.33`; default features | Stream combinators without another async abstraction layer |
| `bytes` | `=1.12.1` | Incremental bounded body/SSE buffers |
| `base64` | `=0.22.1` | Provider image encoding |
| `httpdate` | `=1.0.3` | Standards-compatible `Retry-After` date parsing |

`reqwest`'s reviewed Rustls path currently selects AWS-LC through Rustls and therefore introduces native C/assembly build code even though it avoids OpenSSL. The implementation gate must record this in `rust/DEPENDENCIES.md`, review the complete lockfile/license graph, and document any unavoidable `cargo-deny` duplicate-version exception with an owning dependency path. No `async-trait`, SSE parser crate, provider SDK, secret crate, or general retry crate is added: object-safe boxed futures, the shared incremental parser, a small redacting secret wrapper, and runtime-owned retry scheduling cover those needs.

## Protocol Responsibilities

### OpenAI Chat Completions

- Map system/developer/user/assistant/tool roles and compatible content arrays.
- Encode text, image URLs/data, tool definitions, tool choice, thinking/reasoning compatibility fields, and prompt-cache fields required by selected direct/custom models.
- Assemble streamed text, reasoning details, tool-call IDs/names/arguments, finish reasons, response model/ID, and usage.
- Normalize missing/foreign tool-call IDs without hiding malformed argument data.

### OpenAI Responses

- Map canonical history into Responses input/output items while preserving response/message/reasoning identifiers needed for replay.
- Encode images, function tools/results, reasoning effort, and supported compatibility options.
- Decode output-item lifecycle, text/reasoning deltas, function-call argument deltas, usage, completion, incomplete, and error events.
- Preserve terminal-event and empty-tool-result behavior defined by focused TypeScript tests.

### Anthropic Messages

- Separate system content and map message content blocks, images, tools/results, prompt-cache controls, and thinking options.
- Decode text, thinking, redacted thinking, signatures, eager/partial tool input, usage/cache fields, stop reasons, and errors.
- Preserve provider-compatible tool-name normalization while retaining the canonical requested name/ID semantics.

### Google Generative Language

- Map roles to `contents`/`parts`, including text, inline images, function calls/results, tools, system instruction, and thinking options.
- Preserve thought signatures and routing metadata required for later turns.
- Decode candidate parts, finish/safety states, function calls, usage, blocked responses, and errors.
- Reject unsupported unsigned tool-call situations according to the captured compatibility rules.

### Faux

- Script ordered deltas, complete messages, usage/cache estimates, errors, delays, and cancellation.
- Operate entirely in memory with deterministic IDs and fake time.
- Implement the same canonical interface without any HTTP or credential path.

## Request Configuration and Security

Resolved configuration is immutable for one request. Adapters may add protocol-required headers but cannot inspect global environment variables directly. Base URLs from user `models.json` are permitted by product design, including localhost.

Authorization values, API keys, cookies, signed URLs, and sensitive provider payload fields are redacted from errors, traces, snapshots, and panic messages. Fixture request assertions use synthetic tokens. Error-body capture is bounded before formatting.

## Fixture Architecture

Each real protocol uses a local server that records the request and emits scripted response bytes. Fixtures separately cover:

- minimal and multi-block success;
- thinking/reasoning and identifiers/signatures;
- one and multiple tool calls, partial JSON, malformed/truncated JSON, and tool results;
- image input and image-bearing tool results where supported;
- usage/cache/cost inputs and stop reasons;
- HTTP authentication, rate-limit, timeout, retry-hint, and server errors;
- malformed SSE/JSON, unknown significant events, abrupt EOF, and cancellation;
- chunk splits at every important parser boundary.

Fixtures are synthetic, credential-free, and normalized only for approved IDs/timestamps/headers. No CI test contacts a public endpoint.

## Task Decomposition Decision

Keep the provider protocols in one child task. The four network adapters share the same interface refinement, HTTP client, SSE parser, error model, cancellation behavior, and final cross-protocol conformance matrix, so nested task boundaries would split one coupled compatibility surface. Implementation Steps 1 through 9 remain explicit, independently verified rollback gates; no later adapter starts before the preceding shared gate passes, and no partial adapter set is advertised as complete.

## Important Trade-offs

- Direct protocol adapters require more conformance work than vendor SDKs but keep a single native event/error/cancellation contract.
- Protocol-family support does not automatically advertise every brand using that protocol; brand headers, URLs, auth, and overrides need their own later evidence.
- Unknown metadata can be ignored for forward compatibility, but unknown content or terminal state fails to prevent silent loss.
- Retry classification is owned here; retry scheduling remains in the runtime child to prevent layered retry multiplication.

## Rollback

This child only adds `pi-provider`, test-support extensions, and fixtures. The crate is not reachable from the existing TypeScript `pi` or a production Rust CLI. A failing adapter can remain unregistered later without changing model files or user data.

## Decisions Closed for Start Review

- Five milestone-1 implementations are four network protocols plus injected Faux.
- One shared transport and SSE decoder serves the network adapters.
- Provider selection and credential resolution remain outside adapters.
- Started streams have exactly one canonical terminal outcome.
- Automated validation is local-only and secret-free.
- Direct `openai`, `anthropic`, `google`, and explicitly configured custom endpoints are the only milestone-1 claims.
- The canonical request contains seven protocol-neutral option groups; SDK-only callbacks, transports, and provider-specific controls are excluded.
- Provider errors use closed categories and structured retry metadata; display-string regex matching is not the Rust retry contract.
- Provider-blocking response IDs/models and terminal/tool-call DTOs are typed before transport implementation.
