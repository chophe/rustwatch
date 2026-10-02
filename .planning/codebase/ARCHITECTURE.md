---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
<!-- refreshed: 2026-10-02 -->

# Architecture

**Analysis Date:** 2026-10-02

## System Overview

A local-first activity recorder for macOS. It runs as three cooperating processes over a shared SQLite
event log, with a fourth "embedded" process for MCP. Nothing is hosted; the only network egress is
outbound LLM classification.

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                         INTERFACE / BINARY LAYER                             │
│                                                                              │
│  `rustwatch` CLI        `rustwatch-mcp`         `rustwatchd` (daemon)        │
│  cli/src/main.rs:74     mcp/src/main.rs:9       daemon/src/main.rs:18        │
│  clap → commands.rs     stdio JSON-RPC loop    UnixListener accept loop      │
│  + tui.rs (ratatui)     + handle_tool():76     + IPC handler closure:102     │
└───────┬──────────────────────┬─────────────────────────┬─────────────────────┘
        │                      │                         │
        │ Unix socket          │ direct Store read       │ OS threads (capture)
        │ DaemonClient         │ MemoryEngine::search    │ keytap + active-win-pos-rs
        ▼                      ▼                         ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                            DOMAIN / SERVICE LAYER                            │
│                                                                              │
│  rustwatch-capture      rustwatch-analyze      rustwatch-memory              │
│  CaptureHandle          ActivityClassifier     MemoryEngine (facade)         │
│  → PlatformCapture      → OpenAI/Anthropic     → SqliteMemoryStore (vector) │
│    (macos.rs/stub.rs)   → Redactor (privacy)   → GraphStore (graph)          │
│                          → render_chart          → Embedder (hash/fastembed) │
│                                                    → GraphRag::merge          │
└───────┬──────────────────────┬─────────────────────────┬─────────────────────┘
        │                      │                         │
        └──────────────┬───────┴─────────────┬───────────┘
                       ▼                     ▼
        ┌──────────────────────────┐  ┌──────────────────────────────────┐
        │   rustwatch-core         │  │  SegmentGrouper (pure fold)      │
        │   CaptureEvent           │  │  event stream → SessionSegment    │
        │   SessionSegment         │  │  segment.rs:28                  │
        │   ActivityRecord         │  └──────────────┬───────────────────┘
        │   Config / DataPaths     │                 │
        │   Store (SQLite + refinery)◄───────────────┘
        │   ipc.rs (DaemonClient)  │
        └──────────────┬───────────┘
                       ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                            PERSISTENCE / OUTPUT                              │
