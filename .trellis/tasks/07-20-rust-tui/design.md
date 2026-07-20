# Rust Terminal UI Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-runtime-parity` must be `complete` before this task starts. Linux is the implementation baseline; macOS/Windows and all architecture-specific acceptance belong to the final cross-platform task.

## Evidence and Compatibility Target

Behavior is captured from `packages/tui`, the coding-agent interactive mode/components, `docs/keybindings.md`, slash/session/model/queue tests, and controlled tmux behavior. Compatibility is defined by user workflows, state transitions, action IDs, text/content visibility, input behavior, and cleanup—not identical cells, colors, borders, animation timing, or screenshots.

The Rust TUI consumes shared runtime snapshots/events/actions and the abstract interaction services used by auth/integrations. It does not implement provider, tool, retry, compaction, trust, session, or integration semantics privately.

## Architecture

`pi-tui` is split into:

- application reducer/state: derives UI state from canonical runtime events and user actions;
- action/keybinding registry: stable namespaced IDs, defaults, user overrides, conflicts, and legacy-ID migration;
- terminal/input backend: raw mode, key/mouse/paste/resize/focus events, capability detection, image transport, and teardown;
- view/component layer: editor, messages, tools, thinking, status/footer, queues, overlays, dialogs, selectors, and trees;
- controller: invokes shared runtime/session/model/resource/interaction actions and applies results.

The reducer and components run against an in-memory/test backend without a real terminal. The production backend uses reviewed exact-pinned Ratatui/Crossterm or equivalent crates only after dependency/security/license review.

## Interactive CLI Entry

This task extends `pi-rs` so bare TTY startup and positional TTY prompts select the Rust interactive mode. Text/JSON mode behavior remains unchanged. Interactive-specific current options become supported only when their owning workflow passes; still-deferred options continue to fail explicitly.

Startup follows authoritative session cwd and trust/resource ordering. Initial text/files/images are queued after TUI initialization. Fatal startup errors restore the terminal before diagnostics.

## Key Input and Keybindings

- Preserve current namespaced `tui.*` and `app.*` action IDs and documented default bindings.
- Parse the compatible legacy sequences plus supported Kitty keyboard protocol forms, modifiers, symbols, function keys, and ambiguous escape/control cases.
- Support single or multiple user keys, default replacement, empty bindings, unknown IDs, duplicate-key conflict diagnostics, and legacy config migration.
- Bind components to action IDs through the registry. No isolated `ctrl+x`/raw-sequence checks are allowed outside the central defaults/parser.
- Platform defaults such as no Windows suspend binding are applied by the backend/cross-platform layer, not scattered through components.

## Editor and Input

The editor supports Unicode grapheme-safe cursor movement/deletion, wide/CJK/emoji width, line/word navigation, selection/copy, kill ring/yank/yank-pop, undo, multiline input, tab/autocomplete, bracketed paste, large paste, external-editor round trips, image paste, and history/queued-message restoration.

Rendering and cursor mapping are tested across resize/wrap/scroll. Invalid UTF-8 terminal bytes are handled without panic. IME-composed text is treated as text input rather than hardcoded key commands where the terminal exposes it.

## Runtime and Message Workflows

- Stream text/thinking and update completed messages without duplicating runtime state.
- Render user/assistant/custom/session messages, Markdown/code, tool start/update/end, errors, compaction/branch summaries, and cache/usage/status information required by the approved matrix.
- Expand/collapse thinking and tool output through configurable actions.
- Show steering/follow-up queues, enqueue/dequeue, interrupt/abort, retry countdown/abort, compaction progress, and settled state.
- Preserve event ordering and late tool output behavior from the shared runtime.

## Commands and Dialogs

The selected parity matrix covers:

- slash command completion/invocation and reload;
- new/resume/fork/clone/switch/tree navigation, labels/name/delete/export;
- model selection/cycling/scoped models and thinking selection/cycling;
- settings/theme/tool/skills/context views needed for approved workflows;
- manual compaction and branch navigation;
- trust prompts for protected resources;
- auth login/logout and external integration select/confirm/input/editor/notify/status/widget/title requests through abstract services.

Dialogs never access credential store or integration processes directly. If provider/auth or integration implementations are not yet complete, deterministic fake services prove the UI contract; final integration occurs before takeover.

## Session Tree and Selectors

Tree, session, model, and settings selectors use shared data/services with search, sorting, filtering, paging, cancellation, and configurable actions. Destructive actions require the same confirmation/selection semantics and operate only on explicit targets.

Switch/new/fork operations use runtime rebind events so all components atomically replace session-derived state and no stale subscriptions survive.

## Images and Terminal Capabilities

Display inline images through detected supported terminal protocols when available, with bounded dimensions and cleanup. Unsupported terminals show a stable textual placeholder/metadata rather than corrupting layout. Clipboard/image paste uses a backend abstraction; platform-native implementations are validated in the takeover task.

Capability queries have bounded timeouts and cannot leave terminal responses in the editor input stream.

## Terminal Lifecycle

Production startup records terminal state, enters raw/alternate-screen modes as required, enables paste/mouse/keyboard protocols, and installs panic/signal cleanup. Every exit path restores modes, cursor, screen, input protocols, and signal handlers:

- normal exit;
- Ctrl+C/Ctrl+D/application action;
- provider/tool/runtime failure;
- SIGINT/SIGTERM/SIGHUP;
- suspend/resume where supported;
- panic/unwind and startup partial failure.

Repeated cleanup is idempotent. Child processes and runtime writers settle before final teardown where possible; emergency restoration remains bounded.

## Test Architecture

### Deterministic State Tests

Use Ratatui TestBackend/memory terminal or an equivalent virtual backend to feed input/events and assert reducer state, actions, cursor, visible semantic regions, overlays, and terminal commands. Snapshot cells may aid regressions but are never the sole oracle.

### Compatibility Fixtures

- key parser/bindings/defaults/migration/conflicts;
- Unicode/wide/emoji/regional indicators, wrap/resize/scroll, paste, undo, selection;
- streaming/thinking/tools/errors/queues/retry/compaction;
- session/model/tree/settings/theme/command/dialog workflows;
- images supported/unsupported paths;
- rebind/reload and terminal cleanup.

### tmux Smoke

Run controlled Linux tmux sessions at multiple sizes, start `pi-rs`, submit deterministic Faux/local prompts through injected test composition or local server configuration, exercise selected keys/dialogs, capture semantic panes/state, and kill the session. No public provider or real credential is used.

## Trade-offs and Rollback

- Behavioral parity permits a Rust-native layout/component architecture and avoids brittle screenshot identity.
- A reducer/test backend adds structure but makes terminal failures reproducible without tmux-only tests.
- Linux-first delivery defers platform console/clipboard/job-control differences to the takeover task.
- Rollback keeps `pi-rs` headless modes and the TypeScript interactive `pi`; no user data migration is required.

## Decisions Closed for Start Review

- TUI consumes shared runtime/services and owns rendering/input only.
- Stable action IDs and centralized defaults are compatibility contracts.
- Linux is the implementation baseline; cross-platform behavior is a later gate.
- Deterministic state tests are primary, tmux is integration smoke, screenshots are supplementary.
- Provider/auth and integration dialogs use injectable interaction services.
- Exact visual identity is out of scope.
