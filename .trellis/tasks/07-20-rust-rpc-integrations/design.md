# Rust RPC and External Integrations Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-runtime-parity` must be `complete` before this task starts. It consumes the completed shared runtime; it cannot create a second agent/session implementation.

## Evidence and Scope

Current RPC behavior is captured from `packages/coding-agent/src/modes/rpc/rpc-types.ts`, `rpc-mode.ts`, `rpc-client.ts`, `jsonl.ts`, `docs/rpc.md`, and focused RPC tests. Current RPC is unversioned newline-delimited JSON over stdin/stdout with optional request IDs, command responses, asynchronous runtime events, and extension UI request/response messages.

Existing extension behavior is cataloged from the extension types/runner/docs. Full in-process TypeScript compatibility is explicitly excluded. This task instead provides versioned declarative integration manifests and process/network protocols for the approved replaceable capability classes.

## Crate Boundaries

- `pi-rpc`: current JSONL framing, command dispatcher, responses/events, and client test helpers.
- `pi-integrations`: manifest validation, trust/capability policy, lifecycle supervision, and command/MCP/HTTP/LSP adapters.
- completed shared `pi-runtime`/store/resource services: queues, manual compaction, tree/session actions, and other behaviors used by both RPC and TUI.

`pi-rpc` and `pi-integrations` consume canonical runtime events. They cannot format terminal UI, implement provider protocols, own credential storage, or load native/JavaScript modules.

## Current RPC Compatibility

### Framing

- Read one UTF-8 JSON object per line from stdin.
- Write one compact JSON object per line to stdout.
- Route diagnostics exclusively to stderr.
- Echo optional command `id` in its response; unsolicited events have their current shape.
- Preserve current behavior for blank lines, malformed JSON, non-object values, unknown commands, missing/wrong fields, and EOF.
- Apply bounded input size, output backpressure, cancellation, and orderly writer draining.

No mandatory handshake or version negotiation is invented for the current RPC stream. Any future incompatible RPC version requires a separate additive negotiation design.

### Commands and Events

Port the documented current command matrix: prompt/steer/follow-up/abort/new session, state/model/thinking/queue modes, manual/automatic compaction, retry controls, bash, session stats/export/switch/fork/clone/tree/entries/name/messages, and available commands.

Port compatible responses, base agent/session events, `agent_settled`, queue/compaction/retry events, extension/integration errors, and extension UI wire messages. Commands delegate to shared services; TUI and RPC observe the same queue, model, session, retry, and compaction state.

Concurrent requests have explicit serialization/parallelism rules. Long-running prompt/bash/compaction commands acknowledge or complete according to current fixtures, and abort commands remain responsive while work is active.

## External Integration Manifest

Each integration is declared by a JSON manifest with a required integer `schemaVersion`, stable `id`, source/provenance, transport kind, endpoint/process configuration, requested capabilities, lifecycle/timeouts, and optional user-visible metadata.

Version 1 supports four explicit transport classes:

- `command`: supervised local process using a documented JSONL request/event protocol;
- `mcp`: pinned MCP protocol version over approved stdio or HTTP transport;
- `http`: structured HTTPS/HTTP request/stream hooks with explicit schemas and timeouts;
- `lsp`: pinned LSP/JSON-RPC framing for language-server capabilities.

Unknown schema versions, transports, capabilities, fields that current policy treats as errors, and invalid executable/URL definitions fail before activation. Discovery does not execute a process or perform a network call.

## Capability Model

Capabilities are least-privilege declarations, for example:

- register/invoke tools;
- register/invoke slash commands;
- receive selected lifecycle events;
- contribute diagnostics/resources permitted by its transport;
- request structured UI select/confirm/input/editor/notify/status/widget/title operations;
- LSP language operations;
- read limited runtime/session metadata explicitly exposed by contract.

The runtime grants only requested, supported, trusted capabilities. Tools/commands use canonical schemas and results. Event subscriptions are allowlisted and backpressured; an integration cannot receive credentials or raw private state merely because it runs locally.

Unsupported in-process behaviors remain explicit: arbitrary TUI components/renderers/editors, direct object references, global monkey-patching, native ABI loading, synchronous callbacks into internal state, and executing legacy TypeScript extensions.

## Transport Adapters

### Command

Start an explicit executable/argv with sanitized inherited environment and secrets delivered through safer configured channels where possible. Use new process groups, bounded stdout/stderr, handshake/capability timeout, request IDs, heartbeat/idle policy where specified, and tree termination on cancellation/shutdown.

### MCP

