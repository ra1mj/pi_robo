# Rust dependency ledger

All direct third-party dependencies are pinned exactly in the root `Cargo.toml` and locked in `Cargo.lock`.

| Crate | Version | Source/license | Owner and purpose | Features/build/unsafe review |
| --- | --- | --- | --- | --- |
| `serde` | `1.0.229` | crates.io; MIT OR Apache-2.0 | `pi-protocol`, `pi-tools`: stable JSON contract serialization and strict tool input decoding | Default + `derive`; metadata/config build script; upstream contains unsafe internals but no native ABI |
| `serde_json` | `1.0.151` | crates.io; MIT OR Apache-2.0 | `pi-agent`, `pi-protocol`, `pi-runtime` tests, `pi-store`, `pi-test-support`, `pi-tools`: JSON values, fixtures, preservation tests, read-only settings/model/auth/trust inputs, session JSONL, schemas, and tool details | Default; metadata/config build script; upstream contains unsafe internals but no native ABI |
| `futures-core` | `0.3.33` | crates.io; MIT OR Apache-2.0 | `pi-model`, `pi-test-support`: runtime-neutral stream contract and scripted test streams | Default; no build script or native ABI; upstream contains unsafe internals |
| `futures-util` | `0.3.33` | crates.io; MIT OR Apache-2.0 | `pi-agent`, `pi-provider`: concurrent tool completion and streaming response combinators | Default (`std`, async/await); proc-macro dependency for combinators; no native ABI |
| `bytes` | `1.12.1` | crates.io; MIT | `pi-provider`: bounded incremental HTTP/SSE buffers | Default `std`; no build script or native ABI; upstream contains reviewed unsafe internals |
| `base64` | `0.22.1` | crates.io; MIT OR Apache-2.0 | `pi-provider`, `pi-tools`: provider image request encoding and canonical read-tool image blocks | Default `std`; no build script, native ABI, or unsafe code |
| `httpdate` | `1.0.3` | crates.io; MIT OR Apache-2.0 | `pi-provider`: HTTP-date form of `Retry-After` | Default; no build script, native ABI, or unsafe code |
| `tokio` | `1.53.1` | crates.io; MIT | `pi-agent`, `pi-provider`, `pi-store`, `pi-test-support`, `pi-tools`: bounded event delivery, async network/files/processes, credential-command output/time limits, synchronization, and cancellation signaling | `macros`, `rt-multi-thread`, `sync`, `time`, `net`, `io-util`, `fs`, `process`; proc-macro plus platform socket/process dependencies; no Tokio native ABI |
| `reqwest` | `0.13.4` | crates.io; MIT OR Apache-2.0 | `pi-provider`: shared streaming HTTP client, proxies, TLS, JSON, and decompression | Defaults off; `rustls`, `http2`, `system-proxy`, `stream`, `json`, `gzip`, `brotli`, `deflate`; Rustls selects AWS-LC, whose `aws-lc-sys` build compiles reviewed native C/assembly; no OpenSSL/native-tls |
| `image` | `0.25.10` | crates.io; MIT OR Apache-2.0 | `pi-tools`: bounded decode, resize, BMP normalization, and PNG/JPEG/GIF/WebP handling for `read` | Defaults off; only `bmp`, `gif`, `jpeg`, `png`, `webp`; no crate build script or native ABI. Transitive codecs are Rust-only; reviewed optimized crates contain internal unsafe code |
| `unicode-normalization` | `0.1.25` | crates.io; MIT OR Apache-2.0 | `pi-tools`: TypeScript-compatible NFKC normalization for fuzzy edit matching | Default `std`; no build script or native ABI; reviewed table/iterator internals contain no external native code |

## Reviewed Transitive Exceptions

- The TLS and Brotli paths add permissive BSD-3-Clause, ISC, MIT-0, and CDLA-Permissive-2.0 licenses. `deny.toml` allowlists only these identified SPDX licenses.
- `core-foundation` 0.9.4 is owned by Reqwest's `system-configuration -> hyper-util` proxy path; 0.10.1 is owned by its `rustls-platform-verifier` certificate path. Upstream requirements do not currently unify.
- `syn` 2.0.119 is owned by async/runtime and platform proc macros; 3.0.2 is owned by the exact-pinned Serde derive release. Both are build-time-only proc-macro parser versions.
- New build scripts come from proc-macro/configuration crates plus `aws-lc-sys`; only `aws-lc-sys` introduces native code, compiling the reviewed AWS-LC C/assembly TLS implementation selected by Reqwest's Rustls feature.
- The selected image feature set adds only Rust codecs. `num-traits` uses `autocfg` at build time; no image dependency compiles native code. `zune-jpeg` adds its permissive `Zlib` license, now explicitly allowlisted.
- Tokio's `process` feature adds the platform `signal-hook-registry`/`errno` path used for Linux process-group cleanup; it reuses the existing exact `libc` version and introduces no second version.

New dependencies require an exact version, a documented owner and purpose here, a reviewed lockfile diff, and passing `cargo deny check`.
