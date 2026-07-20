# Rust Rewrite Implementation Plan

## Gate

This is an execution plan, not authorization to edit product code. The child tasks below have been created, and every child PRD records its explicit dependencies. After final planning review and after the workflow permits implementation:

1. verify that `rust-foundation-contracts` is the only dependency-free child;
2. start only `rust-foundation-contracts`;
3. load `trellis-before-dev` before changing code in that child;
4. start later children only after every dependency named in their PRD is `complete`.

Inline mode is required. Do not dispatch implementation or check sub-agents, and do not curate `implement.jsonl`/`check.jsonl` for this workflow.

## Child Task Graph

| Order | Proposed child task | Explicit dependencies | Independent completion gate |
| --- | --- | --- | --- |
| 1 | `rust-foundation-contracts` | None | Pinned workspace, DTOs, catalog artifact, fixture framework, policy checks |
| 2 | `rust-provider-protocols` | `rust-foundation-contracts` complete | Five selected protocol adapters pass local wire fixtures |
| 3 | `rust-agent-tools` | `rust-foundation-contracts` complete | Agent loop, four tools, images, retry, and compaction pass Faux/tool fixtures |
| 4 | `rust-data-resources` | `rust-foundation-contracts` complete | Settings/models/auth/session/trust/context/skills compatibility passes |
| 5 | `rust-headless-cli` | `rust-provider-protocols`, `rust-agent-tools`, and `rust-data-resources` complete | `pi-rs` text/JSON Linux x64 integration and artifact pass |
| 6 | `rust-m1-gate` | `rust-headless-cli` complete | Full milestone-1 acceptance matrix and rollback drill pass |
| 7 | `rust-runtime-parity` | `rust-m1-gate` complete | Shared queues, compaction/tree/session/resource/settings behaviors required by RPC and TUI pass |
| 8 | `rust-provider-auth-parity` | `rust-m1-gate` complete | Remaining provider protocols, provider brands, OAuth, and image-generation scope pass |
| 9 | `rust-rpc-integrations` | `rust-runtime-parity` complete | RPC and command/MCP/HTTP/LSP integration contracts pass |
| 10 | `rust-tui` | `rust-runtime-parity` complete | Behavioral terminal compatibility passes on supported local platforms |
| 11 | `rust-cross-platform-takeover` | `rust-provider-auth-parity`, `rust-rpc-integrations`, and `rust-tui` complete | All platform, packaging, rollback, docs, and replacement gates pass |

Tree position does not imply dependency. Every child artifact must contain its dependency row in prose and must refuse `task.py start` while a dependency is incomplete.

## Phase 1: Foundation and Contracts

### Deliverables

- Add root `Cargo.toml`, committed `Cargo.lock`, exact `rust-toolchain.toml`, shared workspace lints, `rustfmt.toml`, and `deny.toml`.
- Add empty production crate boundaries from `design.md` with dependency directions enforced in manifests.
- Implement `pi-protocol` DTOs for models, messages, content blocks, usage, stream events, agent events, tool results, session headers, and known session entries.
- Implement raw-plus-typed persisted JSON handling and normalization helpers for IDs, timestamps, paths, and allowed parallel event ordering.
- Add a companion model-catalog generator/check that serializes the current generated TypeScript catalog into committed `rust/assets/models.json` without fetching the network or editing `packages/ai/src/models.generated.ts` directly.
- Add fixture directory conventions, secret scanning, and a TypeScript/Rust contract-runner protocol.
- Add `pi-test-support` with an in-memory event sink, fake clock/sleeper, scripted Faux model service, temporary store, and local HTTP server helpers.
- Document allowed licenses/sources and reject git dependencies by default.

### Validation

```bash
cargo fmt --all --check
cargo clippy -p pi-protocol -p pi-model -p pi-test-support --all-targets --all-features --locked -- -D warnings
cargo test -p pi-protocol -p pi-model -p pi-test-support --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
npm run check
```