Pin and record the supported protocol version and transports during implementation from official specifications. Map approved tools/resources/prompts only where they have clear canonical equivalents. Capability negotiation is authoritative; unsupported server claims are ignored/rejected explicitly.

### HTTP

Use structured request/response/event schemas, explicit method/base URL, TLS/proxy policy, bounded bodies, retry/cancellation, redaction, and optional streaming. User-configured localhost/private endpoints are allowed by design but surfaced as trusted executable/network integrations, not sandboxed services.

### LSP

Supervise the language server process and implement `Content-Length` JSON-RPC framing, initialize/shutdown/exit, request IDs, cancellation, diagnostics, workspace roots, and selected language operations. LSP output cannot bypass canonical tool/runtime permission and trust decisions.

## Trust and Activation

Global user integrations may load according to global configuration. Project-local manifests and executable paths are protected resources and require the effective saved/default/CLI trust decision. Trust is resolved before parsing dependent project configuration and before activation.

Trust is not a sandbox. Activated integrations run with user permissions or access configured endpoints. Documentation names process/network/secret risks. No integration auto-starts merely because its manifest was discovered; startup is tied to an enabled capability/use and managed lifecycle.

## UI Request Bridge

The current RPC extension UI request/response wire shapes are retained for compatibility. Requests may now originate from approved external integrations. Headless RPC clients can answer or cancel them; the later TUI can render them through the same abstract interaction service.

Timeout, disconnect, duplicate/unknown response ID, and cancellation behavior are fixture-defined. An absent UI client returns a deterministic unsupported/cancelled result rather than hanging.

## Integration Package Lifecycle

The integration task owns `install`, `remove`, `update`, `list`, and configuration lifecycle for versioned declarative integration/resource bundles. Sources and integrity metadata are explicit and implementation-time support is captured from the current package UX, but no package lifecycle script or bundled JavaScript is executed.

Installation stages into a temporary location, validates manifest/schema/capabilities/source/integrity, then atomically enables the bundle through shared settings/resource services. Failure leaves the prior enabled version intact. Removal disables before deleting owned files and never removes paths outside the managed root. Updates are version/digest aware and rollback to the prior validated bundle on activation failure.

Legacy npm/git/local extension sources receive migration diagnostics. A package is usable by Rust only when it contains supported declarative resources or external command/MCP/HTTP/LSP manifests; arbitrary TypeScript extension entry points remain unsupported.

## Migration Matrix

Documentation maps existing extension classes:

- custom tools -> MCP or command/HTTP tool;
- slash commands -> command/HTTP/MCP command;
- lifecycle hooks -> selected command/HTTP event subscription;
- language intelligence -> LSP;
- external process execution -> command integration;
- structured prompts/notifications/status -> UI request bridge;
- provider/model configuration -> provider/auth task plus `models.json`;
- custom renderers/components/editor/autocomplete and arbitrary in-process state -> unsupported unless a future explicit protocol is approved.

Examples are protocol clients/manifests, not a TypeScript hosting bridge.

## Testing

- Golden current-RPC command/response/event traces, malformed framing, unknown commands, request IDs, ordering, disconnect, backpressure, and shutdown.
- Fake command/MCP/HTTP/LSP peers with negotiation, success/error, invalid messages, timeout, cancellation, reconnect policy, and cleanup.
- Trust matrices prove untrusted project integrations are not parsed into active processes/connections.
- Secret/path scans cover manifests, argv, environment diagnostics, stderr, snapshots, and protocol logs.
- Headless tests require no TUI, public service, real language server, credentials, or paid calls.

## Trade-offs and Rollback

- Keeping current RPC unversioned preserves clients; only new integration manifests are versioned.
- Explicit external protocols cannot preserve arbitrary in-process UI/code behavior, but make trust/lifecycle/capabilities testable.
- One shared UI request contract avoids separate RPC/TUI integration semantics.
- Rollback disables Rust RPC/integration registration; the gated headless CLI remains usable, and TypeScript extensions remain available only through the retained TypeScript distribution.

## Decisions Closed for Start Review

- Current RPC JSONL shapes remain unversioned and compatible.
- New integration manifests start versioned at schema 1.
- Command/MCP/HTTP/LSP are the approved external boundaries.
- No JavaScript/native dynamic plugin host is introduced.
- Project-local integration activation is trust-gated and explicitly unsandboxed.
- Unsupported extension capabilities are documented, not emulated implicitly.
- Package lifecycle validates declarative/external bundles and never runs lifecycle scripts or JavaScript entry points.
