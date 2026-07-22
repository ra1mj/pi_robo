# Rust provider protocol adapters

## Goal

Implement the five milestone-1 provider protocol adapters behind the canonical Rust provider interface, using deterministic local fixtures as the primary gate.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../archive/2026-07/07-20-rust-foundation-contracts` is verified `completed`; this remains the hard prerequisite for task start.
- Implement OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Google Generative Language, and Faux adapters.
- Cover request mapping, streamed event decoding, tool calls/results, text and image content, usage, stop reasons, cancellation, retry classification, and normalized errors.
- Keep provider credentials and model selection outside adapter internals; accept resolved configuration through explicit interfaces.
- Use local HTTP/SSE fixtures for the required test matrix. Live-provider smoke tests require separate explicit authorization and credentials.
- `design.md` and `implement.md` were reviewed and accepted on 2026-07-21, and authorization to modify code was explicitly reconfirmed on 2026-07-22; `task.py start` is deferred only until the workflow enters implementation.

## Acceptance Criteria

- [ ] Each adapter passes success, tool-call, image-input, cancellation, malformed-stream, rate-limit, authentication-error, and retryable-error fixtures where the protocol supports them.
- [ ] Canonical events and usage match the parent compatibility contracts semantically.
- [ ] Adapter tests require no external network access, API keys, or paid tokens.
- [ ] Unknown or unsupported provider events produce explicit, typed failures instead of silent data loss.
- [ ] Provider-specific behavior remains isolated from the agent runtime and CLI.
- [ ] Interface fixtures prove typed request options, wakeable cancellation, closed error categories, structured retry metadata, response ID/model preservation, and typed tool-call/terminal events before HTTP transport work starts.

## Out of Scope

- OAuth flows and providers beyond the milestone-1 protocol set.
- CLI model/auth precedence, agent orchestration, sessions, TUI, or RPC.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the completion check above is mandatory.
