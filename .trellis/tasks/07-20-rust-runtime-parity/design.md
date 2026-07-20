# Rust Shared Runtime and Resource Parity Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-m1-gate` must be `complete` with a written PASS before this task starts. It extends the gated core without changing milestone-1 output/provider/tool contracts.

## Purpose and Boundary

RPC and TUI both need queues, manual compaction, session trees, settings mutations, reload, and declarative resources. These behaviors live once in `pi-runtime`, `pi-store`, and `pi-resources`; neither consumer may implement private variants.

This task has no RPC framing, external executable integration, terminal rendering, provider/auth protocol, binary packaging, or release responsibility.

## Shared Runtime State and Actions

Expose serializable snapshots and async actions for:

- steering/follow-up queues and `all`/`one-at-a-time` modes;
- prompt, steer, follow-up, abort, idle/settled state, and pending counts;
- automatic/manual compaction and retry controls;
- model/thinking selection interfaces backed by existing services;
- direct user bash execution through the existing tool/process service;
- session replacement/rebind notifications;
- command/resource enumeration.

State changes emit canonical events through the existing ordered sink. RPC and TUI observe the same event sequence. Rebinding a session replaces state atomically and cancels/settles old work before new consumers attach.

## Queues

Steering messages affect the active run at the compatible turn boundary; follow-up messages wait until the current agent run settles. Queue modes select whether all queued messages or one message is released per eligible boundary.

Every enqueue/dequeue/mode change emits the full compatible queue snapshot. Cancellation, session switch, compaction, and reload have fixture-defined retention/clearing behavior. No surface mutates queue internals directly.

## Manual Compaction and Branch Summaries

Manual compaction reuses the gated compaction service with explicit instructions, cancellation, start/end events, and persisted v3 entries. Branch summarization uses the same model/credential snapshot rules and records summary provenance without rewriting history.

Compaction and branch operations serialize against conflicting session mutations. Cancelling compaction never leaves a half-written entry or auto-starts a user turn.

## Session Tree Service

Complete v3 operations needed by interactive and RPC workflows:

- list/resume/search metadata;
- get entries/tree/current leaf/messages/stats/last assistant text;
- fork, clone, switch, new session, and navigate tree;
- labels and session display name;
- safe delete of explicitly selected session files;
- export compatible HTML from synthetic/embed-safe templates.

All operations preserve raw unknown entries/fields and prior JSONL lines. Session append remains process-local without a shared TypeScript/Rust lock; operations detect stale snapshots where possible and document unsupported concurrent writers.

Deletion is explicit and path-contained, never a wildcard cleanup. Export escapes untrusted content and embeds no active user-supplied script.

## Mutable Settings, Trust, and Keybindings

Add general JSON resource mutation using compatible `proper-lockfile` directory/mtime ownership:

- serialized read-modify-write under lock;
- atomic replacement after successful validation;
- unknown-field preservation;
- stale/compromised lock diagnostics and bounded retry;
- parent/file permissions appropriate to each resource;
- no mutation on validation or callback failure.

Settings, trust, and keybindings use this service. Auth mutations remain owned by provider/auth parity. Session files do not use this lock because TypeScript sessions do not.

## Declarative Resources

### Prompt Templates

Discover explicit, global, trusted-project, and package-managed Markdown templates with compatible frontmatter, arguments/substitution, ordering, collisions, source metadata, and diagnostics. Execution produces prompt input; it cannot run arbitrary code.

### Theme Catalog

Discover/validate theme JSON and source metadata, preserving current names/colors/schema and collision behavior. This task provides typed theme data only; terminal rendering belongs to TUI.

### Package-Managed Declarative Sources

Discover enabled declarative resources from the compatible package/settings representation and attach provenance/trust. Acquisition/update/removal of executable integrations belongs to the integration task; this task never installs or executes packages.

### Reload and Registry

Reload builds a complete candidate snapshot, validates it, then swaps atomically. Consumers receive one change event and never see mixed old/new settings/resources. The command registry combines built-in runtime commands, prompt templates, skills, and later integration command descriptors with stable source information.

## Test Architecture

- Faux runtime traces for queues, manual compaction, cancellation, reload, settlement, and session rebind.
- Bidirectional TypeScript/Rust session tree fixtures in isolated copies.
- Multi-process lock fixtures for settings/trust/keybindings, including crash/stale/compromised cases.
- Prompt/theme/package resource trees covering trust, ignores, collision, invalid data, and atomic reload.
- HTML export fixtures covering escaping, unknown/custom entries, deterministic normalization, and no script injection.

## Trade-offs and Rollback

- One shared service task adds an extra dependency step but prevents RPC/TUI divergence.
- Theme parsing is shared; visual interpretation remains TUI-owned.
- Package discovery is separated from executable installation/activation to keep trust boundaries explicit.
- Rollback disables these later shared features while retaining the already gated M1 headless core and additive sessions.

## Decisions Closed for Start Review

- RPC and TUI depend on this task explicitly.
- Shared state/actions are serializable and UI/transport neutral.
- Mutable JSON resources use compatible locking; sessions retain the documented no-lock limitation.
- Declarative prompt/theme/package discovery cannot execute code.
- Missing shared behavior returns here rather than being patched in RPC/TUI.
