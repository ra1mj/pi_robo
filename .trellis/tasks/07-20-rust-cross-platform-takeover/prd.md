# Rust cross-platform takeover

## Goal

Complete cross-platform packaging and verify all takeover gates before proposing any change from the existing TypeScript `pi` command to the Rust implementation.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependencies: `../07-20-rust-provider-auth-parity`, `../07-20-rust-rpc-integrations`, and `../07-20-rust-tui` must all be `complete` before this task starts.
- Produce and verify Linux, macOS, and Windows artifacts for supported x64/arm64 targets defined by the child design.
- Validate installation, startup, update, rollback, configuration, sessions, trust, providers/auth, RPC, TUI, and shell behavior on the platform matrix.
- Keep the TypeScript implementation and existing `pi` command available until every takeover gate passes.
- Treat command replacement, release publication, tag/push, CI secret/environment changes, and removing TypeScript functionality as separate external or destructive actions requiring explicit user authorization.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] Checksummed, provenance-bearing artifacts pass clean-machine smoke tests for every approved OS/architecture target; signing is added only if a reviewed release policy and credentials are explicitly authorized.
- [ ] Provider/auth, RPC, TUI, sessions, configuration, and rollback matrices pass on representative native runners.
- [ ] Windows limitations discovered during milestone 1 are resolved or documented as explicit takeover blockers, not silently waived.
- [ ] Supply-chain provenance, reproducibility evidence, dependency/license policy, and release rollback procedures are complete.
- [ ] A final comparison demonstrates no approved critical TypeScript CLI behavior is missing.
- [ ] No command replacement, publication, tag, push, or TypeScript removal occurs without a separate explicit approval.

## Out of Scope

- In-process npm SDK compatibility, TypeScript extension compatibility, or porting the excluded orchestrator.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; all three upstream completion checks are mandatory.