│                                                                              │
│  ~/.rustwatch/rustwatch.db      WAL SQLite: events, segments, screenshots,    │
│                                 activities (migrations/V1__initial.sql)      │
│  ~/.rustwatch/memory.db         SQLite + FTS5: memory_chunks, memory_fts    │
│  ~/.rustwatch/memory-graph.db   SQLite: graph_nodes, graph_edges             │
│  ~/.rustwatch/screenshots/      PNG, sharded by YYYY-MM-DD                   │
│  ~/.rustwatch/daemon.sock       Unix socket (IPC)                           │
│  ~/.rustwatch/daemon.pid        PID file (liveness, not locking)            │
│  https://api.openai.com  |  https://api.anthropic.com   (only egress)       │
└──────────────────────────────────────────────────────────────────────────────┘
```

Crates outside the workspace graph: `rustwatch-memory-backends` (LanceDB + SurrealDB) depends on
`rustwatch-memory` and `rustwatch-core` but nothing depends on it.

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| `Store` | Sole gateway to `rustwatch.db`; owns connection, runs refinery migrations, all event/segment/activity/screenshot CRUD and stats | `crates/rustwatch-core/src/db.rs:12` |
| `SegmentGrouper` | Stateful fold converting a `CaptureEvent` stream into `SessionSegment`s on focus change; caps `text_buffer` at 16 384 chars | `crates/rustwatch-core/src/segment.rs:11` |
| `DataPaths` | Single authority for every on-disk path (db, memory, screenshots, socket, pid, config); creates dirs | `crates/rustwatch-core/src/paths.rs:8` |
| `Config` | TOML-deserialized settings tree (`data`/`capture`/`analyze`/`memory`/`ui`/`privacy`) with defaults; written on first run | `crates/rustwatch-core/src/config.rs:6` |
| `DaemonClient` / `handle_connection` | Length-prefixed JSON request/reply over a Unix socket; the whole IPC wire format | `crates/rustwatch-core/src/ipc.rs:40,70` |
| `CaptureHandle` | Facade over `PlatformCapture`; forwards start/pause/screenshot calls and the static `permissions()` probe | `crates/rustwatch-capture/src/lib.rs:10` |
| `PlatformCapture` (macOS) | Two OS threads (keytap keyboard loop, focus-poll loop) emitting `CaptureEvent`s onto an mpsc channel; xcap screenshots | `crates/rustwatch-capture/src/platform/macos.rs:23` |
| `PlatformCapture` (stub) | Non-macOS no-op returning `Error::UnsupportedPlatform` for every operation | `crates/rustwatch-capture/src/platform/stub.rs:14` |
| `ActivityClassifier` / `build_classifier` | Provider seam: `Box<dyn ActivityClassifier>` selected from `config.analyze.provider`; one impl per vendor | `crates/rustwatch-analyze/src/classifier.rs:9,13` |
| `analyze_pending` | The analysis pipeline: fetch unanalyzed segments → redact → classify → persist activities | `crates/rustwatch-analyze/src/classifier.rs:21` |
| `Redactor` | Privacy gate: regex scrub to `[REDACTED]`, byte truncation to `chunk_max_chars`, app-name exclusion | `crates/rustwatch-analyze/src/redact.rs:4` |
| `render_chart` | Pure renderer for one day's activities → terminal / JSON / HTML | `crates/rustwatch-analyze/src/chart.rs:11` |
| `MemoryEngine` | Facade tying embedder + vector store + graph store together; owns ingest, rebuild, and hybrid search | `crates/rustwatch-memory/src/lib.rs:14` |
| `SqliteMemoryStore` | Vector store: `memory_chunks` (embedding as little-endian `f32` BLOB) + FTS5 `memory_fts`; brute-force cosine scan | `crates/rustwatch-memory/src/sqlite_store.rs:22` |
| `GraphStore` | Property-graph store: activity→app (`used_app`), activity→topic (`about_topic`) edges; JSON `props_json` props | `crates/rustwatch-memory/src/graph.rs:5` |
| `Embedder` | 384-dim embedding; fastembed model behind the `fastembed` feature, deterministic hash fallback otherwise | `crates/rustwatch-memory/src/embedder.rs:7` |
| `GraphRag::merge` | Hybrid fusion: dedupe by `chunk_id` taking max score, +0.2 keyword boost, sort desc | `crates/rustwatch-memory/src/rag.rs:6` |
| CLI dispatch | clap grammar in `main.rs`, one `pub fn` per subcommand in `commands.rs`, TUI event loop in `tui.rs` | `crates/rustwatch-cli/src/main.rs:18`, `crates/rustwatch-cli/src/commands.rs:18` |
| MCP tool router | Hand-rolled JSON-RPC 2.0 over stdio: `initialize` / `tools/list` / `tools/call`, five tools | `crates/rustwatch-mcp/src/main.rs:25,76` |

## Pattern Overview

**Overall:** Layered Cargo workspace with a pipeline-bus core and process-per-interface front ends.

**Key Characteristics:**

- **Crates are layers, not features.** `rustwatch-core` depends on no sibling crate and is depended on by all of them. Every other crate depends only downward. The dependency graph is an acyclic DAG with no dev-dependency back-edges.
- **Three binaries, one shared database, no supervisor.** `rustwatchd` is the only long-running process; the CLI and the MCP server are short-lived or client-driven readers of the same files. There is no migration service, no IPC broker, no lock service.
- **The pipeline currency is `CaptureEvent`.** Capture, persistence, segmentation, IPC, and export all speak `CaptureEvent` (`crates/rustwatch-core/src/events.rs:51`). Everything downstream of capture is pure data manipulation over that type.
- **Configuration is file-first, not env-first.** `~/.rustwatch/config.toml` is created on first run by `load_or_create_config` (`crates/rustwatch-core/src/paths.rs:81`). Only API keys come from the environment.
- **Storage is deliberately pluggable-by-duplication, not by trait.** `Store`, `SqliteMemoryStore`, `GraphStore` are concrete structs. The *LLM provider* is the one trait seam (`ActivityClassifier`); the *memory backends* are not — `rustwatch-memory-backends` reimplements the same operations for LanceDB/SurrealDB without a shared trait, so substitution is impossible without editing `MemoryEngine`.

## Layers

**Interface layer (binaries):**

- Purpose: parse user or client input, open the store, format output.
- Location: `crates/rustwatch-cli/src/`, `crates/rustwatch-mcp/src/`, `crates/rustwatch-daemon/src/`
- Contains: clap `Commands` enum, one function per subcommand, a ratatui loop, a JSON-RPC dispatch table, a `UnixListener` accept loop.
- Depends on: `rustwatch-core`, `rustwatch-capture`, `rustwatch-analyze`, `rustwatch-memory`.
- Used by: humans (CLI/TUI) and MCP hosts (Cursor / Claude Desktop).

**Capture layer:**

- Purpose: turn macOS OS state into `CaptureEvent`s.
- Location: `crates/rustwatch-capture/src/`
- Contains: a thin `CaptureHandle` facade plus one `PlatformCapture` per OS behind `cfg` in `platform/mod.rs:1-9`.
- Depends on: `rustwatch-core` only. It does not know the database exists.
- Used by: `rustwatch-daemon` (continuous capture) and `rustwatch-cli` (one-shot screenshots + permission probe).

**Service layer:**

- Purpose: analysis, redaction, chart rendering, memory retrieval.
- Location: `crates/rustwatch-analyze/src/`, `crates/rustwatch-memory/src/`
- Contains: LLM classifiers, redaction, rendering, vector/graph stores, embedding, RAG fusion.
- Depends on: `rustwatch-core` (types + `Store`). Both take `&Store` / `&Config` as arguments; neither opens a connection itself — except `MemoryEngine::open`, which opens its own two databases from `DataPaths`.
- Used by: `rustwatch-cli`, `rustwatch-mcp`.

**Core layer:**

- Purpose: shared vocabulary, configuration, path resolution, the event database, and the IPC wire format.
- Location: `crates/rustwatch-core/src/`
- Contains: domain types, `Store`, `SegmentGrouper`, `DataPaths`, `Config`, `Error`, IPC codec.
- Depends on: nothing internal. This is the crate that keeps the workspace a DAG.
- Used by: every other crate, including the orphaned backends crate.

**Persistence:**

- Purpose: durable state.
- Location: `crates/rustwatch-core/migrations/V1__initial.sql` (compile-time embedded via `embed_migrations!` at `crates/rustwatch-core/src/db.rs:10`), plus schema created inline at `open()` in `crates/rustwatch-memory/src/sqlite_store.rs:32-52` and `crates/rustwatch-memory/src/graph.rs:15-30`.
- Contains: four tables for capture, two for vectors, two for the graph.
- Depends on: `refinery` (migrations) and `rusqlite` (bundled SQLite).
- Used by: `Store` and the memory stores.

## Data Flow

### Primary Request Path — capture → disk

1. `rustwatchd` starts, resolves paths, opens the store, spawns the writer task, binds the socket — `crates/rustwatch-daemon/src/main.rs:18-90`
2. `CaptureHandle::start` hands an mpsc `UnboundedSender<CaptureEvent>` to the platform, which spawns two OS threads — `crates/rustwatch-capture/src/platform/macos.rs:57-90`
3. The keyboard thread emits `Key` / `TextDelta` / `Paste`; the focus thread emits `FocusChange`, then optionally `Screenshot` and `TextFieldSnapshot` — `crates/rustwatch-capture/src/platform/macos.rs:111-161`, `crates/rustwatch-capture/src/platform/macos.rs:182-229`
4. The writer task pulls each event, feeds `SegmentGrouper::on_event`, and inserts event + any newly-closed segment + screenshot row — `crates/rustwatch-daemon/src/main.rs:58-84`
5. `Store::insert_event` serializes `CaptureEventKind` and `AppContext` into JSON TEXT columns — `crates/rustwatch-core/src/db.rs:29-46`
6. Rows land in `~/.rustwatch/rustwatch.db` in WAL mode — `crates/rustwatch-core/migrations/V1__initial.sql:3-24`

### Secondary Flow — CLI/MCP → daemon (IPC)

1. Client calls `DaemonClient::send(command)` — `crates/rustwatch-core/src/ipc.rs:51`
2. Connect to `~/.rustwatch/daemon.sock`; any connect failure collapses to `Error::DaemonNotRunning` — `crates/rustwatch-core/src/ipc.rs:52-54`
3. Write a 4-byte big-endian length, then the JSON body; read the same framing back — `crates/rustwatch-core/src/ipc.rs:56-66`
4. Daemon accepts, `tokio::spawn`s a per-connection task, and matches on `DaemonCommand` — `crates/rustwatch-daemon/src/main.rs:90-162`
5. Reply serialized back with the same length-prefix framing — `crates/rustwatch-core/src/ipc.rs:70-85`

### Analysis Flow — segments → activities

1. `rustwatch analyze` opens `Store`, shows an indicatif spinner — `crates/rustwatch-cli/src/commands.rs:188-205`
2. `analyze_pending` fetches up to 50 unanalyzed segments via `LEFT JOIN ... LIKE` on `segment_ids_json` — `crates/rustwatch-core/src/db.rs:157-180`
3. Excluded apps dropped; surviving text scrubbed and truncated by `Redactor` — `crates/rustwatch-analyze/src/classifier.rs:27-35`
4. `build_classifier` selects the provider from `config.analyze.provider` — `crates/rustwatch-analyze/src/classifier.rs:13-19`
5. One HTTP round trip returns JSON matching `ActivityBatchResponse` — `crates/rustwatch-analyze/src/classifier.rs:99-114` (OpenAI) / `:149-162` (Anthropic)
6. Each label becomes an `ActivityRecord` and is inserted; the segment id set is attached to *every* label — `crates/rustwatch-analyze/src/classifier.rs:48-65`
7. Newly created activities are handed to `MemoryEngine::ingest_activities` — `crates/rustwatch-cli/src/commands.rs:200-203`

### Memory Flow — ingest and hybrid search

Ingest (`crates/rustwatch-memory/src/lib.rs:56-87`): render a text summary → embed → `SqliteMemoryStore::upsert` (writes both `memory_chunks` and the FTS mirror, `crates/rustwatch-memory/src/sqlite_store.rs:56-84`) → `GraphStore::upsert_activity` fans out one activity node plus app/topic nodes and edges (`crates/rustwatch-memory/src/graph.rs:34-72`).

Search (`crates/rustwatch-memory/src/lib.rs:104-109`):

1. `Embedder::embed_one(query)` → 384-dim vector
2. `SqliteMemoryStore::search` loads **every** chunk's embedding, computes cosine in Rust, sorts, truncates to `k` — `crates/rustwatch-memory/src/sqlite_store.rs:86-115`
3. `GraphStore::expand_around_apps` walks outgoing edges from the activity node whose `props_json.$.chunk_id` matches, discounting to `score * 0.85` — `crates/rustwatch-memory/src/graph.rs:74-103`
4. `GraphRag::merge` dedupes by `chunk_id`, applies a `+0.2` substring keyword boost, sorts descending — `crates/rustwatch-memory/src/rag.rs:6-31`

**State Management:**
There is no application state container. State lives in exactly three places: (a) SQLite files shared by all processes, (b) process-local `Arc`s in the daemon — `Arc<Mutex<Store>>` for the DB, `Arc<AtomicBool> paused` shared by every `CaptureHandle` clone, `Arc<AtomicU64>` counters surfaced in `DaemonState` (`crates/rustwatch-daemon/src/main.rs:36-40`); and (c) the `SegmentGrouper`'s single `Option<ActiveSegment>` (`crates/rustwatch-core/src/segment.rs:12`). The TUI holds no state beyond `paused: bool` and re-reads the store every 250 ms (`crates/rustwatch-cli/src/tui.rs:25-28`).

## Key Abstractions

**`CaptureEvent` + `CaptureEventKind`** (`crates/rustwatch-core/src/events.rs:16,51`):

- Purpose: one tagged union that every stage of the pipeline passes through.
- Examples: `crates/rustwatch-capture/src/platform/macos.rs:130`, `crates/rustwatch-core/src/db.rs:29`, `crates/rustwatch-core/src/segment.rs:29`, `crates/rustwatch-core/src/ipc.rs:35`
- Pattern: internally tagged serde enum (`#[serde(tag = "type", rename_all = "snake_case")]`). Adding a variant is a one-line change here, but every `match` over it is a non-exhaustive-site to update: `SegmentGrouper::on_event` (`crates/rustwatch-core/src/segment.rs:29-60`).

