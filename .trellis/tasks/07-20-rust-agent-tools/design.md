# Rust Agent Runtime and Core Tools Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-foundation-contracts` must be `complete` before this child starts. Provider wire adapters and disk-backed resources are intentionally not prerequisites because this child uses injected Faux and in-memory stores.

## Evidence and Boundary

Behavior is captured from the current agent loop, coding-agent runtime, compaction, and tool implementations/tests, especially:

- `packages/agent/src/agent-loop.ts` and `packages/agent/src/types.ts`;
- `packages/coding-agent/src/core/agent-session-runtime.ts`;
- `packages/coding-agent/src/core/agent-session.ts` retry/compaction behavior;
- `packages/coding-agent/src/core/tools/{read,bash,edit,write,truncate,file-mutation-queue}.ts`;
- focused tests in `packages/agent/test/` and `packages/coding-agent/test/`.

This child owns `pi-agent`, `pi-tools`, and the provider-neutral parts of `pi-runtime`. It does not implement provider HTTP, CLI parsing/output, settings/auth/trust/session-file discovery, TUI, RPC, or extra tools.

## Runtime Layers

### `pi-agent`

Owns one low-level run and its event state machine. Inputs are canonical messages, model service, tool registry, options, cancellation, and an event sink. Outputs are final messages/usage plus ordered events; persistence is abstract.

### `pi-tools`

Owns typed implementations of `read`, `bash`, `edit`, and `write`, common path/truncation/output helpers, and an in-process mutation coordinator. It depends on tool contracts, not on provider or CLI code.

### `pi-runtime`

Owns high-level turn execution, automatic retry, threshold compaction, overflow compact-and-retry, and an abstract session sink. This child supplies only in-memory/session-interface integration; disk session behavior belongs to `rust-data-resources`.

## Agent Event State Machine

For each run:

1. Emit agent and turn start events.
2. Request an ordered model stream and emit message/content deltas.
3. Accumulate a canonical assistant message and validate completed tool calls.
4. If the response is complete and contains valid calls, execute the batch.
5. Emit tool start/update/end events while collecting results.
6. Append tool-result messages in assistant source order.
7. Continue model turns while tool calls require continuation.
8. Emit turn end, then `agent_end` last after all event-sink completions settle.

One bounded ordered channel provides backpressure; events are not dropped. Parallel tools may produce progress/completion events in real completion order, but appended result messages use source order. An unknown tool or invalid arguments become error tool results rather than panics.

If the provider reports an output-limit/truncated assistant response, no partially accumulated tool call is executed. Terminal model error/aborted messages are preserved and end the applicable turn according to the canonical contract.

## Cancellation

A root cancellation token creates child tokens for model streams, tool batches, retry sleeps, compaction, image work, and shell process groups. Cancellation:

- prevents new model/tool work;
- stops retry/compaction sleeps immediately;
- requests cooperative tool cancellation;
- terminates tracked shell process trees;
- emits compatible aborted/error results for work already announced;
- settles the event sink before returning.

Cancellation is a typed outcome, never a panic or silently successful result.

## Tool Registry and Schema

Tool definitions use hand-authored canonical JSON Schema values compared with TypeScript fixtures. Before typed deserialization, compatibility preprocessing handles only explicitly captured legacy inputs, including stringified `edit.edits`. Unknown fields, wrong types, missing required fields, and unknown tool names produce deterministic error results.

Tool execution receives an authoritative cwd, environment/shell policy, image policy, cancellation handle, and partial-update sink. Tools do not discover global settings themselves.

## Shared Filesystem Rules

- Resolve relative paths against the supplied authoritative cwd.
- Canonicalize existing paths; normalize absolute targets for missing paths.
- Maintain an in-process mutation queue keyed by canonical path so symlink aliases serialize together where resolvable.
- Acquire the mutation lease before reading content used to calculate a write.
- Preserve the current security model: no sandbox or permission prompt is claimed.
- Apply the common 2,000-line/50-KiB output limit with UTF-8 and logical-line accounting.