If the catalog companion or a TypeScript test is added, run its package-specific command and the modified test file with the repository-required Vitest invocation. Do not run `npm test`.

### Rollback Point

All additions are side-by-side. Removing the root Cargo files and `rust/` tree restores the prior build; no existing binary, user data, or release workflow changes in this phase.

## Phase 2: Provider Protocols

### Deliverables

- Implement shared HTTP client construction, proxy selection, TLS/root handling, decompression, idle timeout, cancellation, bounded/redacted errors, and incremental SSE parsing.
- Implement OpenAI Chat Completions request/event mapping.
- Implement OpenAI Responses request/event mapping.
- Implement Anthropic Messages request/event mapping.
- Implement Google Generative AI request/event mapping.
- Implement Faux through test-support injection, not a production CLI flag.
- Implement partial tool-call JSON accumulation, reasoning/thinking signatures, response IDs, stop reasons, images, usage/cost mapping, and error finalization.
- Add local mock fixtures for success, multi-block streaming, tool calls, images, malformed SSE/JSON, HTTP errors, timeouts, retry hints, and cancellation.
- Validate direct `openai`, `anthropic`, `google`, and custom `models.json` endpoint/header behavior. Do not advertise other provider brands yet.

### Validation

```bash
cargo fmt --all --check
cargo clippy -p pi-provider --all-targets --all-features --locked -- -D warnings
cargo test -p pi-provider --all-targets --locked
cargo test -p pi-provider --test openai_chat_contract --locked
cargo test -p pi-provider --test openai_responses_contract --locked
cargo test -p pi-provider --test anthropic_contract --locked
cargo test -p pi-provider --test google_contract --locked
npm run check
```

No provider credential or public endpoint may be present in the test environment.

### Rollback Point

Provider crates are unreachable from the existing `pi`. A failing adapter can be disabled from the `pi-rs` registry without altering model files or TypeScript behavior.

## Phase 3: Agent Runtime, Tools, Images, Retry, and Compaction

### Deliverables

- Implement the low-level agent event loop and settle `agent_end` last.
- Implement typed tool registration, argument compatibility preparation, schema validation, unknown-tool errors, output-limit tool-call blocking, parallel batches, and source-order tool-result messages.
- Implement bounded event backpressure and hierarchical cancellation.
- Port head/tail truncation with exact UTF-8 and logical-line accounting.
- Implement `read` text/image behavior and image resize/block/non-vision rules.
- Implement `bash` shell selection, combined streaming output, updates, timeout, temporary full output, process groups, and tree termination.
- Implement `edit` normalization, unique/non-overlapping replacement rules, BOM/line endings, display diff, unified patch, and same-path mutation serialization.
- Implement `write` parent creation and mutation serialization.
- Implement high-level runtime retry events and fake-clock exponential backoff.
- Implement threshold compaction and one overflow compact-and-retry using an abstract session sink.
- Add Faux traces for successful text, thinking, multiple tool turns, parallel tools, failures, aborts, retry, compaction, and event settlement.
- Add TypeScript/Rust tool runners that use isolated copies of fixture trees.

### Validation

```bash
cargo fmt --all --check
cargo clippy -p pi-agent -p pi-tools -p pi-runtime --all-targets --all-features --locked -- -D warnings
cargo test -p pi-agent -p pi-tools -p pi-runtime --all-targets --locked
cargo test -p pi-tools --test typescript_contract --locked
cargo test -p pi-runtime --test event_trace --locked
npm run check
```

If TypeScript fixture code imports package internals, run the relevant existing test file or the new specific test from that package root. Never invoke the full Vitest suite directly.

### Rollback Point

All filesystem mutations occur only inside explicitly supplied cwd/temp fixtures until CLI integration. The child cannot alter the existing session path or register a production executable.

## Phase 4: Data, Resources, and Trust

### Deliverables

