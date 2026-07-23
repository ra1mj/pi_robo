# Rust Data, Resources, and Trust Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-foundation-contracts` must be `complete` before this child starts. It can proceed independently of provider and agent/tool implementation because it exposes typed stores and resource snapshots.

## Evidence and Corrected Trust Contract

The behavioral oracle includes current configuration, session, model, credential, trust, resource, skill, and system-prompt implementations/tests under `packages/coding-agent` plus session/resource behavior under `packages/agent`.

The parent PRD resolves a critical distinction:

- context files load in headless mode unless explicitly disabled;
- protected project settings and project skills require trust;
- trust controls input loading, not tool containment or sandboxing.

This child design follows that distinction. It does not gate ancestor context files on project approval.

## Boundary and Outputs

`pi-store` owns paths, settings, model/auth/trust inputs, credential value resolution, and session-v3 persistence. `pi-resources` owns context/skill discovery and normalized system-prompt assembly. Model-registry views may extend `pi-model` but cannot perform provider HTTP.

The primary output is an immutable runtime snapshot containing:

- authoritative cwd and normalized paths;
- merged allowed settings;
- selected catalog/custom model metadata;
- resolved API-key credential snapshot or actionable error;
- loaded session records/context view;
- context files, skills, diagnostics, and assembled prompt inputs;
- the trust decision and provenance for every protected resource.

The headless CLI later composes these APIs. This child does not parse argv or write stdout/stderr.

## Initialization Support

Resource APIs support the parent's two-phase startup:

1. Bootstrap global paths/settings and session directory.
2. Resolve or create the session and authoritative cwd.
3. Resolve trust for that cwd.
4. Load global plus allowed project settings/resources.
5. Load catalogs, custom models, and a credential snapshot.

No startup-directory project resource can leak into a resumed session whose authoritative cwd differs.

## Paths and Isolation

All stores receive explicit agent-home, cwd, and environment/process seams. Tests use temporary home/project directories and sanitized environment maps. No test reads or writes the developer's real `~/.pi/agent`.

Paths support compatible tilde expansion, normalization, canonical trust matching, per-project session directories, and actionable invalid-path errors. Platform-specific permission behavior beyond Linux belongs to the takeover task.

## Settings

- Parse global `settings.json` first.
- After the trust decision, load protected project settings when allowed.
- Merge project values over global values using the captured top-level plus one-level nested object behavior.
- Preserve defaults, unknown compatible fields, and structured diagnostics.
- Milestone 1 is read-only: settings files and lock directories are never created, refreshed, normalized, or rewritten.

The settings view exposes only values required by milestone 1. Unsupported settings remain preserved/raw and generate diagnostics only where current behavior does.

## Models

- Load the embedded generated catalog from `rust/assets/models.json`.
- Parse user `models.json` after compatible JSON-comment stripping.
- Apply model/provider overrides and custom definitions without mutating the source file.
- Validate direct `openai`, `anthropic`, `google`, and custom selected-protocol definitions for milestone 1.
- Do not advertise other brands merely because they share a wire protocol.
- Preserve unknown/provider-specific compatible properties needed by later milestones.

Model definitions remain credential-blind until runtime credential resolution.

## Authentication and Config Values

Milestone-1 resolution order is:

1. in-memory CLI API-key override supplied by the caller;
2. `auth.json` record with `type: "api_key"`;
3. provider environment variable;
4. provider key from `models.json`.

The value resolver supports literals, `$VAR`/`${VAR}`, escaped dollar/bang prefixes, provider-scoped environment values, and explicit `!command` execution according to captured behavior. Commands execute with user permissions through an injected process runner, bounded output/time, redaction, and cancellation. This is explicitly not a sandbox.

OAuth records are valid raw data but unusable in milestone 1. They are not refreshed, migrated, deleted, normalized, or rewritten. Missing/invalid API-key paths return guidance distinguishing unsupported OAuth from absent credentials.

