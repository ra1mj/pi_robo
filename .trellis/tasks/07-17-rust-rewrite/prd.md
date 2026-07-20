# Rewrite pi in Rust

## Goal

Deliver a standalone native Rust implementation of `pi` through independently verifiable milestones. Preserve the selected CLI, data, provider, tool, JSON API, RPC, and interactive behaviors without porting the published npm SDK APIs or embedding a JavaScript runtime.

Milestone 1 delivers a side-by-side Linux x64 headless executable named `pi-rs`. It must execute real prompts in text or JSON mode, run the four default tools, and interoperate with current pi data. The existing TypeScript `pi` remains the default until the final replacement gate passes.

## Background and Evidence

- The repository contains agent, AI provider, coding-agent, orchestrator, and TUI packages plus extension examples. Agent, AI, coding-agent, and TUI expose published programmatic entry points (`packages/agent/package.json:6`, `packages/ai/package.json:6`, `packages/coding-agent/package.json:12`, `packages/tui/package.json:6`).
- The coding-agent SDK exports session, tool, TUI, RPC, and extension APIs (`packages/coding-agent/src/index.ts:1`). Existing extensions are executable TypeScript/JavaScript modules loaded in-process through `jiti` (`packages/coding-agent/src/core/extensions/loader.ts:1`, `packages/coding-agent/src/core/extensions/loader.ts:390`). A pure Rust process cannot preserve that surface without retaining a JavaScript host.
- Skills, prompt templates, themes, and context files are declarative Markdown/JSON resources and can remain file-compatible (`packages/coding-agent/docs/skills.md:24`, `packages/coding-agent/docs/prompt-templates.md:7`, `packages/coding-agent/docs/themes.md:5`).
- The current product intentionally has no built-in sandbox. Project trust controls resource loading, while tools run with the process user's permissions (`packages/coding-agent/docs/security.md:3`, `packages/coding-agent/docs/security.md:33`).
- The CLI currently exposes interactive, text, JSON, and RPC workflows plus broad session, model, tool, resource, trust, and package options (`packages/coding-agent/src/cli/args.ts:10`, `packages/coding-agent/src/cli/args.ts:12`).
- JSON mode emits a v3 session header followed by JSONL agent/session events (`packages/coding-agent/docs/json.md:1`, `packages/coding-agent/src/modes/print-mode.ts:1`).
- Sessions are append-only JSONL trees at schema version 3 (`packages/coding-agent/src/core/session-manager.ts:30`). Parsing is permissive and extension payloads are open-ended (`packages/coding-agent/src/core/session-manager.ts:46`, `packages/coding-agent/src/core/session-manager.ts:90`, `packages/coding-agent/src/core/session-manager.ts:294`). Session writes append directly and do not use a cross-process lock (`packages/coding-agent/src/core/session-manager.ts:952`).
- Stable user data lives under `~/.pi/agent`, including `settings.json`, `models.json`, `auth.json`, and per-project session JSONL files (`packages/coding-agent/src/config.ts:511`). Settings and authentication writes use `proper-lockfile`; `auth.json` is mode `0600` (`packages/coding-agent/src/core/settings-manager.ts:188`, `packages/coding-agent/src/core/auth-storage.ts:21`).
- The AI package registers 36 built-in provider factories (`packages/ai/src/providers/all.ts:78`) over ten known wire-protocol families (`packages/ai/src/types.ts:16`). The model generator can already emit a language-neutral JSON catalog (`packages/ai/scripts/generate-models.ts:2363`).
- Credential resolution order is CLI override, `auth.json`, provider environment variables, then `models.json` (`packages/coding-agent/docs/providers.md:287`). API-key values may contain environment interpolation or command resolution (`packages/coding-agent/docs/providers.md:140`).
- The four default tools are `read`, `bash`, `edit`, and `write`; `grep`, `find`, and `ls` are opt-in (`packages/coding-agent/src/core/tools/index.ts:138`, `packages/coding-agent/README.md:581`). Tool output is limited to 2,000 lines or 50 KiB (`packages/coding-agent/src/core/tools/truncate.ts:1`).
- The current interactive surface uses configurable named actions from `~/.pi/agent/keybindings.json` and supports session navigation, model selection, queues, slash commands, and tool/thinking expansion (`packages/coding-agent/docs/keybindings.md:3`).
- Current binary releases cover macOS arm64/x64, Linux arm64/x64, and Windows arm64/x64 (`.github/workflows/build-binaries.yml:70`).
- The orchestrator is experimental and has no stable compatibility promise.
- The architectural reference is `xai-org/grok-build` at reviewed commit `8adf9013a0929e5c7f1d4e849492d2387837a28d`. Applicable patterns are a Rust workspace, a small composition root, a shared serializable runtime protocol, separate runtime/tool/config/TUI crates, and executable integrations across process or network boundaries. Its TOML configuration, ACP session layout, xAI-specific behavior, and plugin format are not pi compatibility contracts.
- grok-build is Apache-2.0. Architectural study is allowed; copied or adapted code requires explicit attribution and notice review.

