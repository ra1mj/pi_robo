# Rust Provider and Authentication Parity Design

## Status and Dependency Gate

Planning design only. `../07-20-rust-m1-gate` must be `complete` with a written PASS before this task starts. No later provider row may weaken the already gated milestone-1 contracts.

## Evidence and Scope Control

The implementation-time matrix is generated from the current model/provider generator, `packages/ai/src/types.ts`, `packages/ai/src/providers/all.ts`, provider/auth modules, and focused tests. The present code defines ten known chat protocol families, 36 built-in provider factories, five bundled OAuth flow families, and one OpenRouter image-generation API, but counts are evidence snapshots rather than hardcoded acceptance targets.

The generated matrix is authoritative for the candidate commit. Every row records protocol, brand, endpoint/headers, model overrides, auth methods, transport modes, tool/image/thinking support, fixture owner, support state, and evidence.

This task extends provider, model, credential, and one-shot image services. It does not implement RPC/integrations, terminal UI, packaging, command takeover, npm SDK compatibility, or a JavaScript host.

## Delivery Slices

Provider work is accepted in independently gated slices:

1. Provider-brand rows using the four already gated protocol families.
2. Mistral Conversations.
3. Azure/OpenAI Responses variants.
4. OpenAI Codex Responses, including required transport/replay behavior.
5. Bedrock Converse Stream and AWS credential/config semantics.
6. Google Vertex and its credential/endpoint semantics.
7. `pi-messages` and owning provider brands.
8. API-key/ambient auth parity for every approved provider row.
9. OAuth login/refresh/logout/storage parity.
10. OpenRouter one-shot image generation.

A slice can be marked supported only after its local conformance and negative matrix passes. A provider brand is not enabled merely because its protocol compiles.

## Protocol Architecture

New protocol modules implement the same canonical `ModelService`/event contracts and shared transport policies as milestone 1. Provider-brand configuration remains separate from wire mapping:

- protocol module: request/event transformation and protocol errors;
- provider row: base URL, headers, auth binding, model compatibility, transport defaults, attribution/cache settings;
- model catalog: generated public metadata;
- runtime registry: exposes only rows whose matrix state is `verified`.

Azure and provider variants reuse shared protocol components without forking canonical semantics. Provider-specific deviations are explicit typed compatibility options with fixtures, not brand-name conditionals scattered through the runtime.

WebSocket, SDK-backed, cloud-signing, or native/build dependencies require a separate ledger review. Client defaults for retries/timeouts cannot silently conflict with runtime retry behavior.

## Remaining Protocol Responsibilities

### Mistral Conversations

Cover conversation/entry mapping, tools/results, reasoning, usage, identifiers, streamed/final responses, errors, cancellation, and model-specific schema constraints.

### Azure/OpenAI Responses

Reuse Responses state/event mapping while isolating Azure endpoint, deployment, API-version, header, and authentication resolution. Fixtures cover base URL forms and deployment maps.

### OpenAI Codex Responses

Cover OAuth-derived auth, response/reasoning replay, cache affinity, SSE/WebSocket modes where approved, transport fallback, reconnect/error semantics, and cancellation without leaking account/session data.

### Bedrock Converse Stream

Cover message/tool/image/thinking conversion, usage, stop/error mapping, region/endpoint resolution, bearer/profile/credential sources, SigV4 behavior, custom headers, timeout, and stream cancellation. AWS dependencies/features and credential-chain side effects are explicitly reviewed.

### Google Vertex

Reuse Google content semantics while isolating project/location/publisher endpoint construction, API-key or ADC/token auth, refresh/expiry behavior, and Vertex-specific errors.

### `pi-messages`

Cover its message/event contract, provider metadata/configuration, usage, tools/images, errors, retry classification, and cancellation as captured from current tests.

## Provider-Brand Matrix

For every generated/dynamic approved provider:

- verify exact protocol and endpoint construction;
- verify required/default/suppressible headers and uppercase/value handling;
- verify API-key/environment/ambient/OAuth sources and provider-scoped environment;
- verify model compatibility overrides, thinking/cache/image/tool capabilities, and attribution;
- verify proxy/TLS/timeout/retry/transport behavior;
- provide at least one success and representative negative local fixture.

