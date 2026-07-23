# Rust Headless CLI Milestone Implementation Plan

## Gate

Do not start until `rust-provider-protocols`, `rust-agent-tools`, and `rust-data-resources` are all `complete`, this child's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Freeze CLI and Output Contracts

- Read current CLI/startup/print/output/session-cwd files and focused tests in full.
- Build parser, mode-selection, conflict, deferred-option, stdout/stderr/exit, signal, and JSON lifecycle fixture tables.
- Mark compatibility entries `captured`; do not infer support from a parsed field alone.
- Define the exact tool-filter and session-selector precedence in fixtures before coding.

Gate: every parent milestone-1 option has positive/negative cases and every current out-of-scope option has an explicit rejection case.

## Step 2: Parser and Mode Resolver

- Implement side-effect-free parsing with the exact aliases/value rules.
- Implement supported/deferred/unknown diagnostics and headless mode selection.
- Implement help/version/list-models without loading a production provider or writable session.

```bash
cargo clippy -p pi-cli --all-targets --all-features --locked -- -D warnings
cargo test -p pi-cli --test args_contract --locked
cargo test -p pi-cli --test mode_contract --locked
```

## Step 3: Library Composition and Binary Shell

- Implement injected process I/O/environment/terminal/service interfaces.
- Add the thin `pi-rs` binary without Faux/test registrations.
- Implement two-phase session-cwd/trust/resource/model/runtime initialization.
- Compose completed provider, runtime, tools, stores, resources, retry, and compaction layers.

```bash
cargo test -p pi-cli --test initialization_contract --locked
cargo test -p pi-cli --test production_registry --locked
```

## Step 4: Inputs, Models, Tools, and Sessions

- Implement piped stdin, positional sequencing, `@text`, `@image`, prompts, model/auth/thinking, tool filters, trust flags, and supported session selectors.
- Add isolated failures for missing cwd/files, legacy sessions, stale writers, unsupported OAuth/models/tools, and invalid combinations.

```bash
cargo test -p pi-cli --test input_contract --locked
cargo test -p pi-cli --test session_contract --locked
cargo test -p pi-cli --test model_tool_contract --locked
```

## Step 5: Text/JSON Output and Cleanup

- Implement ordered/backpressured stdout guards, stderr diagnostics, redaction, exit mapping, and JSON header/event writing.
- Implement signals, root cancellation, child-process termination, session/output flushing, and broken-pipe behavior.

```bash
cargo test -p pi-cli --test output_contract --locked
cargo test -p pi-cli --test signal_cleanup --locked
```

## Step 6: End-to-End Local Integration

- Run Faux through the library entry for deterministic semantic traces.
- Run the production binary against isolated local mock servers for all four network protocols.
- Cover tools, images, retry, compaction, sessions, multiple prompts, cancellation, offline mode, and negative paths.
- Assert there is no production Faux flag/provider/environment path.

```bash
cargo test -p pi-cli --test headless_smoke --locked
cargo test -p pi-cli --test protocol_smoke --locked
cargo test -p pi-cli --test no_node_runtime --locked
```

## Step 7: Linux Release Artifact and CI

- Extend the isolated Rust CI job/workflow with exact toolchain and locked release build.
- Pin every newly introduced action SHA.
- Generate the commit-named archive, checksum, and `build-info.json`.
- Copy/unpack outside the checkout and smoke help/version/offline-list/text/JSON/local-tool/local-protocol behavior.
- Upload only a normal Actions artifact with finite retention; never touch `build-binaries.yml` release publication.

```bash
cargo build --release --locked -p pi-cli --target x86_64-unknown-linux-gnu
```

## Step 8: Documentation and Full Verification

- Document milestone-1 usage, supported/deferred flags, trust-versus-sandbox behavior, API-key commands, session concurrent-writer restriction, artifact verification, and rollback.
- Verify existing `pi`, package bins, build scripts, release scripts, and publish workflows are unchanged.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
cargo build --release --locked -p pi-cli --target x86_64-unknown-linux-gnu
./target/x86_64-unknown-linux-gnu/release/pi-rs --help
./target/x86_64-unknown-linux-gnu/release/pi-rs --version
./target/x86_64-unknown-linux-gnu/release/pi-rs --offline --list-models openai
npm run check
```

Run each modified TypeScript test specifically with the repository-required package-local Vitest invocation. Do not run `npm run build`, `npm test`, full Vitest, a live provider, a release command, or a tag/push workflow. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Keep parser/output code in `pi-cli`; do not patch lower-layer behavior to satisfy presentation snapshots.
- Use only temporary home/project/session roots for integration tests.
- Review CI changes for permissions, triggers, cache poisoning, action SHAs, artifacts, and absence of secrets/publication.
- Existing `.github/workflows/build-binaries.yml`, package bins, release scripts, and `pi` entry point remain untouched.
- Rollback is disabling/removing the isolated Rust artifact job and stopping use of `pi-rs`; no user-file migration reversal is required.

## Completion Evidence

- Map every CLI PRD criterion to parser/subprocess/local-protocol tests and artifact evidence.
- Record release binary size/linkage/runtime dependencies and checksum/provenance.
- Record stdout/stderr/signal/session rollback results.
- Leave `rust-m1-gate` as the only next dependency; no later parity child starts before its decision.
