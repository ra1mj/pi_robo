# Rust milestone 1 acceptance gate

## Goal

Independently verify that the Linux x64 `pi-rs` milestone satisfies the approved compatibility, coexistence, security, and rollback gates before later parity work begins.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-headless-cli` must be `complete` before this task starts.
- Audit parent AC1 through AC12 and AC15 against reproducible evidence rather than implementation claims; AC13 and AC14 remain later milestones.
- Run the fixture, contract, negative-path, session-coexistence, rollback, packaging, and existing-TypeScript regression matrices from isolated environments.
- Record gaps as blocking defects; do not weaken contracts or mark unsupported behavior as compatible.
- Bind a final PASS to one source commit, lockfile/toolchain identity, and checksummed CI artifact; local uncommitted preflight cannot substitute for AC12.
- Live-provider smoke tests, CI reruns, release actions, and other external side effects require explicit authorization.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] AC1-AC12 and AC15 each link to passing evidence for the same commit-bound candidate; authorized infrastructure exceptions never waive required product behavior or produce a false PASS.
- [ ] The binary is independently smoke-tested outside the repository without Node.js.
- [ ] TypeScript and Rust mutually read and roll back supported session-v3 fixtures; unsupported concurrent writers are explicitly guarded/documented.
- [ ] `npm run check`, `./test.sh`, and every specifically modified TypeScript test remain green with full output recorded.
- [ ] Rollback consists only of stopping Rust artifact use; the existing `pi` path remains operational.
- [ ] A written gate decision identifies all remaining risks and whether later child tasks may start.

## Out of Scope

- Adding missing product behavior, replacing `pi`, publishing releases, or implementing later parity phases.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the completion check above is mandatory.