**`SegmentGrouper`** (`crates/rustwatch-core/src/segment.rs:11`):

- Purpose: the pure state machine that turns raw events into semantically meaningful time segments.
- Examples: owned by the daemon writer task (`crates/rustwatch-daemon/src/main.rs:59`)
- Pattern: stateful fold over a stream; `on_event(&CaptureEvent) -> Option<SessionSegment>` returns a segment only when one closes, `flush()` drains at shutdown. Fully deterministic and unit-testable with no I/O.

**`Store`** (`crates/rustwatch-core/src/db.rs:12`):

- Purpose: the only door to `rustwatch.db`.
- Examples: daemon (`main.rs:36`), CLI status/export/chart (`commands.rs:78,175,208,260`), TUI (`tui.rs:24`), MCP (`mcp/main.rs:19`), analyze/chart (`classifier.rs:21`, `chart.rs:11`), memory rebuild (`memory/src/lib.rs:90`)
- Pattern: plain synchronous struct wrapping one `rusqlite::Connection`. No pool, no async, no transaction helper. Callers open their own instance per process/command.

**`DataPaths`** (`crates/rustwatch-core/src/paths.rs:8`):

- Purpose: struct-of-paths + directory creation + tilde expansion + config bootstrap.
- Pattern: constructed once per process by all three binaries (`cli/src/main.rs:84`, `daemon/src/main.rs:24`, `mcp/src/main.rs:16`) and threaded by reference everywhere. `DataPaths::new(None)` prefers `ProjectDirs::from("com","chophe","rustwatch")` and falls back to `~/.rustwatch` (`paths.rs:51-56`); `README.md:86-93` and `Config::default()` (`config.rs:63`) both assume the `~/.rustwatch` fallback is what actually happens on macOS — verify before relying on the `ProjectDirs` branch.

