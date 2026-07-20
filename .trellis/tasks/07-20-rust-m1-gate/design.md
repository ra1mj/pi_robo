# Rust Milestone 1 Acceptance Gate Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-headless-cli` must be `complete` before this gate starts. Completion of earlier children is inherited through that dependency but is re-audited from their evidence.

This task verifies a candidate; it does not implement missing behavior. A failed check returns to the owning child rather than being patched inside the gate or waived by changing acceptance criteria.

## Gate Scope

Milestone 1 evaluates parent acceptance criteria AC1 through AC12 and AC15. AC13 (TUI) and AC14 (final cross-platform/live-provider release gate) remain assigned to later tasks and cannot be used to fail or falsely pass milestone 1.

The gate covers:

- architecture/scope and dependency policy;
- Linux x64 headless text/JSON CLI;
- parser/input/output/exit/signal contracts;
- four network protocols plus injected Faux;
- agent loop, four tools, images, retry, and compaction;
- settings/models/auth/session/trust/context/skills;
- TypeScript/Rust interoperability and rollback;
- CI artifact, checksum/provenance, and existing TypeScript regression safety.

It excludes feature implementation, provider expansion, OAuth, RPC, TUI, Windows/macOS, release publication, and command replacement.

## Candidate Identity

Before validation, freeze:

- source commit or an explicitly recorded uncommitted diff digest for local preflight;
- exact `Cargo.lock` digest and Rust toolchain;
- Linux artifact/archive SHA-256;
- `build-info.json` contents;
- compatibility catalog version/state;
- upstream child-task evidence revisions.

A final pass of AC12 requires a CI artifact tied to a source commit. Creating/pushing that commit or rerunning GitHub Actions is an external state change and occurs only with explicit authorization. A local uncommitted candidate may receive preflight results but not a final PASS.

## Evidence Model

The task records `gate-report.md` with one row per parent criterion:

| Field | Meaning |
| --- | --- |
| Criterion | AC1-AC12 or AC15 |
| Contract | Exact behavior being proven |
| Evidence | Named test, command output, artifact, or fixture |
| Candidate identity | Commit/digest and platform |
| Result | PASS or FAIL |
| Owner on failure | Foundation, provider, agent/tools, data/resources, or CLI child |
| Notes/risk | Remaining non-blocking information only |

Authorized exceptions are allowed only for unavailable external infrastructure, never for a product behavior required by milestone 1. Every exception states who authorized it, why it is safe, and what prevents a final PASS if evidence is still missing.

## Acceptance Mapping

- AC1: scope table, crate graph, excluded npm SDK/orchestrator/extension boundaries.
- AC2: exact toolchain, locked formatting/lint/tests/docs/deny checks.
- AC3: outside-checkout Linux binary text/JSON execution and unchanged `pi`.
- AC4: complete supported/deferred/unknown CLI matrix and input conflicts.
- AC5: normalized v3 header/event schema, semantics, and lifecycle ordering.
- AC6: Faux/local HTTP success and negative matrix for all selected protocols.
- AC7: multi-turn/parallel tools, mutation ordering, invalid calls, abort/retry/compaction.
- AC8: TypeScript/Rust four-tool schema, file, diff, error, truncation, update, and cancellation fixtures.
- AC9: image detection/resize/attachment/read/provider/block/non-vision fixtures.
- AC10: settings/models/auth/session interoperability, unknown preservation, and concurrent-writer limitation.
- AC11: context ordering plus protected-resource trust matrix and no-sandbox distinction.
- AC12: CI-built/smoked Linux artifact without public credentials.
- AC15: parent/child design, dependencies, validation, licensing, migration, and rollback completeness.

## Verification Environments

### Local Clean Verification

Use a fresh temporary `CARGO_TARGET_DIR`, isolated home/project/session directories, locked dependencies, and sanitized provider/auth environment variables. Do not run against the developer's actual agent home.

