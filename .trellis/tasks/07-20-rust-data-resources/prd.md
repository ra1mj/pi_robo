# Rust data resources and trust

## Goal

Implement the milestone-1 settings, model, API-key, session-v3, project-context, skill-discovery, and trust resources needed by the Rust headless CLI.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-foundation-contracts` must be `complete` before this task starts.
- Read current settings, `models.json`, API-key auth, project context, and skills using the precedence defined by the parent design.
- Validate direct OpenAI, Anthropic, Google, and custom `models.json` entries needed by milestone 1.
- Append and read session-v3 JSONL while preserving unknown fields and supporting rollback/read compatibility with the TypeScript implementation.
- Do not claim concurrent same-session cross-process write safety; detect/document unsupported coexistence rather than adding an incompatible lock.
- Preserve headless trust behavior: context files load unless disabled, while protected project settings and skills require a saved approval, `defaultProjectTrust: "always"`, or `--approve`; `ask` cannot prompt in headless mode and skips protected resources. Milestone 1 does not provide a sandbox.
- Any later writes to shared settings/auth resources must use `proper-lockfile`-compatible locking; auth files must use mode `0600` where supported.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] Fixture tests prove the documented CLI/auth/env/models precedence and actionable failures for invalid sources.
- [ ] Session-v3 fixtures pass append, reload, rollback, malformed-line, unknown-field, and TypeScript/Rust mutual-read tests.
- [ ] Tests reject or clearly diagnose concurrent same-session writer scenarios rather than implying safety.
- [ ] Trust fixtures prove context loading is independent from protected project settings/skills, and cover saved, default, CLI, and non-interactive `ask` decisions.
- [ ] Resource tests use isolated temporary homes/projects and never modify a developer's real configuration.

## Out of Scope

- OAuth execution, v1/v2 session migration, package/theme/prompt management, provider networking, CLI rendering, or a security sandbox.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the completion check above is mandatory.
