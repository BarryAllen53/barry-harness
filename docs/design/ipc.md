# Barry Harness: IPC Contract (src-tauri <-> Leptos frontend)

## 1. Principles

1. The frontend is untrusted relative to the backend. It runs in WebView2 and renders model-produced content, so any XSS equals access to every exposed command. The backend validates every command argument and makes every policy decision. The frontend never decides permissions, never reads files, never sees secrets.
2. The backend exposes a minimal, explicitly whitelisted set of app commands. No Tauri core `fs`, `shell`, `http`, `opener`-with-arbitrary-URL or `process` plugin permissions are granted to the main window.
3. DTOs are Rust types in `bh-types::ipc` shared by `src-tauri` and the Leptos frontend (one definition, compile-time checked on both sides). Wire format is JSON via Tauri's serde.
4. Streaming uses `tauri::ipc::Channel<T>` (ordered, per-call, fast path) rather than global `emit` events, so a stream's lifetime is tied to its call and no other window or listener can observe it.

## 2. Wire types

```rust
pub struct SessionStream {
    pub session: SessionId,
    pub from_seq: SessionSeq,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamFrame {
    Batch { first_seq: u64, last_seq: u64, events: Vec<UiEvent>, dropped_deltas: u32 },
    Heartbeat { at_ms: u64 },
    Resync { reason: ResyncReason, head_seq: SessionSeq },
    Closed { reason: CloseReason },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    Persisted { seq: SessionSeq, event: AgentEvent },
    Delta(DeltaEvent),
}
```

`UiEvent::Persisted` carries the log seq so the UI can dedupe and resume. `Delta` events are ephemeral (text, reasoning summary, tool arg fragments, tool progress); they carry no seq and are tied to a `(turn, index)` that the later persisted `AssistantBlock` supersedes. Rule for the UI reducer: on `Persisted(AssistantBlock)` for `(turn, step, index)` replace the in-progress buffer for that index with the final block.

Command errors always serialize as `ErrorEnvelope` (contracts.md section 6). Commands return `Result<T, ErrorEnvelope>`.

## 3. Commands

All commands are `#[tauri::command] async fn`, registered in one `generate_handler!`, each also listed in the app manifest (section 5). Names are `snake_case`; the frontend calls them through one typed `ipc` module with a function per command (no stringly-typed `invoke` elsewhere).

### 3.1 Projects and sessions

| Command | Args | Returns | Notes |
|---|---|---|---|
| `project_list` | none | `Vec<ProjectDto>` | excludes archived unless flag |
| `project_add` | `{ path: String }` | `ProjectDto` | path arrives from the native folder picker (`pick_folder` below), re-canonicalized and validated server-side |
| `project_update` | `{ id, name?, settings? }` | `ProjectDto` | settings validated against schema |
| `project_archive` | `{ id }` | `()` | |
| `project_delete` | `{ id, confirm_name }` | `()` | requires exact name to match |
| `pick_folder` | none | `Option<String>` | native dialog invoked from Rust; frontend cannot pass a start path |
| `session_list` | `{ project, cursor?, limit<=100, include_archived }` | `Page<SessionDto>` | keyset pagination |
| `session_create` | `{ project, model, effort, mode }` | `SessionDto` | |
| `session_update` | `{ id, title?, model?, effort?, mode? }` | `SessionDto` | mode change to `Auto` requires `confirm_auto: true` and is audited |
| `session_fork` | `{ id, at_seq }` | `SessionDto` | |
| `session_archive` / `session_delete` | `{ id }` | `()` | delete requires `confirm: true` |
| `session_history` | `{ id, after_seq, limit<=500 }` | `HistoryPage { events, head_seq }` | for initial load and resync |
| `session_search` | `{ query, project?, limit<=50 }` | `Vec<SearchHit>` | FTS5, query sanitized to a phrase/term list |

### 3.2 Agent turns