Network access is disabled for runtime tests except loopback local servers. Dependency acquisition, if required, is separate from protocol tests and uses the reviewed lockfile/source policy.

### Outside-Repository Smoke

Copy and unpack the candidate archive under a temporary directory outside the checkout. Provide only synthetic config/session files and a PATH containing required shell utilities but no Node/Bun requirement. Verify help, version, offline model listing, text, JSON, local protocol, tool, image, session, error, cancellation, and signal behavior.

Inspect binary linkage and process behavior to record native runtime dependencies. “Standalone” means no Node.js/Bun/package workspace; it does not imply musl/static linking unless separately specified.

### CI Artifact Verification

Download the exact normal Actions artifact, verify archive/checksum/build-info/source commit consistency, and rerun its outside-checkout smoke. The gate never triggers the tag release workflow, publishes npm, creates a GitHub Release, or uses production secrets.

## Test Layers

1. Static/policy: manifests, dependency graph, exact pins, licenses/advisories/sources, generated catalog drift, action SHAs, workflow permissions/triggers.
2. Unit/contract: all Rust workspace tests under the exact toolchain with `--locked`.
3. Cross-runtime fixtures: specific TypeScript runners versus Rust protocol/tool/store/resource behavior.
4. Non-e2e TypeScript regression: repository `npm run check` plus `./test.sh`.
5. Release-mode subprocess: copied binary in isolated external directories.
6. CI evidence: commit-bound artifact and workflow logs.

No public provider, paid token, user credential, or private session is used. A live-provider smoke belongs to the final AC14 gate and would require separate explicit authorization.

## Session Interoperability and Rollback

Use only cloned synthetic v3 fixtures:

1. TypeScript creates/appends; Rust reads and appends; TypeScript reopens and rolls back/navigates the supported leaf.
2. Rust creates/appends; TypeScript reads and appends; Rust reopens.
3. Unknown fields, unknown entry kinds, OAuth records, prior lines, and read-only configuration bytes are compared.
4. Detectable stale writers are rejected, and the documentation explicitly prohibits concurrent TypeScript/Rust writers.

The rollback drill stops using `pi-rs`, invokes the existing TypeScript `pi` against the compatible copied session, and verifies no migration/rewrite is needed. It also proves current npm bins and release assets were not changed.

## Security and Privacy Audit

- Scan source fixtures, build logs, archives, checksums, diagnostics, and snapshots for credentials, authorization headers, private paths, and session content.
- Verify config-value commands and tools are documented as unsandboxed user processes.
- Verify trust gates protected resources but is never described as containment.
- Confirm binary/runtime errors redact API keys and sensitive headers.
- Confirm CI has minimum permissions, no secrets requirement, no public provider traffic, and no publication trigger.

## Decision Rules

PASS requires every mapped milestone criterion to have reproducible passing evidence for the same commit-bound candidate and artifact.

FAIL results when any required behavior, regression, security control, provenance check, or rollback drill fails. The report names the owning child and exact failing evidence. The gate task remains incomplete.

Missing authorization for commit/push/CI inspection is reported as external evidence pending; it does not convert a local preflight into PASS and does not weaken AC12.

After a written PASS, `rust-runtime-parity` and `rust-provider-auth-parity` become eligible to start. RPC and TUI remain blocked on `rust-runtime-parity`. The gate never replaces `pi` or authorizes a public release.

## Rollback of the Gate Itself

The gate writes only task-local evidence and disposable temporary artifacts. It does not mutate product code or user files. Temporary directories are removed after evidence capture; external CI/artifacts remain untouched unless separately authorized.

## Decisions Closed for Start Review

- AC1-AC12 and AC15 form the milestone-1 matrix.
- Candidate identity is commit/artifact/digest bound.
- Local preflight cannot substitute for AC12 CI evidence.
- Failures return to owning child; criteria are not weakened in the gate.
- `./test.sh`, not `npm test`, is the full non-e2e TypeScript regression command.
- Live-provider and public release actions are not milestone-1 requirements.