`auth.json`, settings, models, and trust files are read-only in milestone 1. Later write support must implement the current `proper-lockfile` directory/mtime protocol; auth parent/file permissions must be `0700`/`0600` where supported.

## Trust

Trust paths are canonicalized, and the closest stored ancestor decision wins. The effective non-interactive decision combines:

- explicit `--approve` or `--no-approve` supplied by the caller;
- saved trust decision;
- `defaultProjectTrust: "always"`;
- `ask`, which cannot prompt in headless mode and therefore skips protected project settings/skills.

Global settings/skills are not project-controlled. Ancestor context files load unless `--no-context-files` is active. Protected project settings and project-local skills load only when the effective decision allows them. The snapshot records why each protected source was loaded or skipped.

## Session-v3 Store

### Read and Context Reconstruction

- Accept only v3 for milestone-1 mutation; v1/v2 are diagnosable read/defer cases and are never migrated.
- Retain every original JSONL line/raw value, including unknown fields and extension entry kinds.
- Validate the header before append.
- Build model context from known entries along the supported current-leaf path while ignoring unknown entries semantically without deleting them.
- Support the lookup/read behavior needed later by `--continue`, `--session`, `--session-id`, and `--session-dir`.

### Append

- A single Rust store instance serializes appends.
- Append one compact JSON object plus newline and never rewrite prior lines.
- Add supported message, model/thinking, and compaction entries with canonical IDs/timestamps from injected sources.
- Track file identity and last observed length/metadata; refuse append when an externally detectable change occurred since the loaded snapshot.

The last check detects some stale writers but cannot eliminate the race with a simultaneous TypeScript append. No `.lock` guarantee is claimed, and users must not run TypeScript and Rust writers on the same session concurrently. Tests and documentation state this limitation directly.

## Context Discovery

- Load global context first.
- Walk ancestors from filesystem root to authoritative cwd.
- In each directory choose the first supported context filename according to current precedence.
- Preserve ordering, diagnostics, disable flags, and normalized XML/system-prompt representation.
- Load context even when project trust is unresolved/denied unless context loading itself is disabled.

## Skill Discovery

Discover from explicit caller paths, settings, global locations, and trusted project locations. Preserve frontmatter validation/limits, ignore behavior, disable-model-invocation metadata, collision precedence, root-skill preference, and diagnostics.

Project-local skills are protected resources. Package-managed skills and executable extensions remain deferred.

## System Prompt Assembly

`pi-resources` produces normalized prompt inputs from tool snippets, guidelines, appended prompt text, context XML, skill XML, documentation paths, and cwd. It does not know CLI rendering or provider wire formats. Dynamic absolute paths are normalized only in fixtures, not removed from runtime prompts.

## Fixtures and Interoperability

- Settings/models/auth/trust fixtures cover valid, comments, unknown, malformed, merge, precedence, environment, command, and redaction behavior.
- Trust matrices cover saved/default/CLI/ask decisions and prove context is independent from protected settings/skills.
- Session fixtures run both directions: TypeScript create/read, Rust append/read, then Rust create/read and TypeScript append/read/rollback where supported.
- Unknown fields/entries and OAuth records remain semantically and byte-preserved because read-only files are unchanged and prior session lines are never rewritten.
- All process/home paths and IDs are synthetic or normalized by field-aware rules.

## Trade-offs and Rollback

- Read-only shared configuration removes lock/write risk from milestone 1 while still supporting real prompts.
- Stale-file checks improve diagnostics but do not create false cross-process safety.
- Loading context without trust matches current behavior but is a security distinction that documentation and tests must make explicit.
- Rollback is switching back to TypeScript against the same additive v3 session. Shared settings/auth/models/trust bytes remain unchanged.

## Decisions Closed for Start Review

- Settings/auth/models/trust are read-only in milestone 1.
- API-key precedence is fixed; OAuth is preserved but not executed.
- Context loads unless disabled; protected project settings/skills are trust-gated.
- Session coexistence means mutual read/append rollback, not concurrent writing.
- No legacy migration, sandbox, package resources, prompt templates, themes, or extensions are included.
