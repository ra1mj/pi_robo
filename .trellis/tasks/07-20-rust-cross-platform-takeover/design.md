# Rust Cross-Platform Takeover Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-provider-auth-parity`, `../07-20-rust-rpc-integrations`, and `../07-20-rust-tui` must all be `complete` before this task starts. Through those tasks, shared runtime parity and milestone-1 acceptance are also required.

This task first produces a takeover-readiness decision. Changing the `pi` entry point, public release workflows, npm packaging, tags, pushes, or TypeScript distribution requires a separate explicit approval after readiness passes.

## Scope and Target Matrix

The final native artifact matrix is:

| Product target | Rust target baseline |
| --- | --- |
| Linux x64 | `x86_64-unknown-linux-gnu` |
| Linux arm64 | `aarch64-unknown-linux-gnu` |
| macOS x64 | `x86_64-apple-darwin` |
| macOS arm64 | `aarch64-apple-darwin` |
| Windows x64 | `x86_64-pc-windows-msvc` |
| Windows arm64 | `aarch64-pc-windows-msvc` |

Exact minimum OS/glibc/system requirements are measured and documented. Cross-compilation may build candidates, but final behavior/smoke evidence comes from representative native runners or explicitly equivalent hardware/virtual environments.

Windows was intentionally excluded from milestone 1. It remains a mandatory final target; unresolved Windows behavior is a takeover blocker, not a waived limitation.

## Platform Abstraction Audit

Before adding target-specific branches, inventory every platform-sensitive boundary:

- path normalization, separators, drive/UNC paths, symlinks, case sensitivity, temp/home/config directories;
- file permissions, atomic replacement, locks, deletion, rename, executable bits;
- shell selection, quoting, environment, process groups/job objects, timeouts, signals, child-tree cleanup;
- terminal raw mode, console modes, keyboard protocols, resize, suspend, clipboard, image display, external editor;
- proxy/TLS root certificates, DNS/IPv6, socket behavior, local OAuth callbacks;
- archive layout, executable suffix, line endings, installer/launcher/update behavior.

Common behavior stays in portable crates. Platform modules expose narrow traits and fixture-tested implementations; `cfg` branches do not spread through provider/agent/runtime logic.

## Native Behavior Requirements

### Linux

Validate x64/arm64 GNU linkage, baseline glibc/runtime libraries, process groups/signals, permissions/locks, shell fallback, terminal protocols, and artifact execution on clean supported images.

### macOS

Validate both architectures, universal assumptions only where explicit, Keychain/credential interaction if adopted, system roots/proxies, process groups/signals, paths/symlinks, clipboard/images, terminal input modifiers, quarantine/Gatekeeper documentation, and external editor behavior.

### Windows

Validate MSVC binaries, drive/UNC/long paths, case-insensitive collisions, CRLF/BOM behavior, named process/job-object tree cleanup, console control events/modes, no Unix suspend assumption, PowerShell/cmd or configured shell semantics, file sharing/rename/delete, ACL-aware secure credential handling, clipboard/images, and `.exe` packaging.

Security/permission guarantees are expressed per platform. Unix mode numbers are not falsely claimed on Windows; equivalent ACL/storage behavior must be documented and tested.

## Artifact and Provenance Contract

Until takeover approval, artifacts remain side-by-side and named `pi-rs-<platform>-<arch>` to avoid colliding with current `pi-*` release assets.

Every archive contains only required native runtime files, licenses/notices, documentation, and optional assets. It includes or is accompanied by:

- SHA-256 checksums;
- source commit and workspace version;
- exact Rust/Cargo/toolchain/target/profile;
- Cargo.lock digest and dependency/license report;
- build runner identity and provenance/attestation where available;
- reproducibility comparison for deterministic portions.

Signing/notarization is not assumed. If a reviewed release policy and credentials are available, signing is designed as a separate protected CI stage with minimum permissions and verification tests. Missing required signing for a chosen distribution channel becomes a readiness blocker rather than an improvised secret workflow.

## CI Workflow Design

