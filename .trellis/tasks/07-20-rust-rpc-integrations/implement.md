# Rust RPC and External Integrations Implementation Plan

## Gate

Do not start until `rust-runtime-parity` is `complete`, this task's artifacts are reviewed, and `trellis-before-dev` has loaded current specs. Inline implementation/checking only.

## Step 1: Capture RPC and Extension Capability Matrices

- Read current RPC source/docs/tests and extension capability contracts in full.
- Generate golden fixtures for every documented RPC command/response/event/UI message and negative framing case.
- Build the extension-to-external-protocol migration matrix with explicit unsupported rows.
- Pin official MCP/LSP versions and review any protocol/client dependency before addition.

Gate: no capability is labeled compatible without an owning protocol and fixture.

## Step 2: Verify Shared Runtime Services Required by RPC

- Verify steering/follow-up queues and modes, manual compaction, retry controls, session tree/fork/clone/switch/name/stats, direct bash, and command enumeration through the completed shared service interfaces.
- Add RPC-facing adapters only; missing shared behavior returns to `rust-runtime-parity` instead of being implemented here.

```bash
cargo test -p pi-rpc --test runtime_service_contract --locked
```

## Step 3: JSONL RPC Framing and Dispatcher

- Implement bounded newline framing, parsing, command validation/dispatch, ID correlation, response/event writers, backpressure, EOF, signals, and stderr diagnostics.
- Port command/response/event semantics and extension UI wire messages without adding a mandatory handshake.

```bash
cargo clippy -p pi-rpc --all-targets --all-features --locked -- -D warnings
cargo test -p pi-rpc --test framing_contract --locked
cargo test -p pi-rpc --test command_contract --locked
cargo test -p pi-rpc --test event_contract --locked
```

## Step 4: Manifest and Capability Core

- Implement schema-versioned manifest parsing, source/provenance, requested/granted capabilities, diagnostics, discovery, enablement, and trust gating.
- Prove discovery has no process/network side effect and unsupported versions/capabilities fail explicitly.

```bash
cargo test -p pi-integrations --test manifest_contract --locked
cargo test -p pi-integrations --test trust_matrix --locked
```

## Step 5: Command and HTTP Adapters

- Implement supervised JSONL command peer and structured HTTP/stream adapter.
- Cover process groups, env/secret handling, request IDs, timeouts, cancellation, backpressure, malformed output, disconnect, and cleanup.

```bash
cargo test -p pi-integrations --test command_contract --locked
cargo test -p pi-integrations --test http_contract --locked
```

## Step 6: MCP and LSP Adapters

- Implement the pinned MCP negotiation/transport subset and LSP Content-Length JSON-RPC lifecycle.
- Map only approved capabilities and reject unsupported claims.
- Use fake peers/processes for tools/resources/prompts, diagnostics, requests, cancellation, shutdown, and crash behavior.

```bash
cargo test -p pi-integrations --test mcp_contract --locked
cargo test -p pi-integrations --test lsp_contract --locked
```

## Step 7: UI Bridge and Migration Examples

- Route external structured interaction requests through the canonical UI service and current RPC UI wire messages.
- Cover absent client, timeout, cancel, duplicate/unknown IDs, disconnect, and secret redaction.
- Add concise migration examples for tool, command, event hook, HTTP, MCP, and LSP patterns plus unsupported UI/plugin cases.

```bash
cargo test -p pi-rpc --test integration_ui_contract --locked
```

## Step 8: Integration Package Lifecycle

- Implement supported source fetching/resolution, integrity checks, staged validation, atomic enable/remove/update/list/config, and rollback through shared settings/resources.
- Prove lifecycle scripts and JavaScript entry points are never executed.
- Add migration diagnostics for legacy packages and path-containment tests for removal.

```bash
cargo test -p pi-integrations --test package_lifecycle_contract --locked
```

## Step 9: Full Verification

```bash
cargo fmt --all --check
cargo clippy -p pi-runtime -p pi-store -p pi-rpc -p pi-integrations --all-targets --all-features --locked -- -D warnings
cargo test -p pi-runtime -p pi-store -p pi-rpc -p pi-integrations --all-targets --locked
cargo deny check
npm run check
```

Run every modified TypeScript RPC/extension test specifically with the required package-local Vitest command. Do not run `npm test`, full Vitest, public integrations, or real credentials. Run `trellis-check` before completion review.

## Risk and Rollback Points

- Protocol parser/process/network dependencies receive exact-pin, license, build-script, and unsafe review.
- Tests use temporary roots, fake peers, loopback only, and sanitized environments.
- Never pass secrets on argv unless the external protocol makes no safer channel possible and the risk is explicitly reviewed.
- No legacy TypeScript extension is executed by Rust.
- Rollback removes/disables Rust RPC/integrations while preserving the gated headless core and TypeScript distribution.

## Completion Evidence

- Golden parity for every current RPC command/response/event and negative framing case.
- Versioned manifest and command/MCP/HTTP/LSP lifecycle/trust evidence.
- Published migration matrix with unsupported capabilities explicit.
- Process/network cleanup, redaction, and rollback results.
- Leave final takeover blocked until provider/auth and TUI siblings also complete.
