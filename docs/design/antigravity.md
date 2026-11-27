# Antigravity provider: architecture

`bh-antigravity` lets Barry Harness use the models of a signed-in Antigravity installation. It never touches Google credentials. Antigravity's local language server already owns the account, quota and entitlement, and exposes a Connect (JSON over HTTP) API on the loopback interface. The provider drives that local hub the same way the Antigravity client does.

## Why the local hub

The cloud endpoint behind Antigravity is gRPC over HTTP/2 with reflection disabled, and its generation call requires an entitlement message that the client builds from account state it obtains elsewhere. Reproducing it would mean copying OAuth client material and private request shapes out of the Antigravity binary. The local hub avoids all of that: it accepts plain JSON, authenticates the caller with a per-process CSRF token, and handles tokens, refresh and entitlement on its own.

## Hub discovery

1. Antigravity writes one `hub_*.json` file per language server process into `~/.gemini/antigravity/daemon/`. Each file carries `pid`, `httpPort` and `csrfToken`.
2. The files outlive their servers, so stale ones are common. The provider reads at most 64 KiB per file, ignores files without a port or a token, and sorts the rest newest first.
3. It lists running `language_server*` processes (`tasklist` on Windows, `ps` elsewhere, with a 5 second timeout) and keeps only files whose `pid` is alive. If the process list cannot be read, every file stays a candidate.
4. The first candidate that answers `GetAvailableModels` wins. When none answers, the user sees "Antigravity is not running. Open Antigravity and sign in, then send again."

The HTTP client is restricted to `127.0.0.1`, bypasses proxies, never follows redirects, and marks the CSRF header as sensitive so it never appears in logs.

## Calls

Every call is `POST http://127.0.0.1:<httpPort>/exa.language_server_pb.LanguageServerService/<Method>` with these headers:

```
content-type: application/json
connect-protocol-version: 1
x-codeium-csrf-token: <csrfToken>
```

| Method | Request | Purpose |
| --- | --- | --- |
| `GetAvailableModels` | `{}` | Account catalog: model id, display name, thinking support and budget, token limits, remaining quota fraction and reset time, supported MIME types. |
| `StartCascade` | `{"source": 1}` | Creates a conversation and returns `cascadeId`. Source 1 is the cascade client; string names are rejected. |
| `StreamAgentStateUpdates` | envelope, see below | Server stream of conversation state. |
| `SendUserCascadeMessage` | see below | Submits a user turn. Answers `200 {}` at once; the work happens on the stream. |
| `CancelCascadeInvocation` | `{"cascadeId": "<id>"}` | Cancels the running turn. |

Connect JSON uses the lowerCamelCase form of every protobuf field name, and enum values travel as their full string names. Unknown enum names are dropped silently by the server's parser, so a misspelt model looks like a missing one.

## Model resolution

`GetAvailableModels` returns `models` as a map keyed by model id. Each value's `model` field is the enum name to send, for example `MODEL_PLACEHOLDER_M320` for `gemini-3.8-flash-low`. Placeholder names differ between accounts and releases, so the provider always resolves them from the catalog and never hard-codes them. Catalog entries marked `disabled` fail with an error step.

## Sending a turn

```json
{
  "cascadeId": "<id from StartCascade>",
  "items": [{ "text": "<prompt>" }],
  "cascadeConfig": {
    "plannerConfig": {
      "planModel": "<enum name>",
      "requestedModel": { "model": "<enum name>" },
      "toolConfig": { "forceDisableToolCalling": true }
    }
  }
}
```

The relevant schema, taken from the protobuf descriptors the hub serves to its own client:

| Message | Field | Number | Type |
| --- | --- | --- | --- |
| `SendUserCascadeMessageRequest` | `cascadeId` | 1 | string |
| | `items` | 2 | repeated `TextOrScopeItem`, oneof `chunk`, `{"text": "..."}` |
| | `cascadeConfig` | 5 | `CascadeConfig` |
| | `blocking` | 8 | bool |
| | `media` | 14 | repeated `Media` |
| `CascadeConfig` | `plannerConfig` | 1 | `CascadePlannerConfig` |
| `CascadePlannerConfig` | `planModel` | 1 | enum `Model` |
| | `maxOutputTokens` | 6 | uint32 |
| | `toolConfig` | 13 | `CascadeToolConfig`, `forceDisableToolCalling` (49, bool) |
| | `requestedModel` | 15 | `ModelOrAlias`, oneof `model` or `alias` |

