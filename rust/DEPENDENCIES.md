# Rust dependency ledger

All direct third-party dependencies are pinned exactly in the root `Cargo.toml` and locked in `Cargo.lock`.

| Crate | Version | Source/license | Owner and purpose | Features/build/unsafe review |
| --- | --- | --- | --- | --- |
| `serde` | `1.0.229` | crates.io; MIT OR Apache-2.0 | `pi-protocol`: stable JSON contract serialization | Default + `derive`; metadata/config build script; upstream contains unsafe internals but no native ABI |
| `serde_json` | `1.0.151` | crates.io; MIT OR Apache-2.0 | `pi-agent`, `pi-protocol`, `pi-test-support`: JSON values, fixtures, and preservation tests | Default; metadata/config build script; upstream contains unsafe internals but no native ABI |
| `futures-core` | `0.3.33` | crates.io; MIT OR Apache-2.0 | `pi-model`, `pi-test-support`: runtime-neutral stream contract and scripted test streams | Default; no build script or native ABI; upstream contains unsafe internals |
| `futures-util` | `0.3.33` | crates.io; MIT OR Apache-2.0 | `pi-provider`: streaming response combinators | Default (`std`, async/await); proc-macro dependency for combinators; no native ABI |
| `bytes` | `1.12.1` | crates.io; MIT | `pi-provider`: bounded incremental HTTP/SSE buffers | Default `std`; no build script or native ABI; upstream contains reviewed unsafe internals |
| `base64` | `0.22.1` | crates.io; MIT OR Apache-2.0 | `pi-provider`: provider image request encoding | Default `std`; no build script, native ABI, or unsafe code |
| `httpdate` | `1.0.3` | crates.io; MIT OR Apache-2.0 | `pi-provider`: HTTP-date form of `Retry-After` | Default; no build script, native ABI, or unsafe code |
| `tokio` | `1.53.1` | crates.io; MIT | `pi-provider`, `pi-test-support`: async network runtime, timeouts, synchronization, and cancellation test signaling | `macros`, `rt-multi-thread`, `sync`, `time`, `net`, `io-util`; proc-macro plus platform socket dependencies; no Tokio native ABI |
| `reqwest` | `0.13.4` | crates.io; MIT OR Apache-2.0 | `pi-provider`: shared streaming HTTP client, proxies, TLS, JSON, and decompression | Defaults off; `rustls`, `http2`, `system-proxy`, `stream`, `json`, `gzip`, `brotli`, `deflate`; Rustls selects AWS-LC, whose `aws-lc-sys` build compiles reviewed native C/assembly; no OpenSSL/native-tls |

## Reviewed Transitive Exceptions

- The TLS and Brotli paths add permissive BSD-3-Clause, ISC, MIT-0, and CDLA-Permissive-2.0 licenses. `deny.toml` allowlists only these identified SPDX licenses.
- `core-foundation` 0.9.4 is owned by Reqwest's `system-configuration -> hyper-util` proxy path; 0.10.1 is owned by its `rustls-platform-verifier` certificate path. Upstream requirements do not currently unify.
- `syn` 2.0.119 is owned by async/runtime and platform proc macros; 3.0.2 is owned by the exact-pinned Serde derive release. Both are build-time-only proc-macro parser versions.
- New build scripts come from proc-macro/configuration crates plus `aws-lc-sys`; only `aws-lc-sys` introduces native code, compiling the reviewed AWS-LC C/assembly TLS implementation selected by Reqwest's Rustls feature.

New dependencies require an exact version, a documented owner and purpose here, a reviewed lockfile diff, and passing `cargo deny check`.
