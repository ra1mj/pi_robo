# Rust foundation and compatibility contracts

## Goal

Create the pinned Rust workspace, compatibility contracts, catalog artifacts, and fixture infrastructure that every later Rust workstream depends on.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependencies: none. This is the first executable child task.
- Define the crate/module boundaries described by the parent design without adding product behavior prematurely.
- Pin the Rust toolchain and direct dependencies, and commit reproducible dependency metadata.
- Model the canonical message, content-block, tool-call/result, usage, stop-reason, model, provider, session-v3, settings, and error contracts.
- Generate or validate a machine-readable compatibility catalog that maps supported TypeScript behavior to Rust contracts and fixtures.
- Establish fixture loaders, golden/snapshot comparison helpers, deterministic clocks/IDs where needed, and policy checks for unsafe code, licenses, and supply-chain metadata.
- Preserve unknown session-v3 fields in the contract model.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [x] The pinned workspace and all declared crates pass their scoped formatting, lint, type, and unit checks.
- [x] Contract round-trip tests cover representative messages, tools, usage, provider errors, and session-v3 entries.
- [x] Unknown session-v3 fields survive parse and serialize tests.
- [x] The compatibility catalog is validated in CI and rejects duplicate or unowned contract entries.
- [x] Fixture helpers can run deterministically without network access or provider credentials.
- [x] Dependency, license, and unsafe-code policies fail closed with documented exceptions.

## Out of Scope

- Provider HTTP/SSE implementations.
- The agent loop, executable tools, resource stores, CLI, TUI, RPC, or production artifacts.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Status remains `planning`; no implementation is authorized by this decomposition alone.