**`ActivityClassifier`** (`crates/rustwatch-analyze/src/classifier.rs:9`):

- Purpose: the one place where a vendor could be substituted.
- Pattern: `#[async_trait]` object-safe trait + `build_classifier(config) -> Box<dyn ActivityClassifier>` factory keyed on a string config value.
- Known limitation: `analyze_pending` constructs its own classifier from `&Config` instead of accepting `&dyn ActivityClassifier` (`classifier.rs:21,41`), so the seam is not reachable for injection. `docs/TESTING_PLAN.md:78` lists lifting this as a prerequisite for behavior tests.

**`MemoryEngine`** (`crates/rustwatch-memory/src/lib.rs:14`):

- Purpose: the single entry point into the memory subsystem; the closest thing the codebase has to a service object.
- Pattern: composition of three collaborators opened in `open()` — vector store, graph store, embedder — with ingest/rebuild/search as the three verbs.
- Known limitation: it hardcodes `paths.root.join("memory.db")` and `paths.root.join("memory-graph.db")` (`lib.rs:24-25`) instead of adding those to `DataPaths`, and it ignores `config.memory.vector_backend` / `graph_backend` entirely.

**`MemoryChunk` / `ScoredChunk`** (`crates/rustwatch-memory/src/sqlite_store.rs:2,15`):

