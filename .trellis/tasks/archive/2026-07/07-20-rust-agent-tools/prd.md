# Rust agent runtime and core tools

## Status

Implementation and verification completed on 2026-07-22. The archived `rust-foundation-contracts` dependency was verified complete before work started; no open product or scope question remains.

## Goal

Implement the provider-neutral Rust agent loop and the milestone-1 `read`, `bash`, `edit`, and `write` tools with image, retry, and compaction behavior.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-foundation-contracts` must be `complete` before this task starts.
- Implement turn orchestration, streaming event propagation, tool dispatch, cancellation, abort/error handling, and usage accounting against the canonical contracts.
- Implement project-root-aware `read`, `bash`, `edit`, and `write` tools with explicit inputs, outputs, truncation, and error semantics.
- Support image content in messages and tool results where the parent contracts permit it.
- Implement automatic retry and automatic compaction with deterministic policy seams and observable events.
- Use Faux and local fixtures; do not require production provider APIs.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [x] Multi-turn text and tool-call loops pass deterministic Faux fixtures.
- [x] Each core tool passes success, invalid-input, missing-path, permission, cancellation, and output-truncation tests applicable to that tool.
- [x] Image content survives agent and tool round trips without silent degradation.
- [x] Retry tests prove bounded retries, backoff policy selection, cancellation, and terminal error propagation.
- [x] Compaction tests prove trigger selection, summary insertion, token/usage accounting, and continued execution.
- [x] The runtime is usable through an in-memory store and provider interface without CLI or disk-resource dependencies.

## Out of Scope

- Provider wire protocols, production CLI composition, TUI/RPC, OAuth, or deferred tools such as grep/find/ls.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the completion check above is mandatory.