## Scope

| Existing area | Rust target | Delivery |
| --- | --- | --- |
| `packages/ai` provider behavior | Rust model types, protocol adapters, streaming, authentication, usage, and catalog loading | Five milestone-1 protocols; remaining protocol/auth families before replacement |
| `packages/agent` | Rust agent loop, tool execution, events, cancellation, retry, and compaction | Core runtime in milestone 1 |
| `packages/coding-agent` CLI/runtime | Rust composition root, configuration, sessions, resources, tools, text/JSON modes | Core headless subset in milestone 1; remaining behavior later |
| `packages/tui` | Rust-native terminal UI with behavioral compatibility | Later milestone |
| JSON RPC mode | Rust client/server protocol using the shared runtime event types | Later milestone before replacement |
| Declarative skills, prompts, themes, and context | Preserve compatible formats and discovery/trust rules | Context and skills in milestone 1; prompt templates and themes with TUI |
| Executable extensions | Replace in-process TypeScript execution with command, MCP, HTTP, LSP, or equivalent explicit protocols | Later milestone; no native dynamic plugin ABI |
| npm programmatic SDKs | Not ported and not a Rust compatibility target | TypeScript packages remain available during migration; retirement is a separate approval |
| `packages/orchestrator` | Not part of the Rust replacement gate | Retained as an experimental TypeScript package |
| Existing TypeScript `pi` | Rollback implementation and compatibility oracle | Retained until the final replacement gate |

## Requirements

### R1. Architecture and Boundaries

- Implement a Cargo workspace with a small `pi-rs` composition root and separate crates for protocol DTOs, provider transports, agent runtime, tools, persistence/configuration, resource loading, and later RPC/TUI integrations.
- Keep one agent runtime and one serializable event contract for text, JSON, RPC, and TUI consumers.
- Use provider wire-protocol families rather than provider-specific duplicate transports.
- Generate or consume a language-neutral model catalog from the existing generator; do not manually fork `packages/ai/src/models.generated.ts`.
- Do not embed Node.js, Bun, a JavaScript VM, or a legacy extension IPC bridge in the Rust process.
- Pin the Rust toolchain and direct Cargo dependencies, commit `Cargo.lock`, and add formatting, lint, test, license, advisory, and source-policy checks.

### R2. Milestone-1 CLI and Output Contract

- Ship a side-by-side executable named `pi-rs`; do not change the existing `pi` entry point.
- Support Linux x64 only in milestone 1.
- Support positional prompts, piped stdin, `@text` files, and `@image` attachments.
- Support this explicit CLI subset: `--print/-p`, `--mode text|json`, `--provider`, `--model`, `--api-key`, `--system-prompt`, repeatable `--append-system-prompt`, `--thinking`, `--continue/-c`, `--session`, `--session-id`, `--session-dir`, `--no-session`, `--name/-n`, `--tools/-t`, `--exclude-tools/-xt`, `--no-tools/-nt`, `--skill`, `--no-skills/-ns`, `--no-context-files/-nc`, `--approve/-a`, `--no-approve/-na`, `--list-models`, `--offline`, `--help/-h`, and `--version/-v`.
- Reject bare interactive startup, `--mode rpc`, recognized deferred flags, and unknown flags with an explicit unsupported-option error. Never silently ignore them.
- Text mode writes only the final assistant text to stdout. JSON mode writes only compatible JSONL records to stdout. Diagnostics go to stderr.
- Preserve current success/error semantics: success exits 0; input, configuration, provider, and terminal assistant failures exit 1; termination performs cleanup and returns conventional signal-derived status.
- `--offline` disables startup catalog/update/telemetry traffic but does not block the explicitly requested provider call.