## Tool Behavior

### `read`

- Apply one-based offset/limit and compatible head truncation for UTF-8 text.
- Detect jpg/png/gif/webp/bmp from content rather than extension alone.
- Decode and resize images on a blocking worker with bounded dimensions/memory.
- Return canonical image content plus explanatory text, respect `images.blockImages`, and add the explicit omission note for non-vision models at the owning runtime boundary.
- Cancellation is checked before and after blocking work.

### `bash`

- Resolve shell as supplied setting, then `/bin/bash`, `bash` on `PATH`, and `sh` for the Linux milestone.
- Apply `shellCommandPrefix` once and execute in the authoritative cwd.
- Start a new process group, stream combined stdout/stderr, enforce optional timeout, and terminate the process tree on abort.
- Accumulate output with partial updates; when truncated, retain the full stream in a secure temporary file and report its path according to the contract.
- Drain late process output and wait for cleanup before the final event.

### `edit`

- Acquire the canonical-path mutation lease before reading.
- Detect/preserve UTF-8 BOM and line endings.
- Match all replacements against the same normalized original text.
- Reject missing, duplicate, and overlapping matches.
- Apply non-overlapping edits, restore encoding details, and write before releasing the lease.
- Return both display diff and unified patch fixtures.

### `write`

- Acquire the mutation lease before parent creation and write.
- Create missing parents and write complete UTF-8 content.
- Preserve existing permission/symlink semantics unless an atomic-replace strategy is proven compatible; otherwise use the captured current behavior.
- Return only after data is written or a typed error occurs.

## Retry Design

`pi-runtime` consumes normalized retry classification from model errors. Agent-level transient retry defaults to three retry attempts with 2/4/8-second delays, subject to compatible settings. It emits the current retry start/countdown/end events and uses injected sleeper/clock functions.

Cancellation interrupts backoff. Non-retryable errors, exhausted attempts, or a different explicit provider policy terminate deterministically. Provider adapters do not independently schedule the same agent retry.

## Compaction Design

- Threshold compaction runs after a completed response when token policy requires it; it does not silently initiate a new user turn.
- Context-overflow handling compacts once and retries the interrupted turn once.
- Compaction receives canonical history and produces a summary plus a compatible session entry through an abstract sink.
- It preserves usage/accounting inputs and earlier history; disk append is outside this child.
- Manual compaction, branch summaries/navigation, steering, and follow-up queues remain deferred.

All compaction models, token estimates, clocks, and sinks are injectable. Tests never wait on production backoff.

## Contract Fixtures

- Faux event traces: text, thinking, multi-turn tools, parallel tools, invalid/unknown calls, tool failures, abort, retry, threshold compaction, overflow compaction, and event settlement.
- TypeScript/Rust tool runners operate on separate clones of the same synthetic temporary tree and compare schema, events, files, diffs, truncation, and errors.
- Image fixtures are small synthetic files for every supported format plus malformed/oversized cases.
- Shell fixtures use local commands only and validate timeout/process-tree cleanup without external services.

Allowed nondeterminism is limited to normalized IDs/timestamps/temp roots and genuinely parallel progress order. Final tool-result message order remains asserted.

## Trade-offs and Rollback

- In-process file mutation serialization prevents same-process stale writes but is not a cross-process lock or sandbox.
- Abstract stores make runtime tests independent and defer data-layout coupling to the data task.
- Combined stdout/stderr preserves current observable behavior at the cost of losing original stream identity.
- Rollback removes Rust runtime/tool additions only. Until the headless CLI child, no production executable or user session path reaches them.

## Decisions Closed for Start Review

- One provider-neutral state machine owns event ordering.
- Only four default tools are implemented.
- Tools receive resolved runtime policy instead of reading global configuration.
- Retry scheduling belongs to `pi-runtime`; adapters only classify.
- Compaction persists through an abstract sink in this child.
- Filesystem mutation coordination is process-local and keyed by canonical target.