Build/test jobs use native OS runners and a target matrix with independently diagnosable failures. New action references are pinned to full SHAs, permissions default to none/minimum, and caches are keyed by lockfile/toolchain/target without accepting untrusted executable artifacts across trust boundaries.

Before public approval, workflows only build, test, attest, and upload ordinary finite-retention Actions artifacts. They do not run tag-triggered publication, npm trusted publishing, GitHub Release creation, or secret-backed signing.

The existing TypeScript CI and `.github/workflows/build-binaries.yml` remain unchanged until a separately reviewed packaging/release transition.

## Full Compatibility Matrix

Takeover readiness re-runs the approved critical behavior matrix on representative native targets:

- headless text/JSON and complete CLI option/command surface;
- provider brands/protocols, API-key/ambient/OAuth auth, proxy/TLS, image generation;
- tools, images, shell/process cancellation, retry/compaction/queues;
- settings/models/auth/trust/keybindings, sessions/tree/export, resources/reload;
- RPC, command/MCP/HTTP/LSP integrations and migration examples;
- TUI input/editor/keybindings/dialogs/images/terminal cleanup;
- install/update/uninstall, artifact verification, configuration discovery, and rollback.

Not every provider needs a live call on every platform. Protocol behavior uses local fixtures everywhere; at least one representative live-provider smoke outside CI is required by the final parent gate and only runs with explicit authorization.

## Packaging and Command Transition

Three separately reviewed states are maintained:

1. `pi-rs` side-by-side artifacts, no user-facing replacement.
2. Candidate packaging/launcher that can select the native Rust binary while retaining current npm SDK packages and an explicit TypeScript legacy path.
3. Rust owns `pi` only after final approval and all readiness gates pass.

Any npm launcher conversion must preserve installation/version/update behavior while keeping programmatic TypeScript SDK exports available. It is not smuggled into a binary build change. Existing native archive names are changed only in the approved takeover commit.

The TypeScript CLI remains distributable under an explicit legacy name/path for at least one rollback release. No session/config downgrade or rewrite is required to switch back.

## Clean-Machine and Release Smoke

For every target, install/extract outside the repository on a clean environment and verify checksum, help, version, model/account listing, text, JSON, interactive startup/reply, RPC, tools, sessions, trust, integrations, update/rollback, and terminal cleanup with local fixtures.

The final authorized release follows repository release policy: changelog audit (`/cl` confirmation), unpublished local release smoke outside the repository, full required Node and Bun/Rust paths, one intended real-provider prompt, reviewed lock/shrinkwrap diffs, then the single release script. No local `npm publish`, release-script rerun after a pushed tag, force push, or manual partial publication.

These release actions are not executed merely because takeover readiness passes.

## Final Readiness Report

`takeover-report.md` maps every parent replacement criterion and platform row to commit-bound tests/artifacts. Results are:

- READY: all critical rows pass, rollback distribution/procedure is tested, and only explicit external approval remains.
- NOT READY: one or more product/platform/security/packaging rows fail; owning task and evidence are named.
- EXTERNAL EVIDENCE PENDING: signing, native hardware, live provider, or CI/release evidence needs authorization/access; this is not READY.

The report separately identifies excluded npm SDK/orchestrator/in-process extension compatibility so exclusions cannot be mistaken for missing implementation.

## Rollback

Before command takeover, rollback is stopping use of side-by-side `pi-rs` artifacts. After an approved release, rollback uses the retained TypeScript CLI distribution/legacy path and compatible data, plus documented package/launcher rollback. Public artifacts/tags are never deleted or rewritten as an ad-hoc rollback.

## Decisions Closed for Start Review

- Six OS/architecture targets are mandatory for final readiness.
- Native runner evidence is required for platform behavior.
- Windows failures block takeover despite milestone-1 exclusion.
- Checksums/provenance are mandatory; signing is conditional on reviewed policy/credentials.
- Side-by-side artifact staging precedes any command/package transition.
- Public release and `pi` replacement require separate explicit approval.
