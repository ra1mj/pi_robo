# Rust dependency ledger

All direct third-party dependencies are pinned exactly in the root `Cargo.toml` and locked in `Cargo.lock`.

| Crate | Version | Source/license | Owner and purpose | Features/build/unsafe review |
| --- | --- | --- | --- | --- |
| `serde` | `1.0.229` | crates.io; MIT OR Apache-2.0 | `pi-protocol`: stable JSON contract serialization | Default + `derive`; metadata/config build script; upstream contains unsafe internals but no native ABI |
| `serde_json` | `1.0.151` | crates.io; MIT OR Apache-2.0 | `pi-agent`, `pi-protocol`, `pi-test-support`: JSON values, fixtures, and preservation tests | Default; metadata/config build script; upstream contains unsafe internals but no native ABI |
| `futures-core` | `0.3.33` | crates.io; MIT OR Apache-2.0 | `pi-model`, `pi-test-support`: runtime-neutral stream contract and scripted test streams | Default; no build script or native ABI; upstream contains unsafe internals |

New dependencies require an exact version, a documented owner and purpose here, a reviewed lockfile diff, and passing `cargo deny check`.
