# Rust Cross-Platform Takeover Implementation Plan

## Gate

Do not start until `rust-provider-auth-parity`, `rust-rpc-integrations`, and `rust-tui` are all `complete`, this task's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Freeze Readiness and Platform Matrices

- Inventory all approved critical TypeScript/Rust behaviors, target triples, minimum OS/runtime assumptions, platform boundaries, artifacts, packaging, and rollback requirements.
- Create `takeover-report.md` with owners/evidence for every row.
- Record exact native runner/hardware and external authorization needs.

Gate: no target or behavior is represented only by “compiles.”

## Step 2: Portable Core and Platform Traits

- Audit existing target-specific code and extract narrow path/filesystem/process/terminal/clipboard/security adapters.
- Add platform contract fixtures before implementations.
- Keep provider/agent/runtime behavior portable and reject scattered unreviewed `cfg` workarounds.

## Step 3: Linux arm64 and Baseline Hardening

- Extend the gated Linux x64 workflow to Linux arm64.
- Validate linkage/minimum runtime, paths/permissions/locks, shell/process groups/signals, TUI, clipboard/images, RPC/integrations, and clean-machine archives on native arm64 evidence.

## Step 4: macOS x64/arm64

- Implement/validate paths, permissions/locks, process groups/signals, TLS/proxy, terminal input, clipboard/images, external editor, OAuth callbacks, archive/quarantine guidance, and both architectures on native runners.

## Step 5: Windows x64/arm64

- Implement/validate drive/UNC/long paths, case collisions, CRLF/BOM, job objects/process trees, console events/modes, shell semantics, file sharing/atomic writes/locks, secure credentials, clipboard/images, external editor, OAuth callbacks, and `.exe` archives.
- Treat every unresolved Windows limitation as a report blocker.

## Step 6: Artifact, Checksum, and Provenance Pipeline

- Build commit-named side-by-side `pi-rs` archives for all six targets.
- Generate/verify SHA256SUMS, build-info, lock/toolchain digests, notices, dependency policy, and CI provenance.
- Use pinned actions/minimum permissions and ordinary finite-retention artifacts only.
- Add signing/notarization only after separate policy/credential approval.

## Step 7: Full Native Compatibility and Clean-Machine Smoke

- Run local fixtures and representative native matrices for CLI, provider/auth, tools/images/processes, data/resources/sessions, RPC/integrations, and TUI.
- Extract/install outside repositories and exercise help/version/list, text/JSON/interactive/RPC, tools, sessions, trust, integrations, update, and rollback.
- Record exact full results per target/architecture.

## Step 8: Authorized Live and External Evidence

- Prepare the least-cost representative live-provider smoke with provider/model/prompt, credential source, redaction, cost, cleanup, and platforms.
- Execute only after explicit authorization and outside CI.
- Inspect/download remote native artifacts or protected signing evidence only with required authorization/access.

## Step 9: Final Readiness Decision

- Run all Rust checks, required TypeScript checks/tests, dependency policy, privacy scans, interoperability, and rollback drills for one candidate commit/artifact set.
- Complete READY/NOT READY/EXTERNAL EVIDENCE PENDING report.
- Run `trellis-check` before presenting the decision.
- Do not modify `pi`, public workflows, npm packaging, tags, or releases in this step.

## Step 10: Separately Authorized Packaging/Release Transition

Only after READY and explicit user authorization:

- design/review the `pi` entry-point and npm launcher/legacy TypeScript path change as an isolated patch;
- confirm `/cl` was run on latest `main` before release;
- run repository local release smoke outside the checkout, including required Node/Bun/Rust interactive and real-provider checks;
- review lockfile/shrinkwrap/artifact/release workflow diffs;
- use the single approved release script once;
- let CI trusted publishing handle npm; never run local `npm publish`;
- retain the documented TypeScript rollback distribution for at least one release.

Do not force push, rerun a release script after its tag is pushed, delete public history, or remove TypeScript SDK functionality without separate approval.

## Validation Commands

Exact target commands are fixed after toolchain/runner audit. Every target includes the applicable equivalents of:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
npm run check
./test.sh
```

Do not run `npm run build`, `npm test`, full Vitest, a live provider, release, tag, or push unless the specific owning step is explicitly authorized. Modified TypeScript tests run specifically with repository-required commands.

## Risk and Rollback Points

- Platform tests use disposable user/config/session roots and synthetic credentials.
- Native runner failures are preserved as evidence, not bypassed by cross-compiled success.
- Review target/native/build dependencies and licenses before lock changes.
- Keep side-by-side artifact names until takeover approval.
- Before takeover, rollback is immediate artifact disuse; after takeover, use the retained TypeScript legacy distribution and compatible data.

## Completion Evidence

- Six-target build/native/clean-machine matrix and checksummed provenance artifacts.
- Full critical behavior, security, interoperability, and rollback report.
- Explicit Windows resolution record.
- Authorized live-provider evidence required by final AC14.
- Separate approval record for any packaging, public release, tag/push, command replacement, or TypeScript removal.
