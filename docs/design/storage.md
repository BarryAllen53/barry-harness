# Barry Harness: Storage Design (bh-store)

Status: design target. Release 0.1.0 keeps chat history in a JSON file (`%APPDATA%\BarryHarness\sessions.db`) and settings in `settings.json`; the SQLite schema, event log, blobs, migrations and backups below are not implemented yet. See `implementation-status.md`.

## 1. Decisions and assumptions

- Single local user, single desktop process (plus the CLI, which may open the same database). Expected scale: tens of projects, thousands of sessions, up to ~100k events per large session, blobs up to hundreds of MB total. SQLite is more than sufficient; no server database.
- rusqlite 0.43 with the `bundled` feature (pinned SQLite, no system dependency on Windows). JSON1 and FTS5 enabled.
- Append-only event log is the source of truth for sessions; other tables are indexes or projections that can be rebuilt from it, except `projects`, `blobs`, `media_jobs` and settings.
- Secrets never touch SQLite or any file under the data directory. They live only in the OS keyring (section 7).
- Data directory: `%LOCALAPPDATA%\BarryHarness\` containing `barry.db` (+ `-wal`, `-shm`), `blobs\` (content-addressed files), `logs\`, `backups\`. Overridable via `BH_DATA_DIR` for tests and the CLI. Config (non-secret, user-editable) lives in `%APPDATA%\BarryHarness\config.toml` and is owned by `bh-config`, not the database.

## 2. Connection model

- One dedicated writer thread owning a single `rusqlite::Connection`; all writes are sent over a bounded `tokio::sync::mpsc` (capacity 1024) as closures-free command enums, executed in batches inside one transaction (group commit: drain up to 256 commands or 5 ms). This removes `SQLITE_BUSY` between writers and gives backpressure to the agent when the disk stalls.
- A read pool of N=4 read-only connections (`SQLITE_OPEN_READ_ONLY`) used through `spawn_blocking`. WAL lets reads proceed during writes.
- The CLI, when run while the app is open, opens the database as another process: writes go through the same busy-timeout path (`busy_timeout = 5000`) and are short; a session is locked at the application level by `sessions.lock_owner` + `lock_heartbeat` (section 3.3) so two processes never drive the same session.

Pragmas, applied on every connection open:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA temp_store = MEMORY;
PRAGMA trusted_schema = OFF;
PRAGMA cell_size_check = ON;
```

`synchronous = NORMAL` in WAL can lose the last transactions on power failure but never corrupts; acceptable for a local harness and the agent re-validates dangling tool calls on open (contracts.md section 5). The event append of a `TurnFinished` is written with `PRAGMA synchronous = FULL` semantics by issuing `PRAGMA wal_checkpoint(PASSIVE)` after turn completion; this is a cheap durability point per turn.

## 3. Schema (migration 0001)

All timestamps are INTEGER unix milliseconds UTC. Ids are TEXT (UUIDv7). `STRICT` tables everywhere.

### 3.1 projects

```sql
CREATE TABLE projects (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    root_path     TEXT NOT NULL,
    root_path_key TEXT NOT NULL UNIQUE,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    archived_at   INTEGER,
    settings_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(settings_json))
) STRICT;
```

