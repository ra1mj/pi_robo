# Rust Rewrite Technical Design

## Status

Planning design for the parent rewrite task. This document fixes architecture and compatibility boundaries; it does not authorize implementation.

## Design Principles

1. Compatibility is measured at observable boundaries: CLI behavior, JSON events, provider requests/responses, tool contracts, files, and terminal workflows.
2. The TypeScript implementation remains the compatibility oracle and rollback path until the final gate.
3. A single Rust runtime serves headless, RPC, and TUI surfaces through serializable events.
4. Protocol families, not provider brands, are the unit of transport implementation.
5. Durable user data remains JSON/JSONL during migration. No compatibility-phase TOML or new session layout is introduced.
6. Executable integrations cross explicit process/network boundaries. The Rust process does not load arbitrary native or JavaScript modules.
7. The first milestone is independently useful and releasable as a CI artifact, but it cannot replace `pi`.

## Repository Layout

The root becomes a mixed npm/Cargo workspace without nesting Rust inside an npm package:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
deny.toml
rust/
  assets/
    models.json                 # generated, committed model catalog
  crates/
    pi-protocol/                # serializable DTOs and compatibility values
    pi-model/                   # model registry, auth resolution, transport traits
    pi-provider/                # HTTP/SSE provider protocol implementations
    pi-agent/                   # low-level agent/tool loop
    pi-tools/                   # read/bash/edit/write implementations
    pi-store/                   # settings, auth, trust, session JSON/JSONL
    pi-resources/               # context, skills, and system prompt assembly
    pi-runtime/                 # session host, retry, compaction, event orchestration
    pi-cli/                     # pi-rs library composition and binary
    pi-test-support/            # Faux, mock servers, fixture normalization; not shipped
  fixtures/
    cli/
    events/
    providers/
    sessions/
    tools/
```

Later milestones add `pi-rpc`, `pi-tui`, and `pi-integrations` without moving milestone-1 behavior out of its owning crates.

## Dependency Direction

| Crate | May depend on | Must not depend on |
| --- | --- | --- |
| `pi-protocol` | serialization/value crates | Tokio, HTTP, filesystem, terminal crates |
| `pi-model` | `pi-protocol` | concrete provider clients, CLI, TUI |
| `pi-provider` | `pi-protocol`, `pi-model` | agent, tools, persistence, CLI |
| `pi-agent` | `pi-protocol`, `pi-model` | concrete providers, filesystem tools, sessions, UI |
| `pi-tools` | `pi-protocol`, `pi-agent` | providers, sessions, CLI, UI |
| `pi-store` | `pi-protocol` | providers, agent, UI |
| `pi-resources` | `pi-protocol`, `pi-store` | providers, agent loop, UI |
| `pi-runtime` | protocol/model/agent/tools/store/resources abstractions | CLI formatting, terminal rendering |
| `pi-cli` | all milestone-1 production crates | test support in production builds |

`pi-test-support` implements the same model/tool/store interfaces as production code. Faux is injected through the `pi-cli` library entry in tests; there is no hidden production flag or public test endpoint.

## Core Contracts

### Serializable Values

`pi-protocol` owns the Rust equivalents of:

- model metadata, thinking levels, usage, costs, stop reasons, and compatibility fields;
- user, assistant, and tool-result messages;
- assistant stream events and agent/session events;
- tool specifications, calls, partial results, and final results;
- v3 session header and known entry views.

Persisted records are first parsed as `serde_json::Value`. Typed views validate fields needed by the runtime while retaining the original value for pass-through. Known persisted structs use flattened extra fields when they must be re-serialized. Milestone 1 appends new session entries but never rewrites pre-existing lines.

The public JSON writer serializes contract DTOs directly. Internal error enums and Rust-specific state never leak into JSON output.

### Async Interfaces

The following object-safe interfaces use boxed futures/streams rather than coupling base crates to a concrete provider:

```text
ModelService.stream(model, context, options, cancellation) -> ordered assistant event stream
Tool.execute(call_id, args, context, cancellation, update_sink) -> tool result
SessionStore.load/append/query -> validated session records
EventSink.emit(event) -> backpressured completion
```

Once `ModelService.stream` starts, request/runtime failures are encoded as a final assistant error or aborted message, matching the current stream contract. Configuration failures before a stream starts return a normal Rust error to the composition root.

### Cancellation and Backpressure

A root cancellation token owns child tokens for provider requests, retry sleeps, compaction, tool batches, and child processes. Cancelling the root:

1. stops accepting provider chunks;
2. cancels pending tool work and retry timers;
3. terminates tracked shell process groups;
4. emits the compatible aborted terminal events where a run already started;
5. flushes session and stdout writers before exit.

Every event passes through one ordered, bounded channel. Producers await capacity; events are never dropped to keep streaming fast. Parallel tools may finish in nondeterministic order, but tool-result messages are restored to assistant source order where the current contract requires it.

## Runtime Initialization and Data Flow

Initialization is intentionally two-phase because `--session` can select a session whose working directory differs from the startup directory.

```text
argv/env
  -> parse and reject unsupported options
  -> load global bootstrap settings and session directory
  -> resolve/create session and authoritative cwd
  -> resolve trust for that cwd
  -> load global + trusted project settings/resources
  -> load model catalog, models.json, and credential snapshot
  -> resolve model, thinking, tools, system prompt
  -> create shared runtime
  -> process each prompt sequentially
       -> provider stream
       -> agent events
       -> tool calls/results
       -> retry/compaction when required
       -> append session entries
       -> text or JSON output sink
  -> flush and exit