### R3. Milestone-1 Provider and Agent Runtime

- Implement Faux test support plus OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, and Google Generative AI wire protocols.
- Officially validate the direct `openai`, `anthropic`, and `google` providers and custom `models.json` providers using those protocols. Other brands sharing a protocol are not claimed compatible until their fixtures pass.
- Preserve streaming text, thinking, tool-call deltas, stop reasons, usage/cost fields, provider errors, response identifiers/signatures, and abort behavior required by the existing message/event contract (`packages/ai/src/types.ts:321`, `packages/ai/src/types.ts:464`).
- Execute complete multi-turn agent loops until the model stops, including multiple and parallel tool calls. Serialize mutations targeting the same file within the process.
- Implement current automatic transient-error retry and threshold/overflow compaction using compatible settings and JSON events. Defer manual compaction and branch summarization commands.
- Use deterministic Faux and local mock HTTP servers for automated tests; no CI test may require credentials, paid tokens, or a public provider endpoint.

### R4. Milestone-1 Tools and Images

- Implement only `read`, `bash`, `edit`, and `write` with compatible names, JSON schemas, path resolution, validation, results, error flags, partial updates, cancellation, and 2,000-line/50-KiB truncation behavior.
- `bash` must stream combined stdout/stderr, support optional timeouts, preserve truncated full output in a temporary file, and terminate the child process tree on cancellation.
- `edit` must preserve BOM and line-ending behavior, require unique non-overlapping replacements, return display and unified diffs, and avoid stale concurrent writes.
- `write` creates parent directories and writes UTF-8 content.
- Support jpg, png, gif, webp, and bmp detection; compatible image resizing; provider-specific image request encoding; `images.blockImages`; and an explicit omission note for non-vision models.

### R5. Milestone-1 Data, Resources, and Trust

- Read global and trusted project `settings.json` with current deep-merge/default behavior. Read JSON-with-comments `models.json` and its current supported schema.
- Resolve API keys in this order: CLI override, compatible `auth.json` API-key record, provider environment, then `models.json`. Preserve current environment interpolation and explicit command-based value resolution.
- Treat OAuth records as unsupported but valid: do not use, refresh, delete, migrate, or rewrite them in milestone 1.
- Read and append v3 session JSONL without rewriting existing entries. Preserve unknown fields and unknown extension entries. Legacy v1/v2 migration is deferred.
- Coexistence means TypeScript and Rust can read sessions produced by the other and the user can roll back. Concurrent TypeScript/Rust writes to the same session file are unsupported because the current format has no shared write lock.
- Load global and ancestor `AGENTS.md`/`CLAUDE.md` context according to current ordering. Load configured/global/trusted-project skills with compatible metadata and discovery behavior. Defer prompt-template commands, themes, package-managed resources, and executable extensions.
- Preserve current non-interactive project-trust behavior: context files load unless disabled; protected project settings/skills load only after a saved decision, `defaultProjectTrust: "always"`, or `--approve`; `ask` cannot prompt in headless mode and therefore skips protected resources.
- Preserve the current security model: trust gates input loading but is not a sandbox; tools run with the Rust process user's permissions.

### R6. JSON and Persistence Compatibility

- JSON mode must emit the current v3 session header and preserve existing event type names, required fields, field meanings, and lifecycle ordering.
- IDs, timestamps, JSON key order, provider chunk boundaries, and independently completing parallel-tool event order need not be byte-identical.
- Settings/auth write support added in later milestones must interoperate with the current `.lock` directory/mtime protocol and preserve mode `0600` for `auth.json`.
- Compatibility is defined by normalized contract fixtures and cross-runtime interoperability, not by implementation structure or byte-for-byte output.

### R7. Later Milestones and Replacement Gate

