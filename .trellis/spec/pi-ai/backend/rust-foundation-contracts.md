# Rust Foundation Contracts

## 1. Scope / Trigger

Use this contract whenever a task changes `rust/**`, the Rust model catalog exporter, persisted session DTOs, or a TypeScript/Rust compatibility fixture. It prevents provider and runtime crates from redefining shared JSON shapes or silently dropping future fields.

## 2. Signatures

- Toolchain: Rust `1.97.0`, edition `2024`, resolver `3`.
- Catalog commands:
  - Generate: `npm run generate:rust-model-catalog`
  - Verify: `npm run check:rust-model-catalog`
- Persisted record API: `PersistedSessionRecord::parse(&str) -> Result<PersistedSessionRecord, ContractError>`.
- Model boundary: `ModelService::stream(ModelRequest, &dyn Cancellation) -> ModelFuture`.
- Compatibility validation: `validate_compatibility_catalog(&Path) -> Result<CompatibilityCatalog, Vec<String>>`.

## 3. Contracts

- `pi-protocol` owns shared JSON DTOs; downstream crates import them rather than defining provider-local copies.
- Known extensible objects use `#[serde(flatten)]` extension maps. Session persistence keeps both the raw JSON value and an optional typed record. Unknown session record kinds must round-trip unchanged.
- `rust/assets/models.json` is derived offline from `packages/ai/src/models.generated.ts`. Never edit either generated artifact by hand; update the owning generator and regenerate.
- Production crate direction is enforced by `workspace_policy.rs`. No production crate may depend on `pi-test-support`.
- Direct third-party crates are exact-pinned in `Cargo.toml`, locked in `Cargo.lock`, and recorded in `rust/DEPENDENCIES.md`.
- Foundation tests require no credentials or outbound provider access. Local HTTP tests bind only to `127.0.0.1`.

## 4. Validation & Error Matrix

| Condition | Required result |
| --- | --- |
| Invalid JSON | `ContractErrorCategory::InvalidJson` |
| Non-object or missing `type` | `InvalidShape` with a JSON path |
| Recognized session type with missing/invalid fields | `InvalidShape`; do not reinterpret as an unknown record |
| Unknown session type | Preserve raw value and return `known() == None` |
| Stale model artifact | `check:rust-model-catalog` fails |
| Duplicate compatibility ID, invalid owner, missing evidence, or root normalizer | Compatibility validation fails closed |
| Wildcard, unapproved license, git source, or registry | `cargo deny check` fails |
| Forbidden internal dependency edge | `workspace_policy` fails |

## 5. Good / Base / Bad Cases

- Good: a future field on a known session record is available in the typed extension map and survives serialization.
- Base: current v3 records decode to typed variants and reserialize to equivalent JSON.
- Bad: decoding a session entry into a closed struct with default Serde field dropping.
- Bad: fetching live catalog data from a Rust build or test.

## 6. Tests Required

- Protocol fixtures assert message, event, settings, model-catalog, and session round trips.
- Session tests assert malformed JSON, missing discriminants, malformed recognized records, unknown fields, and unknown record kinds.
- Test-support tests assert cancellation, bounded sinks, deterministic time/IDs/sleeps, fixture confinement, credential scanning, local-only HTTP, and normalization allowlists.
- Compatibility tests assert valid evidence and fail-closed invalid catalogs.
- Required gates: format, locked Clippy, locked tests, rustdoc warnings, `cargo deny check`, and `npm run check`.

## 7. Wrong vs Correct

Wrong:

```rust
#[derive(serde::Deserialize)]
struct SessionEntry {
    id: String,
}
```

This silently discards unknown persisted fields.

Correct:

```rust
#[derive(serde::Deserialize, serde::Serialize)]
struct SessionEntry {
    id: String,
    #[serde(default, flatten)]
    extensions: pi_protocol::Extensions,
}
```

For discriminated session records, also retain the original `serde_json::Value` through `PersistedSessionRecord`.