```

This ordering prevents startup-directory settings or resources from leaking into a resumed session from another project.

## Provider Design

### Shared Transport

`pi-provider` uses one reusable asynchronous HTTP client with:

- streamed response bodies and an incremental SSE decoder;
- explicit header/body idle timeout and overall cancellation;
- `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, and the compatible `httpProxy` setting;
- TLS certificate validation and platform root behavior verified by integration tests;
- decompression required by supported provider responses;
- bounded error-body capture with credential/header redaction;
- a test-injectable base URL and clock.

The implementation should use the current pinned releases of [Tokio](https://docs.rs/tokio/latest/tokio/), [tokio-util cancellation tokens](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html), and [reqwest](https://docs.rs/reqwest/latest/reqwest/). Only required Cargo features are enabled. The exact TLS feature set is fixed by the foundation task after proxy, platform-root, and custom-certificate contract tests pass.

### Protocol Adapters

Each adapter maps the common request and event contracts without owning model selection:

| Adapter | Milestone-1 responsibility |
| --- | --- |
| Faux | Deterministic scripted messages, deltas, usage, errors, cancellation, and cache estimates for tests |
| OpenAI Chat Completions | roles/content, images, thinking compatibility modes, streamed tool-call arguments, usage, finish reasons |
| OpenAI Responses | input/output item mapping, reasoning IDs/signatures, response IDs, tool calls, usage, error events |
| Anthropic Messages | content blocks, prompt caching fields, thinking/redaction signatures, tools, usage, stop/error mapping |
| Google Generative AI | contents/parts, inline images, function calls/results, thought signatures, safety/errors, usage |

Malformed or truncated provider tool-call JSON is retained as a failed tool call. If the assistant response hits the output limit, no apparently valid tool call from that truncated response is executed.

### Models and Credentials

A companion TypeScript serializer imports the current generated model catalog and emits deterministic `rust/assets/models.json` without fetching the network or hand-editing generated sources. Rust builds embed this committed artifact, and a check fails when it drifts from `packages/ai/src/models.generated.ts`.

`models.json` is parsed after stripping compatible comments. Provider/model definitions remain credential-blind until runtime resolution.

Milestone-1 credential resolution is:

1. in-memory CLI `--api-key` override;
2. `auth.json` record with `type: "api_key"`;
3. provider-specific environment variable;
4. `models.json` provider key.

The value resolver supports literal values, `$VAR`/`${VAR}` interpolation, escaped dollar/bang prefixes, provider-scoped `env`, and explicit `!command` execution. Command resolution uses the same unsandboxed process boundary as `bash` and is documented accordingly. OAuth values remain raw preserved JSON and never enter the milestone-1 resolver.

Only direct OpenAI, Anthropic, Google, and custom selected-protocol providers are advertised as milestone-1 compatible. A provider brand becomes supported only after request/response fixtures cover its headers, URL, authentication, and compatibility overrides.

## Agent and Runtime Design

### Low-Level Agent Loop

`pi-agent` owns one turn loop:

1. emit agent/turn/message lifecycle events;
2. transform compatible context and request the model stream;
3. accumulate text, thinking, and tool calls;
4. validate tool name and arguments;
5. execute the tool batch in parallel unless any selected tool requires sequential execution;
6. emit tool partial/final events and append results in assistant source order;
7. continue while tool calls or queued continuation require another model turn;
8. emit `agent_end` last and await event settlement.

Tool definitions expose hand-authored JSON schema values checked against TypeScript fixtures. Execution deserializes into typed Rust inputs after schema validation; compatibility shims such as stringified `edit.edits` are applied before validation.

### Session Runtime

`pi-runtime` wraps the low-level loop with persistence, automatic retry, and automatic compaction:

- agent-level transient retry defaults to 3 attempts at 2/4/8 seconds and emits current retry events;
- provider-level timeout/retry settings remain separate;
- context-overflow errors compact once and retry once;
- threshold compaction runs after a completed response and does not silently start another user turn;
- compaction appends a compatible v3 entry and preserves earlier history;
- manual compaction, branch navigation, branch summaries, steering queues, and follow-up queues are later features unless required internally for event compatibility.

Retry and compaction use injected clocks/sleep functions in tests. No test waits on production delays.

## Tool Design

### Shared Rules

- Resolve relative paths against the authoritative session cwd.
- Canonicalize existing paths; use normalized absolute paths for missing targets.
- Key the in-process mutation queue by canonical path so symlink aliases cannot race within one process.
- Enforce current UTF-8 byte and logical-line accounting.
- Keep full shell output in a secure temporary file only after truncation is required.
- Treat cancellation as an error result with compatible aborted state, not a panic.

### `read`

Read access is checked before loading. Text uses one-based offset/limit and head truncation. Images are detected from content, decoded/resized on a blocking worker, and returned as base64 content plus the current explanatory text. The planned image implementation uses the pinned [image crate](https://docs.rs/image/latest/image/) with only jpg/png/gif/webp/bmp features.

### `bash`

On Linux, shell resolution is explicit `settings.shellPath`, then `/bin/bash`, then `bash` on `PATH`, then `sh`. Commands run in a new process group with combined stdout/stderr capture, streaming updates, optional timeout, and process-tree termination on abort. `shellCommandPrefix` is applied once before execution.

### `edit`

The mutation queue is acquired before reading. The implementation detects BOM and line endings, matches all replacements against the original normalized text, rejects missing/duplicate/overlapping targets, applies non-overlapping edits, restores line endings/BOM, and writes while still holding the queue. It returns both display diff and unified patch.

### `write`

The mutation queue is acquired before creating parents and replacing the file. UTF-8 content is written completely before the final event. Atomic replacement is used only if fixtures prove it does not change current permissions/symlink semantics.

## Persistence and Resource Design

### Sessions

- Session IDs use UUIDv7; entry IDs retain the current short collision-checked form.
- The header is validated before any append. Only v3 is accepted for milestone-1 mutation.
- Existing JSONL lines are retained verbatim in memory and on disk. New known records are appended with one JSON object and newline per write.
- Unknown entries are ignored when building model context but remain available to later runtimes.
- A single `pi-rs` process serializes its appends. No `.lock` claim is made for sessions, and simultaneous TypeScript/Rust writers are explicitly rejected operationally.

### Settings, Models, Auth, and Trust

- Global settings load first. Trusted project settings deep-merge over them, including one-level nested object merge compatible with the current implementation.
- Milestone 1 reads but does not write `settings.json`, `models.json`, `auth.json`, or `trust.json`.
- Later writers implement the current `proper-lockfile` directory protocol: atomically create `<file>.lock`, maintain mtime before staleness, retry with compatible limits, detect compromised ownership, and remove on release.
- Later `auth.json` writes create parent mode `0700`, file mode `0600`, preserve unknown/OAuth records, and use write-under-lock.
- Trust paths are canonicalized. The closest saved ancestor decision wins.

### Context and Skills

Context discovery loads global context first, then ancestor context from filesystem root to cwd, choosing the first supported filename per directory. Context is loaded even for untrusted projects unless disabled.

Skills load from explicit CLI paths, settings, global locations, and trusted project locations. Discovery, frontmatter limits, disable-model-invocation behavior, ignore files, collision order, and diagnostics are fixture-tested. Package-managed skill discovery is deferred.

The system prompt is produced from a normalized fixture covering tool snippets, guidelines, appended prompt text, context XML, skill XML, documentation paths, and cwd. Exact dynamic absolute paths are normalized during comparison.

## CLI and Output Design

The CLI uses a pinned [clap](https://docs.rs/clap/latest/clap/) builder/derive definition but maps parse failures to pi-compatible text and exit code 1 rather than exposing clap's default exit behavior unchecked.

The composition root owns stdout/stderr:

- text sink: final assistant text blocks only;
- JSON sink: session header first, then one compact JSON object per event;
- diagnostic sink: stderr only, with secrets redacted;
- signal handler: cancel, drain writers, terminate children, return 129/130/143 as applicable.

If stdin is not a TTY, it becomes prompt content and selects text mode unless JSON mode was explicit. Bare TTY startup or a positional prompt that would select the current interactive mode fails with guidance to use `-p` or `--mode text`.

## Compatibility Test Architecture

### Fixture Sources

1. Pure JSON fixtures generated from exported TypeScript types and current documented examples.
2. TypeScript contract runners that execute current tools/session/resource logic in temporary directories.
3. Local HTTP recordings authored from provider tests and validated against each adapter.
4. Rust-only property tests for parser boundaries, truncation, event ordering, and cancellation.

Fixtures contain no credentials or live response bodies from private user sessions. Nondeterministic values are normalized by field-aware code, never by deleting entire event bodies.

### Required Layers

| Layer | Validation |
| --- | --- |
| Protocol | serde round trips, unknown-field preservation, JSON schema snapshots |
| Provider | local server asserts request and streams success/error/malformed/cancel responses |
| Agent | Faux scripts assert exact normalized event traces and tool ordering |
| Tools | TypeScript/Rust runners operate on cloned temporary trees and compare results/diffs/files |
| Store/resources | bidirectional fixture reads, append checks, trust matrix, permissions/locking tests |
| CLI | subprocess stdout/stderr/exit/signal snapshots |
| Milestone smoke | release-mode Linux binary runs help/version/list-models and Faux-injected library integration |

No CI test accesses a paid or public provider. A live-provider smoke test occurs only at the final release gate with explicit user authorization.

## CI and Dependency Policy

Milestone 1 adds a separate Rust CI job so TypeScript-only feedback remains independently diagnosable:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
cargo build --release --locked -p pi-cli
pi-rs smoke tests
upload pi-rs-linux-x64 artifact
```

The Rust toolchain is pinned to an exact stable release in `rust-toolchain.toml`; current planning baseline is Rust 1.97.0. Direct dependency versions are exact, `Cargo.lock` is committed, default features are disabled where practical, and new build scripts/native dependencies receive explicit review. `cargo deny` enforces allowed licenses, known advisories, registry/git sources, and duplicate-version policy.

The reviewed foundation candidates are Serde for typed JSON ([Serde](https://docs.rs/serde/latest/serde/), [serde_json](https://docs.rs/serde_json/latest/serde_json/)), Tokio for async process/network/signal work, reqwest for HTTP streaming/proxies/TLS, clap for parsing, `uuid` with v7 support for session IDs, and the image crate for image processing. Later TUI work evaluates [Ratatui's Crossterm backend and test backend](https://docs.rs/ratatui/latest/ratatui/backend/index.html) without adding it to milestone-1 binaries.

## Migration, Packaging, and Rollback

### Milestone 1

- CI artifact name: `pi-rs-linux-x64`.
- No npm package, release asset name, shell shim, or existing script is replaced.
- Users opt in by invoking `pi-rs` explicitly.
- Rollback is immediate: stop using `pi-rs` and resume with TypeScript `pi` against the compatible session.

### Later Milestones

- Side-by-side Rust release assets are added only after platform-specific smoke tests.
- Existing npm SDK packages remain available during migration. Converting the CLI npm package to a native installer/launcher or retiring programmatic exports requires a separate reviewed packaging change.
- Rust owns the `pi` name only after every final gate passes. The former TypeScript CLI remains available for at least one rollback release under an explicit legacy name or documented installation path.

Rollback never relies on downgrading or rewriting user files. If a later schema becomes necessary, it must be additive, versioned, and preceded by backup/restore tests.

## Security and Licensing

- Project trust is documented as resource selection only, not containment.
- API keys and authorization headers are redacted from errors, traces, fixtures, and panic output.
- `auth.json` is never included in crash reports or diagnostics.
- Tool and credential commands execute with user permissions; no partial sandbox claim is made.
- Provider URLs from `models.json` are user-controlled and allowed by design, including localhost.
- Direct grok-build code reuse is prohibited by default. Any exception records source file, commit, modification, license, and notice impact before code lands.
- Cargo dependency licenses and sources must pass policy before merge.

## Important Trade-offs

- A larger milestone 1 is accepted to include images, automatic retry, and automatic compaction because otherwise the promised tool and JSON contracts would be materially partial.
- Exact TUI rendering is rejected in favor of behavioral compatibility so Rust-native terminal architecture remains possible.
- Session coexistence is lossless but not concurrently writable; inventing a Rust-only lock would not protect a TypeScript writer.
- A committed generated model catalog duplicates serialized data in the repository, but avoids network-dependent builds and prevents TypeScript/Rust catalog drift.
- Protocol adapters are implemented independently rather than wrapping vendor SDKs, increasing initial work but enabling one consistent event/error/cancellation contract and a self-contained native binary.
