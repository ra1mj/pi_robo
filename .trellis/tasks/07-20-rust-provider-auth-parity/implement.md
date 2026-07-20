# Rust Provider and Authentication Parity Implementation Plan

## Gate

Do not start until `rust-m1-gate` has a written PASS, this task's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Generate and Freeze the Matrix

- Inspect the current generator, provider registry, auth flows, image registry, and focused tests in full.
- Generate the candidate provider/protocol/auth/image matrix with ownership and fixture paths.
- Separate already verified milestone-1 rows from missing brand/protocol/auth behavior.
- Record exact dependency/build-script/native/cloud-SDK risks before adding crates.

Gate: every current approved row has a planned slice and unsupported reason; generated TypeScript model source is untouched.

## Step 2: Existing-Protocol Provider Brands

- Add brand endpoint/header/auth/model-override fixtures over the four gated wire protocols.
- Enable rows one by one only after local success and negative cases pass.
- Keep unsupported brands explicit in list/selection diagnostics.

## Step 3: New Protocol Slices

Implement and gate in this order unless evidence reveals a dependency requiring plan revision:

1. Mistral Conversations.
2. Azure/OpenAI Responses variants.
3. OpenAI Codex Responses and approved transports.
4. Google Vertex.
5. Bedrock Converse Stream.
6. `pi-messages`.

For each slice:

```bash
cargo clippy -p pi-provider --all-targets --all-features --locked -- -D warnings
cargo test -p pi-provider --test <protocol>_contract --locked
cargo test -p pi-provider --test <protocol>_negative --locked
```

No slice advances if tool/image/thinking/usage/error/cancellation semantics remain partial.

## Step 4: API-Key and Ambient Auth Matrix

- Implement provider-scoped keys/env, command values, cloud files/profiles/ADC, keyless local sources, availability checks, and request auth.
- Test stored-credential ownership and no silent ambient fallback after stored credential failure.

```bash
cargo test -p pi-model --test provider_auth_matrix --locked
```

## Step 5: Compatible Credential Writes

- Implement lock-compatible read-modify-write/delete, staleness/ownership handling, unknown record preservation, atomic replacement, and Linux permissions.
- Add two-process TypeScript/Rust contention, refresh, login/logout race, crash/stale-lock, corruption, and permission tests in isolated homes.

```bash
cargo test -p pi-store --test credential_store_contract --locked
cargo test -p pi-store --test credential_store_interop --locked
```

## Step 6: OAuth Flows

- Implement provider-neutral interactions and each current flow family in independently tested batches.
- Use fake token/device/callback authorities and fake time for login, expiry, refresh, invalid grant, revoke/logout, cancellation, and state/PKCE tests.
- Prove double-checked refresh performs one global rotation under contention.

```bash
cargo test -p pi-model --test oauth_contract --locked
cargo test -p pi-model --test oauth_refresh_concurrency --locked
```

## Step 7: OpenRouter Image Generation

- Add the separate image model registry/service and OpenRouter request/result mapper.
- Cover options, generated image payloads, usage, response IDs, stop/errors, headers, auth, timeout, malformed responses, and cancellation.

```bash
cargo test -p pi-provider --test openrouter_images_contract --locked
```

## Step 8: Matrix and Security Gate

- Run all local protocol/auth/image conformance and secret scans.
- Ensure only verified rows register as supported and matrix/catalog drift checks pass.
- Record any live-smoke plan with provider, scope, cost, credential handling, and cleanup; execute only after explicit authorization.

## Step 9: Full Verification

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
npm run check
```

Run every modified TypeScript provider/auth/image test specifically with the required package-local Vitest command. Do not run `npm test`, full Vitest, or any e2e/live test without explicit authorization. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Review dependency and lockfile changes as code; exact pins only, no unreviewed git/build/native sources.
- Keep cloud SDK/default credential chains out of tests unless fully injected and isolated.
- Never expose credentials in test failures or process arguments where avoidable.
- Enable registry rows incrementally; rollback disables the affected Rust row/flow without changing TypeScript providers.
- Auth file schema remains compatible, so rollback requires no credential migration.

## Completion Evidence

- Final generated matrix with every approved row verified or explicitly blocking completion.
- Local conformance and security results for every protocol/auth/image slice.
- Cross-process auth storage/refresh interoperability and permission evidence.
- Authorized representative live-smoke results when required for completion.
- Leave final takeover blocked until RPC/integrations and TUI siblings also complete.