- Implement agent/config/session path resolution and tilde/path normalization.
- Implement global/project settings parsing, defaults, one-level nested deep merge, validation diagnostics, and read-only milestone-1 behavior.
- Implement comment-compatible `models.json` parsing, model overrides, custom providers, and selected-protocol validation.
- Implement raw `auth.json` loading and API-key view without writes; support literals, environment interpolation, escapes, provider-scoped env, and explicit command values.
- Implement trust store reads, canonical/ancestor decision lookup, CLI override, and headless `defaultProjectTrust` semantics.
- Implement v3 session parse, context reconstruction, branch selection for the current leaf, append-only writes, model/thinking entries, compaction entries, and session lookup for the supported CLI subset.
- Reject legacy session mutation and simultaneous writer conditions that can be detected; document the remaining operational restriction.
- Implement global/ancestor context discovery, configured/global/trusted-project skill discovery, frontmatter diagnostics, ignores, collisions, and system prompt assembly.
- Add bidirectional TypeScript/Rust fixture tests, including unknown entries/fields and unchanged OAuth records.
- Add future lock-protocol tests now as ignored/design fixtures only if milestone-1 code remains read-only; do not add unused production locking abstractions.

### Validation

```bash
cargo fmt --all --check
cargo clippy -p pi-store -p pi-resources --all-targets --all-features --locked -- -D warnings
cargo test -p pi-store -p pi-resources --all-targets --locked
cargo test -p pi-store --test typescript_interop --locked
cargo test -p pi-resources --test trust_matrix --locked
npm run check
```

Run permission tests on Linux. Platform-specific permission behavior belongs to the takeover task.

### Rollback Point

Milestone-1 settings/auth/trust/model access is read-only. Session appends are enabled only after fixture parity passes and never rewrite existing lines, so rollback is switching back to TypeScript `pi`.

## Phase 5: Headless CLI and Linux Artifact

### Deliverables

- Implement the exact milestone-1 clap argument surface and custom error mapping.
- Implement two-phase initialization around authoritative session cwd and trust.
- Compose production provider, agent, tools, store, resources, retry, and compaction services.
- Implement text and JSON stdout guards, diagnostic redaction, exit mapping, signals, writer flushing, and child cleanup.
- Support argument/stdin/multiple-message input, `@text`, `@image`, system prompts, sessions, tool selection, trust flags, list-models, and offline startup.
- Reject bare interactive mode, RPC, deferred flags, unknown flags, unsupported tools, OAuth-only models, and legacy-session mutation with actionable errors.
- Add CLI subprocess tests for stdout, stderr, exit status, signals, sessions, files, and normalized JSON traces.
- Add a separate Rust CI job. Pin every added GitHub Action to a full commit SHA or use preinstalled `rustup` directly.
- Build, smoke-test, and upload `pi-rs-linux-x64`. Do not change `scripts/build-binaries.sh`, npm package bins, tag release assets, or the `pi` command.
- Add milestone-1 usage/security/compatibility documentation and rollback instructions.

