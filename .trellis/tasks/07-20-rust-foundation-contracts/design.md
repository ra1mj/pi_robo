# Rust Foundation and Compatibility Contracts Design

## Status

Planning design for `rust-foundation-contracts`. The parent task remains active, this child remains `planning`, and this document does not authorize product-code changes.

## Inputs and Constraints

- Parent requirements: `../07-17-rust-rewrite/prd.md`.
- Parent architecture: `../07-17-rust-rewrite/design.md`.
- Parent execution phase: Phase 1 in `../07-17-rust-rewrite/implement.md`.
- Architecture reference only: `../07-17-rust-rewrite/grok-build-research.md` at upstream commit `8adf9013a0929e5c7f1d4e849492d2387837a28d`.
- This child has no prerequisite task, but it must pass its own review before `task.py start`.
- The existing TypeScript implementation is the observable-behavior oracle and remains untouched as the default product path.

## Deliverable Boundary

This child creates a compiling mixed npm/Cargo workspace and stable contracts. It does not implement provider networking, execute tools, load user resources, create a usable CLI, or publish an artifact.

The planned tree is:

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
rustfmt.toml
deny.toml
rust/
  assets/
    models.json
  crates/
    pi-protocol/
    pi-model/
    pi-provider/
    pi-agent/
    pi-tools/
    pi-store/
    pi-resources/
    pi-runtime/
    pi-cli/
    pi-test-support/
  fixtures/
    compatibility.json
    protocol/
    events/
    sessions/