Dynamic providers such as Radius have explicit configuration schema and are not inferred from the generated catalog alone. Unsupported rows remain visible with a precise reason rather than silently falling back.

## Credential Store and Auth Resolution

The Rust credential store implements the current one-record-per-provider `auth.json` shape with raw unknown-field preservation. The only mutation path is serialized read-modify-write:

1. acquire the current `proper-lockfile`-compatible directory/mtime lock;
2. re-read the current file under lock;
3. run the per-provider mutation;
4. write atomically without dropping unrelated/unknown records;
5. preserve parent/file permissions (`0700`/`0600` where supported);
6. release/clean the owned lock and report compromise/staleness errors.

Login, refresh, and logout all use this path. Read/status listing never exposes secret values or executes API-key commands.

Auth resolution preserves the rule that a stored credential owns the provider: after a stored OAuth refresh failure or unmatched credential type, ambient env fallback does not silently take over. Explicit request overrides remain non-persistent.

## OAuth Service

OAuth is exposed through provider-neutral interaction callbacks for text/secret/select/manual-code prompts and info/URL/device-code/progress notifications. TUI and future external clients provide the interaction surface; this task contains no terminal renderer.

Current flow families are implemented only after fixtures cover their specific PKCE, callback/device/manual-code, token exchange, refresh, revocation/logout, base URL/header derivation, timeout, cancellation, and redaction behavior.

Expired tokens use double-checked refresh under the cross-process store lock. Concurrent processes refresh once, persist rotated credentials before release, and observe logout/login races deterministically. Refresh failures preserve actionable categories without logging tokens or authorization URLs containing secrets.

## API-Key and Ambient Auth

Each provider row defines side-effect-free availability checks separately from request-time resolution where resolution may run commands or cloud credential chains. Provider-scoped environment overlays ambient environment field-by-field. AWS profiles/files, Google ADC, Cloudflare account/gateway IDs, local keyless endpoints, and other non-single-key providers receive explicit fixtures.

## One-Shot Image Generation

Image generation is a separate service and model registry. The initial verified row is OpenRouter using `openrouter-images`. It maps prompt/context/options to a single final `AssistantImages` result with generated image content, usage, stop reason, response ID, errors, timeout, headers, cancellation, and the owning provider's auth.

It does not reuse chat streaming, fabricate assistant text events, or expose image generation through an unreviewed CLI/TUI command. Consumer integration is handled by its owning later surface.

## Tests and Live Evidence

Automated tests use synthetic local HTTP/SSE/WebSocket servers, fake OAuth authorities/callbacks, fake cloud credential sources, fake clocks, and isolated auth homes. No CI test needs credentials or public endpoints.

Live smoke is a separate manual gate. It samples representative protocol/auth families only after explicit authorization, uses least-privilege credentials, records no prompts/tokens, and treats provider cost/side effects as external. Lack of authorization leaves live evidence pending; it never causes tests to fall back to real ambient credentials.

## Security, Compatibility, and Rollback

- All secrets are redacted from logs, errors, fixtures, panic output, URLs, and artifacts.
- OAuth callback listeners bind narrowly, validate state/PKCE, time out, and close on cancellation.
- Auth writes preserve TypeScript interoperability and secure permissions.
- Model catalogs are generated; `packages/ai/src/models.generated.ts` is never edited directly.
- Each slice can be disabled from the Rust registry without affecting milestone-1 providers or TypeScript `pi`.
- Rollback keeps `auth.json` compatible and returns use to TypeScript without schema migration.

## Decisions Closed for Start Review

- Matrix rows, not protocol names or provider counts, define parity.
- New protocols and provider brands ship in independently verified slices.
- Credential mutations use compatible cross-process locking and secure permissions.
- OAuth logic is UI-neutral and double-checks refresh under lock.
- Image generation remains a separate one-shot service.
- Live provider tests are explicit manual evidence, never CI defaults.
