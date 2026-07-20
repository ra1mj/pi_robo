# grok-build Reference Review

Reviewed upstream commit: `8adf9013a0929e5c7f1d4e849492d2387837a28d`.

## Applicable Patterns

- Keep the executable crate as a composition root; place behavior in narrower library crates.
- Separate TUI, agent runtime, tools, host workspace operations, configuration, model/sampling, MCP, hooks, sandboxing, and protocol DTOs.
- Use serializable protocol DTOs between runtime surfaces so TUI, headless mode, and external clients share one agent engine.
- Put extension execution behind process or network boundaries: command hooks, HTTP hooks, MCP, LSP, and structured plugin manifests.
- Separate plugin enablement from execution trust, especially for repository-local resources.
- Keep append-only JSONL as the durable event log and derive searchable/index state separately.
- Validate crates independently; avoid making full-workspace checks the only feedback loop.

## Pi-Specific Differences

- Pi is currently both a CLI and a set of published npm libraries; grok-build is primarily a standalone Rust product with protocols for external clients.
- Pi's in-process TypeScript extension API can register tools, commands, event handlers, renderers, and TUI components. grok-build's plugin format does not provide an equivalent arbitrary in-process language API.
- Pi has a broad multi-provider model/API layer. grok-build contains xAI-specific product, auth, sampling, telemetry, and service assumptions that must not become pi requirements.
- Pi's current settings, models, sessions, and RPC contracts differ from grok-build's TOML configuration, ACP session log, and multi-file session directory.

## Recommended Interpretation

Build a native Rust CLI/runtime using grok-build-like crate boundaries, but define pi-owned protocols and compatibility tests. Preserve pi's CLI, provider breadth, settings, and session behavior where selected. Replace the in-process TypeScript extension model with explicit protocol boundaries unless temporary TypeScript hosting is a stated requirement.

## Licensing

grok-build first-party code is Apache-2.0 and includes third-party and port-specific notices. Prefer independent implementation from documented behavior and architectural patterns. Any copied or adapted implementation must retain the applicable license and notice obligations and be recorded during review.