Either `planModel` or `requestedModel` starts the run; the provider sends both, as the Antigravity client does. `forceDisableToolCalling` keeps the hub's own agent from calling tools, which is what a chat provider needs. Without a `cascadeConfig` the run never leaves idle. Every turn on a cascade carries the config again.

## Stream

The provider opens `StreamAgentStateUpdates` before sending the message, with `content-type: application/connect+json`. Request and response use the Connect envelope: one flags byte, a big-endian u32 length, then the JSON payload. The request payload is `{"conversationId": "<cascadeId>"}`; the cascade id is the conversation id. Each server message is `{"update": AgentStateUpdate}`. A flags byte with bit `0x02` marks the Connect end-of-stream message, but the hub does not close the stream when a turn ends, so the provider decides when to stop reading.

`AgentStateUpdate` fields used:

- `status`: `CASCADE_RUN_STATUS_IDLE` 1, `RUNNING` 2, `CANCELING` 3, `BUSY` 4.
- `fullyIdle`.
- `mainTrajectoryUpdate.stepsUpdate`: parallel arrays `indices` and `steps`, plus `totalLength`.

A turn looks like this:

1. Snapshot: `status` idle, every existing step with its index, `totalLength` equal to the number of steps so far. The provider records `totalLength` and ignores steps below it, because the snapshot replays the whole history.
2. `status` running, then a `CORTEX_STEP_TYPE_USER_INPUT` step that is done.
3. A `CORTEX_STEP_TYPE_PLANNER_RESPONSE` step, repeated with status `CORTEX_STEP_STATUS_GENERATING`.
4. The same step with status `CORTEX_STEP_STATUS_DONE` and `plannerResponse.stopReason`, normally `STOP_REASON_STOP_PATTERN`.
5. Metadata updates, then `status` idle with `fullyIdle` true.

The turn is complete when the planner step is done and an idle status follows a running one.

### Text and reasoning

- Reply text is `steps[k].plannerResponse.response` for the planner step at index `indices[k]`.
- It is cumulative: each update carries the full text so far, often repeating the previous value. The provider emits `response[len(previous):]` per step index as the delta.
- Thinking models fill `plannerResponse.thinking` the same cumulative way before `response` starts; it is mapped to reasoning deltas.

### Errors and cancellation

Failures still answer `200 {}` to `SendUserCascadeMessage`. They arrive on the stream as a `CORTEX_STEP_TYPE_ERROR_MESSAGE` step with the text in `steps[k].errorMessage.error` (`userErrorMessage`, `modelErrorMessage`, `shortError`, `fullError`). The provider reports the first non-empty of `shortError`, `modelErrorMessage` and `userErrorMessage`, truncated, and never forwards `fullError`, which contains a server stack trace. Model errors map to `ModelUnavailable`, quota errors to `QuotaExceeded`, everything else to `ProviderProtocol`.

| Cause | `shortError` |
| --- | --- |
| no `cascadeConfig`, or an unknown enum name | `failed to construct executor: plan model not specified; you must specify a valid model` |
| a model the account lacks | `failed to construct executor: unknown model key <name>: model not found` |
| a disabled catalog entry | `NOT_FOUND (code 404): Requested entity was not found.` |

Cancel calls `CancelCascadeInvocation`. The stream then shows `CANCELING`, the planner step finishes with `STOP_REASON_CLIENT_STREAM_ERROR` and its partial text, an error step is appended and the run goes idle. That error step stays in the history the hub's model sees on the next turn.

## Limits

- Tool use through the hub is disabled. Enabling it would produce further step types (tool calls, command runs, approval waits) that need their own mapping.
- Quota exhaustion and signed-out states arrive as error steps; quota is recognised by keyword, other wording is reported as is.
- The hub never ends a stream, so each turn bounds its read with the first-event and idle timeouts from `bh_antigravity::Timeouts`.
