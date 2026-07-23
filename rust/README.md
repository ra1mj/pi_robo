# `pi-rs` headless milestone

`pi-rs` is an experimental, side-by-side Rust CLI for Linux x64. It does not replace the existing `pi` command or modify npm package binaries. Milestone 1 supports headless text and JSON operation only.

## Run

Build the GNU Linux x64 binary:

```bash
cargo build --release --locked -p pi-cli --target x86_64-unknown-linux-gnu
./target/x86_64-unknown-linux-gnu/release/pi-rs --version
```

Run a text request:

```bash
OPENAI_API_KEY=... ./target/x86_64-unknown-linux-gnu/release/pi-rs \
  --provider openai --model gpt-5.4 -p "Summarize this repository"
```

Run JSONL output:

```bash
ANTHROPIC_API_KEY=... ./target/x86_64-unknown-linux-gnu/release/pi-rs \
  --provider anthropic --model claude-sonnet-4-5 --mode json "Inspect Cargo.toml"
```

Piped stdin selects text mode unless `--mode json` is explicit. Bare TTY startup is intentionally rejected because the Rust TUI is not part of milestone 1.

## Supported surface

The supported options are:

- modes and input: `--print/-p`, `--mode text|json`, positional messages, piped stdin, `@text-file`, and `@image-file`;
- model and auth: `--provider`, `--model`, `--api-key`, `--thinking`, `--list-models`, and `--offline`;
- prompts and resources: `--system-prompt`, repeatable `--append-system-prompt`, `--skill`, `--no-skills/-ns`, and `--no-context-files/-nc`;
- sessions: `--continue/-c`, `--session`, `--session-id`, `--session-dir`, `--no-session`, and `--name/-n`;
- tools and trust: `--tools/-t`, `--exclude-tools/-xt`, `--no-tools/-nt`, `--approve/-a`, and `--no-approve/-na`;
- metadata: `--help/-h` and `--version/-v`.

The production binary supports `openai-completions`, `openai-responses`, `anthropic-messages`, and `google-generative-ai`. Custom endpoints use the existing `~/.pi/agent/models.json` format.

Current TypeScript-only options fail with an explicit milestone-1 error. These include interactive/TUI behavior, `--mode rpc`, resume/fork/tree, model cycling, extensions, prompt templates, themes, export, package management, and verbose mode. Unknown options use a distinct unknown-option error.

## Credentials

Built-in providers resolve `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY`. An in-memory override is also available:

```bash
pi-rs --provider openai --model gpt-5.4 --api-key "$OPENAI_API_KEY" -p "..."
```

`--api-key` can be visible in process listings. Prefer environment variables or `~/.pi/agent/auth.json`. Credential entries may resolve a command:

```json
{
  "anthropic": {
    "type": "api_key",
    "key": "!op read 'op://vault/anthropic/credential'"
  }
}
```

OAuth login and refresh are not implemented in `pi-rs` milestone 1.

## Trust, tools, and sessions

`--approve` permits project settings and skills for the selected session working directory. `--no-approve` skips those protected project resources. These options are not a sandbox: enabled `read`, `bash`, `edit`, and `write` tools still operate with the process permissions. Use `--no-tools` or an explicit allowlist when tool execution is not wanted.

Sessions use append-only v3 JSONL files. A resumed session's stored working directory is authoritative. The CLI rejects a missing stored directory. Do not run TypeScript `pi` and Rust `pi-rs` as concurrent writers to the same session file.

`--offline` suppresses implicit startup network work; it does not block the provider request explicitly selected by the user.

## Output and exit status

Text stdout contains only the final assistant text. JSON stdout begins with a v3 session header followed by compact JSONL events. Diagnostics and redacted failures go to stderr.

Success exits `0`; input, configuration, provider, or terminal agent failures exit `1`. On Unix, SIGHUP, SIGINT, and SIGTERM return `129`, `130`, and `143` after cancellation and cleanup.

## CI artifact

The isolated Rust CI job uploads:

```text
pi-rs-linux-x64-<git-commit>.tar.gz
SHA256SUMS
build-info.json
```

Verify an extracted Actions artifact before use:

```bash
sha256sum -c SHA256SUMS
tar -xzf pi-rs-linux-x64-<git-commit>.tar.gz
./pi-rs --version
```

`build-info.json` records the schema version, source commit, workspace version, release target/profile, exact Rust tools, and `Cargo.lock` digest. This artifact is not published to a GitHub Release.

Rollback requires no migration: stop invoking `pi-rs` and continue using `pi`. Removing the isolated artifact steps from `.github/workflows/ci.yml` does not affect TypeScript CI, npm publishing, or release workflows.
