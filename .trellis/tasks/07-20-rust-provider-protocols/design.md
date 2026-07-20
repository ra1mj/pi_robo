# Rust Provider Protocol Adapters Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-foundation-contracts` must be `complete` before this child can start. The dependency must be checked from task state; directory position is not sufficient.

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

No adapter silently retries a started stream. Adapters normalize HTTP status, retry headers, and provider error payloads into retry metadata consumed by `pi-runtime`. Any safe pre-stream transport retry must be explicitly specified and fixture-tested rather than inherited from client defaults.

## Adapter Interface and State Machine

Each adapter has three stages:

1. Validate canonical input and build a provider request.
2. Decode ordered wire events into canonical assistant events while accumulating the final assistant message.
3. Emit exactly one terminal outcome: complete, length-limited, error, or aborted.

Once a stream begins, failures are represented through the canonical terminal assistant/error sequence. Configuration failures before sending return a normal typed error. Unknown events are accepted only when the protocol documents them as ignorable metadata; unknown content-bearing or terminal events fail explicitly with redacted context.

Tool-call JSON is accumulated incrementally. Malformed or truncated arguments remain a failed tool call and are never converted into an apparently executable call. Output-limit responses cannot expose a truncated tool call as valid.

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
