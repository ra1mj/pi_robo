# Rust terminal UI

## Goal

Implement a native Rust terminal UI with behavioral parity for the approved interactive workflows, using the gated Rust core as its only execution backend.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-runtime-parity` must be `complete` before this task starts; that task itself requires the milestone-1 PASS.
- Define parity behavior for rendering, editing, keybindings, dialogs, streaming updates, tools, images, sessions, errors, and terminal resize before choosing implementation details.
- Keep keybindings configurable through the Rust equivalent of shared default binding tables; do not hardcode isolated key checks.
- Preserve cancellation, retry, compaction, trust, and session behavior from the headless core.
- Cover selected slash commands, configurable action IDs, session/model/thinking workflows, steering/follow-up queues, tree navigation, tool/thinking expansion, and login/integration dialogs through injectable services.
- Build deterministic terminal-model tests and controlled tmux smoke tests; do not make screenshot appearance the sole oracle.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] The approved interactive workflow matrix passes in deterministic terminal fixtures and representative tmux smoke tests.
- [ ] Configurable keybindings, Unicode/wide characters, resize, paste, scrolling, streaming, tool panels, and error dialogs have regression coverage.
- [ ] Interactive sessions remain readable by the supported TypeScript/Rust session-v3 paths.
- [ ] Terminal teardown restores the user's terminal after normal exit, cancellation, panic, and provider failure.
- [ ] The TUI introduces no separate provider, tool, trust, or persistence semantics.
- [ ] Selected command, session-tree, model/thinking, queue, and dialog workflows match the behavioral compatibility matrix.

## Out of Scope

- Pixel-identical rendering, provider/auth expansion, RPC, npm SDK compatibility, release publication, or default-command takeover.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the milestone-1 gate decision is mandatory.
