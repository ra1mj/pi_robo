# Rust Data, Resources, and Trust Implementation Plan

## Gate

Do not start until `rust-foundation-contracts` is `complete`, this child's PRD/design/plan are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Capture Data and Trust Contracts

- Read the selected settings, models, auth, trust, session, context, skill, and system-prompt source/tests in full.
- Build synthetic isolated fixtures and TypeScript contract runners where static JSON is insufficient.
- Record the corrected context-versus-protected-resource trust matrix in the compatibility catalog.
- Define exact read-only and append-only invariants before Rust storage code.

Gate: fixture roots are temporary, no real home/user data is accessed, and entries are `captured` rather than prematurely verified.

## Step 2: Paths and Settings

- Implement explicit agent-home/cwd/session path services and tilde/canonical normalization.
- Implement global settings parsing, trusted project merge, defaults, unknown compatible values, and diagnostics.
- Prove all settings operations are read-only and do not create lock files.

```bash
cargo test -p pi-store --test paths_contract --locked
cargo test -p pi-store --test settings_contract --locked
```

## Step 3: Model Sources

- Load the committed generated catalog.
- Implement comment-compatible `models.json`, overrides, custom providers, and selected-protocol validation.
- Add drift/schema/version and unsupported-brand diagnostics without editing generated TypeScript model source.

```bash
cargo test -p pi-store --test models_contract --locked
```

## Step 4: Auth and Value Resolution

- Implement raw `auth.json` loading with API-key typed views and untouched OAuth values.
- Implement CLI/auth/environment/models precedence.
- Implement literals, interpolation, escapes, provider-scoped environment, and explicit command resolution through injected process/cancellation seams.
- Add redaction, bounded-output, failure, timeout, and unsupported-OAuth tests.

```bash
cargo test -p pi-store --test auth_contract --locked
cargo test -p pi-store --test config_value_contract --locked
```

## Step 5: Trust

- Implement canonical/ancestor saved-decision lookup and caller-supplied CLI/default decisions.
- Implement non-interactive `ask` as skip for protected project settings/skills.
- Prove context-file loading remains independent unless explicitly disabled.

```bash
cargo test -p pi-store --test trust_matrix --locked
```

## Step 6: Session-v3 Store

- Implement raw-line/raw-value parsing, known typed views, current-leaf context reconstruction, lookup, and diagnostics.
- Implement append-only new entries with injected IDs/timestamps and no prior-line rewrite.
- Implement in-process append serialization and best-effort stale external-change detection without claiming a shared lock.
- Add bidirectional TypeScript/Rust create/read/append/rollback fixtures, malformed lines, unknown entries/fields, legacy refusal, and concurrent-writer diagnostics.

```bash
cargo test -p pi-store --test session_v3_contract --locked
cargo test -p pi-store --test typescript_interop --locked
```

## Step 7: Context, Skills, and Prompt Inputs

- Implement global/ancestor context discovery and supported filename ordering.
- Implement explicit/settings/global/trusted-project skill discovery, frontmatter, ignores, collisions, disable-model-invocation, and diagnostics.
- Implement normalized system-prompt inputs without CLI or provider coupling.

```bash
cargo clippy -p pi-resources --all-targets --all-features --locked -- -D warnings
cargo test -p pi-resources --test context_contract --locked
cargo test -p pi-resources --test skills_contract --locked
cargo test -p pi-resources --test system_prompt_contract --locked
cargo test -p pi-resources --test trust_matrix --locked
```

## Step 8: Full Verification

```bash
cargo fmt --all --check
cargo clippy -p pi-store -p pi-resources --all-targets --all-features --locked -- -D warnings
cargo test -p pi-store -p pi-resources --all-targets --locked
cargo test -p pi-store --test typescript_interop --locked
cargo test -p pi-resources --test trust_matrix --locked
cargo deny check
npm run check
```

Run each modified TypeScript test specifically with the repository-prescribed package-local Vitest command. Do not run `npm test`, full Vitest, real credential commands, or real home-directory fixtures. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Snapshot original fixture bytes before every interoperability operation and prove read-only files remain byte-unchanged.
- Run credential commands only through synthetic local executables/scripts in temporary directories.
- Never add session locking that claims compatibility with a TypeScript writer.
- Never write settings/auth/trust/models in milestone 1; future locking belongs to its owning later task.
- Rollback removes Rust store/resource additions. Existing TypeScript files and prior session lines remain usable and unchanged.

## Completion Evidence

- Map every PRD criterion to named fixture tests and byte-preservation evidence.
- Record unsupported legacy/OAuth/concurrent-write cases with actionable diagnostics.
- Record the trust matrix, especially context loading versus protected project resources.
- Leave `rust-headless-cli` blocked until provider and agent/tool siblings also complete.
