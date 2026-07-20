# Rust Shared Runtime and Resource Parity Implementation Plan

## Gate

Do not start until `rust-m1-gate` has a written PASS, this task's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Capture Shared Behavior

- Read current queue, compaction/branch, session tree, settings/trust/keybinding, resource, command, reload, and export source/tests in full.
- Capture TypeScript fixtures and service-level event/action/state contracts.
- Define which operations are shared and reject any RPC/TUI-specific rendering/framing concerns.

## Step 2: Queues and Runtime Actions

- Implement steering/follow-up queues/modes, lifecycle events, idle/settled state, prompt/abort controls, and session rebind semantics.
- Cover all/one-at-a-time, concurrent enqueue, cancellation, compaction, switch, reload, and settlement.

```bash
cargo test -p pi-runtime --test queue_contract --locked
cargo test -p pi-runtime --test session_rebind_contract --locked
```

## Step 3: Manual Compaction and Branch Behavior

- Add manual compaction, branch summarization/navigation, cancellation, persistence, and conflicting-operation serialization through shared services.

```bash
cargo test -p pi-runtime --test manual_compaction_contract --locked
cargo test -p pi-runtime --test branch_contract --locked
```

## Step 4: Session Tree and Export

- Implement list/resume/search/tree/entries/stats/last text, fork/clone/switch/new/navigate, labels/names/delete, and safe HTML export.
- Add bidirectional TypeScript/Rust fixtures and unknown/raw preservation checks.

```bash
cargo test -p pi-store --test session_tree_contract --locked
cargo test -p pi-store --test session_operations_interop --locked
cargo test -p pi-resources --test export_html_contract --locked
```

## Step 5: Settings, Trust, and Keybinding Writes

- Implement compatible lock/read-modify-write/atomic replace and resource-specific validation/permissions.
- Run multi-process TypeScript/Rust contention, crash/stale/compromise, callback failure, and unknown-field tests.

```bash
cargo test -p pi-store --test mutable_json_contract --locked
cargo test -p pi-store --test proper_lockfile_interop --locked
```

## Step 6: Prompt Templates, Themes, and Package Resources

- Implement discovery, parsing, trust, source metadata, collisions, diagnostics, substitutions, and typed theme catalog.
- Implement package-managed declarative discovery only; no install or execution.
- Implement atomic reload and combined command registry.

```bash
cargo test -p pi-resources --test prompt_template_contract --locked
cargo test -p pi-resources --test theme_catalog_contract --locked
cargo test -p pi-resources --test package_resource_contract --locked
cargo test -p pi-resources --test reload_contract --locked
```

## Step 7: Full Verification

```bash
cargo fmt --all --check
cargo clippy -p pi-runtime -p pi-store -p pi-resources --all-targets --all-features --locked -- -D warnings
cargo test -p pi-runtime -p pi-store -p pi-resources --all-targets --locked
cargo deny check
npm run check
```

Run each modified TypeScript test specifically with the repository-required package-local Vitest invocation. Do not run `npm test`, full Vitest, real home resources, or executable packages. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Destructive session tests operate only on disposable copied session directories.
- Lock tests use isolated files/processes and never actual user settings/trust/keybindings.
- Export/template fixtures contain synthetic untrusted content and assert escaping.
- Do not add RPC/TUI dependencies to shared crates.
- Rollback removes/disables later shared features while preserving M1 paths and existing TypeScript files.

## Completion Evidence

- Shared state/action API and event trace evidence.
- Session tree/export interoperability and rollback record.
- Cross-process mutable resource lock/permission evidence.
- Prompt/theme/package/reload matrices.
- Written confirmation that `rust-rpc-integrations` and `rust-tui` are now independently unblocked.
