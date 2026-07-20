# Rust shared runtime and resource parity

## Goal

Complete the shared non-UI runtime, session, settings, and declarative resource behaviors deferred from milestone 1 so RPC and TUI consume one verified implementation.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-m1-gate` must be `complete` with a written PASS before this task starts.
- Implement steering and follow-up queues/modes, manual compaction, branch summarization, tree navigation, and shared runtime state/actions.
- Complete session-v3 tree operations, fork/clone/switch/labels/names/stats, safe delete/list/resume support, and compatible HTML export without rewriting prior entries.
- Add compatible settings/trust/keybinding write services using the current `proper-lockfile` protocol; session files retain their explicit no-cross-process-lock limitation.
- Add reloadable prompt-template, theme-catalog, and package-managed declarative resource discovery with trust, source metadata, collision, and diagnostic behavior.
- Provide one command/resource registry consumed by RPC and TUI; do not duplicate behavior in either surface.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] Deterministic runtime tests cover steering/follow-up queue modes, cancellation, settlement, manual compaction, branch summaries, and reload.
- [ ] TypeScript/Rust session fixtures cover tree navigation, fork/clone/switch, labels/names/stats, export, delete/list/resume, unknown preservation, and rollback.
- [ ] Cross-process settings/trust/keybinding write tests prove lock compatibility, atomicity, unknown-field preservation, and applicable permissions.
- [ ] Resource fixtures cover prompt templates, theme catalogs, package-managed declarative sources, trust, ordering, collisions, reload, and diagnostics.
- [ ] RPC and TUI can exercise the same service interfaces without importing each other.

## Out of Scope

- Provider/auth protocols, executable command/MCP/HTTP/LSP integrations, terminal rendering, RPC serialization, cross-platform packaging, or command takeover.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the milestone-1 PASS is mandatory.
