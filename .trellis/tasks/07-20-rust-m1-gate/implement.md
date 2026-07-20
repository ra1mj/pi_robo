# Rust Milestone 1 Acceptance Gate Execution Plan

## Gate

Do not start until `rust-headless-cli` is `complete`, this task's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline verification only. This task does not fix product code.

## Step 1: Freeze the Candidate

- Record source status/commit, diff digest when applicable, exact Rust/Cargo versions, Cargo.lock digest, compatibility catalog revision, and child-task evidence.
- Verify all milestone children report complete with their named validation outputs.
- Refuse final PASS for an uncommitted candidate or an artifact not tied to the recorded commit.
- Request explicit authorization before any commit, push, workflow rerun, or remote artifact download not already available locally.

## Step 2: Build the Acceptance Matrix

- Create `gate-report.md` with AC1-AC12 and AC15 rows, evidence owners, commands/artifacts, and initial state.
- Audit exclusions and dependency graph before running behavior tests.
- Reject duplicate, missing, stale, or falsely `verified` compatibility entries.

Gate: every criterion has a concrete evidence path and failure owner.

## Step 3: Clean Rust and Policy Verification

Use a fresh temporary target directory instead of destructive repository cleanup.

```bash
cargo fmt --all --check
CARGO_TARGET_DIR=/tmp/pi-rs-m1-target cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
CARGO_TARGET_DIR=/tmp/pi-rs-m1-target cargo test --workspace --all-targets --locked
CARGO_TARGET_DIR=/tmp/pi-rs-m1-target RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
```

Record exact full output and candidate identity. No `tail`-only evidence.

## Step 4: Contract and Negative Matrices

- Run selected protocol local-server tests, Faux traces, agent/tool/runtime tests, image fixtures, and malformed/error/cancellation cases.
- Run TypeScript/Rust four-tool contract runners on cloned temporary trees.
- Confirm normalized JSON event lifecycle and no over-broad normalizers.
- Confirm all tests operate without provider credentials or public endpoints.

Failures are assigned to provider or agent/tools children; do not patch here.

## Step 5: Data, Trust, and Interoperability

- Run settings/models/auth/trust/context/skills matrices in isolated homes/projects.
- Execute both TypeScript-to-Rust and Rust-to-TypeScript v3 session read/append/reopen/rollback flows.
- Compare original configuration/OAuth bytes and prior session lines.
- Exercise stale-writer diagnostics without claiming concurrent-writer safety.

Failures are assigned to data/resources or protocol-foundation children.

## Step 6: Release Binary Outside the Repository

- Build the exact Linux x64 release candidate with `--locked` into the temporary target.
- Stage archive/checksum/build-info and copy/unpack under `/tmp/pi-rs-m1-smoke` or another disposable external directory.
- Use synthetic PATH/home/config/session values and local mock servers.
- Smoke help, version, offline model listing, text, JSON, tools, images, sessions, retry, compaction, errors, cancellation, signal cleanup, and checksum/build metadata.
- Record native linkage/runtime requirements and prove no Node.js/Bun/workspace dependency.

```bash
CARGO_TARGET_DIR=/tmp/pi-rs-m1-target cargo build --release --locked -p pi-cli --target x86_64-unknown-linux-gnu
```

Failures are assigned to `rust-headless-cli` or the owning lower layer.

## Step 7: Existing TypeScript Regression

```bash
npm run check
./test.sh
```

Run with full output. Never run `npm run build`, `npm test`, or the full Vitest suite directly. Also rerun every TypeScript test modified by a milestone child using its required package-local command.

Any new regression is a gate failure even when Rust tests pass.

## Step 8: CI Artifact and Workflow Audit

After explicit authorization and a commit-bound candidate exists:

- inspect the exact Rust workflow run and job logs;
- verify action SHAs, permissions, triggers, no secret/public-provider use, and no release/publish job;
- download the normal Actions artifact;
- verify archive, SHA256SUMS, build-info, source commit, and retention metadata;
- rerun the outside-checkout smoke against the downloaded artifact.

Do not trigger or rerun `.github/workflows/build-binaries.yml`, publish npm, create a release, or push a tag.

## Step 9: Rollback Drill

- Stop using the copied `pi-rs` binary.
- Reopen the copied interoperable session with the existing TypeScript `pi` path.
- Verify no settings/auth/trust/models migration or restore step is needed.
- Verify npm bins, release scripts/assets, and default `pi` remain unchanged.
- Record commands and results in the report.

## Step 10: Decision and Handoff

- Complete every evidence row and security/privacy scan.
- Record PASS only if all required rows pass for the same candidate/artifact.
- On failure, leave this task incomplete and reopen/name the owning child with the exact evidence; do not weaken the contract.
- Run `trellis-check` over task artifacts and repository state before presenting the decision.
- Without the written PASS, do not unblock `rust-runtime-parity` or `rust-provider-auth-parity`; RPC and TUI additionally remain blocked until runtime parity completes.

## Risk Controls

- All data and filesystem operations use disposable isolated roots.
- No live provider, credential, paid token, public release, npm publication, tag, or command replacement.
- No destructive git command, worktree switch, or unrelated-file staging.
- No product fix is made in the verification task.
- Temporary directories may be removed only after evidence is safely recorded; repository/user files are never cleaned as part of the gate.

## Completion Evidence

- `gate-report.md` with AC1-AC12/AC15 mappings and candidate identity.
- Full command/test outputs or stable CI/artifact links where authorized.
- Artifact checksum/build-info/linkage and privacy scan results.
- TypeScript/Rust session interoperability and rollback record.
- Written PASS or FAIL decision with next allowed task(s).
