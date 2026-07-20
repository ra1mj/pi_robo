# Rust RPC and external integrations

## Goal

Implement behavioral parity for the supported RPC protocol and external integration surfaces on top of the gated Rust core.

## Requirements

- Parent task: `../07-17-rust-rewrite`.
- Dependency: `../07-20-rust-runtime-parity` must be `complete` before this task starts; that task itself requires the milestone-1 PASS.
- Freeze the supported RPC message, streaming, error, cancellation, and lifecycle contracts from the current TypeScript behavior before implementation.
- Implement RPC transport and integration adapters without coupling them to the TUI.
- Define versioned declarative manifests and explicit process/network adapters for command, MCP, HTTP, and LSP integrations; do not load TypeScript/JavaScript extensions or a native dynamic plugin ABI.
- Implement install/remove/update/list/config lifecycle for supported declarative integration/resource bundles without executing package lifecycle scripts or in-process JavaScript.
- Publish a migration matrix showing which existing extension capabilities map to external protocols and which remain intentionally unsupported.
- Preserve project trust, authentication redaction, session semantics, and backpressure/cancellation behavior.
- Use local fake peers and process fixtures for conformance; external-service calls require explicit authorization.
- Add `design.md` and `implement.md` for this child and obtain review before `task.py start`.

## Acceptance Criteria

- [ ] Golden request/response/event fixtures prove current unversioned RPC JSONL semantic parity, including malformed input; separate fixtures validate versioned integration manifests.
- [ ] Integration tests cover startup, shutdown, cancellation, disconnect, backpressure, and child-process cleanup.
- [ ] Untrusted project data cannot activate integrations before trust approval.
- [ ] Secrets are redacted from logs, errors, snapshots, and process arguments where feasible.
- [ ] The layer can be tested headlessly and does not require terminal rendering.
- [ ] Command/MCP/HTTP/LSP fixtures prove capability negotiation, trust gating, lifecycle cleanup, and explicit rejection of unsupported extension behavior.
- [ ] Package lifecycle fixtures prove source validation, integrity, atomic enablement/rollback, no lifecycle-script execution, and compatible resource discovery.

## Out of Scope

- TUI implementation, provider expansion, npm SDK compatibility, release publication, or default-command takeover.

## Notes

- Source of truth: `../07-17-rust-rewrite/prd.md`, `../07-17-rust-rewrite/design.md`, and `../07-17-rust-rewrite/implement.md`.
- Directory hierarchy is not a dependency; the milestone-1 gate decision is mandatory.
