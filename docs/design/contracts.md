# Barry Harness: Core Contracts (bh-types and trait surfaces)

Status: design, implementation-ready. Rust snippets are illustrative signatures; the real code carries no comments.

## 0. Principles

1. `bh-types` is pure data plus serde. No tokio, no reqwest, no rusqlite, no tauri. It must compile to `wasm32-unknown-unknown` because the Leptos frontend depends on it for IPC DTOs.
2. The session event log is the source of truth. `Message` history is a projection of persisted events, and what is sent to a provider is a pure function of that projection plus the cache plan (see caching.md).
3. Reasoning blocks are opaque. They are stored and replayed as the exact JSON bytes the provider emitted. No code path may parse, trim, reorder, merge or filter them except by explicit `ReasoningBlock` handling.
4. Traits live in the crate that consumes them: `Provider` and `ProviderEvent` in `bh-provider`, `Tool` in `bh-tools`, policy in `bh-policy`. Pure data shared across crates lives in `bh-types`.

Dependency direction: `bh-types` <- everything. `bh-provider` <- `bh-anthropic`, `bh-openai`, `bh-media`. `bh-tools` <- `bh-mcp`, `bh-sandbox` (sandbox is used by tools, not the reverse: `bh-tools` depends on `bh-sandbox`). `bh-policy` depends on `bh-types` only. `bh-agent` depends on provider, tools, policy, store.

## 1. bh-types

### 1.1 Identifiers

All ids are newtypes over UUIDv7 (time-ordered, sortable, generated client side, no coordination). Serialized as lowercase hyphenated strings. Stored in SQLite as TEXT.

```rust
macro_rules! id_type { ($name:ident) => {
    #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct $name(Uuid);
}; }
id_type!(ProjectId);
id_type!(SessionId);
id_type!(EventId);
id_type!(TurnId);
id_type!(CheckpointId);
id_type!(MediaJobId);
id_type!(ApprovalId);
id_type!(AgentRunId);
```

Provider-assigned ids are never newtype-wrapped into the above; they are opaque strings:

```rust
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolCallId(String);

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobHash(String);
```

`ToolCallId` carries Anthropic `tool_use.id` (`toolu_...`) or OpenAI `call_id` (`call_...`). `BlobHash` is lowercase hex SHA-256.

`SessionSeq(u64)` is the per-session monotonically increasing event sequence, starting at 1, gap-free.

### 1.2 Provider and model identity

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind { Anthropic, OpenAi }

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelRef { pub provider: ProviderKind, pub model: String }

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort { None, Minimal, Low, Medium, High, Xhigh, Max }
```

`Effort` is the union. Each provider adapter maps it onto its accepted range and reports the accepted set in `ProviderCapabilities::efforts`; the agent clamps to the nearest supported value and emits `AgentEvent::Notice` when clamping.

### 1.3 Message and ContentBlock

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub origin: Option<ModelRef>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role { User, Assistant }

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Reasoning(ReasoningBlock),
    ToolUse(ToolUseBlock),
    ToolResult(ToolResultBlock),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TextBlock { pub text: String }

#[derive(Clone, Serialize, Deserialize)]
pub struct ImageBlock { pub blob: BlobHash, pub mime: String, pub alt: Option<String> }
```

Role mapping: tool results are carried in `Role::User` messages (Anthropic native form; OpenAI adapter converts to `function_call_output` items). There is no `System` role in `Message`: the system prompt is a separate `SystemPrompt` input (see 1.5).

