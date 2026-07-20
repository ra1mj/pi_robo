# Rust Rewrite Planning Traceability

## Status

Planning is converged. This document maps the parent requirements and acceptance criteria to their owning child tasks; it does not authorize implementation.

## Dependency Graph

```text
rust-foundation-contracts
├── rust-provider-protocols ─┐
├── rust-agent-tools ────────┼── rust-headless-cli ── rust-m1-gate ─┬── rust-provider-auth-parity ─┐
└── rust-data-resources ─────┘                                      └── rust-runtime-parity         │
                                                                       ├── rust-rpc-integrations ──┼── rust-cross-platform-takeover
                                                                       └── rust-tui ───────────────┘
```

Only `rust-foundation-contracts` is dependency-free. Tree order is descriptive; each child PRD is the authoritative start guard.

## Requirement Ownership

| Parent requirement | Primary owner | Dependent verification |
| --- | --- | --- |
| R1 Architecture and boundaries | `rust-foundation-contracts` | All later children preserve the dependency direction and shared contracts |
| R2 Milestone-1 CLI and output | `rust-headless-cli` | `rust-m1-gate` |
| R3 Milestone-1 providers and agent runtime | `rust-provider-protocols`, `rust-agent-tools` | `rust-headless-cli`, `rust-m1-gate` |
| R4 Milestone-1 tools and images | `rust-agent-tools` | `rust-headless-cli`, `rust-m1-gate` |
| R5 Milestone-1 data, resources, and trust | `rust-data-resources` | `rust-headless-cli`, `rust-m1-gate` |
| R6 JSON and persistence compatibility | `rust-foundation-contracts`, `rust-data-resources`, `rust-headless-cli` | `rust-m1-gate`, then `rust-runtime-parity` for compatible writers |
| R7 Later milestones and replacement gate | `rust-runtime-parity`, `rust-provider-auth-parity`, `rust-rpc-integrations`, `rust-tui`, `rust-cross-platform-takeover` | Final takeover matrix |

## Acceptance-Criteria Ownership

| Criterion | Owning evidence task(s) | Planned evidence |
| --- | --- | --- |
| AC1 | Parent design, `rust-foundation-contracts` | Scope/crate-boundary review and exclusion checks |
| AC2 | `rust-foundation-contracts` | Pinned toolchain, workspace checks, dependency policy, docs |
| AC3 | `rust-headless-cli` | Outside-repository Linux x64 text/JSON execution |
| AC4 | `rust-headless-cli` | Complete milestone-1 CLI option and rejection matrix |
| AC5 | `rust-foundation-contracts`, `rust-headless-cli` | Normalized v3 JSON contract fixtures and subprocess traces |
| AC6 | `rust-provider-protocols` | Faux/local HTTP protocol success and failure fixtures |
| AC7 | `rust-agent-tools` | Agent, retry, compaction, cancellation, and ordering fixtures |
| AC8 | `rust-agent-tools` | TypeScript/Rust four-tool contract comparison |
| AC9 | `rust-provider-protocols`, `rust-agent-tools`, `rust-headless-cli` | Image detection, resize, transport, tool, and CLI fixtures |
| AC10 | `rust-data-resources` | Bidirectional settings/models/auth/session interoperability fixtures |
| AC11 | `rust-data-resources` | Context, skill, trust, and no-sandbox matrix |
| AC12 | `rust-headless-cli`, `rust-m1-gate` | Commit-bound checksummed Linux x64 CI artifact and smoke evidence |
| AC13 | `rust-runtime-parity`, `rust-tui` | Shared runtime behavior plus deterministic TUI/tmux workflows |
| AC14 | `rust-cross-platform-takeover` | Native OS/architecture artifact and authorized live-smoke matrix |
| AC15 | Parent planning artifacts and all child plans | Boundary, dependency, validation, migration, and rollback review |

## Authorization Gates

The plans do not authorize external or destructive actions. Live-provider calls, CI reruns that consume protected environments, dependency-secret changes, commits, pushes, tags, release publication, npm launcher changes, signing, TypeScript removal, and replacement of the `pi` command remain separately controlled actions. Milestone-1 code and artifacts remain side-by-side and keep TypeScript `pi` as the rollback path.

## Coverage Decision

Every parent requirement and acceptance criterion has an owner and downstream verification point. The intentionally excluded npm SDK surface, in-process TypeScript extensions, native dynamic plugins, orchestrator rewrite, built-in sandbox, exact TUI appearance, and concurrent cross-runtime session writers remain explicit exclusions rather than uncovered work.
