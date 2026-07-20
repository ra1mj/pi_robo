# Rust Headless CLI Milestone Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-provider-protocols`, `../07-20-rust-agent-tools`, and `../07-20-rust-data-resources` must all be `complete` before this child starts. Task-tree position does not satisfy the gate.

## Evidence and Boundary

The current observable CLI behavior is captured from `packages/coding-agent/src/main.ts`, `src/cli/args.ts`, `src/cli/file-processor.ts`, `src/cli/initial-message.ts`, `src/cli/list-models.ts`, `src/modes/print-mode.ts`, `src/core/output-guard.ts`, `src/core/session-cwd.ts`, `docs/json.md`, and focused CLI/print/stdout tests.

This child composes already completed Rust layers. It may add integration glue and CLI-specific contracts, but it cannot reimplement provider, agent/tool, or persistence semantics inside `pi-cli`.

Milestone 1 produces only a side-by-side Linux x64 `pi-rs`. It does not replace `pi`, publish a release, modify npm package bins, support Windows/macOS, or expose TUI/RPC/OAuth behavior.

## Binary and Library Split

`pi-cli` has two entry layers:

- a testable library composition entry receiving argv, environment/path snapshot, stdin/stdout/stderr writers, terminal capabilities, clock, cancellation, and service factories;
- a thin `pi-rs` binary that binds those interfaces to the real process and production services.

Faux is injectable only through the library/service factory used by tests. The production binary has no Faux provider name, flag, environment variable, endpoint, or alternate hidden entry. Production subprocess tests use local mock servers through explicit custom `models.json` configuration.

No Rust path launches Node.js, Bun, or a JavaScript runtime. Credential `!command` values and the `bash` tool may execute user-requested processes, but neither is a runtime dependency or test backdoor.

## Supported CLI Surface

The parser recognizes exactly the milestone-1 subset:

- modes/input: `--print/-p`, `--mode text|json`, positional messages, piped stdin, `@text`, and `@image`;
- model/auth: `--provider`, `--model`, `--api-key`, `--thinking`, `--list-models`, `--offline`;
- prompts/resources: `--system-prompt`, repeatable `--append-system-prompt`, `--skill`, `--no-skills/-ns`, `--no-context-files/-nc`;
- sessions: `--continue/-c`, `--session`, `--session-id`, `--session-dir`, `--no-session`, `--name/-n`;
- tools/trust: `--tools/-t`, `--exclude-tools/-xt`, `--no-tools/-nt`, `--approve/-a`, `--no-approve/-na`;
- metadata: `--help/-h`, `--version/-v`.

Missing values, invalid thinking/mode values, conflicting session/trust options, empty names/tool lists, and ambiguous model selection produce explicit input errors. `--api-key` requires a resolvable provider/model and remains in memory.

Recognized current flags outside this set return an `unsupported in pi-rs milestone 1` error with the nearest supported alternative where one exists. This includes interactive-only, RPC, resume/fork/tree, model-cycle, extension, prompt-template, theme, export, package-management, and verbose behavior. Unknown short/long flags return a distinct unknown-option error. Nothing is silently stored for an extension to interpret later.

## Mode Selection

- `--mode json` selects JSON mode.
- `--mode text` or `--print/-p` selects text mode.
- Non-TTY stdin selects text mode unless JSON is explicit.
- Bare TTY startup and a positional prompt that would otherwise select current interactive behavior fail with guidance to add `-p` or `--mode text`.
- `--mode rpc` fails as deferred, not as an unknown mode.
- Metadata commands do not require a writable session or provider call.

Thus positional messages are supported as headless input when headless mode is explicit or selected by piped stdin; milestone 1 does not silently reinterpret an interactive invocation.

## Argument and Input Assembly

Parsing is side-effect free and returns typed diagnostics. After the authoritative cwd is known:

- resolve `@` paths relative to that cwd with compatible tilde/path behavior;
- wrap text file contents in the compatible file markup;
- detect/process supported images through the completed tool/image layer;
- combine piped stdin, file text, and the first positional message into the initial prompt according to captured ordering;
- send remaining positional messages sequentially in the same session;
- reject missing/unreadable paths and invalid images with stderr diagnostics and exit 1.

Input limits and secret redaction are explicit. Tests never use actual home/session files.

## Two-Phase Initialization

The composition root follows this order:

1. Parse argv and handle help/version syntax without loading project code.
2. Load global bootstrap settings and resolve the session directory.
3. Resolve/create the selected session and its authoritative cwd.
4. Reject a missing stored cwd in headless mode with actionable paths.
5. Resolve project trust for that cwd.
6. Load allowed settings, models, credentials, context, and skills.
7. Resolve model/thinking/tools/system prompt and create the runtime.
8. Process prompts sequentially, persist supported entries, drain output, and exit.

