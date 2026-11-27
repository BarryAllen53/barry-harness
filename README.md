# Barry Harness

[![CI](https://github.com/BarryAllen53/barry-harness/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/BarryAllen53/barry-harness/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Barry Harness is a Windows desktop chat client and command line tool for large language models, written in Rust. One window talks to Claude, ChatGPT, Gemini and the models of a local Antigravity installation, streams replies with their reasoning, and keeps every credential in the Windows Credential Manager.

The desktop app is built with [Tauri 2](https://tauri.app) and a [Leptos](https://leptos.dev) frontend compiled to WebAssembly. The provider layer is a set of small, independent crates that the app and the `bh` command line share.

## Features

- **Four providers behind one interface.** Anthropic Messages API, OpenAI Responses API, Gemini API and the local Antigravity hub, each as its own crate implementing the `Provider` trait.
- **Sign in or bring a key.** OAuth 2.0 with PKCE for Claude, ChatGPT and Google accounts, including a paste-code flow for Claude, or API keys entered in the app or read from environment variables. Tokens refresh automatically.
- **Streaming with reasoning.** Text and thinking stream as they arrive. Reasoning blocks are replayed to the provider byte for byte on follow-up turns, and blocks from another provider are left out.
- **Prompt caching.** Anthropic cache breakpoints are planned from the request structure so the cached prefix stays byte-stable across turns; cache reads, writes and the hit ratio are reported.
- **Resilient transport.** rustls TLS, HTTP/2, bounded retries with exponential backoff and jitter, `Retry-After` support, connect, first-event and idle timeouts, and cancellation that aborts the request.
- **Desktop app.** Model picker, effort and system prompt settings, projects, chat history, light, dark and high-contrast themes, font scaling, screen reader announcements and keyboard navigation.
- **Locked-down shell.** The frontend has no network access. Its content security policy allows only the app's own assets and the IPC endpoint, and its capability file grants only the app's own commands.
- **Command line.** `bh chat` streams one reply from the Anthropic API to stdout, with usage and cache statistics on stderr.

## Requirements

- Windows 10 or 11 with the [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (preinstalled on Windows 11).
- To build: Rust 1.100.0 with the `wasm32-unknown-unknown` target (installed automatically from `rust-toolchain.toml` by `rustup`), the Visual Studio C++ build tools, and [Trunk](https://trunkrs.dev) 0.22.2.

## Install

Download `barry-harness-<version>-windows-x64.zip` from the [releases page](https://github.com/BarryAllen53/barry-harness/releases), extract it and run `barry.exe`. The archive also contains the `bh.exe` command line.

## Build from source

```powershell
git clone https://github.com/BarryAllen53/barry-harness.git
cd barry-harness
cargo install --locked trunk@0.22.2
trunk build --release
cargo build --release --locked --package bh-app --features tauri/custom-protocol
cargo build --release --locked --package bh-cli
```

The binaries are `target\release\barry.exe` and `target\release\bh.exe`. `trunk build` compiles the frontend into `dist\`, which the release build embeds.

For development, `run.bat` serves the frontend with `trunk serve` on port 1420 and starts a debug build of the app against it. With the Tauri CLI installed (`cargo install --locked tauri-cli@2.12.6`), `cargo tauri dev` does the same with hot reload. `run-browser.bat` opens the frontend alone in a browser, without chat or sign-in.

## Configuration

### Credentials

Open **Settings** in the app to sign in or to paste an API key per provider. Credentials are stored in the Windows Credential Manager under the service `com.barryharness.desktop` and never written to disk in plain text. When no stored credential exists, the app falls back to these environment variables:

| Variable | Used for |
| --- | --- |
| `ANTHROPIC_API_KEY` | Claude through the Anthropic API, and the `bh` command line. |
| `OPENAI_API_KEY` | OpenAI models through the Responses API. |
| `GEMINI_API_KEY` | Gemini models through the Gemini API. |
| `BH_GEMINI_CLIENT_SECRET` | Google account sign-in for Gemini, see below. |

### Google sign-in for Gemini

Google sign-in uses an installed-app OAuth client. Its client id is in `crates/bh-auth/src/endpoints.rs`, but the client secret is not shipped with Barry Harness. Set `BH_GEMINI_CLIENT_SECRET` to the secret that belongs to that client id, or to the secret of your own Google Cloud OAuth client if you change the id, before you start the app:

```powershell
[Environment]::SetEnvironmentVariable("BH_GEMINI_CLIENT_SECRET", "<client secret>", "User")
```

Open a new terminal or sign out and back in so the app sees the variable. Without it, signing in with Google fails with an error that names `BH_GEMINI_CLIENT_SECRET`. When a stored Google sign-in expires and the variable is missing, the token cannot be refreshed. Barry Harness then falls back to a Gemini API key saved in the app or to `GEMINI_API_KEY`, which can change how the requests are billed, and logs that it switched. Without an API key it reports that the Google sign-in expired and cannot be refreshed: set the variable, then sign in again, or add a Gemini API key. API keys never need the variable.

### Claude sign-in

Signing in with a Claude account uses the OAuth client of Anthropic's Claude Code command line. Claude subscription OAuth works with this identity, so on this path Barry Harness sends requests the way Claude Code does: it adds the system prompt line "You are Claude Code, Anthropic's official CLI for Claude." in front of your own system prompt and sends the `claude-cli` user agent together with the OAuth beta headers. Barry Harness is not an official Claude client and is not affiliated with or endorsed by Anthropic. You are responsible for making sure this use complies with Anthropic's terms for your plan. With an Anthropic API key none of this applies: requests carry only your own system prompt and the `BarryHarness/<version>` user agent.

### Antigravity

Select **Antigravity** as the provider while Antigravity is running and signed in. Barry Harness finds its local language server through `%USERPROFILE%\.gemini\antigravity\daemon\hub_*.json` and uses the account's model catalog; it never reads Google credentials. See [docs/design/antigravity.md](docs/design/antigravity.md).

### Files

| Path | Content |
| --- | --- |
| `%APPDATA%\BarryHarness\settings.json` | Provider, model, effort, system prompt, theme, font scale and projects. |
| `%APPDATA%\BarryHarness\sessions.db` | Chat history as JSON. |
| `%LOCALAPPDATA%\BarryHarness\logs\` | Application log. Keys and tokens are never logged. |

## Command line

```powershell
$env:ANTHROPIC_API_KEY = "<your key>"
bh chat --model claude-opus-5-5 --effort high "Explain this error"
Get-Content prompt.txt | bh chat --show-thinking
```

`bh chat` streams one reply to stdout and prints the stop reason, token usage and prompt cache statistics to stderr. The prompt comes from the arguments, or from stdin when no prompt argument is given. `--model` defaults to `claude-opus-5-5`; unknown model ids get conservative capabilities. `--effort` accepts `low`, `medium` (default) or `high`. `--show-thinking` prints summarized thinking to stderr. Ctrl+C cancels the request; the exit code is 130 after a cancel, 2 for usage errors and 1 for other failures.

## Project layout

| Path | Purpose |
| --- | --- |
| `crates/bh-types` | Provider-neutral data: messages, opaque reasoning blocks, tool specs, usage, stop reasons, error codes. Builds for `wasm32-unknown-unknown`. |
| `crates/bh-provider` | The `Provider` trait, request and event types, cache plan types, retry policy, SSE decoder and the `ApiKey` wrapper. |
| `crates/bh-anthropic` | Anthropic Messages API adapter with prompt cache planning. |
| `crates/bh-openai` | OpenAI Responses API adapter, API key or ChatGPT sign-in. |
| `crates/bh-gemini` | Gemini adapter, API key or Google sign-in. |
| `crates/bh-antigravity` | Provider that drives the local Antigravity language server. |
| `crates/bh-auth` | OAuth with PKCE, loopback redirect listener, token refresh and credential storage. |
| `crates/bh-config` | Versioned settings file. |
| `crates/bh-store` | Chat history store. |
| `crates/bh-cli` | The `bh` command line. |
| `frontend` | Leptos frontend built by Trunk. |
| `src-tauri` | Tauri desktop shell (binary `barry`). |
| `xtask` | Repository automation, run as `cargo xtask <command>`. |
| `docs/design` | Architecture and design documents. |

## Security

- Credentials live only in the Windows Credential Manager or in environment variables you set. Keys are never printed by `Debug`, travel only in HTTP headers marked sensitive, never in URLs, and are redacted from provider error messages.
- Provider base URLs must use https; plain http is accepted only for loopback addresses. Redirects are never followed, so a key cannot leak to another host.
- No OAuth client secret is embedded in the code. Google sign-in reads its secret from `BH_GEMINI_CLIENT_SECRET` at run time.
- Signing in with a consumer account (Claude, ChatGPT, Google, Antigravity) uses the same OAuth clients or local endpoints as the vendors' own tools. Check that this use is allowed by the terms of your plan.

Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the build, the quality gates and the no-comments rule, and follow the [Code of Conduct](CODE_OF_CONDUCT.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

Barry Harness is released under the [MIT License](LICENSE).