| Command | Args | Returns | Notes |
|---|---|---|---|
| `session_subscribe` | `{ stream: SessionStream, on_frame: Channel<StreamFrame> }` | `SubscriptionId` | replays persisted events from `from_seq` then goes live; one active subscription per (window, session) |
| `session_unsubscribe` | `{ subscription }` | `()` | |
| `turn_send` | `{ session, blocks: Vec<UserBlock>, overrides? }` | `TurnId` | `UserBlock` is text or attachment ref (`BlobHash` created by `attachment_add`); returns immediately, progress via subscription; fails with `SessionBusy` if a turn is running |
| `turn_cancel` | `{ session }` | `()` | cancels running turn, in-flight tool, and pending approvals (resolved as deny) |
| `approval_respond` | `{ approval: ApprovalId, response: ApprovalResponse }` | `()` | only accepted for currently pending approvals of the subscribed session; `GrantScope::Project` additionally audited |
| `rewind` | `{ session, checkpoint, restore_files: bool }` | `RewindReport` | refuses while a turn runs; conflicts returned for user decision via `rewind_resolve` |
| `rewind_resolve` | `{ report_id, decisions: Vec<ConflictDecision> }` | `RewindReport` | |
| `attachment_add` | `{ name, mime, bytes: Vec<u8> }` | `BlobRef` | capped 20 MiB, mime sniffed server-side (not trusted from client), images only in v1 |
| `blob_get` | `{ hash }` | `BlobPayload { mime, bytes }` | used for images; returned as bytes through IPC (<= 20 MiB) or served via the asset scheme (section 6) |

### 3.3 Providers, models, secrets, config

| Command | Args | Returns |
|---|---|---|
| `account_list` | none | `Vec<AccountDto { id, provider, label, last_four, base_url }>` |
| `secret_set` | `{ provider, label, key: String, base_url? }` | `AccountDto` (key is write-only; backend validates by a lightweight authenticated call before saving, failure returns `AuthInvalid` and stores nothing) |
| `account_remove` | `{ id }` | `()` (deletes keyring entry then row) |
| `model_list` | `{ account }` | `Vec<ModelInfoDto>` (includes capabilities subset the UI needs: efforts, reasoning display modes, vision) |
| `config_get` | none | `ConfigDto` (non-secret user config) |
| `config_set` | `{ patch: ConfigPatch }` | `ConfigDto` (typed patch, unknown keys rejected) |
| `price_table_get` / `price_table_set` | | per-model price entries |

`secret_set` is the only command that accepts a secret. Its argument struct implements `Debug` redacted; the IPC layer's tracing never logs args for this command. The frontend input is a password field, cleared immediately after the call resolves.

### 3.4 Tools, MCP, hooks, policy

| Command | Args | Returns |
|---|---|---|
| `mcp_list` | none | `Vec<McpServerDto { name, state, tool_count, trusted, transport }>` |
| `mcp_upsert` | `{ server: McpServerConfigDto }` | `McpServerDto` (new or changed server starts untrusted; stdio command is shown in full for user review) |
| `mcp_trust` | `{ name, fingerprint }` | `()` (fingerprint of command+args+env keys must match what was shown; mismatch refuses) |
| `mcp_remove` | `{ name }` | `()` |
| `mcp_restart` | `{ name }` | `McpServerDto` |
| `hook_list` / `hook_upsert` / `hook_remove` | | hook definitions (security.md section 5) |
| `rule_list` / `rule_add` / `rule_remove` | | permission rules by scope |
| `audit_list` | `{ after_seq?, limit<=200 }` | `Vec<AuditDto>` |

### 3.5 Media

| Command | Args | Returns |
|---|---|---|
| `media_generate` | `{ project, session?, model, prompt, size, quality, n<=4 }` | `MediaJobId` (idempotency key generated server-side, per-call UUID, client may pass `client_request_id` to dedupe double clicks) |
| `media_edit` | `{ project, session?, model, prompt, images: Vec<BlobHash>, mask?: BlobHash }` | `MediaJobId` |
| `media_job_list` | `{ project, cursor?, limit<=50 }` | `Page<MediaJobDto>` |
| `media_job_cancel` | `{ id }` | `()` |
| `media_job_retry` | `{ id }` | `MediaJobId` (new job) |
| `media_subscribe` | `{ on_frame: Channel<MediaFrame> }` | `SubscriptionId` (job status updates; low volume, no batching needed) |

