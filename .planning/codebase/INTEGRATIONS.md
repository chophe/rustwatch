---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
# External Integrations

**Analysis Date:** 2026-10-02

## APIs & External Services

**LLM — activity classification (the only network calls in the codebase):**

- OpenAI Chat Completions — labels captured activity segments
  - SDK/Client: `reqwest` 0.12 (`rustls-tls`, no OpenSSL), `crates/rustwatch-analyze/src/classifier.rs:68-116`
  - Endpoint: `POST https://api.openai.com/v1/chat/completions`
  - Auth: `OPENAI_API_KEY` env var, sent as `Authorization: Bearer` (read at `classifier.rs:76`; missing key → `anyhow!("OPENAI_API_KEY not set")`)
  - Model: `analyze.model` from `config.toml` (default `gpt-4o-mini`); `response_format: {"type": "json_object"}`; system prompt demands `{"activities":[{label,category,confidence,started_at,ended_at,apps,topics}]}`
- Anthropic Messages — same role, same prompt, alternate provider
  - SDK/Client: `crates/rustwatch-analyze/src/classifier.rs:118-164`
  - Endpoint: `POST https://api.anthropic.com/v1/messages`
  - Auth: `ANTHROPIC_API_KEY` env var via `x-api-key` header, plus pinned `anthropic-version: 2023-06-01` (read at `classifier.rs:126`)
  - Model: `analyze.model` — **the default is `gpt-4o-mini`, an OpenAI name**; switching `analyze.provider = "anthropic"` without editing `analyze.model` sends an OpenAI model id to Anthropic
- Provider selection: `build_classifier` (`classifier.rs:13-19`) matches the `analyze.provider` string; anything other than `"openai"` / `"anthropic"` bails with `unsupported provider: {other}`
- No retries, no timeouts, no backoff, no rate-limit handling. One request per run, capped at 50 segments (`store.list_unanalyzed_segments(50)`, `classifier.rs:22`). Non-2xx surfaces via `error_for_status()`.
- Vision is **not** wired up: `analyze.vision_model` (`gpt-4o`) and `privacy.send_screenshots_to_llm` are config fields that nothing reads. Screenshots never leave the machine.

**macOS platform services (local, not network):**

- CGEventTap — global keyboard tap via `keytap` 0.4; blocked without Input Monitoring (`crates/rustwatch-capture/src/platform/macos.rs:105-107`)
- Active window metadata — `active-win-pos-rs` 0.11 (`macos.rs:229-238`); app name, window title, PID
- Screen/window capture — `xcap` 0.9; writes PNGs under `~/.rustwatch/screenshots/<YYYY-MM-DD>/` (`macos.rs:315-356`)
- Clipboard read — `arboard` 3 / NSPasteboard (`macos.rs:241-244`)
- Focused-text snapshot via Accessibility API — stubbed to return `None` (`macos.rs:246-249`)
- Permission probing — placeholder returning all-false (`macos.rs:45-55`)

**Local model runtime (optional):**

- `fastembed` 4.9.1 + ONNX Runtime (`ort` 2.0.0-rc.9) behind the `fastembed` cargo feature — downloads `BGE-small-en-v1.5` weights from HuggingFace on first use, producing 384-dim vectors (`crates/rustwatch-memory/src/embedder.rs:15-27`)
- Default builds skip this entirely and use a deterministic in-process hash embedding (`embedder.rs:46-61`) — no model download, no network

## Data Storage

**Databases:**

- SQLite via `rusqlite` 0.32.1 `bundled` — three separate database files under the data root, all created by `ensure_dirs`/`Store::open`:

| Database | Path | Schema | Owner |
|---|---|---|---|
| Primary | `~/.rustwatch/rustwatch.db` | `events`, `segments`, `screenshots`, `activities` (migration `V1__initial.sql`, refinery, WAL journal mode at `db.rs:22`) | `crates/rustwatch-core/src/db.rs` |
| Vector + FTS | `~/.rustwatch/memory.db` | `memory_chunks` (embedding stored as little-endian `f32` BLOB) + `memory_fts` FTS5 virtual table | `crates/rustwatch-memory/src/sqlite_store.rs` |
| Graph | `~/.rustwatch/memory-graph.db` | `graph_nodes`, `graph_edges` | `crates/rustwatch-memory/src/graph.rs` |

  - Connection: embedded, per-process, no server, no auth. WAL mode on the primary DB only.
  - Client: raw `rusqlite` — no ORM, no query builder. Hand-written SQL in each store.
  - Note: `MemoryEngine::open` (`crates/rustwatch-memory/src/lib.rs:22-29`) ignores `data.sqlite_path` and hardcodes `memory.db` / `memory-graph.db` under the root, so `config.toml` path overrides for those two are inert.