`root_path` is the canonical display path. `root_path_key` is the normalized comparison key (see security.md path rules: canonicalized, `\\?\` stripped, case-folded with Unicode simple folding, trailing separator removed), UNIQUE so the same folder cannot be added twice under different spellings. `settings_json` holds project-scoped non-secret overrides (default model, effort, permission mode, allowed MCP servers); validated by `bh-config` against a schema on read.

Soft delete via `archived_at` (a project's sessions are history the user may want back). Hard delete is an explicit "Delete project data" action that removes rows in one transaction and then garbage-collects blobs.

### 3.2 provider accounts (metadata only)

```sql
CREATE TABLE provider_accounts (
    id          TEXT PRIMARY KEY,
    provider    TEXT NOT NULL CHECK (provider IN ('anthropic','openai')),
    label       TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 100),
    key_ref     TEXT NOT NULL UNIQUE,
    base_url    TEXT,
    created_at  INTEGER NOT NULL,
    last_used_at INTEGER,
    last_four   TEXT CHECK (last_four IS NULL OR length(last_four) = 4)
) STRICT;
```

`key_ref` is the keyring account name (section 7). `last_four` is the last four characters of the key for display; it is the only key-derived value stored. `base_url` is optional (enterprise gateway/proxy) and must be https unless host is loopback; validated at write.

### 3.3 sessions

```sql
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    title           TEXT NOT NULL DEFAULT '',
    title_source    TEXT NOT NULL DEFAULT 'auto' CHECK (title_source IN ('auto','user')),
    provider        TEXT NOT NULL CHECK (provider IN ('anthropic','openai')),
    model           TEXT NOT NULL,
    effort          TEXT NOT NULL,
    permission_mode TEXT NOT NULL CHECK (permission_mode IN ('plan','ask','accept_edits','auto')),
    account_id      TEXT REFERENCES provider_accounts(id) ON DELETE SET NULL,
    last_seq        INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    archived_at     INTEGER,
    parent_session  TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    forked_at_seq   INTEGER,
    lock_owner      TEXT,
    lock_heartbeat  INTEGER,
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cost_micros     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX sessions_project_updated ON sessions(project_id, archived_at, updated_at DESC);
CREATE INDEX sessions_parent ON sessions(parent_session) WHERE parent_session IS NOT NULL;
```

Notes:

- `last_seq` is the high-water mark of the event log, updated in the same transaction as each event append (monotonic counter source; see 3.4).
- Token and cost columns are a denormalized running total, justified by the session list needing them without scanning events; they are rebuildable from `UsageReported` events by `bh-store::rebuild_projections`.
- Fork: a fork copies no events; it records `parent_session` and `forked_at_seq`, and the projection reader concatenates parent events up to `forked_at_seq` then own events. Depth is capped at 8 (reads flatten into one materialized prefix when depth exceeds the cap).
- Lock: `lock_owner` is a random per-process id; a lock is stale if `lock_heartbeat` is older than 30 s. Acquire is a single `UPDATE ... WHERE lock_owner IS NULL OR lock_heartbeat < :now - 30000` and checking `changes() == 1`. The running agent refreshes every 10 s.
- Soft delete via `archived_at`; "Delete session" is hard delete (events cascade) and is a distinct, confirmed action.

### 3.4 event log (append-only)

```sql
CREATE TABLE events (
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    id          TEXT NOT NULL UNIQUE,
    turn_id     TEXT,
    kind        TEXT NOT NULL,
    payload     BLOB NOT NULL,
    payload_enc INTEGER NOT NULL DEFAULT 0 CHECK (payload_enc IN (0,1)),
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (session_id, seq)
) STRICT, WITHOUT ROWID;

CREATE INDEX events_turn ON events(session_id, turn_id, seq) WHERE turn_id IS NOT NULL;
CREATE INDEX events_kind ON events(session_id, kind, seq);

CREATE TRIGGER events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;

CREATE TRIGGER events_no_delete BEFORE DELETE ON events
WHEN (SELECT 1 FROM sessions WHERE id = OLD.session_id) IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
```

- `payload` is the JSON serialization of the `AgentEvent` (UTF-8 JSON bytes; `payload_enc = 0`). Reasoning blocks inside are the raw bytes via `RawValue`, so the stored JSON retains the provider's exact text for the `raw` field. `payload_enc = 1` is reserved for zstd-compressed payloads when a payload exceeds 64 KiB (level 3); decoding is transparent in `bh-store`. Payloads above 1 MiB are not allowed: large tool output and images are externalized to blobs and the event stores a `BlobHash`.
- `kind` mirrors the `AgentEvent` `type` tag, stored in a column so the log can be filtered and indexed without parsing JSON. The set of persisted kinds is fixed by contracts.md section 5.
- `seq` allocation: inside the writer transaction, `UPDATE sessions SET last_seq = last_seq + 1 ... RETURNING last_seq` yields the next seq; guarantees gap-free ordering even across processes.
- The `events_no_delete` trigger allows deletion only through the cascade of a session delete (the parent row is already gone when the cascade fires, so the `WHEN` guard is false). A direct `DELETE FROM events` fails.
- Compaction never rewrites events. It appends a `Compacted` event; the projection reader skips events with `seq <= replaced_through` (except `Compacted` itself and user-visible ones for the UI timeline) when building provider history. The UI continues to show the full log.
- `payload` blob size and event counts are bounded by retention only through explicit user action; no automatic deletion.

Full-text search over message text:

```sql
CREATE VIRTUAL TABLE event_text USING fts5(
    text,
    session_id UNINDEXED,
    seq UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2',
    content = ''
);
```

Populated by the writer for `UserMessage` and `AssistantBlock(text)` events only (never reasoning, tool args or tool output, to keep secrets and noise out of the index). Contentless table, so deletion on session delete is done by `bh-store` issuing the FTS5 `'delete'` command with the original text during hard delete; this requires recomputing text from events before they are cascaded, handled in one transaction.

### 3.5 checkpoints

A checkpoint is a restorable workspace state at a turn boundary, used for rewind. File content is deduplicated through `blobs`.

```sql
CREATE TABLE checkpoints (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id     TEXT NOT NULL,
    event_seq   INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    label       TEXT
) STRICT;