This prevents startup-directory project settings/skills from leaking into a resumed session from another project. Context remains governed by the separately approved context/trust contract.

## Model, Tool, and Session Resolution

- Model/provider patterns, thinking suffix/override, API-key precedence, and direct/custom protocol validation are delegated to completed model/store services.
- Only `read`, `bash`, `edit`, and `write` can be selected. Deferred/unknown tools fail explicitly.
- `--no-tools` disables all four; allow/deny combinations use one documented deterministic precedence captured in CLI fixtures.
- Session selectors implement only the approved continue/path-or-ID/exact-ID/directory/ephemeral/name behavior.
- Legacy session mutation and detectable stale-writer conditions fail before the prompt.
- Unsupported concurrent TypeScript/Rust writes remain an operational prohibition.

## Output Contract

The composition root exclusively owns process output:

- text stdout: final assistant text blocks only, each written in canonical order;
- JSON stdout: v3 session header first, then one compact JSON object per compatible event;
- stderr: warnings, diagnostics, progress not represented as JSON, and redacted failures;
- help/version/model listing: their documented metadata output only.

All stdout writers are ordered and backpressured. JSON serialization writes contract DTOs directly; debug/internal Rust fields cannot leak. A diagnostic from any library is routed through stderr rather than accidental stdout logging.

Text mode exits 1 for a terminal assistant `error` or `aborted` result. JSON mode emits its terminal compatible event and also exits 1 for terminal failure. Input/config/provider failures exit 1; success exits 0.

## Signals and Cleanup

SIGINT, SIGTERM, and SIGHUP where supported cancel the root token, terminate tracked shell process groups, stop provider streams/retry/compaction, settle session appends, drain stdout/stderr, and return 130/143/129 respectively. Repeated termination may use a bounded forced-exit path documented and tested separately.

Broken pipe and output backpressure errors are handled explicitly. Secrets are redacted before any stderr or panic boundary.

## Offline Behavior

`--offline` disables startup catalog refresh, update/version checks, and telemetry/network discovery. It does not block the provider request explicitly selected by the user. The milestone-1 CLI performs no implicit public network request in any mode.

## Integration Test Architecture

- Parser table tests cover every supported flag, alias, missing value, conflict, deferred flag, and unknown flag.
- Library tests inject Faux for deterministic text/JSON, tools, images, retry, compaction, sessions, and cancellation.
- Production binary tests use isolated temp home/project/session directories and local mock servers for the four network protocols.
- Subprocess tests assert stdout, stderr, exit status, signal cleanup, session files, child-process cleanup, and JSON lifecycle order.
- Release-mode smoke tests execute the copied binary from outside the repository with no Node.js/Bun dependency.

## CI Artifact Contract

Extend the isolated Rust CI workflow/job; do not alter the tag-triggered release/publish workflow. New action references are pinned to full commit SHAs.

The Linux job builds `x86_64-unknown-linux-gnu` release mode with `--locked`, runs the complete Rust checks, and stages:

```text
pi-rs-linux-x64-<git-commit>.tar.gz
SHA256SUMS
build-info.json
```

`build-info.json` contains a schema version, source commit, workspace version, target, profile, exact rustc/cargo versions, and Cargo.lock digest. The GitHub Actions artifact is named `pi-rs-linux-x64-<git-commit>` with finite retention. It is not a GitHub Release asset and triggers no npm/publication job.

The staged binary is smoke-tested after copying to a directory outside the checkout. Artifact tests verify checksum and build-info consistency.

## Trade-offs and Rollback

- The GNU Linux target is accepted for milestone 1; static/musl and wider Linux compatibility require separate evidence.
- Explicit headless selection avoids pretending positional TTY input has interactive parity.
- A library injection seam keeps deterministic Faux tests without weakening the production binary.
- Rollback disables/removes the isolated Rust artifact job and stops invoking `pi-rs`. Existing `pi`, TypeScript CI, npm packages, release workflows, and user configuration remain intact.

## Decisions Closed for Start Review

- Production command is `pi-rs`, Linux x64 only.
- Faux never appears in the production binary surface.
- Output ownership and mode selection are centralized in `pi-cli`.
- Deferred and unknown options have distinct explicit errors.
- Artifact identity is commit-based with checksum and build metadata.
- No release publication, tag workflow, npm change, or `pi` replacement occurs.
