# Rust Terminal UI Implementation Plan

## Gate

Do not start until `rust-runtime-parity` is `complete`, this task's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Freeze the Interactive Matrix

- Read current keybinding, terminal/editor/component, interactive workflow, and focused tests in full.
- Map approved actions/commands/dialogs to shared runtime services and mark visual-only differences out of contract.
- Capture key sequences, semantic state transitions, cursor/layout invariants, and teardown fixtures.
- Review exact TUI/terminal dependencies, features, unsafe/native/build behavior, and licenses before lockfile changes.

## Step 2: Application State and Test Backend

- Implement reducer/state, canonical event subscriptions, action dispatch, controller interfaces, and memory/test terminal.
- Cover streaming, rebind, reload, error, late event, and settlement without production terminal I/O.

```bash
cargo clippy -p pi-tui --all-targets --all-features --locked -- -D warnings
cargo test -p pi-tui --test app_state_contract --locked
```

## Step 3: Input Parser, Keybindings, and Editor

- Implement legacy/Kitty key parsing, central action defaults, user config/migration/conflicts, paste/resize, Unicode editor, selection, kill ring, undo, autocomplete, history, external editor, and image paste abstractions.
- Prohibit hardcoded action-specific key checks outside the registry/parser.

```bash
cargo test -p pi-tui --test keys_contract --locked
cargo test -p pi-tui --test keybindings_contract --locked
cargo test -p pi-tui --test editor_contract --locked
```

## Step 4: Messages, Tools, Queues, and Status

- Implement semantic rendering for messages/Markdown/code/thinking/tools/images/errors/compaction/branch summaries and status/footer.
- Implement expand/collapse, queues, retry/abort, streaming updates, scrolling, and late-output behavior.

```bash
cargo test -p pi-tui --test message_contract --locked
cargo test -p pi-tui --test runtime_workflows --locked
```

## Step 5: Commands, Selectors, Trees, and Dialogs

- Implement slash completion/invocation, session/model/thinking/settings/theme/tool selectors, tree workflows, destructive confirmations, trust prompts, and generic auth/integration UI interactions.
- Use fake interaction providers until owning sibling implementations are available; do not add provider/auth/integration behavior to `pi-tui`.

```bash
cargo test -p pi-tui --test command_contract --locked
cargo test -p pi-tui --test selector_tree_contract --locked
cargo test -p pi-tui --test interaction_dialog_contract --locked
```

## Step 6: Production Terminal Backend and Images

- Implement raw/alternate screen, capabilities, cursor, resize/focus/paste/mouse/keyboard protocols, inline image fallback, signals, suspend/resume abstraction, and idempotent cleanup.
- Add panic/partial-startup/provider/tool/signal teardown tests using pseudo terminals where possible.

```bash
cargo test -p pi-tui --test terminal_backend_contract --locked
cargo test -p pi-tui --test terminal_cleanup --locked
```

## Step 7: CLI Integration and tmux Smoke

- Enable interactive mode in `pi-rs` for TTY startup/initial prompts without changing text/JSON behavior.
- Run controlled tmux workflows for prompt, cancel, model/session selection, queues, tool/thinking expansion, resize, dialogs, and teardown at representative sizes.
- Use Faux through test composition or loopback provider fixtures only.

## Step 8: Full Verification

```bash
cargo fmt --all --check
cargo clippy -p pi-tui -p pi-cli --all-targets --all-features --locked -- -D warnings
cargo test -p pi-tui -p pi-cli --all-targets --locked
cargo deny check
npm run check
```

Run every modified TypeScript TUI/coding-agent test specifically with the repository-required package-local Vitest invocation. Do not run `npm test`, full Vitest, public providers, or real credentials. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Review terminal crates and any unsafe/native dependencies before addition.
- Keep production terminal tests isolated in pseudo terminals/tmux and always execute cleanup.
- Avoid snapshot-only acceptance and platform-specific assumptions in reducer/components.
- Do not modify key behavior through hardcoded raw keys.
- Rollback disables interactive Rust mode while retaining headless `pi-rs` and TypeScript interactive `pi`.

## Completion Evidence

- Approved interactive workflow/action matrix with deterministic tests.
- Unicode/editor/keybinding/terminal/image and cleanup evidence.
- tmux smoke records and session interoperability results.
- Explicit list of platform-specific items deferred to takeover.
- Leave final takeover blocked until provider/auth and RPC/integrations siblings also complete.