Cost confirmation: media jobs above a user-configured cost threshold (default 0, i.e. always confirm) go through the same approval flow (`ApprovalRequested` with `Capability::MediaGenerate`), not a UI-only dialog, so a compromised frontend cannot skip it.

### 3.6 App

`app_info` (versions, data dir display path, platform), `diagnostics_export` (writes a redacted bundle to a user-chosen path chosen via native dialog in Rust), `open_external` (`{ url }`: only `https` scheme, host re-validated, opened by backend via the OS default browser; no `file:` or custom schemes), `reveal_in_explorer` (`{ project }` or `{ blob }`: path derived server-side from ids, never from client-supplied strings).

### 3.7 Argument validation

Every command struct derives `Deserialize` with `#[serde(deny_unknown_fields)]`, then a `validate()` enforcing: id formats, string length caps (prompt 200k chars, names 200, labels 100), numeric ranges, enum membership, path-valued fields only where listed above and always canonicalized and checked against the project root. IPC payload hard limit: 32 MiB per invoke (the transport default is larger; commands that take bytes enforce their own lower cap before copying). A per-window token bucket (50 commands/s sustained, burst 200) returns `RateLimited` to a runaway frontend; streaming channels are exempt.

## 4. Events and streaming

### 4.1 Which transport

- Agent and media streams: `Channel<StreamFrame>` passed into `session_subscribe`. Channels are ordered and scoped to the call.
- A small set of global app events via `app.emit_to("main", ...)`: `app://theme-changed`, `app://update-available`, `app://keyring-state`. These carry no session content. The frontend listens with the narrowly scoped `core:event:allow-listen` permission restricted by event name allowlist.

### 4.2 Emitter pipeline (backpressure and batching)

Per subscription, the backend runs a small emitter task:

```
agent broadcast (tokio::sync::broadcast, cap 4096 persisted events)
        |
   coalescer (merges deltas)
        |
   bounded mpsc (cap 64 frames)
        |
   Channel::send (serializes + hands to webview)
```

Rules:

1. Persisted events are never dropped. They come from the log: if the subscriber lags (broadcast `Lagged`), the emitter discards its buffer and sends `Resync { reason: Lagged, head_seq }`; the frontend then calls `session_history(after_seq = last_applied)` and continues. Source of truth is the log, so loss is impossible by design.
2. Deltas are lossy-tolerant and coalesced: consecutive `TextDelta`/`ReasoningSummaryDelta` for the same `(turn, index)` are concatenated into one; `ToolProgress` chunks for one call are concatenated up to 16 KiB per frame. Tool argument fragments (`ToolArgumentsDelta`) are collapsed to a "last known byte count" indicator, not forwarded as text, unless the UI has the tool-args panel open (per-subscription flag).
3. Batching: frames flush when 16 ms (one display frame) elapsed since the first buffered item or the buffer reaches 32 KiB or 64 events, whichever comes first. A persisted event that ends a block (`AssistantBlock`, `ToolCallFinished`) forces an immediate flush so the UI never shows deltas after the final block.
4. Backpressure: the frame mpsc is bounded (64). When full (webview slow), the coalescer stops pulling from the broadcast and merges further deltas into the pending buffer (growth bounded at 256 KiB, after which deltas for that index are dropped and `dropped_deltas` is incremented in the next frame; the next `AssistantBlock` repairs the content). Persisted events are never merged away; if the pending persisted-event backlog exceeds 4096 the subscription gets `Resync`.
5. Because a slow webview cannot slow the agent: the agent publishes into the broadcast channel (non-blocking) and its own persistence path is independent. Provider reading is never blocked by the UI.
6. Heartbeat every 5 s while a turn is active so the UI can show "stalled" on a silent stream and detect a dead subscription (no frame in 15 s -> UI resubscribes with `from_seq = last_applied + 1`).
7. Ordering and idempotence: frontend reducer applies `Persisted` events only when `seq == last_applied + 1`; `seq <= last_applied` is ignored (duplicate after resync); a gap triggers `Resync`.