`origin` is set on assistant messages and records which model produced them. It drives the cross-provider rule in 1.4.

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolUseBlock {
    pub call_id: ToolCallId,
    pub name: String,
    pub arguments: JsonText,
    pub provider_item_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolResultBlock {
    pub call_id: ToolCallId,
    pub content: Vec<ToolResultPart>,
    pub is_error: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultPart {
    Text { text: String },
    Image { blob: BlobHash, mime: String },
}
```

`JsonText` is a newtype over `String` holding syntactically valid JSON, validated on construction. For Anthropic the string is the compact serialization of the `input` object assembled from `partial_json` fragments. For OpenAI it is the `arguments` string byte-for-byte. Keeping the string (not `serde_json::Value`) preserves exact bytes for replay and avoids key reordering. `provider_item_id` is the OpenAI function_call item `id` (`fc_...`), required on replay together with `call_id`.

### 1.4 Opaque reasoning

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ReasoningBlock {
    pub provider: ProviderKind,
    pub raw: Box<RawValue>,
    pub summary: Option<String>,
}
```

`raw` is the complete provider JSON object exactly as received:

- Anthropic: the whole `{"type":"thinking","thinking":"...","signature":"..."}` or `{"type":"redacted_thinking","data":"..."}` object. With `display:"omitted"` the `thinking` text is empty but the `signature` is present and still replayed.
- OpenAI: the whole `{"type":"reasoning","id":"rs_...","summary":[...],"encrypted_content":"..."}` item.

`serde_json` is built with the `raw_value` feature, and `raw` is never deserialized into a typed struct outside `bh-anthropic` / `bh-openai` response assembly. `summary` is a UI-only extract (concatenated thinking or summary text) and is never sent back.

Rules enforced in `bh-agent` and to be verified by tests once it exists:

1. Block order inside an assistant message is exactly provider emission order. Adapters append blocks at `content_block_stop` / `output_item.done` in index order.
2. No filter in the codebase may match on `ContentBlock::Text`-style variants and drop the rest. The only code that inspects `Reasoning` is: persistence, replay serialization, and UI summary rendering.
3. Within one logical assistant turn that spans tool calls (assistant -> tool_result -> assistant), all reasoning blocks of every step are replayed, in original order, unmodified.
4. Cross-provider switch: reasoning blocks whose `provider` differs from the target adapter are omitted from the outgoing request (they are meaningless and rejected cross-provider). A switch is only permitted at a turn boundary (no pending tool calls). The agent rejects a mid-turn switch with `AgentError::ProviderSwitchMidTurn`. The omission invalidates the cache epoch, which the planner records.
5. Compaction (summarizing old history) replaces whole turns, never part of a turn, and only turns that are complete. Reasoning inside a replaced span is dropped with the span.

### 1.5 Request inputs

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Box<RawValue>,
    pub origin: ToolOrigin,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin { Builtin, Mcp { server: String } }

#[derive(Clone, Serialize, Deserialize)]
pub struct SystemPrompt { pub segments: Vec<SystemSegment> }

#[derive(Clone, Serialize, Deserialize)]
pub struct SystemSegment { pub text: String, pub stability: Stability }

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stability { Static, Project, Session }
```

`ToolSpec.input_schema` is raw JSON with keys in a canonical order (see caching.md section 2). `name` matches `^[a-zA-Z0-9_-]{1,64}$`; MCP tools are namespaced `mcp__{server}__{tool}` after sanitization, collisions are a startup error. Tool lists are always sorted: builtins by name, then MCP by (server, name).

`Stability` orders system segments from most to least stable so the planner can place cache breakpoints at stability boundaries.

### 1.6 Usage

```rust
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_5m_tokens: u64,
    pub cache_write_1h_tokens: u64,
    pub reasoning_tokens: u64,
}
```

Normalization (`input_tokens` always means uncached, non-cache-read input tokens, so that total input = `input + cache_read + cache_write_*`):

- Anthropic: `input_tokens` as is; `cache_read_input_tokens` -> `cache_read_tokens`; `cache_creation.ephemeral_5m_input_tokens` / `ephemeral_1h_input_tokens` -> the two write fields; if `cache_creation` object is absent, `cache_creation_input_tokens` is attributed to 5m. `message_delta.usage` is cumulative: the adapter overwrites, never sums. `reasoning_tokens` is 0 (included in `output_tokens`).
- OpenAI: `input_tokens_details.cached_tokens` -> `cache_read_tokens`; `input_tokens` minus cached -> `input_tokens`; write fields 0 (OpenAI does not bill writes separately); `output_tokens_details.reasoning_tokens` -> `reasoning_tokens` (a subset of `output_tokens`).

`Usage::total_input()`, `Usage::cache_hit_ratio()` (`read / (read + write + input)`) are the only derived methods. Cost computation lives in `bh-agent` using a user-editable price table in config, never hard-coded in `bh-types`.

### 1.7 Stop reasons

```rust
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason { EndTurn, ToolUse, MaxTokens, StopSequence, Refusal, PauseTurn, ContextExceeded, Other }
```

Anthropic `end_turn|tool_use|max_tokens|stop_sequence|refusal|pause_turn|model_context_window_exceeded` map directly; unknown strings map to `Other` and the raw string is kept in `ProviderEvent::MessageStop.raw_stop`. OpenAI: `response.completed` with any `function_call` output item -> `ToolUse`, `incomplete_details.reason = max_output_tokens` -> `MaxTokens`, `content_filter` -> `Refusal`, otherwise `EndTurn`.

## 2. Provider (bh-provider)

### 2.1 Request

```rust
pub struct ProviderRequest {
    pub model: String,
    pub system: SystemPrompt,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    pub max_output_tokens: u32,
    pub reasoning: ReasoningConfig,
    pub cache: CachePlan,
    pub session_key: SessionId,
    pub stop_sequences: Vec<String>,
}

pub enum ToolChoice { Auto, Any, None, Tool(String) }

pub struct ReasoningConfig {
    pub effort: Effort,
    pub display: ReasoningDisplay,
}

pub enum ReasoningDisplay { Summarized, Omitted, Updates }
```

There is deliberately no `temperature`, `top_p`, `top_k` and no `budget_tokens` field anywhere in the type system. Adapters send `thinking: {type:"adaptive", display}` plus `output_config: {effort}` on Anthropic; on OpenAI they send `reasoning: {effort, summary}` with `ReasoningDisplay::Summarized -> "auto"`, `Updates -> "detailed"`, `Omitted -> summary omitted` and always `include: ["reasoning.encrypted_content"]`, `store: false`.

`CachePlan` is produced by the planner in `bh-agent::cache` (caching.md) and consumed verbatim by adapters; adapters do not make caching decisions.

```rust
pub struct CachePlan {
    pub breakpoints: Vec<Breakpoint>,
    pub openai_key: Option<String>,
    pub openai_ttl: Option<CacheTtl>,
    pub epoch: CacheEpoch,
}

pub struct Breakpoint { pub at: BreakpointAt, pub ttl: CacheTtl }

pub enum BreakpointAt {
    LastTool,
    SystemSegment(usize),
    MessageBlock { message: usize, block: usize },
}

pub enum CacheTtl { FiveMinutes, OneHour }
```

### 2.2 Trait

Dyn-safe (providers are selected at runtime), no `async_trait` macro dependency, edition 2024:

```rust
pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn capabilities(&self, model: &str) -> Option<ProviderCapabilities>;
    fn list_models(&self) -> BoxFuture<'_, Result<Vec<ModelInfo>, ProviderError>>;
    fn stream(
        &self,
        request: ProviderRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<ProviderStream, ProviderError>>;
    fn count_tokens(&self, request: &ProviderRequest) -> BoxFuture<'_, Result<u64, ProviderError>>;
}

pub type ProviderStream = BoxStream<'static, Result<ProviderEvent, ProviderError>>;
```

`stream` returns after HTTP headers arrive with a 2xx status (so auth, 400 and 429 errors surface as `Err` before any event), then the stream yields events. `count_tokens` uses the provider endpoint when available, otherwise a local estimate (`chars / 3.5`, flagged `estimate` in `ModelInfo`); the planner only needs the estimate.

Cancellation: dropping the stream or cancelling the token aborts the HTTP request (reqwest body drop closes the connection). The adapter yields no further events after cancel and the agent persists whatever completed blocks it already received, then a `TurnCancelled` event.

### 2.3 ProviderCapabilities

```rust
pub struct ProviderCapabilities {
    pub context_window: u64,
    pub max_output_tokens: u32,
    pub efforts: Vec<Effort>,
    pub reasoning_display: Vec<ReasoningDisplay>,
    pub tools: bool,
    pub parallel_tool_calls: bool,
    pub vision_input: bool,
    pub cache: CacheCapabilities,
    pub streaming_tool_args: bool,
}

pub struct CacheCapabilities {
    pub mode: CacheMode,
    pub max_breakpoints: u8,
    pub min_prefix_tokens: u32,
    pub ttls: Vec<CacheTtl>,
    pub lookback_blocks: u32,
}

pub enum CacheMode { ExplicitBreakpoints, AutomaticPrefixWithKey }
```

Capabilities come from a built-in table keyed by model id prefix, overridable and extendable through `bh-config` (`models.toml` in the config dir) so new model ids work without a release. Anthropic 4.7+ models: `max_breakpoints = 4`, `min_prefix_tokens = 512` for the 5.5 family (config table carries per-family values), `ttls = [5m, 1h]`, `lookback_blocks = 20`. OpenAI: `mode = AutomaticPrefixWithKey`; `ttls` is `[OneHour]`-style only where `prompt_cache_options` is supported (GPT-5.6+), otherwise empty.

Unknown model ids are allowed; they get conservative defaults (no cache TTL option, 4 breakpoints on Anthropic, efforts `[Low, Medium, High]`) and a `Notice`.

### 2.4 ProviderEvent

Normalized, index-addressed, provider-agnostic. Adapters assemble blocks; consumers never see raw SSE.

```rust
pub enum ProviderEvent {
    MessageStart { id: String, model: String, usage: Usage },
    BlockStart { index: u32, kind: BlockKind },
    TextDelta { index: u32, text: String },
    ReasoningSummaryDelta { index: u32, text: String },
    ToolArgumentsDelta { index: u32, fragment: String },
    BlockDone { index: u32, block: ContentBlock },
    UsageUpdate { usage: Usage },
    MessageStop { stop_reason: StopReason, raw_stop: Option<String>, usage: Usage },
    Warning { code: String, message: String },
}

pub enum BlockKind { Text, Reasoning, ToolUse { call_id: ToolCallId, name: String } }
```

Contract:

- `index` is dense per message, assigned by the adapter in emission order. For Anthropic it equals `content_block` index. For OpenAI it is the `output_index`, renumbered densely if gaps occur.
- `BlockDone` always arrives exactly once per started block, carrying the final assembled block. This is the only event whose payload is persisted. Deltas are UI-only and lossy-tolerant.
- For reasoning, `BlockDone` carries `ContentBlock::Reasoning` with `raw` built as follows. Anthropic: accumulate `thinking_delta` text and `signature_delta` signature, then serialize `{"type":"thinking","thinking":<text>,"signature":<sig>}`; for `redacted_thinking` the `content_block_start` object is kept verbatim. OpenAI: the `response.output_item.done` item JSON is kept verbatim (never rebuilt from deltas).
- For tool use, `BlockDone` carries the validated `JsonText`. Empty accumulated fragments become `{}` (Anthropic). Invalid JSON yields `ProviderError::MalformedToolArguments { call_id, raw }` after the block; the agent then returns a tool error result to the model instead of aborting the turn.
- `UsageUpdate` is cumulative and idempotent; consumers keep the latest. `MessageStop.usage` is final.
- Unknown SSE event types and unknown delta types produce no event (optionally a `Warning` once per type per stream). `ping` is swallowed. SSE `error` events become `Err(ProviderError::Stream { .. })` terminating the stream; any blocks already `BlockDone` remain valid and are persisted by the agent as a partial turn.
- A stream that ends without `MessageStop` is `ProviderError::TruncatedStream`.

### 2.5 Retry placement

`bh-provider` exposes `RetryPolicy` (exponential backoff with full jitter, base 500 ms, factor 2, cap 30 s, max 5 attempts, honour `retry-after`). Retry applies only to failures before the first `MessageStart` is yielded (connect errors, 408, 409, 429, 5xx, 529). After the stream has started, no automatic retry: the agent decides (it may resume as a new request, since persisted completed blocks are replayable). A per-provider circuit breaker opens after 5 consecutive retryable failures for 30 s (half-open single probe).

Timeouts: connect 10 s, time to first event 60 s (effort `high`+ extended to 180 s), idle between events 90 s (pings reset it), no total timeout on streams.

## 3. Tool (bh-tools)

```rust
pub trait Tool: Send + Sync {
    fn spec(&self) -> &ToolSpec;
    fn classify(&self, input: &serde_json::Value, ctx: &ToolContext) -> Result<Vec<Capability>, ToolError>;
    fn call(&self, input: serde_json::Value, ctx: ToolContext) -> BoxFuture<'_, Result<ToolOutput, ToolError>>;
}

pub struct ToolContext {
    pub session: SessionId,
    pub turn: TurnId,
    pub call_id: ToolCallId,
    pub workspace: WorkspaceRoot,
    pub cancel: CancellationToken,
    pub progress: ProgressSink,
    pub sandbox: Arc<dyn Sandbox>,
    pub deadline: Instant,
    pub limits: OutputLimits,
}

pub struct ToolOutput {
    pub parts: Vec<ToolResultPart>,
    pub is_error: bool,
    pub display: Option<ToolDisplay>,
    pub truncated: bool,
    pub side_effects: Vec<SideEffect>,
}

pub enum SideEffect {
    FileWritten { path: WorkspacePath, before: Option<BlobHash>, after: BlobHash },
    FileDeleted { path: WorkspacePath, before: BlobHash },
    ProcessSpawned { pid: u32 },
    NetworkRequest { host: String },
}
```

Rules:

1. `classify` is pure and cheap. It turns validated input into the `Capability` list the policy engine evaluates (section 4). The agent calls `classify` before every `call`; a tool cannot perform a capability it did not declare (enforced by the context handles: file access goes through `ctx.workspace`, process execution through `ctx.sandbox`, network through an `HttpGate` that checks the declared hosts).
2. Input is validated against `input_schema` (jsonschema crate) before `classify`. Validation failure produces an `is_error` result `invalid_input: ...` returned to the model; it never panics or aborts the turn.
3. `ToolOutput.parts` are what the model sees. `display` is UI-only rich output (diff, table, image). Large outputs are truncated at `OutputLimits` (default 64 KiB model-visible, head 40% plus tail 40%, with a marker line stating omitted bytes); the full output is stored as a blob and referenced in `display`.
4. `side_effects` feed checkpoints (storage.md) and the audit log. File writes must go through `ctx.workspace.write()`, which snapshots the prior content into the blob store before overwrite.
5. Every call has a `deadline` (default 120 s for shell, 30 s for file tools, overridable per call up to a policy max) and honours `cancel` within 500 ms.
6. Tools never return `Err` for model-recoverable failures (file not found, nonzero exit). They return `ToolOutput { is_error: true }`. `Err(ToolError)` is reserved for harness faults (sandbox failure, I/O fault), surfaced to the user and reported to the model as a generic error.
7. Parallelism: the agent runs tool calls of one model message concurrently only when every call is declared `read_only` by classification (all capabilities are `FsRead`/`Network(Get)`); otherwise sequentially in emitted order. Concurrency cap 8.

Builtin v1 tools: `read_file`, `write_file`, `edit_file`, `list_dir`, `glob`, `grep`, `shell` (PowerShell), `web_fetch`, `generate_image`, `edit_image`, `todo`, `ask_user`. MCP tools are adapted by `bh-mcp` into `Tool` impls: `classify` returns `Capability::McpCall { server, tool, read_only_hint }` and the server's annotations are treated as hints only, never as trust.

## 4. Permission and policy types (bh-types for data, bh-policy for evaluation)

```rust
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capability {
    FsRead { path: WorkspacePath },
    FsWrite { path: WorkspacePath },
    FsDelete { path: WorkspacePath },
    FsReadOutside { path: PathBuf },
    FsWriteOutside { path: PathBuf },
    Exec { program: String, args_preview: String, shell: bool },
    Network { host: String, method: NetMethod },
    McpCall { server: String, tool: String, read_only_hint: bool },
    MediaGenerate { provider: ProviderKind, estimated_cost_micros: u64 },
    SecretAccess { name: String },
    UserPrompt,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode { Plan, Ask, AcceptEdits, Auto }

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision { Allow, Ask, Deny }

pub struct Rule {
    pub id: RuleId,
    pub decision: Decision,
    pub matcher: Matcher,
    pub scope: RuleScope,
}

pub enum RuleScope { Session, Project, User, Managed }

pub struct PolicyVerdict {
    pub decision: Decision,
    pub reason: ReasonCode,
    pub matched_rule: Option<RuleId>,
    pub hook_notes: Vec<String>,
}

pub struct ApprovalRequest {
    pub id: ApprovalId,
    pub call_id: ToolCallId,
    pub tool: String,
    pub capabilities: Vec<Capability>,
    pub summary: String,
    pub diff: Option<DiffPreview>,
    pub risk: RiskLevel,
    pub offered_scopes: Vec<GrantScope>,
}

pub enum ApprovalResponse {
    Allow { scope: GrantScope },
    Deny { feedback: Option<String> },
}

pub enum GrantScope { Once, Session, Project }
```

Evaluation (full semantics in security.md): deny rules win over everything, then hooks, then mode defaults, then allow rules, then ask. `Managed` scope rules (file in `%ProgramData%\BarryHarness\managed-policy.toml`, optional) can never be overridden by lower scopes. `Auto` mode is not "bypass": hard-deny list and sandbox still apply.

## 5. AgentEvent (bh-agent output, persisted and streamed)

`AgentEvent` is both the live stream to the UI/CLI and, for the persisted variants, the content of the append-only event log. One enum, a `persist()` classifier, no second representation.

```rust
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    TurnStarted { turn: TurnId, model: ModelRef, effort: Effort, mode: PermissionMode },
    UserMessage { turn: TurnId, blocks: Vec<ContentBlock> },
    RequestSent { turn: TurnId, request_id: String, cache_epoch: CacheEpoch, plan_summary: CachePlanSummary },

    AssistantTextDelta { turn: TurnId, index: u32, text: String },
    ReasoningSummaryDelta { turn: TurnId, index: u32, text: String },
    ToolArgumentsDelta { turn: TurnId, index: u32, call_id: ToolCallId, fragment: String },
    AssistantBlock { turn: TurnId, step: u32, index: u32, block: ContentBlock },

    ToolCallStarted { turn: TurnId, call_id: ToolCallId, name: String, arguments: JsonText },
    ApprovalRequested { turn: TurnId, request: ApprovalRequest },
    ApprovalResolved { turn: TurnId, approval: ApprovalId, response: ApprovalResponse, by: ResolvedBy },
    ToolProgress { turn: TurnId, call_id: ToolCallId, chunk: ProgressChunk },
    ToolCallFinished { turn: TurnId, call_id: ToolCallId, result: ToolResultBlock, display: Option<ToolDisplay>, duration_ms: u64 },

    UsageReported { turn: TurnId, step: u32, usage: Usage, cost_micros: Option<u64> },
    CheckpointCreated { turn: TurnId, checkpoint: CheckpointId },
    Compacted { turn: TurnId, replaced_through: SessionSeq, summary_blob: BlobHash },
    MediaJobUpdated { job: MediaJobId, status: MediaStatus },

    Notice { level: NoticeLevel, code: String, message: String },
    TurnFinished { turn: TurnId, stop_reason: StopReason, total_usage: Usage },
    TurnCancelled { turn: TurnId },
    TurnFailed { turn: TurnId, error: ErrorEnvelope },
}
```

Persistence classification: the `*Delta`, `ToolArgumentsDelta` and `ToolProgress` variants are ephemeral (never written). Everything else is appended to the event log in order with its `SessionSeq`. `ToolProgress` final aggregated output lives in `ToolCallFinished`. On crash mid-turn, replay of the log yields completed blocks only; the agent appends a synthetic `TurnFailed { code: "interrupted" }` on next open and synthesizes `ToolCallFinished` with `is_error` and text "interrupted before completion" for any dangling `ToolCallStarted`, so history stays valid for providers (every tool_use gets a tool_result).

Ordering guarantees: within a turn, events are totally ordered by `SessionSeq`. `AssistantBlock` for a step precedes the `ToolCallStarted` events of that step. `ToolCallFinished` for all calls of a step precede the next step's `RequestSent`.

## 6. Error taxonomy

One top-level envelope for the wire and UI, typed errors per layer in Rust (`thiserror`).

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub user_action: Option<UserAction>,
    pub request_id: Option<String>,
    pub correlation_id: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    AuthMissing, AuthInvalid, PermissionDenied, RateLimited, Overloaded, QuotaExceeded,
    InvalidRequest, ContextExceeded, ContentRefused, ModelUnavailable,
    NetworkUnreachable, Timeout, TruncatedStream, ProviderProtocol,
    ToolInvalidInput, ToolFailed, ToolTimeout, PolicyDenied, SandboxFailure,
    McpUnavailable, McpProtocol,
    StoreCorrupt, StoreBusy, MigrationFailed, KeyringUnavailable,
    ConfigInvalid, Cancelled, Internal,
}
```

HTTP mapping (both providers): 400 -> `InvalidRequest` (special case: message mentioning prompt/context length -> `ContextExceeded`), 401 -> `AuthInvalid`, 403 -> `PermissionDenied`, 404 on model -> `ModelUnavailable`, 408 -> `Timeout`, 413 -> `InvalidRequest`, 429 -> `RateLimited` (or `QuotaExceeded` when body type indicates billing/insufficient quota; not retryable), 500/502/503/504 -> `Overloaded` retryable, 529 -> `Overloaded` retryable. Provider `request-id` header is captured into `request_id` for every error and logged.

Layer types:

- `ProviderError { Http{status, body_excerpt, request_id, retry_after}, Transport, Stream, TruncatedStream, MalformedToolArguments, Protocol, Auth, Cancelled }`. `body_excerpt` is capped at 2 KiB and passed through the secret redactor.
- `ToolError { Sandbox, Io, Internal, Cancelled, Timeout }`.
- `PolicyError { InvalidRule, ManagedConflict }`.
- `StoreError { Busy, Corrupt, Migration, Constraint, Io }`.
- `AgentError`: wraps all of the above plus `ProviderSwitchMidTurn`, `SessionBusy` (a second turn started while one is running), `CheckpointMissing`.

Classification rules: `retryable` is true only for `RateLimited`, `Overloaded`, `Timeout`, `NetworkUnreachable` before stream start. `user_action` is a closed enum (`AddApiKey`, `ReplaceApiKey`, `ReduceContext`, `ChangeModel`, `Retry`, `OpenSettings`, `ReviewPolicy`) so the UI renders deterministic remedies. Error messages never include request bodies, prompts, headers or key material; only status, request id, and redacted excerpt.

Alertable taxonomy for logs/metrics: every error increments `bh_errors_total{code,layer}`. `StoreCorrupt`, `MigrationFailed`, `SandboxFailure` and `ProviderProtocol` are "page-level" and always show a persistent banner in the UI (a harness is local, the "alert" is the banner plus a diagnostics bundle export).

## 7. Observability contract

`tracing` with a JSON layer to `%LOCALAPPDATA%\BarryHarness\logs\` (daily rotation, 14 days). Span hierarchy: `session > turn > step > provider_request | tool_call`. Every span carries `correlation_id` (= `TurnId` for agent work) and `session_id`. A global redactor layer scrubs strings matching key patterns (`sk-ant-`, `sk-`, bearer tokens) and never logs message text, tool arguments or tool output at INFO or above; those are DEBUG and off by default, and the debug setting resets on restart. Metrics (in-process counters exposed to a diagnostics panel and the CLI `bh doctor`): request count/latency/errors by provider (RED), tokens by kind, cache hit ratio, tool latency, queue depth of the IPC emitter (USE).