- LanceDB (`~/.rustwatch/lance/`) — vector store, table `memory_chunks` with a 384-dim `FixedSizeList` `vector` column and L2 `_distance` scoring. Declared in `crates/rustwatch-memory-backends/src/lance.rs`, **not built and not called by any binary** (the crate is outside the workspace members list and absent from `Cargo.lock`).
- SurrealDB (`~/.rustwatch/surreal/`) — graph store, but `SurrealGraphStore::open` uses `engine::local::Mem`, i.e. **in-memory only**; the configured path is created and never persisted to. Namespaces: `rustwatch` / db `memory`. Graph edges: `activity -used_app-> app`, `activity -about_topic-> topic`. Declared in `crates/rustwatch-memory-backends/src/surreal.rs`, likewise unbuilt.
- `Config.memory.vector_backend = "lancedb"` and `graph_backend = "surrealdb"` are the config defaults, but the runtime path is the SQLite pair above — the defaults describe a backend that does not run.

**File Storage:**

- Local filesystem only. Screenshots as PNG under `~/.rustwatch/screenshots/<date>/<millis>-<window|screen>.png`; activity charts as `~/.rustwatch/chart-<date>.html`; segment exports as `~/.rustwatch/export-<timestamp>.json`
- Daemon runtime files in the data root: `daemon.sock` (Unix socket), `daemon.pid`, `config.toml`

**Caching:**

- None. No cache layer, no Redis/memcached. The FTS5 `memory_fts` table in `memory.db` is maintained on write but never queried — `SqliteMemoryStore::search` (`sqlite_store.rs:86-115`) does a full-table scan plus in-process cosine similarity.

## Authentication & Identity

**Auth Provider:**

- None. No user accounts, no OAuth, no sessions, no multi-user model — the tool runs entirely as the invoking local user.
- "Authentication" is limited to two macOS TCC grants (Input Monitoring, Screen Recording) that are requested implicitly by the OS on first capture and detected by the OS, not by this code.
- API keys are read straight from the process environment at classifier construction (`classifier.rs:76`, `classifier.rs:126`). They are never persisted to `config.toml` or any file in the repo.

**Local trust boundary:**

- Unix domain socket at `~/.rustwatch/daemon.sock` accepts unauthenticated commands (`Ping`, `Status`, `Pause`, `Resume`, `Tail`, `Screenshot`) with length-prefixed JSON framing — 4-byte big-endian length + payload, in `crates/rustwatch-core/src/ipc.rs:51-86`. Protection is filesystem permissions on the socket only; there is no token or peer-credential check.

## Monitoring & Observability

**Error Tracking:**

- None. No Sentry or any external telemetry. Errors propagate as `anyhow`/`thiserror` values to the CLI and are printed (`crates/rustwatch-core/src/error.rs` defines the 7-variant `Error` enum).

**Logs:**

- `tracing` + `tracing-subscriber` with `EnvFilter::from_default_env()` → `RUST_LOG`, initialized in all three binaries: `rustwatch-cli/src/main.rs:76-81` (adds `tracing-indicatif` for progress bars), `rustwatch-daemon/src/main.rs:20-23`, `rustwatch-mcp/src/main.rs:11-14` (forced to stderr so stdout stays clean for JSON-RPC)
- Under launchd, stdout/stderr are redirected to `/tmp/rustwatchd.out.log` and `/tmp/rustwatchd.err.log` by `deploy/macos/com.rustwatch.plist`
- Daemon counters exposed over the socket via `DaemonState` (`ipc.rs:9-17`): `events_captured`, `segments_written`, `paused`, `started_at`
- Capture thread failures are logged at `warn!` and the thread exits silently (`macos.rs:72`, `macos.rs:82`); event-writer DB errors are swallowed with `let _ =` (`crates/rustwatch-daemon/src/main.rs:63-67`), so capture can silently stop persisting.