CREATE INDEX checkpoints_session ON checkpoints(session_id, event_seq);

CREATE TABLE checkpoint_files (
    checkpoint_id TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
    rel_path      TEXT NOT NULL,
    blob_hash     TEXT REFERENCES blobs(hash),
    existed       INTEGER NOT NULL CHECK (existed IN (0,1)),
    mode_readonly INTEGER NOT NULL DEFAULT 0 CHECK (mode_readonly IN (0,1)),
    PRIMARY KEY (checkpoint_id, rel_path)
) STRICT, WITHOUT ROWID;
```

Semantics: lazy per-file pre-image capture. A checkpoint is created at the start of each turn. The first time a file is modified or deleted in that turn (reported via `SideEffect::FileWritten.before` / `FileDeleted.before`), a row records the file's pre-turn content (`existed = 1` with blob) or its absence (`existed = 0`, `blob_hash NULL`, for newly created files). Rewind to checkpoint C means: for every row in checkpoints with `event_seq >= C.event_seq` in this session, restore in reverse order (newest first) each file to its recorded pre-image (delete if `existed = 0`), then append a `Rewound` event and mark the projection head. Files changed outside the harness are detected: before restoring, the current on-disk SHA-256 is compared to the `after` hash from the last write event; a mismatch is surfaced as a conflict (user chooses overwrite or skip). Only files touched by harness file tools are tracked; shell side effects are not (explicitly stated in the UI: "shell changes are not rewound").

Restrictions: files over 20 MiB and binary types on a deny list are not snapshotted; the write tool refuses (policy `FileTooLargeForCheckpoint`) unless the user opts out for that path.

### 3.6 blobs

```sql
CREATE TABLE blobs (
    hash        TEXT PRIMARY KEY CHECK (length(hash) = 64),
    size        INTEGER NOT NULL CHECK (size >= 0),
    mime        TEXT NOT NULL,
    storage     TEXT NOT NULL CHECK (storage IN ('file','inline')),
    inline_data BLOB,
    compressed  INTEGER NOT NULL DEFAULT 0 CHECK (compressed IN (0,1)),
    created_at  INTEGER NOT NULL,
    last_ref_at INTEGER NOT NULL,
    CHECK ((storage = 'inline') = (inline_data IS NOT NULL))
) STRICT;