- Purpose: the DTO pair that every memory backend speaks, which is what makes `rustwatch-memory-backends` able to mirror the API without depending on `MemoryEngine`.
- Pattern: plain data structs; `ScoredChunk` is produced by a store and consumed by the graph store and by `GraphRag::merge`.

**`DaemonCommand` / `DaemonReply` / `DaemonState`** (`crates/rustwatch-core/src/ipc.rs:10,21,32`):

- Purpose: the daemon's public control surface.
- Pattern: two internally tagged enums (`tag = "cmd"`, `tag = "reply"`) plus a state snapshot struct. Any new daemon capability requires a variant in all three of `DaemonCommand`, `DaemonReply`, and the match at `crates/rustwatch-daemon/src/main.rs:103`.

**`Redactor`** (`crates/rustwatch-analyze/src/redact.rs:4`):

- Purpose: the privacy boundary. Nothing reaches an LLM without passing through `scrub`.
- Pattern: compiled-once regex list + exclusion list + length cap, all sourced from `Config.privacy` / `Config.capture.exclude_apps`.

## Entry Points

**`rustwatch` (CLI binary):**

- Location: `crates/rustwatch-cli/src/main.rs:74` (`[[bin]] name = "rustwatch"` at `crates/rustwatch-cli/Cargo.toml:9-11`)
- Triggers: human at a shell.
- Responsibilities: install tracing + indicatif, parse the clap grammar (`main.rs:18-55`), resolve paths, load-or-create config, dispatch to `commands::*`, exit `anyhow::Result`.
- Sub-entry: `tui::run` (`crates/rustwatch-cli/src/tui.rs:12`) for the raw-mode ratatui dashboard.

**`rustwatchd` (daemon binary):**

- Location: `crates/rustwatch-daemon/src/main.rs:18` (`[[bin]] name = "rustwatchd"` at `crates/rustwatch-daemon/Cargo.toml:9-11`)
- Triggers: spawned by `rustwatch start` (`crates/rustwatch-cli/src/commands.rs:50-54`) or by launchd via `deploy/macos/com.rustwatch.plist`.
- Responsibilities: write the pid file, start capture threads, run the writer task, bind and serve the Unix socket, remove a stale socket at boot (`main.rs:31-34`).

**`rustwatch-mcp` (MCP server binary):**

- Location: `crates/rustwatch-mcp/src/main.rs:9` (`[[bin]] name = "rustwatch-mcp"` at `crates/rustwatch-mcp/Cargo.toml:9-11`)
- Triggers: an MCP host (Cursor, Claude Desktop) spawns it and speaks newline-delimited JSON-RPC on stdin/stdout.
- Responsibilities: open `Store` + `MemoryEngine`, route `initialize` / `tools/list` / `tools/call`, answer with JSON text content blocks.
- **Hard rule: all tracing goes to stderr** (`main.rs:13`) — stdout is the protocol channel and any stray write corrupts it.

**`rustwatch install` (installer):**

- Location: `crates/rustwatch-cli/src/commands.rs:18`
- Triggers: human, once per machine.
- Responsibilities: render `deploy/macos/com.rustwatch.plist` with `{{RUSTWATCHD_PATH}}` substituted for the sibling `rustwatchd` binary and write it to `~/Library/LaunchAgents/`.

**Library roots:** `crates/rustwatch-core/src/lib.rs:1`, `crates/rustwatch-capture/src/lib.rs:1`, `crates/rustwatch-analyze/src/lib.rs:1`, `crates/rustwatch-memory/src/lib.rs:1`, `crates/rustwatch-memory-backends/src/lib.rs:5`.

## Architectural Constraints