## CI/CD & Deployment

**Hosting:**

- Local macOS host only. No container, no cloud service, no web server.

**CI Pipeline:**

- None. No `.github/`, no `.gitlab-ci.yml`, no `rust-toolchain.toml`, no pre-commit config. `docs/TESTING_PLAN.md` Phase 0 proposes `.github/workflows/test.yml` (fmt → clippy → nextest on macOS + ubuntu) and `.config/nextest.toml`, but neither exists.

**Deployment mechanism:**

- `rustwatch install` (`crates/rustwatch-cli/src/commands.rs:18-36`) copies `deploy/macos/com.rustwatch.plist` to `~/Library/LaunchAgents/com.rustwatch.plist`, substituting `{{RUSTWATCHD_PATH}}` with the sibling `rustwatchd` binary path, then prints the `launchctl load -w` command for the user to run.
- `rustwatch start` spawns `rustwatchd` detached (`commands.rs:38-59`); `rustwatch stop` sends `SIGTERM` and removes `daemon.pid` + `daemon.sock` (`commands.rs:61-75`).
- The plist has `RunAtLoad` + `KeepAlive`, so the capture daemon restarts indefinitely — there is no first-run consent gate in the binary itself.

## Environment Configuration

**Required env vars:**

- `OPENAI_API_KEY` — required only when `analyze.provider = "openai"` (default)
- `ANTHROPIC_API_KEY` — required only when `analyze.provider = "anthropic"`
- `RUST_LOG` — optional, controls tracing filter in all binaries
- `HOME` — required for the default data root (`paths.rs:62`) and for `rustwatch install`'s LaunchAgents path (`commands.rs:19`)

**Secrets location:**

- Environment only. No `.env` file, no dotenv loader, no keychain/`security` integration, no secret file in the repo. Keys are held in memory for the lifetime of the process that builds the classifier.

**File config:**

- `~/.rustwatch/config.toml`, auto-created from `Config::default()` (`crates/rustwatch-core/src/config.rs:61-104`). Typed serde with no `#[serde(default)]`, so a partial file (e.g. adding only `[analyze]`) fails to parse — users must keep every field present.

## Webhooks & Callbacks

**Incoming:**

- No HTTP listener of any kind. The only inbound surfaces are the local Unix socket (`~/.rustwatch/daemon.sock`) and the MCP stdio transport.

**MCP server (stdio JSON-RPC 2.0) — the outbound-facing integration surface:**

- Served by `crates/rustwatch-mcp/src/main.rs`, one JSON object per stdin line. Handles `initialize` (protocolVersion `2024-11-05`), `tools/list`, `tools/call`
- Exposed tools: `search_activity_memory`, `get_activity_timeline`, `get_segment_context`, `pause_capture`, `resume_capture` (`main.rs:44-51`)
- `pause_capture` / `resume_capture` tunnel through to the daemon over the Unix socket (`main.rs:114-122`)
- Consumers per `README.md`: Cursor and Claude Desktop via an `mcpServers` entry pointing at the absolute `target/release/rustwatch-mcp` path
- The declared `rmcp` dependency (feature `rmcp`) is unused — the protocol is hand-rolled, so an unknown method returns `{"error": "..."}` inside a `result` field rather than a JSON-RPC `error` object (`main.rs:58`)

**Outgoing:**

- Only the two LLM POSTs listed above. No telemetry, no update checks, no error reporting, no third-party analytics.

**Data that leaves the machine:**

- Per classify request: segment id, app name, window title, start/end timestamps, and the segment text buffer — after regex redaction via `privacy.redact_patterns` (default `sk-[A-Za-z0-9]+`) and truncation to `memory.chunk_max_chars` (2000) in `Redactor::scrub` (`crates/rustwatch-analyze/src/redact.rs:25-34`)
- Segments from apps matching `capture.exclude_apps` (default `1Password`, `Keychain Access`) are dropped entirely before the request (`classifier.rs:28-35`)

---

*Integration audit: 2026-10-02*