CREATE TABLE blob_refs (
    hash        TEXT NOT NULL REFERENCES blobs(hash),
    owner_kind  TEXT NOT NULL CHECK (owner_kind IN ('event','checkpoint','media_job','project')),
    owner_id    TEXT NOT NULL,
    PRIMARY KEY (hash, owner_kind, owner_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX blob_refs_owner ON blob_refs(owner_kind, owner_id);
```

- Content addressed by SHA-256 of the uncompressed bytes. Blobs <= 16 KiB are stored `inline` in the row; larger ones are files at `blobs\<hash[0..2]>\<hash[2..4]>\<hash>` (fan-out avoids huge directories). Files are written to `blobs\tmp\<random>`, fsynced, then atomically renamed; the DB row is inserted after the rename, so a crash leaves at most an orphan file (collected by GC), never a row without a file.
- Text blobs over 4 KiB are zstd-compressed (`compressed = 1`); images are stored as is.
- Reference tracking via `blob_refs`, inserted in the same transaction as the owning row. GC (`bh-store::gc_blobs`) runs at startup (after 60 s idle) and on user request: delete `blobs` rows with no `blob_refs` and `last_ref_at` older than 24 h, then unlink files; then scan the `blobs` directory for files lacking a row older than 24 h and unlink them. Grace period protects in-flight writes.
- Integrity: reads verify the hash for files only in `bh doctor --verify`, not on every read.

### 3.7 media jobs

```sql
CREATE TABLE media_jobs (
    id            TEXT PRIMARY KEY,
    session_id    TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    provider      TEXT NOT NULL CHECK (provider IN ('openai')),
    operation     TEXT NOT NULL CHECK (operation IN ('generate','edit')),
    model         TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('queued','running','succeeded','failed','cancelled')),
    request_json  TEXT NOT NULL CHECK (json_valid(request_json)),
    idempotency_key TEXT NOT NULL UNIQUE,
    attempt       INTEGER NOT NULL DEFAULT 0,
    error_code    TEXT,
    error_message TEXT,
    cost_micros   INTEGER,
    created_at    INTEGER NOT NULL,
    started_at    INTEGER,
    finished_at   INTEGER
) STRICT;

CREATE INDEX media_jobs_status ON media_jobs(status, created_at);
CREATE INDEX media_jobs_project ON media_jobs(project_id, created_at DESC);

CREATE TABLE media_outputs (
    job_id      TEXT NOT NULL REFERENCES media_jobs(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    blob_hash   TEXT NOT NULL REFERENCES blobs(hash),
    mime        TEXT NOT NULL,
    width       INTEGER,
    height      INTEGER,
    revised_prompt TEXT,
    PRIMARY KEY (job_id, idx)
) STRICT, WITHOUT ROWID;

CREATE TABLE media_inputs (
    job_id      TEXT NOT NULL REFERENCES media_jobs(id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('image','mask')),
    blob_hash   TEXT NOT NULL REFERENCES blobs(hash),
    PRIMARY KEY (job_id, idx, role)
) STRICT, WITHOUT ROWID;
```

`request_json` stores the sanitized request (prompt, size, quality, count, model; never keys). `b64_json` responses are decoded and stored as blobs immediately (the base64 never persists in the DB). Startup recovery: jobs in `running` with no live worker become `failed` with `error_code = 'interrupted'` (media generation is not safely resumable because OpenAI image calls are not idempotent server-side; user can re-run, which creates a new job with a new key). `idempotency_key` guards against double-submission from the UI/agent (same key returns the existing job). Cost is estimated before the call (shown in the approval prompt) and recorded after.

### 3.8 settings and housekeeping

```sql
CREATE TABLE app_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE permission_grants (
    id          TEXT PRIMARY KEY,
    project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,
    session_id  TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    rule_json   TEXT NOT NULL CHECK (json_valid(rule_json)),
    decision    TEXT NOT NULL CHECK (decision IN ('allow','deny')),
    created_at  INTEGER NOT NULL,
    CHECK (project_id IS NOT NULL OR session_id IS NOT NULL)
) STRICT;

CREATE TABLE audit_log (
    seq         INTEGER PRIMARY KEY AUTOINCREMENT,
    at          INTEGER NOT NULL,
    session_id  TEXT,
    category    TEXT NOT NULL,
    subject     TEXT NOT NULL,
    detail_json TEXT NOT NULL CHECK (json_valid(detail_json))
) STRICT;

CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;

CREATE TRIGGER audit_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit log is append-only'); END;
```

`audit_log` records security-relevant facts independent of sessions (so deleting a session does not erase them): API key added/removed (label only), permission grants, mode changes, `Auto` mode enabled, hook changes, MCP server added/trusted, sandbox failures, denied hard-deny attempts. It stores no prompt content and no secrets. Pruning is user-triggered only ("clear audit log" drops and recreates the triggers inside a migration-like admin routine that logs one `audit_cleared` row first).

## 4. Query patterns and indexes

Verify with `EXPLAIN QUERY PLAN` (a planned test asserts each query below uses the named index and no `SCAN` on `events`):

| Query | Index |
|---|---|
| Sessions of project, newest first | `sessions_project_updated` |
| Load session history: `WHERE session_id=? AND seq > ? ORDER BY seq` | PK (session_id, seq) |
| Events of a turn | `events_turn` |
| Latest `Compacted` for session | `events_kind` |
| Pending media jobs | `media_jobs_status` |
| Checkpoints newest first | `checkpoints_session` |

No N+1: the session list uses the denormalized totals; the history loader reads events in pages of 500 with keyset pagination on `seq`.

## 5. Migrations approach

- Forward-only, numbered SQL files embedded with `include_str!` in `bh-store/migrations/NNNN_name.sql`, applied by a small in-house runner (about 80 lines) instead of a crate: the requirements are tiny, and a dependency would add surface. Version tracked by `PRAGMA user_version`; a mirror row in `app_meta('schema_version')` and a checksum table is unnecessary because files are embedded and immutable once released (a planned CI check hashes released migration files against a committed manifest `migrations.lock` to detect edits).
- Each migration runs in one transaction (`BEGIN IMMEDIATE`). DDL is transactional in SQLite so a failure rolls back cleanly. Foreign keys are toggled off only for table-rebuild migrations (`PRAGMA foreign_keys = OFF` outside the transaction, `PRAGMA foreign_key_check` before commit).
- Before applying any pending migration to a non-empty database: copy `barry.db` using the SQLite online backup API to `backups\barry-v<from>-<timestamp>.db` and keep the last 3. If the backup fails, the migration is not applied and the app shows `MigrationFailed`.
- Downgrade protection: if `user_version` is higher than the binary's, the app refuses to open the database (`StoreCorrupt` with message "database is newer than this version") rather than risk data loss. No auto-downgrade.
- Expand and contract (applies once there is more than one released version): a column is added nullable or with default (expand) in release N, code writes both and reads new with fallback, backfill runs as a startup batch job in chunks of 1000 rows, and the old column is dropped (contract) in release N+1 or later. Table rebuilds use create-new / copy / drop-old / rename.
- Event payload evolution: `AgentEvent` variants are only added or gain `#[serde(default)]` fields; renames are done through `#[serde(alias)]`. Persisted JSON is never rewritten by migrations; readers handle all historical shapes. Unknown `kind` values are surfaced to the UI as a generic "unsupported event" row, never a load failure.
- Planned tests: (1) apply all migrations to an empty DB and compare the resulting schema to a committed `schema.snapshot.sql` dump; (2) for each released version N keep a fixture database file and test migrating N -> head with data intact; (3) kill-during-migration test via failpoint verifying rollback; (4) newer-database refusal test.

## 6. Integrity and recovery

- On startup run `PRAGMA quick_check` (fast); if it fails, move the database aside to `backups\corrupt-<timestamp>.db`, restore the newest backup if present, else start empty and show a blocking dialog naming the preserved file. A weekly (on app start, if last run over 7 days) online backup to `backups\` keeps 3.
- `bh doctor` (CLI) runs `integrity_check`, `foreign_key_check`, blob verification and projection rebuild verification (recompute `sessions` totals from events and diff).
- Disk-full and read-only filesystem map to `StoreError::Io` -> `ErrorCode::StoreBusy`/`StoreCorrupt` banner; the agent pauses (writer backpressure) rather than losing events: if an append fails, the turn is aborted with `TurnFailed` kept in memory and retried once the writer recovers; if still failing, the UI shows the unsaved events and offers export.

## 7. Secrets: keyring only

- `keyring` 4.2 with the Windows Credential Manager backend. Service name `BarryHarness`, account name `provider/<provider>/<account_id>` (the value stored in `provider_accounts.key_ref`). Value is the API key as a UTF-8 string (well under the Credential Manager blob limit).
- Write path: the UI sends the key once through the `secret_set` command (ipc.md), the backend stores it in the keyring and immediately zeroizes the buffer (`zeroize` on the `String`/`SecretString`). The key is never returned to the frontend; the UI receives only `{ account_id, label, last_four }`.
- Read path: `bh-config::SecretStore::get(&AccountId) -> SecretString` is called by the provider factory when constructing a client. The `SecretString` type (secrecy crate) has a redacting `Debug` and no `Serialize`; it is injected into the `Authorization`/`x-api-key` header via `HeaderValue::set_sensitive(true)` so it never shows in debug output.
- Environment fallback: `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` env vars are honoured by the CLI only when no stored account exists, and are never persisted.
- Not stored anywhere else: not in `config.toml`, not in `project.settings_json`, not in logs, not in events, not in crash dumps (panic hook scrubs; `RUST_BACKTRACE` output contains no secret because keys live only in `SecretString`). MCP server credentials follow the same rule: MCP server configs reference a keyring entry (`mcp/<server>/<var>`) for each secret env var or header, never the literal value. Exporting a project or session (backup, transcript) excludes `provider_accounts` and everything keyring-related by construction (export only reads `events`, `blobs`, `media_*`).
- Keyring failure (locked, unavailable) yields `ErrorCode::KeyringUnavailable` with `UserAction::OpenSettings`; there is no plaintext fallback, deliberately.
- A key removed from the keyring behind the app's back is detected at use (`NoEntry`) and surfaces `AuthMissing`.

## 8. Retention defaults

Nothing is deleted automatically except: orphan blobs (24 h grace), backups beyond 3, log files beyond 14 days. A storage panel shows size by project and session with delete actions. Tool-output blobs for events older than 90 days may be offered for pruning (the event stays; the blob reference becomes `pruned`), an opt-in setting off by default.
