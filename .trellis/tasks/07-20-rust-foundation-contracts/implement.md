# Rust Foundation and Compatibility Contracts Implementation Plan

## Gate

This plan is not implementation authorization. Keep `rust-foundation-contracts` in `planning` until the user reviews its PRD, design, and this plan. Only then may `task.py start` be considered. Before any product-code edit, load `trellis-before-dev` and re-read the relevant `.trellis/spec/` guidance.

Inline execution is required. Do not dispatch implementation or checking to sub-agents, and do not curate JSONL manifests for this workflow.

## Step 1: Baseline Audit and Dependency Ledger

### Work

- Read root workspace/package configuration, current CI workflows, `.gitignore`, model generator scripts, package scripts, and relevant TypeScript contract/session types in full.
- Confirm there is no overlapping Cargo workspace or uncommitted user work in planned paths.
- Verify the Rust 1.97.0 toolchain candidate and identify the minimum direct dependencies/features required for foundation scope.
- Create a dependency ledger covering exact version, source, license, features, build scripts/native code, unsafe implications, and rationale.
- Confirm the companion model-catalog runner can use existing repository tooling without a new npm dependency.

### Gate

- No file is added until overlapping work and dependency/security questions are resolved.
- Any intentional existing behavior that would need removal or replacement is returned for user approval instead.
- The dependency ledger contains no floating direct version or unreviewed git source.

## Step 2: Workspace and Crate Skeleton

### Work

- Add the pinned root Cargo workspace, lockfile, toolchain, formatting, lints, and policy configuration.
- Add every milestone-1 crate boundary with minimal package metadata, documentation, and compile-safe entry points.
- Encode the approved internal dependency direction in manifests.
- Keep `pi-cli` non-functional and ensure no executable is installed or substituted for `pi`.
- Add a metadata/policy test that rejects forbidden dependency edges and production dependencies on `pi-test-support`.

### Target Paths

- `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`.
- `rust/crates/*/Cargo.toml` and minimal `src/lib.rs` or future binary entry points.

### Gate

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo test -p pi-test-support --test workspace_policy --locked
```

- No root npm package, package bin, release script, or existing CLI path changes.

## Step 3: Protocol DTOs and Persisted Records

### Work

- Capture synthetic JSON fixtures from the current TypeScript contract shapes before implementing Rust serializers.
- Implement model/provider, message/content, usage/stop, stream/agent event, tool, error, and session-v3 DTOs in `pi-protocol`.
- Implement raw-plus-typed persisted record parsing with flattened extra fields and unknown-entry retention.
- Declare provider/model service interfaces in `pi-model` with documented ordering, cancellation, and error semantics.
- Add property and fixture tests for valid, missing, unknown, malformed, and round-trip cases.

### Target Paths

- `rust/crates/pi-protocol/src/**` and `rust/crates/pi-protocol/tests/**`.
- `rust/crates/pi-model/src/**` and `rust/crates/pi-model/tests/**`.
- `rust/fixtures/protocol/**`, `rust/fixtures/events/**`, and `rust/fixtures/sessions/**`.

### Gate

```bash
cargo clippy -p pi-protocol -p pi-model --all-targets --all-features --locked -- -D warnings
cargo test -p pi-protocol -p pi-model --all-targets --locked
```

- Unknown session fields and unknown entry kinds survive their defined round trips.
- Rust-specific enum names or error internals do not appear in public JSON.

## Step 4: Deterministic Catalogs

### Work

- Implement the companion TypeScript model serializer after fully inspecting the existing generator and generated type exports.
- Generate and commit `rust/assets/models.json` with stable ordering and a drift-check mode.
- Add `rust/fixtures/compatibility.json` and its schema/validator.
- Seed only foundation-owned entries; future behavior remains `captured`, not falsely `verified`.
- Add fixture secret/path scanning and deterministic-output tests.

### Target Paths

- A reviewed companion script under the existing AI generator/script structure.
- `rust/assets/models.json`.
- `rust/fixtures/compatibility.json` and validator tests in the owning test-support/tooling path.
- The narrow package script or root check hook required to run drift validation.

### Gate

- Running generation twice produces no diff.
- Drift check fails after a controlled temporary mutation and passes after restoration.
- Duplicate IDs, missing paths, unowned entries, unsupported normalizers, and invalid verified states are rejected.
- No direct edit is made to `packages/ai/src/models.generated.ts`.

## Step 5: Test Support

### Work

- Implement fixture discovery, deterministic IDs/clocks/sleeps, normalization, in-memory event/store seams, scripted Faux behavior, and local-server helpers.
- Keep helpers generic enough for provider and agent child tasks but do not implement those production layers.
- Add tests for bounded event collection, cancellation, fixture-root independence, normalization allowlists, and automatic-golden-write rejection.

### Target Paths

- `rust/crates/pi-test-support/src/**` and `rust/crates/pi-test-support/tests/**`.
- Synthetic foundation fixture files only.

### Gate

```bash
cargo clippy -p pi-test-support --all-targets --all-features --locked -- -D warnings
cargo test -p pi-test-support --all-targets --locked
```

- Tests pass with outbound network unavailable and no provider/auth environment variables.
- No production crate has a normal dependency on `pi-test-support`.

## Step 6: Policy and CI Wiring

### Work

- Configure license, advisory, duplicate, registry, and git-source policy from the reviewed dependency ledger.
- Add the isolated foundation Rust CI job and deterministic catalog checks.
- Pin new action references to full commit SHAs or use reviewed runner-installed tools.
- Verify that existing TypeScript jobs, build/release scripts, npm package bins, and publish workflows are unchanged.

### Gate

```bash
cargo deny check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

- A controlled forbidden source/license fixture or policy test proves failure is closed.
- CI has no live-provider call, secret requirement, binary upload, publish, tag, or release side effect.

## Step 7: Full Foundation Verification

### Required Commands

```bash
cargo fmt --all --check
cargo clippy -p pi-protocol -p pi-model -p pi-test-support --all-targets --all-features --locked -- -D warnings
cargo test -p pi-protocol -p pi-model -p pi-test-support --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
npm run check
```

If a TypeScript test file is created or modified, run that exact test with the repository-required package-local Vitest invocation and iterate until it passes. Do not run `npm run build`, `npm test`, or the full Vitest suite.

### Completion Evidence

- Map every child PRD acceptance criterion to a named test, command, catalog check, or policy output.
- Record all dependency/license decisions and every changed path.
- Confirm `pi`, npm packages, user data, session paths, and release workflows are unaffected.
- Record the rollback drill: remove only a disposable copy of the new Cargo/Rust additions and confirm the TypeScript checks remain independent.
- Run `trellis-check` before requesting completion review.

## Handoff

Do not commit unless the user asks. On completion, leave the next three children (`rust-provider-protocols`, `rust-agent-tools`, and `rust-data-resources`) blocked only by this child's status and provide the evidence needed to mark this child `complete`.