```

Later child tasks may add provider, tool, resource, CLI, RPC, and TUI fixture subtrees. Empty production crates contain only documented boundaries and the minimum compile-safe library/binary entry points.

## Toolchain and Workspace Policy

- Start from the parent baseline Rust `1.97.0`, edition 2024, and Cargo resolver 3.
- Pin the exact toolchain in `rust-toolchain.toml`, including `rustfmt` and `clippy` components and the minimal profile.
- If that toolchain is unavailable or incompatible with a required reviewed dependency, stop and revise this design; do not silently float the channel or lower the repository's code.
- Commit `Cargo.lock` and use `--locked` in validation.
- Pin every direct Cargo dependency to an exact version. Disable default features unless their inclusion is reviewed and recorded.
- For each direct dependency, record source, license, enabled features, build-script/native-code presence, unsafe-code implications, and why it is needed.
- Forbid git dependencies by default. Registry/source and license exceptions require a documented review before configuration changes.
- Use workspace lints to forbid unsafe code in first-party crates initially and deny warnings in checks. A later narrowly scoped unsafe exception requires separate design review.

## Crate Boundaries

The dependency graph follows the parent design and is encoded in Cargo manifests rather than comments alone:

| Crate | Foundation responsibility | Allowed internal dependencies |
| --- | --- | --- |
| `pi-protocol` | Serializable DTOs and persisted-record views | None |
| `pi-model` | Provider/model identifiers and object-safe service contracts | `pi-protocol` |
| `pi-provider` | Empty transport implementation boundary | `pi-protocol`, `pi-model` |
| `pi-agent` | Empty low-level agent boundary | `pi-protocol`, `pi-model` |
| `pi-tools` | Empty tool implementation boundary | `pi-protocol`, `pi-agent` |
| `pi-store` | Empty persistence boundary | `pi-protocol` |
| `pi-resources` | Empty context/resource boundary | `pi-protocol`, `pi-store` |
| `pi-runtime` | Empty orchestration boundary | approved lower-layer crates only |
| `pi-cli` | Non-functional future composition root | milestone-1 production crates only |
| `pi-test-support` | Faux, fixtures, fake time, in-memory seams | contract and test-only dependencies |

`pi-test-support` must never become a normal dependency of a shipped crate. A workspace metadata check rejects reverse-layer or production-to-test-support edges.

## Contract Model

### Canonical DTOs

`pi-protocol` owns Rust representations for:

- model/provider metadata, thinking levels, usage/cost, stop reasons, and compatibility fields;
- user, assistant, and tool-result messages;
- text, thinking, image, tool-call, and tool-result content blocks;
- assistant-stream and agent/session lifecycle events;
- tool definitions, calls, partial updates, final results, and normalized failures;
- session-v3 header and known append-only entry views.

Serialization names and omission/null behavior follow captured TypeScript fixtures. Rust enum/variant names are internal and must not leak into JSON. Public contract types use only erasable JSON values at boundaries; internal errors are mapped explicitly.

### Persisted JSON

Persisted session records use a raw-plus-typed representation:

1. Parse every line into a retained raw JSON value.
2. Validate only the known fields required for the typed view.
3. Preserve unknown object fields through typed reserialization.
4. Preserve unknown entry kinds as raw records.
5. Never rewrite pre-existing JSONL lines in this child.

Round-trip tests compare semantic JSON for typed records and exact retained raw values for unknown records. Byte-for-byte compatibility is not required for newly serialized object key order, IDs, or timestamps.

### Service Interfaces

`pi-model` declares the object-safe contracts needed by later tasks for model streaming, cancellation, event delivery, tools, and stores. The foundation may define types and traits but cannot include an HTTP client, filesystem store, shell process, terminal dependency, or hidden global runtime.

Interfaces use explicit owned/borrowed data and boxed futures/streams where object safety requires them. Trait contracts document event ordering, terminal events, cancellation, and error boundaries before implementations exist.

## Model Catalog Artifact

A companion script reads the current generated TypeScript model catalog and emits `rust/assets/models.json` deterministically:

- no network access;
- no direct edit to `packages/ai/src/models.generated.ts`;
- stable sorting and newline behavior;
- an explicit schema/version field owned by the Rust consumer;
- `--check` behavior that generates in memory or a temporary location and fails on drift;
- secret-free output containing only public model metadata.

The implementation phase must first inspect the existing generator and package scripts in full, then choose the companion script location and runner that adds no unnecessary npm dependency.

## Compatibility Catalog

`rust/fixtures/compatibility.json` is a machine-readable ownership index, not a claim that future behavior already works. Each entry contains:

- stable contract ID and milestone;
- observable area and owning crate;
- TypeScript oracle/source reference;
- fixture or runner path;
- allowed field-aware normalizers;
- state: `captured`, `implemented`, or `verified`.

A validator rejects duplicate IDs, nonexistent paths, unknown owners/states, unsupported normalizer names, and a `verified` entry without a runnable fixture. Foundation entries cover protocol/session shapes and the catalog itself; later tasks append their own entries.

## Fixture and Test-Support Design

All fixtures are synthetic and credential-free. No fixture may contain a developer home path, real session content, authorization header, or public-provider response copied from a private run.

`pi-test-support` supplies:

- fixture-root discovery independent of process cwd;
- deterministic ID, clock, and sleeper seams;
- normalized JSON/event comparisons with explicit field-aware rules;
- bounded in-memory event sinks and stores;
- a scripted Faux model service;
- local-server helpers required by the next provider task, without public network calls.

Golden updates are explicit; tests never overwrite committed fixtures automatically. Normalization is allowlisted per catalog entry and cannot delete whole event bodies.

## CI and Policy Integration

Foundation validation is a separate Rust CI job or an equivalent isolated job block so existing TypeScript failures remain independently diagnosable. It runs only foundation-scope checks initially:

```text
cargo fmt --all --check
cargo clippy -p pi-protocol -p pi-model -p pi-test-support --all-targets --all-features --locked -- -D warnings
cargo test -p pi-protocol -p pi-model -p pi-test-support --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
model catalog drift check
compatibility catalog validation
```

Any newly added GitHub Action is pinned to a full commit SHA. Prefer existing runner tools or reviewed installation steps. This child does not upload a binary or change release/publish workflows.

Repository-required `npm run check` remains the final local gate because the catalog companion touches the mixed workspace. If a TypeScript test file is created or modified, run that specific test with the repository-prescribed Vitest command; never use `npm test`.

## Failure and Rollback Behavior

- Contract parse failures include a stable category and field/path context without echoing secrets.
- Catalog drift and invalid fixture ownership fail checks rather than regenerating silently.
- Dependency/license/advisory violations fail closed; exceptions are reviewed records, not broad allowlists.
- Rollback removes only the new root Cargo files and `rust/` tree plus the isolated CI block. No existing command, package, user file, session, release script, or publish path changes.

## Decisions Closed for Start Review

- The workspace is side-by-side at repository root.
- Rust 1.97.0 is the initial exact toolchain candidate.
- All direct dependencies are exact-pinned and reviewed before addition.
- Persisted contracts preserve unknown fields/entries and do not rewrite existing lines.
- Model and compatibility catalogs are committed deterministic artifacts with drift checks.
- Test support is injectable and cannot ship in production dependencies.
- No grok-build source code is copied in this child.
- No product behavior or usable CLI is part of this child.