Large payloads: tool results and file diffs above 64 KiB are not sent inline. `ToolCallFinished.display` carries a `BlobHash` plus a 4 KiB preview; the UI fetches the rest through `blob_get` on expansion (lazy). Images are never inlined as base64 in events.

### 4.3 Frontend reducer and rendering performance

- Leptos signals per block (`RwSignal<String>` for streaming text) so a delta updates one text node. Markdown is re-rendered at most once per animation frame for the block being streamed, incremental parse limited to the last block; finished blocks are rendered once and memoized.
- Message list virtualization once more than 200 rows (windowed list), because sessions can contain thousands of events.
- Markdown to HTML goes through a sanitizing renderer (pulldown-cmark with raw HTML disabled; no `inner_html` of model text under any circumstance; links rendered as `<a data-href>` handled by the app's click handler calling `open_external`, never `target=_blank` navigation; images from model content rendered only if they are `blob:`/`asset:` handled internally, never remote `http(s)` URLs, to prevent exfiltration through image fetches).

## 5. Capability whitelist (Tauri 2)

`src-tauri/capabilities/main.json` grants one window `main`. The app-command permissions are defined by an app manifest in `build.rs` so each command is individually allowed (default is deny for app commands once a manifest lists them):

```rust
tauri_build::try_build(
    tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "project_list", "project_add", "project_update", "project_archive", "project_delete",
            "pick_folder", "session_list", "session_create", "session_update", "session_fork",
            "session_archive", "session_delete", "session_history", "session_search",
            "session_subscribe", "session_unsubscribe", "turn_send", "turn_cancel",
            "approval_respond", "rewind", "rewind_resolve", "attachment_add", "blob_get",
            "account_list", "secret_set", "account_remove", "model_list",
            "config_get", "config_set", "price_table_get", "price_table_set",
            "mcp_list", "mcp_upsert", "mcp_trust", "mcp_remove", "mcp_restart",
            "hook_list", "hook_upsert", "hook_remove", "rule_list", "rule_add", "rule_remove",
            "audit_list", "media_generate", "media_edit", "media_job_list", "media_job_cancel",
            "media_job_retry", "media_subscribe", "app_info", "diagnostics_export",
            "open_external", "reveal_in_explorer",
        ]),
    ),
)
```

Capability file:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "main-window",
  "description": "Main window permissions",
  "windows": ["main"],
  "local": true,
  "permissions": [
    "core:event:allow-listen",
    "core:event:allow-unlisten",
    "allow-project-list",
    "allow-project-add",
    "allow-session-list",
    "allow-session-subscribe",
    "allow-turn-send",
    "allow-approval-respond"
  ]
}
```

The real file lists one `allow-<command>` per manifest command (the list above abbreviated for readability). Keeping the `#[tauri::command]` functions registered in `generate_handler!`, the `APP_COMMANDS` manifest in `src-tauri/src/app_commands.rs` (fed to `tauri-build`) and the capability permissions in step is a review requirement today: a command missing from the capability is rejected by Tauri at run time, but no automated parity check exists yet.

Explicitly NOT granted: `core:window:*` beyond `allow-set-title`, `allow-minimize`, `allow-maximize`, `allow-close`, `allow-start-dragging` (custom titlebar if used), `core:webview:*`, `core:path:*`, `core:app:*` except version, any `fs:`, `shell:`, `http:`, `dialog:` (dialogs are opened by Rust), `opener:`, `process:`, `os:`, `clipboard-manager:` (clipboard write of selected text uses the standard web `navigator.clipboard.writeText` on user gesture; reads are not needed), `global-shortcut:`, `updater:` (the updater is driven from Rust, not the frontend).

Remote content: capabilities set `local: true`, no `remote` URLs. The window loads only the bundled frontend (`tauri://localhost`/`http://tauri.localhost`).

### 5.1 CSP and webview hardening

`tauri.conf.json` `app.security.csp`:

```
default-src 'none';
script-src 'self' 'wasm-unsafe-eval';
style-src 'self' 'unsafe-inline';
img-src 'self' blob: bhblob: http://bhblob.localhost;
font-src 'self';
connect-src ipc: http://ipc.localhost;
object-src 'none';
base-uri 'none';
form-action 'none';
frame-ancestors 'none';
```

Notes: `'wasm-unsafe-eval'` is required for the Leptos CSR wasm module (no `unsafe-eval` for JS). `style-src 'unsafe-inline'` is needed for Leptos/CSS-in-attributes; accepted with the trade-off that styles cannot execute code, and markdown is sanitized. `connect-src` has no network hosts: the frontend can never talk to the internet; all outbound traffic goes through the backend, which makes exfiltration by an injected script impossible even if XSS occurred (it could still invoke whitelisted commands, which is why command-level validation and the approval model are authoritative). `dangerousDisableAssetCsp` stays false. DevTools disabled in release builds. `freezePrototype: true`. Navigation: `on_navigation` handler denies everything except the app origin; `on_new_window` denies all (external links go through `open_external`). Drag-and-drop file handling is disabled at the webview level (`disable_drag_drop_handler` left default and drops handled by Rust, which reads the dropped path list and offers `attachment` or project add, never auto-reading arbitrary files without a user-visible confirm).

Asset serving: blobs for images are served through a custom URI scheme protocol registered in Rust (`bhblob://<hash>`), which looks up the hash in the store, verifies it exists in `blobs`, and returns bytes with `Content-Type` from the stored (sniffed) mime and `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox`. Only image mimes are served; SVG is never served inline (it is served as `image/svg+xml` only inside `<img>`, but v1 refuses SVG attachments and generated media is raster only). The Tauri `asset` protocol is disabled in config so the webview has no path-based filesystem access; `img-src` allows only `bhblob:`.

## 6. Lifecycle and consistency

- App state in Rust: `AppState { store, sessions: DashMap<SessionId, SessionRuntime>, providers, tools, policy, mcp, media }`, managed via `tauri::Manager::manage`. A `SessionRuntime` owns the agent task, broadcast sender, pending approvals map and cancellation token.
- Window close: graceful shutdown cancels running turns (persist `TurnCancelled`), closes MCP child processes via the Job Object (security.md), flushes the writer queue, runs `wal_checkpoint(TRUNCATE)`. A hard 5 s cap then exit.
- Single instance: `tauri-plugin-single-instance` so two app instances never fight over the database; second launch focuses the first. The CLI is the one sanctioned second process (session lock handles it).
- Reload safety: a webview reload (F5 disabled in release, but crashes happen) drops all channels; on boot the frontend calls `session_list`, then `session_subscribe(from_seq = 0 or last cached)`. Running turns continue in the backend uninterrupted, because the agent task is not tied to the subscription. Pending approvals persist in `SessionRuntime` and are re-announced to a new subscriber as `ApprovalRequested` replays (they are persisted events with no matching `ApprovalResolved`).
- Approval timeout: pending approvals do not auto-expire in `Ask`/`AcceptEdits` modes (the turn simply waits, cancellable). In headless CLI use, approvals are answered on the TTY or the non-interactive default is deny.
- Versioning: `app_info` returns `ipc_version: u32`. Frontend and backend ship in one bundle so mismatch cannot happen in production; the check exists to fail fast in dev when one side is stale.

## 7. Testing the contract

Status: planned. None of these checks ship in release 0.1.0.

- Command/manifest/capability parity test (section 5).
- Property tests on coalescer: concatenation of coalesced deltas equals concatenation of originals; persisted events preserved in order; no frame exceeds 256 KiB.
- Lag test: slow consumer + fast producer; assert `Resync` and that `session_history` replay reconstructs identical state to the log.
- Reducer test in the frontend crate (native target, no browser): feed a recorded `StreamFrame` sequence with duplicates, gaps and resyncs; assert final UI state equals state derived from the log.
- Fuzz the `validate()` of each command with arbitrary JSON (cargo-fuzz or proptest) asserting no panic and that rejects produce `InvalidRequest`.
- Secret test: run the full IPC flow with a canary key; grep logs, DB, and event payloads for the canary; must not appear anywhere except the keyring.