### Validation

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
cargo build --release --locked -p pi-cli
./target/release/pi-rs --help
./target/release/pi-rs --version
./target/release/pi-rs --offline --list-models openai
cargo test -p pi-cli --test headless_smoke --locked
npm run check
```

Do not run `npm run build`, `npm test`, or a live provider smoke unless the user separately requests it.

### Rollback Point

The CI Rust job and artifact are independent. Disabling the job or deleting the artifact cannot affect TypeScript CI, npm publication, release tags, or user installations.

## Phase 6: Milestone-1 Gate

### Checklist

- Map AC1 through AC12 and AC15 from `prd.md` to named tests, artifacts, and CI logs. AC13 and AC14 remain assigned to later milestones.
- Run all Rust checks with a clean Cargo target and `--locked`.
- Run `npm run check` with full output.
- Run every modified TypeScript test file specifically during its owning phase, then run `./test.sh` at the milestone gate because the catalog/compatibility support touches the TypeScript workspace. Never run `npm test`.
- Build the release binary in CI and in a clean local environment outside the repository.
- Smoke help, version, offline model listing, text, JSON, tool, image, continue-session, error, cancellation, and signal cleanup with Faux/local mocks.
- Copy a TypeScript-produced v3 session, append with Rust, reopen with TypeScript, then reverse the direction.
- Verify `auth.json`, settings, trust, and OAuth records are byte-unchanged after milestone-1 runs.
- Verify no credentials, home-directory paths, private session content, or unredacted headers exist in artifacts/logs.
- Verify the existing `pi` command and all current release assets are unchanged.
- Perform the rollback drill and record it in the child task notes.

### Completion Rule

Milestone 1 is complete only when every mapped criterion passes. Partial provider or tool success is not enough to advertise the binary. Failures are fixed inside the owning child task; acceptance criteria are not weakened to close the gate.

## Later Milestones

### Shared Runtime and Resource Parity

- Complete steering/follow-up queues, manual compaction, branch summaries/tree navigation, session management/export, settings/trust writes, reload, prompt templates, and package-managed declarative resource discovery once behind shared service contracts.
- Keep these behaviors independent from RPC serialization and TUI rendering so both later consumers use one implementation.

### Provider/Auth Parity

- Add protocol families in independently fixture-tested slices: Bedrock Converse Stream, Azure/OpenAI variants, Google Vertex, OpenAI Codex, Mistral Conversations, image generation, and provider-specific compatibility behavior.
- Add OAuth login/refresh/logout with lock-compatible storage only after cross-process auth tests pass.
- Advertise provider brands individually after URL/header/auth/catalog/live opt-in smoke evidence exists.

### RPC and External Integrations

- Port current JSON RPC commands/events over the shared runtime.
- Define versioned command/MCP/HTTP/LSP integration manifests, trust prompts, lifecycle, timeouts, cancellation, and logs.
- Provide migration examples for common legacy extension classes without hosting TypeScript in-process.

### TUI

- Implement behavior on top of `pi-runtime`; do not duplicate agent state.
- Port named configurable actions, input/editor behavior, model/session selection, queues, slash commands, tool/thinking expansion, cancellation, and terminal cleanup.
- Use memory/test backends for deterministic state tests and tmux only for terminal integration smoke tests.

### Cross-Platform Takeover

- Add macOS/Linux/Windows arm64/x64 builds one platform at a time with native shell, process-tree, path, permissions, signal, image, and terminal fixtures.
- Design native packaging and any npm launcher transition as a separately reviewed compatibility change.
- Run the repository release smoke process from outside the repository.
- Request explicit authorization before any live-provider prompt, public release, npm change, tag, push, legacy rename, or `pi` entry-point replacement.
- Preserve the TypeScript rollback distribution for at least one release cycle.

## Risky Files and Review Points

- `packages/ai/scripts/generate-models.ts` and generated model files: prefer a companion serializer; never hand-edit `packages/ai/src/models.generated.ts`.
- Root `package.json` and `package-lock.json`: avoid new npm dependencies; if metadata changes, refresh with `npm install --package-lock-only --ignore-scripts` and follow the lockfile approval rule.
- `.github/workflows/ci.yml`: keep Rust as a separate job and pin newly added actions.
- `scripts/build-binaries.sh`, release scripts, package bins, and publish workflows: untouched in milestone 1.
- Session/auth/settings fixtures: synthetic only; never copy user files into the repository.
- Cargo native/build dependencies: explicit source, license, build-script, and cross-platform review before addition.
- grok-build references: architecture only unless a source/commit/license/notice record is approved.

## Session Handoff Requirements

At the end of every child task, record:

- acceptance criteria completed and tests proving them;
- files changed by this session only;
- dependency or license changes reviewed;
- known incompatibilities that remain explicitly deferred;
- rollback result;
- the next unblocked child task.

Do not commit unless the user asks. If asked, stage only paths changed in that child task and follow the repository commit format.
