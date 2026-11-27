# Barry Harness: Prompt Cache Planner

Lives in `bh-agent::cache`. Input: the assembled request skeleton (system segments, sorted tools, projected messages) plus per-session `CacheState`. Output: `CachePlan` (contracts.md section 2.1) consumed verbatim by adapters. The planner is a pure function (`plan(&Skeleton, &CacheState, &ProviderCapabilities, &PlannerConfig) -> (CachePlan, CacheState)`), which is what makes it testable.

## 1. Goals and invariants

1. Every request is a byte-stable extension of the previous request's prefix, except at deliberate, logged "epoch" changes.
2. Cache breakpoints are placed where the next request will read, not only where this request writes.
3. The harness never silently pays for useless writes: no breakpoint below the model's minimum cacheable prefix, none on content that will not recur.
4. What the planner decides is observable: each `RequestSent` event stores a `CachePlanSummary`, each `UsageReported` shows read/write tokens, so regressions are detectable from the log.

Ordering of the Anthropic prompt: `tools` -> `system` -> `messages`. Changing anything earlier invalidates everything after it. Specific invalidation facts used by the planner (from verified API behaviour): changing `tools` invalidates all levels; changing `tool_choice`, thinking mode or `effort` invalidates tools+system levels (i.e. the whole cache for practical purposes, because messages sit after system); breakpoints are max 4; 1h entries must precede 5m entries; a top-level `cache_control` (automatic) consumes one breakpoint; minimum cacheable prefix is 512 tokens on 5.5 models (value from capabilities table). The planner uses explicit breakpoints only, never top-level automatic, to keep control and deterministic accounting.

## 2. Deterministic serialization (precondition)

Without this nothing else works. Rules implemented in `bh-types::canonical` and used by both adapters:

1. Tools sorted: builtins by name, then MCP by `(server, name)`. The list is frozen per session in `CacheState.tool_set_hash` and changes only at an explicit epoch (MCP server connect/disconnect/tool-list-changed notification is queued and applied at the next turn boundary, never mid-turn; the user sees "tools changed, cache will be rebuilt").
2. `input_schema` JSON is canonicalized once at tool registration: object keys sorted lexicographically (RFC 8785-style, no insignificant whitespace, numbers normalized). The canonical bytes are cached on `ToolSpec`; adapters emit those bytes with `RawValue`, so the serializer cannot reorder.
3. Request bodies are produced with a single hand-ordered struct per provider (field order fixed by struct definition; no `HashMap` anywhere in request types; `serde_json` `preserve_order` irrelevant because we use `RawValue` for foreign JSON and structs for our own).
4. System text is composed from `SystemSegment`s ordered by `Stability` then fixed position. Nothing time-varying is allowed in segments of stability `Static` or `Project`. Volatile facts (current date, git status, open files, cwd listing) are NOT put in the system prompt; they go in a `Session`-stability segment only if constant for the session (e.g. OS, shell, project root), and otherwise into a trailing "environment" text block appended to the latest user message (after cached content). Date granularity: day only, and only in that trailing block.
5. History is replayed from persisted blocks, not re-rendered: tool results are the stored bytes (post-truncation, truncation done once at execution time and frozen), reasoning blocks are the opaque raw bytes, assistant text is the stored string. No re-formatting, re-truncation, timestamp injection or "helpful" rewrites between requests.
6. Image blocks reference blobs; their base64 encoding is deterministic from the blob bytes.
7. The trailing environment block is persisted as part of the user message that carried it, so replay is identical forever.

## 3. Anthropic planner

### 3.1 Breakpoint budget (4)

