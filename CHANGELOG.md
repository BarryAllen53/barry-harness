# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-27

### Added

- Windows desktop app built with Tauri 2 and a Leptos frontend: streaming chat with visible reasoning, cancel, new chat, model picker, effort and system prompt settings, projects, chat history, system, light, dark and high-contrast themes, font scaling and screen reader announcements.
- Provider crates behind a shared `Provider` trait: Anthropic Messages API, OpenAI Responses API, Gemini API and the local Antigravity hub.
- Sign-in with OAuth 2.0 and PKCE for Claude, ChatGPT and Google accounts, a paste-code flow for Claude, automatic token refresh and credential storage in the Windows Credential Manager.
- API keys from the app settings or from `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and `GEMINI_API_KEY`.
- `BH_GEMINI_CLIENT_SECRET` environment variable for the Google OAuth client secret, which is not embedded in the code.
- Anthropic prompt cache planning with a byte-stable request prefix and cache statistics.
- Retry with exponential backoff and jitter, `Retry-After` support, connect, first-event and idle timeouts, and https-only provider URLs except for loopback.
- `bh chat` command line for one-shot streaming chats with the Anthropic API.
- `cargo xtask check` and `cargo xtask lint-comments` repository gates, CI on Windows and a release workflow for tagged versions.

[Unreleased]: https://github.com/BarryAllen53/barry-harness/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/BarryAllen53/barry-harness/releases/tag/v0.1.0
