# Rust headless CLI milestone

## Goal

Compose the completed Rust provider, agent/tool, and data/resource layers into a Linux x64 `pi-rs` headless text/JSON CLI without changing the existing `pi` command.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependencies: `../07-20-rust-provider-protocols`, `../07-20-rust-agent-tools`, and `../07-20-rust-data-resources` must all be `complete` before this task starts.
- Produce a standalone Linux x64 binary named `pi-rs`; no Node.js runtime may be required.
- Implement the exact milestone-1 CLI flag and exit-code surface defined in the parent PRD/design for text and JSON modes.
- Compose API-key/model resolution, project trust, sessions, provider streaming, agent execution, tools, images, retry, and compaction.
- Reject deferred or unknown flags with explicit errors and migration guidance; do not silently ignore TypeScript-only behavior.
- Add CI jobs for scoped Rust checks, fixture tests, and the Linux x64 artifact without disrupting existing TypeScript jobs.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] The release-mode `pi-rs` runs from outside the repository without Node.js; the injectable CLI library completes deterministic Faux text/JSON runs, while the production binary completes local mock-protocol text/JSON runs without a hidden Faux flag or endpoint.
- [ ] Local-fixture runs cover all five milestone-1 protocols, tool calls, image input, retry, compaction, sessions, and cancellation.
- [ ] Supported flags, output schemas, exit codes, stderr behavior, and unsupported-flag errors match the parent contract.
- [ ] Existing `pi`, npm packages, and TypeScript CI remain unchanged and passing.
- [ ] CI uploads a versioned Linux x64 `pi-rs` artifact with provenance/checksum metadata defined by the child design.

## Out of Scope

- Replacing `pi`, Windows/macOS artifacts, TUI, RPC, OAuth, npm SDK parity, or release publication.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; all three upstream completion checks are mandatory.
