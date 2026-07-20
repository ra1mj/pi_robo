# Rust provider and authentication parity

## Goal

Expand the gated Rust core from milestone-1 protocols and API keys to the approved provider catalog and authentication parity required for eventual takeover.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-m1-gate` must be `complete` with a passing gate decision before this task starts.
- Inventory the current generated model/provider catalog from its generator source; never edit `packages/ai/src/models.generated.ts` directly.
- Cover the remaining known wire families (`mistral-conversations`, Azure/OpenAI Responses variants, OpenAI Codex Responses, Bedrock Converse Stream, Google Vertex, and `pi-messages`) plus provider-brand configuration rows over already supported families.
- Add provider adapters and authentication methods in reviewed, independently testable batches.
- Preserve canonical streaming, tool, image, usage, error, retry, and cancellation semantics across providers.
- Add OAuth only with explicit credential-storage, refresh, redaction, locking, and file-permission contracts.
- Implement the separate one-shot OpenRouter image-generation protocol and model/auth contracts without routing it through chat streaming.
- Keep direct dependencies pinned and refresh lock metadata using the repository's secure install rules.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] The approved provider/auth matrix has an owner, fixture source, support state, and passing evidence for every row.
- [ ] Protocol conformance and negative-path fixtures run without real credentials.
- [ ] Authorized live smoke tests cover representative providers without leaking secrets into logs or artifacts.
- [ ] OAuth refresh/storage tests cover expiry, revocation, corruption, locking, redaction, and secure permissions.
- [ ] OpenRouter image-generation fixtures cover request options, images, usage, errors, cancellation, and credential resolution independently from chat APIs.
- [ ] Unsupported providers/auth methods fail explicitly until their matrix row passes.

## Out of Scope

- RPC, external integrations, TUI, default-command takeover, and npm SDK compatibility.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the milestone-1 gate decision is mandatory.
