# Implementation status

The documents in `docs/design/` describe the full target architecture. This page maps them to what the workspace implements in release 0.1.0 and lists the parts that are designed but not built yet. Crates named in the designs that do not exist in the workspace (`bh-agent`, `bh-tools`, `bh-mcp`, `bh-policy`, `bh-sandbox`, `bh-media`) are planned components.

## Implemented

| Area | Crate | What exists |
| --- | --- | --- |
| Shared types | `bh-types` | Messages and content blocks (text, opaque reasoning replayed byte for byte, tool use, tool results), tool specs, system prompt segments with stability classes, usage normalization with `total_input` and `cache_hit_ratio`, stop reasons with the raw provider value, error codes, user actions and the IPC error envelope. Serde only; builds for `wasm32-unknown-unknown`. |
| Provider contract | `bh-provider` | The `Provider` trait, neutral request and event types, cache plan types, capabilities, retry with exponential backoff and full jitter (base 500 ms, factor 2, cap 30 s, 5 attempts, `Retry-After` honoured and capped), an incremental SSE decoder with line and event size limits, and the `ApiKey` wrapper that never prints its value. |
| Anthropic | `bh-anthropic` | Messages API adapter with API key or OAuth access token, prompt cache breakpoint planning (budget of 4: tools, stable system segments, tail anchor), request serialization with a byte-stable prefix, SSE assembly, truncation and mid-stream error handling, and a capability table keyed by model prefix. |
| OpenAI | `bh-openai` | Responses API adapter with API key or ChatGPT sign-in, request body mapping, stream assembly and model capabilities. |
| Gemini | `bh-gemini` | Gemini API adapter with API key, or a Google sign-in routed through the Cloud Code endpoint, plus stream assembly and model capabilities. |
| Antigravity | `bh-antigravity` | A provider that drives the local Antigravity language server; see `antigravity.md`. |
| Sign-in | `bh-auth` | OAuth 2.0 authorization code flow with PKCE for Claude, ChatGPT and Google, a loopback redirect listener, a paste-code flow for Claude, token refresh, and credential storage in the Windows Credential Manager through `keyring`. |
| Settings | `bh-config` | Versioned `settings.json` under `%APPDATA%\BarryHarness`: provider, model, effort, system prompt, font scale, theme, permission mode and projects, sanitized on load and save, unknown keys preserved. |
| History | `bh-store` | Chat history in a JSON file under `%APPDATA%\BarryHarness`, written atomically through a temporary file. |
| Command line | `bh-cli` | `bh chat`, a streaming one-shot Anthropic chat with thinking display, usage and cache statistics. |
| Desktop app | `src-tauri`, `frontend` | Tauri 2 shell and Leptos frontend: streaming chat with a frame coalescer, cancel, new chat, provider and model picker, sign-in and API key management, settings, project list, theme and font scaling, a screen reader announcer, a strict content security policy and a minimal capability file. |
| Repository tooling | `xtask` | `cargo xtask lint-comments` and `cargo xtask check`. |

## Designed, not built yet

| Design | Missing parts |
| --- | --- |
| `contracts.md` | UUIDv7 identifiers and `SessionSeq`; image blocks and the blob store; the tool trait and registry (section 3); permission and policy types (section 4); `AgentEvent` and the agent loop (section 5); `ToolError`, `PolicyError` and `AgentError`; the JSON log layer, rotation and metrics (section 7); per-provider circuit breaker; `list_models` and `count_tokens` on the provider trait. |
| `caching.md` | Stateful cache planning in the agent (`CacheState`, epochs, idle expiry) and the OpenAI cache key planner. The stateless Anthropic planner exists. |
| `security.md` | Permission modes beyond the stored setting, hooks, path rules, the sandbox, auditing, the outbound secret scanner and `web_fetch` egress rules. Network egress rules for provider clients are in place; secret handling covers keyring storage, the `ApiKey` wrapper, sensitive headers and redaction of keys in error messages (see section 9 of `security.md` for what is still planned). |
| `storage.md` | The append-only event log and its projections. History is currently a JSON file. |
| `ipc.md`, `ux.md`, `design-system.md`, `icons.md` | Tool call, diff, approval and media views, the command palette and the parts of the event protocol that depend on the agent loop. |
