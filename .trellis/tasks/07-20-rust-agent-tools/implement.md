# Rust Agent Runtime and Core Tools Implementation Plan

## Gate

Do not start until `rust-foundation-contracts` is `complete`, this child's final artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Capture Runtime and Tool Contracts

- Read the selected TypeScript source and focused non-e2e tests in full.
- Capture canonical schemas, event traces, error results, truncation, images, retry, and compaction fixtures.
- Implement TypeScript fixture runners only where static fixtures cannot prove filesystem behavior.
- Use isolated copied trees and the coding-agent faux-provider harness; never real providers.

Gate: fixtures validate, contain no real user paths/data, and are cataloged as `captured`.

## Step 2: Low-Level Agent Loop

- Implement event state machine, stream accumulation, tool-call validation, bounded backpressure, cancellation, and final settlement.
- Implement parallel batch execution with source-ordered result messages.
- Cover invalid/unknown calls, output-limit calls, provider error/abort, and late event-sink settlement.

```bash
cargo clippy -p pi-agent --all-targets --all-features --locked -- -D warnings
cargo test -p pi-agent --all-targets --locked
cargo test -p pi-agent --test event_trace --locked
```

## Step 3: Shared Tool Infrastructure

- Implement authoritative-cwd path resolution, canonical mutation keys/leases, schema preprocessing/validation, output accumulation, UTF-8 line/byte truncation, and partial-update contracts.
- Add symlink-alias, cancellation, invalid-input, and concurrency tests before individual mutation tools.

```bash
cargo test -p pi-tools --test path_contract --locked
cargo test -p pi-tools --test truncation_contract --locked
cargo test -p pi-tools --test mutation_queue --locked
```

## Step 4: `read` and Images

- Implement text offset/limit/truncation and content-based image detection.
- Add reviewed image dependency/features and bounded blocking resize path.
- Cover every supported format, blockImages, malformed data, cancellation, and non-vision omission semantics.

```bash
cargo test -p pi-tools --test read_contract --locked
cargo test -p pi-tools --test image_contract --locked
```

## Step 5: `bash`

- Implement Linux shell resolution, prefix application, process groups, combined streaming output, partial updates, timeout, full-output temp file, cancellation, and cleanup.
- Prove child/grandchild termination and late-output drain.

```bash
cargo test -p pi-tools --test bash_contract --locked
cargo test -p pi-tools --test bash_process_tree --locked
```

## Step 6: `edit` and `write`

- Implement edit normalization/matching, unique/non-overlapping replacements, BOM/line endings, diffs, mutation leasing, and stale-write prevention.
- Implement parent creation and complete UTF-8 writes under the same lease rules.
- Decide atomic replacement only from permission/symlink fixtures; do not assume it is compatible.

```bash
cargo test -p pi-tools --test edit_contract --locked
cargo test -p pi-tools --test write_contract --locked
cargo test -p pi-tools --test typescript_contract --locked
```

## Step 7: Retry and Compaction Runtime

- Implement injected retry policy/events and cancellable fake-time backoff.
- Implement threshold compaction and one overflow compact-and-retry through an abstract session sink.
- Cover token/usage inputs, summary insertion, error paths, cancellation, and no unintended follow-up turn.

```bash
cargo clippy -p pi-runtime --all-targets --all-features --locked -- -D warnings
cargo test -p pi-runtime --test retry_contract --locked
cargo test -p pi-runtime --test compaction_contract --locked
```

## Step 8: Full Verification

```bash
cargo fmt --all --check
cargo clippy -p pi-agent -p pi-tools -p pi-runtime --all-targets --all-features --locked -- -D warnings
cargo test -p pi-agent -p pi-tools -p pi-runtime --all-targets --locked
cargo test -p pi-tools --test typescript_contract --locked
cargo test -p pi-runtime --test event_trace --locked
cargo deny check
npm run check
```

Run each modified TypeScript test specifically using the repository-required package-local Vitest invocation. Coding-agent suite additions must use `test/suite/harness.ts` and Faux. Do not run real providers, `npm test`, or the full Vitest suite. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Land/check each tool independently; do not mask a contract failure by weakening shared normalization.
- Review new image/process dependencies and build scripts before lockfile changes.
- Execute filesystem tests only in temporary roots; never point runners at the actual repository or home configuration.
- No existing tool registration, binary, session path, or release workflow changes.
- Rollback removes the new Rust crate code/fixtures; existing TypeScript behavior remains the path in use.

## Completion Evidence

- Map PRD criteria to named Faux and TypeScript/Rust fixture tests.
- Record tool parity gaps explicitly rather than marking partial behavior compatible.
- Record cancellation/process-tree and rollback results.
- Leave `rust-headless-cli` blocked until provider and data/resource siblings also complete.