- **Threading:** Hybrid, and deliberately so. The Tokio runtime owns everything async (the daemon's accept loop, the mpsc consumer, `UnixStream` I/O in `DaemonClient`). Capture itself is *not* on the runtime: `keytap::Tap::iter()` is a blocking iterator and `xcap`/`arboard` are synchronous, so `PlatformCapture::start` spawns two plain `std::thread`s (`crates/rustwatch-capture/src/platform/macos.rs:70,76`) and the focus loop uses `std::thread::sleep` (`macos.rs:183`) rather than a tokio timer. Consequence: adding a Tokio-only capture source means the event-sending half must stay non-blocking (`let _ = tx.send(...)` — never `.await`).
- **Sync I/O inside `async fn`:** `Store` is synchronous, and so is every memory store. `MemoryEngine::open`, `ingest_segments`, `ingest_activities`, `rebuild_from_store`, and `search` are all `async fn` that perform only blocking rusqlite work (`crates/rustwatch-memory/src/lib.rs:22,31,56,89,104`). Harmless today because the CLI and MCP call them from `main`'s single-task context, but `MemoryEngine` must not be shared across `tokio::spawn` tasks as written.
- **Cross-process SQLite with no coordination:** `rustwatchd`, `rustwatch`, and `rustwatch-mcp` each open `~/.rustwatch/rustwatch.db` independently. WAL mode (`crates/rustwatch-core/src/db.rs:22`) is the only concurrency mechanism. Inside the daemon, `Arc<Mutex<Store>>` serializes writer and IPC reader — but both use `try_lock()` and silently skip when contended (`crates/rustwatch-daemon/src/main.rs:62`, `:126`), so a `Tail` request can cause the capture writer to drop events. The pid file is a liveness hint only; nothing takes an exclusive lock.
- **Global state:** No `OnceLock`/`lazy_static`/`static mut` anywhere. The shared mutable state is confined to the daemon's `Arc`s (`crates/rustwatch-daemon/src/main.rs:36-39`) and the `Arc<AtomicBool> paused` cloned into every `CaptureHandle` (`crates/rustwatch-capture/src/platform/macos.rs:25,29-35`), which is how `DaemonCommand::Pause` reaches the OS threads.
- **Circular imports:** None. The workspace dependency graph is a DAG rooted at `rustwatch-core`. `rustwatch-memory-backends` sits outside the workspace entirely (`Cargo.toml:3-11`), so nothing in it can create a cycle even accidentally.
- **Configuration authority is split:** `DataPaths` computes the real paths; `Config.data.{dir,sqlite_path,lance_path,surreal_path}` declares a *second*, competing set (`crates/rustwatch-core/src/config.rs:16-21`) reachable only through `Config::paths()` (`config.rs:107`), which no caller invokes. Memory database paths bypass both and are hardcoded in `MemoryEngine::open`. Three sources of truth for where data lives; only `DataPaths` is live.
- **Binaries only — no lib/bin split:** `rustwatch-cli`, `rustwatch-daemon`, and `rustwatch-mcp` declare `[[bin]]` and nothing else. No logic in those crates is reachable from an integration test or from another crate. `docs/TESTING_PLAN.md:99-104` calls out the daemon and CLI splits as prerequisite refactors.
- **Optional heavy dependencies are compile-time gated, not runtime-gated:** `fastembed` (feature on `rustwatch-memory`), `rmcp` (unused optional dep on `rustwatch-mcp`), and the whole `rustwatch-memory-backends` crate. The config keys that would select backends at runtime (`memory.vector_backend`, `memory.graph_backend`, `memory.surreal_engine`) are read by nothing.
- **Embedded assets are compile-time:** migrations via `embed_migrations!("migrations")` (`crates/rustwatch-core/src/db.rs:10`) and the launchd plist via `include_str!("../../../deploy/macos/com.rustwatch.plist")` (`crates/rustwatch-cli/src/commands.rs:29`). Editing either file changes compiled binaries; `rustwatch install` must be re-run to refresh an installed plist.

## Anti-Patterns

### Hardcoded paths that bypass `DataPaths`

**What happens:** `MemoryEngine::open` builds `paths.root.join("memory.db")` and `paths.root.join("memory-graph.db")` inline (`crates/rustwatch-memory/src/lib.rs:24-25`), while `DataPaths` dutifully creates `lance/` and `surreal/` directories that nothing ever reads (`crates/rustwatch-core/src/paths.rs:40-41`).

**Why it's wrong here:** `DataPaths` is the documented single authority (`crates/rustwatch-core/src/paths.rs:8`), yet the two most user-visible databases are derived ad hoc. A new backend path requires editing service code, and `DataPaths` already advertises two backend directories that will never contain a backend.

**Do this instead:** Add `memory: PathBuf` and `memory_graph: PathBuf` (and use the existing `lance`/`surreal`) to `DataPaths` at `crates/rustwatch-core/src/paths.rs:26-35`, create them in `ensure_dirs`, and have `MemoryEngine::open` read `&paths.memory` / `&paths.memory_graph`.

### Declaring a trait seam that nothing can inject through

**What happens:** `ActivityClassifier` and `build_classifier` exist as the vendor abstraction (`crates/rustwatch-analyze/src/classifier.rs:9,13`), but the only consumer, `analyze_pending(store: &Store, config: &Config)`, builds its own classifier internally (`crates/rustwatch-analyze/src/classifier.rs:41`) and reads the API key from `std::env` at construction (`classifier.rs:76,127`).

**Why it's wrong here:** The abstraction buys compile-time polymorphism but delivers zero testability or pluggability — the exact opposite of why a trait is introduced. Any behavior test must therefore stand up a real HTTP endpoint or mutate process environment.

**Do this instead:** Change the signature to `analyze_pending(store: &Store, classifier: &dyn ActivityClassifier)` and let `build_classifier(config)` be called by the caller (`crates/rustwatch-cli/src/commands.rs:197`). This is the refactor `docs/TESTING_PLAN.md:78` already prescribes.

### Synchronous database work declared `async`

**What happens:** `MemoryEngine::open`, `ingest_segments`, `ingest_activities`, `rebuild_from_store`, and `search` are all `async fn` with no `.await` on any I/O (`crates/rustwatch-memory/src/lib.rs:22-109`). `SqliteMemoryStore::search` additionally scans the entire table and scores in Rust (`crates/rustwatch-memory/src/sqlite_store.rs:86-115`).

**Why it's wrong here:** The `async` signature advertises non-blocking behavior it does not deliver. The moment `MemoryEngine` is moved inside a `tokio::spawn` (the natural next step for a background ingest task), the full-table cosine scan blocks a runtime worker for as long as it takes, starving the daemon's IPC loop if they ever share a runtime.

**Do this instead:** Either keep these `fn` (honest) and wrap the engine in `tokio::task::spawn_blocking`, or move the scan behind a real index — `sqlvec`/`sqlite-vec` in the same schema, or the LanceDB store that already exists in `crates/rustwatch-memory-backends/src/lance.rs:70-107`.

### Opening a fresh `Store` per command instead of a context object

**What happens:** `Store::open` is called independently in `commands::status`, `commands::export`, `commands::chart`, `commands::analyze`, `commands::memory_ingest`, `tui::run`, and `rustwatch-mcp` `main` (`crates/rustwatch-cli/src/commands.rs:78,175,196,208,260`, `crates/rustwatch-cli/src/tui.rs:24`, `crates/rustwatch-mcp/src/main.rs:19`).

**Why it's wrong here:** Every call re-runs the refinery migration check (`crates/rustwatch-core/src/db.rs:23`) and re-establishes a connection. It also means schema versioning has no single owner, and there is nowhere to hang a handle for a background flush or a shared cache.

**Do this instead:** Open once in `main` after config load (`crates/rustwatch-cli/src/main.rs:86`) and pass `&Store` into each `commands::*` function, the way `render_chart(store, ...)` and `analyze_pending(store, ...)` already take it.

### Swallowing errors on the persistence path

**What happens:** Every write in the daemon's writer task discards its result — `let _ = store.insert_event(&event)` (`crates/rustwatch-daemon/src/main.rs:63`), segments (`:65`, `:81`), screenshots (`:69-76`) — and `try_lock()` failure drops the event entirely (`:62`).

**Why it's wrong here:** `SQLITE_BUSY`, `SQLITE_FULL`, and disk-full are invisible. The daemon keeps reporting `events_captured`/`segments_written` in `DaemonState` (`crates/rustwatch-core/src/ipc.rs:15-16`) while persisting nothing, so `rustwatch status` reports a healthy system with an empty database.

**Do this instead:** `if let Err(err) = store.insert_event(&event) { error!(?err, id = %event.id, "event write failed"); }` and add a `write_errors: u64` counter to `DaemonState` alongside the existing atomics.

### Configuration fields with no consumer

**What happens:** `Config` declares nine fields that no code reads: `analyze.vision_model`, `analyze.batch_interval_minutes`, `memory.vector_backend`, `memory.graph_backend`, `memory.surreal_engine`, `ui.tui_enabled`, `ui.progress_bars`, `privacy.send_screenshots_to_llm`, and all of `data.*` (`crates/rustwatch-core/src/config.rs:35-58`). `accessibility_poll_ms` is threaded through four layers and discarded at the leaf (`crates/rustwatch-capture/src/platform/macos.rs:61`).

**Why it's wrong here:** They serialize into `~/.rustwatch/config.toml`, so they read as supported settings. `send_screenshots_to_llm = true` in particular reads as a privacy control while no vision call exists to consult it.

**Do this instead:** Delete any field with no reader, or add the reader in the same change. Do not add a config field without the code that honors it.

## Error Handling

**Strategy:** Two-tier. `rustwatch-core` defines a typed error enum; everything above it uses `anyhow`.

**Patterns:**

- Typed core errors via `thiserror` with `#[from]` conversions — `Io`, `Db`, `Serde` are auto-converted; `Config`, `UnsupportedPlatform`, `Other` are constructed manually (`crates/rustwatch-core/src/error.rs:5-21`). The crate-wide alias is `crate::Result<T>` (`error.rs:3`), re-exported as `rustwatch_core::Result`.
- Service and binary layers return `anyhow::Result` and use `?` freely; `main` returns `anyhow::Result<()>` so the runtime prints the chain (`crates/rustwatch-cli/src/main.rs:75`, `crates/rustwatch-daemon/src/main.rs:19`, `crates/rustwatch-mcp/src/main.rs:10`).
- **Connection failures are intentionally lossy:** `DaemonClient::send` maps every connect error to `Error::DaemonNotRunning` (`crates/rustwatch-core/src/ipc.rs:52-54`). Callers therefore cannot distinguish "daemon down" from "socket path wrong" — and `commands::screenshot` (`crates/rustwatch-cli/src/commands.rs:148-157`) uses that ambiguity to fall back to in-process capture.
- **IPC errors travel in-band:** the handler closure returns `DaemonReply::Error { message }` rather than failing the connection (`crates/rustwatch-daemon/src/main.rs:129-137`), and `commands::tail` converts it back into an `anyhow::bail!` (`crates/rustwatch-cli/src/commands.rs:141`).
- **Silent fallbacks in the read path:** `parse_ts` substitutes `Utc::now()` for any unparseable timestamp (`crates/rustwatch-core/src/db.rs:252-256`), and JSON columns decode with `.unwrap_or_default()` (`db.rs:197-199`), so a corrupt row becomes empty data rather than an error.
- **Panic risk on the privacy path:** `Redactor::scrub` calls `String::truncate(max_chars)` on a byte length (`crates/rustwatch-analyze/src/redact.rs:31`), which panics on a non-char boundary. `docs/TESTING_PLAN.md:120` flags this for a proptest.

## Cross-Cutting Concerns

**Logging:** `tracing` + `tracing_subscriber` throughout, filtered by `RUST_LOG` via `EnvFilter::from_default_env()`. Three initializations, each different and each deliberate:

- Daemon: plain `fmt()` — `crates/rustwatch-daemon/src/main.rs:20-22`
- MCP: `fmt().with_writer(io::stderr)` — `crates/rustwatch-mcp/src/main.rs:13`. **Never stdout**: stdout carries JSON-RPC frames.
- CLI: `registry().with(EnvFilter).with(IndicatifLayer).with(fmt::layer())` — `crates/rustwatch-cli/src/main.rs:76-81`, so indicatif spans render as progress bars.
- `rustwatch-memory` and `rustwatch-analyze` declare `tracing` as a dependency but emit no events.

**Validation:** Ad hoc and edge-only; there is no validation layer.

- Outbound text is scrubbed and length-capped by `Redactor::scrub` before any LLM call (`crates/rustwatch-analyze/src/classifier.rs:32`).
- Segment text is capped at `MAX_BUFFER_CHARS = 16_384` with oldest-first eviction (`crates/rustwatch-core/src/segment.rs:9,106-114`).
- Graph ids are slugified to `[a-z0-9_]` (`crates/rustwatch-memory/src/graph.rs:112`).
- MCP tool arguments are read with `.and_then(...).unwrap_or(default)` and never schema-checked, even though `tools/list` publishes an `inputSchema` (`crates/rustwatch-mcp/src/main.rs:68-74` vs `:85-88`).
- Time inputs use fixed formats: RFC3339 for timestamps, `%Y-%m-%d` for dates (`crates/rustwatch-cli/src/commands.rs:210,289`, `crates/rustwatch-mcp/src/main.rs:96`).

**Authentication:** None locally, at any layer.

- API keys are read from `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` at classifier construction and held in memory (`crates/rustwatch-analyze/src/classifier.rs:76,127`); nothing is written to `config.toml`.
- The daemon socket has no authentication and no authorization. Any local process that can open `~/.rustwatch/daemon.sock` can issue `Pause`, `Resume`, `Screenshot`, or `Tail` (`crates/rustwatch-core/src/ipc.rs:21-28`). The socket file's permissions are whatever `UnixListener::bind` leaves (`crates/rustwatch-daemon/src/main.rs:86`).
- Excluded apps (`Config.capture.exclude_apps`, default `1Password`, `Keychain Access`) are filtered by substring match at both capture (`crates/rustwatch-capture/src/platform/macos.rs:191`) and analysis (`crates/rustwatch-analyze/src/redact.rs:36-40`) — this is the only access control on captured content, and it is name-based, not capability-based.

---

*Architecture analysis: 2026-10-02*
