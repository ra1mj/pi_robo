# Rust Provider Protocol Adapters Implementation Plan

## Gate

Do not start until `rust-foundation-contracts` is `complete`, this child's PRD/design/plan are reviewed, and `trellis-before-dev` has loaded current project specs. Inline implementation and checking only.

## Step 1: Refine Foundation Interfaces and Lock Dependencies

- Verify the archived `rust-foundation-contracts` task is `complete` and read its completion evidence and Rust code-spec.
- Capture exact protocol-neutral request options from the selected TypeScript types before extending `ModelRequest`.
- Add wakeable cancellation and typed/redacted error metadata with object-safety, cancellation-wakeup, and serialization tests.
- Add only the exact dependencies and features approved in `design.md`; refresh `Cargo.lock` and the dependency ledger.
- Review every new build script, native-code path, license, duplicate version, registry, and enabled feature before transport code.

Gate:

```bash
cargo clippy -p pi-model -p pi-provider -p pi-test-support --all-targets --all-features --locked -- -D warnings
cargo test -p pi-model -p pi-test-support --all-targets --locked
cargo deny check
```

## Step 2: Freeze Protocol Fixtures

- Read each selected TypeScript adapter and its focused tests in full before changing the owning Rust area.
- Add synthetic canonical request/event cases and local wire fixtures to the compatibility catalog.
- Mark entries `captured`; do not claim implementation before Rust assertions pass.
- Define the exact normalized error categories, retry metadata, and allowed nondeterminism.

Gate: fixture validation and secret scanning pass without network or credentials.

## Step 3: Shared HTTP and SSE Layer

- Use the exact-pinned HTTP/async dependencies locked in Step 1; do not expand features during transport implementation without repeating the review gate.
- Implement injected client construction, proxy/TLS/decompression behavior, bounded errors, timeouts, cancellation, and redaction.
- Implement incremental SSE parsing independently of provider logic.
- Add byte-split, keepalive, multiline, malformed, EOF, timeout, and cancellation tests.

```bash
cargo clippy -p pi-provider --all-targets --all-features --locked -- -D warnings
cargo test -p pi-provider --test transport_contract --locked
cargo test -p pi-provider --test sse_contract --locked
```

## Step 4: OpenAI Chat Completions

- Implement request mapping and streamed response accumulation.
- Cover roles/content, images, thinking/reasoning, cache controls, tools/results, IDs, usage, finish reasons, and errors.
- Test partial/multiple tool calls and output-limit blocking.

```bash
cargo test -p pi-provider --test openai_chat_contract --locked
```

## Step 5: OpenAI Responses

- Implement input/output-item mapping and response/reasoning replay identifiers.
- Cover text/reasoning/function-call deltas, images, empty results, usage, incomplete/error/terminal events, and abrupt EOF.

```bash
cargo test -p pi-provider --test openai_responses_contract --locked
```

## Step 6: Anthropic Messages

- Implement system/message/content mapping, images, tools/results, cache controls, thinking/redaction signatures, eager tool input, usage, stop reasons, and errors.

```bash
cargo test -p pi-provider --test anthropic_contract --locked
```

## Step 7: Google Generative Language

- Implement contents/parts, inline images, functions/results, tools, thought signatures, safety/blocked results, usage, stop reasons, and errors.

```bash
cargo test -p pi-provider --test google_contract --locked
```

## Step 8: Faux and Cross-Protocol Conformance

- Extend `pi-test-support` with scripted Faux implementation, fake time, cancellation, usage, cache estimates, and deterministic errors.
- Run one canonical semantic matrix across all applicable adapters.
- Prove provider-specific modules do not depend on CLI, agent, tool, persistence, or resource crates.

```bash
cargo test -p pi-test-support --test faux_contract --locked
cargo test -p pi-provider --test cross_protocol_contract --locked
```

## Step 9: Full Verification

```bash
cargo fmt --all --check
cargo clippy -p pi-provider -p pi-test-support --all-targets --all-features --locked -- -D warnings
cargo test -p pi-provider -p pi-test-support --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc -p pi-provider --no-deps --locked
cargo deny check
npm run check
```

Run every modified TypeScript test file specifically from `packages/ai`; do not run the e2e tests, `npm test`, or a public-provider smoke. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Add one protocol at a time; do not combine unfinished adapters into one compatibility claim.
- Review any new dependency/build script/native code before lockfile changes.
- Keep all base-URL overrides test-only or explicit model configuration; no hidden production endpoint.
- Rollback is removal/unregistration of `pi-provider` additions and fixtures. No existing `pi`, user data, release, or publish path changes.

## Completion Evidence

- Map each PRD criterion to named local fixture tests.
- Record supported and explicitly unsupported protocol features.
- Record dependency/license changes and secret-scan results.
- Leave `rust-headless-cli` blocked until this child, `rust-agent-tools`, and `rust-data-resources` are all complete.