- Add the remaining CLI, provider protocol/auth, OAuth, resource/package, manual compaction, session-tree, RPC, and TUI behaviors through separate verifiable milestones.
- Preserve TUI commands, configurable named keybindings, session/model workflows, message queues, cancellation, and tool/thinking expansion. Exact cells, colors, borders, and screenshots are not compatibility requirements.
- Add macOS arm64/x64, Linux arm64, and Windows arm64/x64 only after the Linux x64 headless contract is stable.
- Keep the TypeScript implementation and rollback instructions until every replacement criterion passes.
- The Rust executable may take the `pi` name only after headless, RPC, TUI, provider/auth, data migration, extension-boundary, cross-platform, packaging, documentation, and smoke-test gates pass.

## Acceptance Criteria

- [ ] AC1: The subsystem scope table is reflected in `design.md`; no excluded npm SDK or orchestrator compatibility is implemented implicitly.
- [ ] AC2: The Cargo workspace builds with the pinned toolchain and passes formatting, lint, unit/integration tests, dependency policy, and documentation checks.
- [ ] AC3: `pi-rs` executes prompts to completion in text and JSON modes on Linux x64 without changing the existing `pi` executable.
- [ ] AC4: CLI tests cover every milestone-1 option, argument/stdin/file/image input, conflicts, missing values, and explicit rejection of deferred or unknown options.
- [ ] AC5: JSON fixtures compare session headers, event types, required fields, semantics, and lifecycle order after normalizing allowed nondeterminism.
- [ ] AC6: Faux and local HTTP fixtures cover successful streaming, thinking, tool calls, images, usage, malformed chunks, authentication failures, timeouts, rate limits, server errors, cancellation, and each selected wire protocol.
- [ ] AC7: Agent tests cover multi-turn execution, parallel tools, same-file mutation serialization, invalid/unknown tool calls, output-limit tool calls, aborts, automatic retry, threshold compaction, and overflow compact-and-retry.
- [ ] AC8: Tool contract fixtures compare TypeScript and Rust `read`, `bash`, `edit`, and `write` behavior, including schemas, path cases, failures, truncation, partial updates, cancellation, diffs, BOM/line endings, and stale mutations.
- [ ] AC9: Image fixtures cover MIME detection, resizing limits, CLI attachment, `read` results, provider encoding, blocked images, and non-vision fallback.
- [ ] AC10: Data fixtures prove TypeScript/Rust interoperability for settings, models, API-key/OAuth auth records, and v3 sessions, including unknown-field preservation. Tests explicitly reject unsupported concurrent writers rather than claiming a nonexistent lock guarantee.
- [ ] AC11: Trust/resource tests cover global and ancestor context ordering, trusted/untrusted project settings and skills, saved/default/CLI trust decisions, and the distinction between trust and isolation.
- [ ] AC12: CI produces and smoke-tests a Linux x64 `pi-rs` artifact using Faux/local fixtures without public provider credentials.
- [ ] AC13: Later TUI tests cover selected commands, configurable keybinding actions, session/model workflows, queues, cancellation, and tool/thinking expansion without screenshot identity.
- [ ] AC14: The final gate builds and smoke-tests macOS arm64/x64, Linux arm64/x64, and Windows arm64/x64 native binaries, including at least one explicitly authorized live-provider smoke test outside CI.
- [ ] AC15: `design.md` defines crate boundaries, contracts, migration/rollback, and licensing; `implement.md` defines ordered child tasks, explicit dependencies, validation commands, and rollback points.

## Out of Scope

- Product-code changes before the planning artifacts are reviewed and approved.
- Compatibility for published npm programmatic APIs.
- Rewriting the experimental TypeScript orchestrator.
- Loading existing TypeScript/JavaScript extensions in-process.
- An in-process native Rust plugin ABI or dynamically loaded Rust plugins.
- Built-in permission prompts, tool approval policies, containers, VMs, or OS-level sandboxing.
- Exact TUI cell layout, colors, borders, or screenshot identity.
- Milestone-1 TUI, RPC, OAuth, legacy v1/v2 session migration, prompt templates, themes, package management, image generation, `grep`, `find`, or `ls`.
- Concurrent TypeScript and Rust writers to one session file.
- Direct reuse of grok-build code without a separate license and notice review.