| Slot | Position | TTL | Rationale |
|---|---|---|---|
| B1 | last tool definition | 1h | tools rarely change in a session; one entry covers all tools |
| B2 | last `Static`+`Project` system segment | 1h | system prompt plus project instructions (AGENTS-style files) are stable for hours |
| B3 | "anchor": end of the last assistant/tool turn that the previous request already wrote (the previous request's tail breakpoint) | 5m | guarantees a read hit on the previous prefix even if the new tail write fails the lookback |
| B4 | final block of the final message (current tail) | 5m | writes the cache for the next request |

Ordering constraint satisfied: B1, B2 (1h) precede B3, B4 (5m).

If tools are empty, B1 is skipped and its budget goes to a mid-history anchor (B3b). If the stable prefix (tools+system) estimate is below `min_prefix_tokens`, B1/B2 are merged into a single breakpoint placed at B2 (cheaper and still below threshold means no caching until the first message pushes the prefix over; the tail breakpoints then cover it).

### 3.2 Algorithm

```
plan_anthropic(skel, state, caps, cfg):
    tokens = estimate_prefix_tokens(skel)                  // cumulative per block, conservative (chars/3.5 rounded up, images by size table)
    bps = []

    // 1. epoch detection
    key = hash(model, tools_hash, system_hash(Static+Project), tool_choice, effort, thinking_display)
    epoch_changed = key != state.epoch_key
    if epoch_changed: state.epoch += 1; state.anchor = None; log CacheEpochChanged(reason = diff(state.epoch_key, key))

    // 2. stable prefix
    stable_end = tokens_at(end_of_system_stable)
    if has_tools and tokens_at(last_tool) >= caps.min_prefix_tokens:
        bps.push(LastTool, 1h)
    if stable_end >= caps.min_prefix_tokens:
        bps.push(SystemSegment(last_stable_idx), 1h)

    // 3. anchor from previous request (read side)
    if state.anchor is Some(a) and a is a valid position in skel.messages
       and skel prefix up to a is byte-identical to state.anchor_prefix_hash:
        bps.push(MessageBlock(a), 5m)

    // 4. lookback guard
    //    distance from the last breakpoint before the tail to the tail must be <= caps.lookback_blocks (20),
    //    otherwise the tail write cannot find the previous entry and a full rewrite occurs.
    while blocks_between(last_bp_position, tail) > caps.lookback_blocks - 2 and bps.len() < caps.max_breakpoints - 1:
        bps.push(MessageBlock(midpoint(last_bp_position, tail)), 5m)

    // 5. tail
    tail = last block of last message (skip trailing environment block: place the breakpoint on the last block BEFORE the volatile environment block)
    if tokens_at(tail) >= caps.min_prefix_tokens:
        bps.push(tail, 5m)

    // 6. enforce budget, TTL ordering, dedupe
    bps = dedupe_by_position(bps); sort by position (tools < system < messages)
    while bps.len() > caps.max_breakpoints: drop the lowest-value one (prefer dropping midpoint guards, then anchor, never B1/B2/tail)
    assert ttl non-increasing along position

    state.anchor = tail (this request's tail becomes next request's anchor)
    state.anchor_prefix_hash = hash(prefix up to tail)
    return plan
```

Notes:

- The volatile trailing environment block sits after the tail breakpoint so it never poisons the next request's read; on the next request the previous request's environment block is part of the stored message, so the anchor must be positioned on the last block before it. `Skeleton` marks each block with `volatile_tail: bool` and the planner places breakpoints on the last non-volatile block.
- Parallel tool calls: a user message containing N `tool_result` blocks counts N blocks toward the lookback; the guard in step 4 handles bursts.
- Tool turns inside one agent loop: every step is a new request; the anchor/tail pair rolls forward each step (write the new tail, read the prior tail). Expected steady state per step: `cache_read ≈ previous full prefix`, `cache_write ≈ only the new blocks`, `input_tokens ≈ trailing uncached bits`.
- Subagents or any parallel request lanes get their own `CacheState` (separate prefixes).
- Cache TTL policy: tools+system at 1h because a coding session has pauses longer than 5 minutes and the prefix is large and reused across many sessions in a project (system+tools identical across sessions of a project, so a new session within the hour reads it for free). The conversation tail is 5m because it is rewritten every turn anyway. The cost trade-off: 1h writes cost more than 5m writes (prices from the user-editable price table); the 1h slots pay off after one extra read beyond 5m expiry. `PlannerConfig.long_ttl_stable = true` default; a setting can force 5m everywhere for users who do short bursts.
- Idle expiry handling: if `now - state.last_request_at > 5m`, the tail entries are expired: skip the anchor (step 3), because requesting a read on an expired anchor just wastes a breakpoint; if `> 1h`, the 1h entries are also expired, which is just a normal write.
- Compaction, rewind and fork: each produces a new message sequence; the planner is told via `CacheState::reset(reason)`, which clears the anchor and bumps the epoch. Fork can reuse the parent's stable prefix (tools+system identical) because B1/B2 are content-addressed by the API.

### 3.3 Thinking blocks and cache

Reasoning blocks are part of message content and are replayed byte-identical, so they are part of the cached prefix like any other block. Changing `thinking.display` mode or `effort` mid-session changes the key in step 1 (verified invalidation of tools+system level), so the UI warns: "Changing effort rebuilds the cache" and the planner logs the epoch. Switching `display` between `summarized` and `omitted` is therefore not offered as a per-turn toggle.

### 3.4 Size estimator

`estimate_prefix_tokens` must never over-promise (a breakpoint below the real threshold silently does nothing and wastes a slot). It uses a conservative undercount (chars/4 for code-heavy, chars/3.5 for prose; the planner uses chars/4 so it tends to skip rather than waste; the downside is delaying the first cacheable breakpoint slightly). It is recalibrated per session from observed usage: after each response, `ratio = (input + cache_read + cache_write) / estimated_total` updates an EMA (alpha 0.3) applied to the next estimates, bounded to [0.6, 1.6]. `count_tokens` (provider endpoint) is not called on the hot path.

## 4. OpenAI planner (Responses API)

OpenAI caching is automatic prefix matching with no breakpoints; the planner's job is prefix stability, routing affinity and TTL options.

1. Request order OpenAI hashes: tools/instructions/input items in order. We send `instructions` (system text, stable first), `tools` (sorted, canonical schemas, identical rules as section 2), then `input` items (replayed in original order including `reasoning` items with `encrypted_content`, `function_call` and `function_call_output`).
2. `store: false` and `include: ["reasoning.encrypted_content"]` always. No `previous_response_id`: the full item list is replayed each step (this is the price of zero retention; cache hits make it affordable).
3. Item replay is verbatim: stored `reasoning` item JSON (`raw`) is emitted unchanged, including `id`; `function_call` items replay `id`, `call_id`, `name`, `arguments` exactly as stored (`ToolUseBlock.provider_item_id` and `arguments`); the corresponding `function_call_output` follows. Order within the turn is the original emission order.
4. `prompt_cache_key`: set to `bh:{project_id_short}:{session_id}:{lane}` hashed to 32 hex chars (never raw ids or paths to the provider). Same key across all steps of a session keeps requests routed together; subagent lanes use a different `lane` suffix. Requests with the same prefix and key from many parallel lanes would overload one cache shard, so the key includes the lane.
5. `prompt_cache_options{ttl}`: sent only when `caps.cache.ttls` is non-empty (GPT-5.6+): `ttl = OneHour` when `long_ttl_stable` (default), mirroring the Anthropic 1h policy for the same reason. The planner emits `CachePlan.openai_ttl`; adapters omit the field otherwise. Because retention applies to the whole prefix here, there is no separate tail TTL.
6. Minimum prefix: OpenAI caches only prefixes at or above its minimum (value from capabilities, `min_prefix_tokens`, default 1024 and granularity in increments); the planner cannot influence below it; it only reports `cache_eligible = estimated_prefix >= min`.
7. Epoch key for OpenAI: `hash(model, tools_hash, instructions_hash, reasoning.effort, reasoning.summary, tool_choice)`; a change bumps epoch and is logged, same as Anthropic. Any change to the `reasoning` parameters is treated as a cache-breaking change conservatively.
8. Read side metric: `usage.input_tokens_details.cached_tokens`; expected steady state is cached close to the previous request's total input, rounded down to the provider's granularity.

## 5. Observability

- `CachePlanSummary` in `RequestSent`: epoch, breakpoint positions with token estimates and TTL, `openai_key_hash` (hash only), `estimated_prefix_tokens`, `prefix_hash` (SHA-256 of the canonical request prefix up to the tail).
- After each response, the planner emits a `CacheOutcome` into the `UsageReported` handling: `expected_read` (previous request's `prefix_hash` equal to this request's matching prefix, so tokens up to previous tail expected as read) vs `actual_read`. If `actual_read < 0.5 * expected_read` and the gap is not explained by TTL expiry or an epoch change, emit `Notice { code: "cache_miss_unexpected" }` and capture a diagnostic (the two request skeleton hashes by segment: tools, system, each message index) in the log to find the first diverging segment. This is the production tripwire for prefix-stability bugs.
- Dashboard metrics: per-session hit ratio (`cache_read / (cache_read + cache_write + input)`), cumulative saved cost estimate, epoch count with reasons.

## 6. Prefix-stability test plan

Status: planned. None of the checks in this section ship in release 0.1.0; the repository contains no test suites or fixtures yet. The section records the verification the design calls for.

Goal: prove, offline and deterministically, that consecutive requests share a byte-identical prefix and that breakpoints land as planned; then verify against the live APIs behind an opt-in flag.

### 6.1 Unit and property tests (planned, CI, no network)

1. **Canonical serialization**: for 1000 random JSON schemas (proptest), serialize through two different code paths and insertion orders; bytes identical. Round trip `canonical(canonical(x)) == canonical(x)`.
2. **Tool ordering**: registering tools in any permutation yields identical request bytes (proptest permutations); MCP tool-list change mid-turn does not alter the in-flight request chain and applies at the next turn boundary with an epoch bump.
3. **Prefix extension property** (the core one): generate random conversations (text, reasoning blocks with random opaque raw JSON, tool_use/tool_result groups, images, N up to 60 turns). Serialize request k and k+1 for both providers. Assert `body_k[..anchor_k] == body_{k+1}[..anchor_k]` at the byte level on the HTTP body (messages array prefix, compared on the serialized `messages` token stream up to the last message of request k), and that the only differences are appended blocks. Failure prints the first divergent byte offset and the owning segment.
4. **Opaque reasoning fidelity**: random thinking/redacted_thinking/reasoning items with adversarial content (unicode escapes `\u0000`, `\/`, key order, whitespace, huge `encrypted_content`/`signature`, duplicated keys); store -> load from SQLite -> serialize into the request; the substring of the request body for each block equals the original bytes exactly. Also assert ordering preserved relative to text and tool blocks.
5. **No type-filter regression**: a lint test (grep-based `xtask` check in CI) fails if `ContentBlock::Reasoning` is matched with a catch-all `_ =>` drop or `retain`/`filter` appears on `blocks` outside the allowed module; plus a behavioural test: a turn with `[reasoning, text, tool_use]` then tool_result then `[reasoning, text]` replays all four assistant blocks in the exact order.
6. **Planner golden tests**: table-driven fixtures (cases below) assert the exact breakpoint list:
   - empty tools, tiny system: no breakpoints, no panic.
   - large tools + system, first message: B1, B2, tail.
   - second request: B1, B2, anchor (= previous tail), tail.
   - long tool loop (60 blocks between requests): midpoint guards inserted, total <= 4.
   - parallel tool results (12 blocks in one message).
   - volatile environment block present: tail breakpoint sits before it.
   - TTL ordering: always `1h` entries before `5m` in position order.
   - idle > 5m: anchor skipped; idle > 1h: no stale assumptions.
   - below min prefix tokens: skipped breakpoints, and merge rule for B1/B2.
   - budget overflow never exceeds 4; never uses top-level `cache_control`.
7. **Epoch tests**: changing tool_choice, effort, thinking display, tool set, or model bumps epoch and clears the anchor; changing only message content does not. Cross-provider switch bumps epoch and drops foreign reasoning blocks from the request while preserving them in storage.
8. **Determinism under concurrency**: run the planner and serializer on multiple threads with shuffled hashing seeds (`RUSTFLAGS` differing hasher seeds via `ahash` random state in a test build); outputs identical.
9. **Volatile content guard**: a test scans the `Static`/`Project` system segment and tool descriptions produced by the real builders for date/time patterns, random ids, absolute temp paths and non-deterministic collections; fails on match.
10. **OpenAI items**: replay test asserting `reasoning`, `function_call`, `function_call_output` order and verbatim `id`/`call_id`/`arguments`; `prompt_cache_key` stable across steps of a session and different across lanes; omitted `prompt_cache_options` on models without the capability.

### 6.2 Recorded-wire tests (planned, CI, no network)

A planned local mock HTTP server (wiremock-style test utilities for `bh-anthropic`/`bh-openai`) would capture request bodies from a scripted multi-step agent run using recorded SSE fixtures (including `ping`, unknown event types, and `message_delta` cumulative usage). Assertions: request k+1 body is a byte extension of request k up to the expected point (section 6.1 test 3 applied to real captured bodies); headers include `anthropic-version: 2026-10-21`; no forbidden fields (`temperature`, `top_p`, `top_k`, `budget_tokens`) present in any body; at most 4 `cache_control` markers; none is top-level.

### 6.3 Live verification (planned, opt-in, `BH_LIVE_TESTS=1`, uses the developer's keys, `#[ignore]` by default, never in CI)

Scenario per provider, using a padded fixed system prompt over the minimum prefix (about 2000 tokens):

1. Request 1 (cold): expect `cache_write > 0` (Anthropic `cache_creation_input_tokens > 0`; OpenAI `cached_tokens == 0`).
2. Request 2 (same prefix + one new user message): expect `cache_read >= 0.9 * prefix_tokens` (Anthropic `cache_read_input_tokens`; OpenAI `cached_tokens > 0`).
3. Tool loop of 3 steps with a thinking-enabled model: each step reads prior prefix; assert the API accepts replayed reasoning blocks (no 400) and that hit ratio stays above 0.8 from step 2.
4. Negative control: change one early tool description byte; expect read drops to ~0 and an epoch change logged.
5. TTL check (Anthropic): assert `cache_creation.ephemeral_1h_input_tokens > 0` for the stable prefix and `ephemeral_5m_input_tokens > 0` for the tail.

Failures print the `CachePlanSummary` and diverging-segment diagnostic. A scheduled weekly run in a developer environment catches provider behaviour drift (new minimums, changed semantics).

### 6.4 Regression policy (planned)

Once the suites above exist, every production `cache_miss_unexpected` bug gets a minimized fixture added to a golden set, and the prefix-extension property test and planner goldens become a release gate. A snapshot of canonical request bytes for a sample conversation would be committed with them, and intentional changes would update the snapshot with a PR note about expected cache invalidation for existing sessions.
