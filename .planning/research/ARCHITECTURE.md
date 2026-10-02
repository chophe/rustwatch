# Architecture Research

**Domain:** Local-first activity memory / personal productivity tracker (macOS, Rust)
**Researched:** 2026-10-02
**Confidence:** HIGH (patterns confirmed across 5+ independent implementations + rustwatch's own codebase map)

## Standard Architecture

Every system in this domain — Rewind.ai (commercial), Retrace (macOS on-device rewind),
2ndm1nd (local-first ambient capture + daily LLM consolidation), Mnemosyne (local Rewind
clone with Graph RAG), activity-frames (deterministic capture compiler), and the family of
SQLite hybrid-search MCP servers (sqlite-rag-mcp, vstash, sqlite-memory-mcp) — converges on
the same layered shape: **dumb-fast capture → raw event ledger → batch cognition →
dual-index memory → thin frontends**. The LLM is never in the capture hot path; it is a
batch consumer of the ledger. This matches rustwatch's existing workspace layout, which is
the right skeleton and should be hardened, not restructured.

### System Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        FRONTEND / PROCESS LAYER                          │
│  (short-lived or client-driven; never own the database)                  │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐         │
│  │ CLI      │  │ TUI      │  │ MCP      │  │ Daemon ctrl      │         │
│  │ verbs    │  │ dashboard│  │ stdio    │  │ start/stop/tail  │         │
│  │ (clap)   │  │ (ratatui)│  │ JSON-RPC │  │ over Unix socket │         │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────────┬─────────┘         │
│       │             │             │                 │                    │
│       └─────────────┴──────┬──────┴─────────────────┘                    │
│                            │ read Store + MemoryEngine                   │
│                            │ (read-only handles; WAL N-readers)          │
├────────────────────────────┴────────────────────────────────────────────┤
│                        SERVICE LAYER (domain logic)                      │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────────────┐ │
│  │ Segmenter /  │  │ Classifier   │  │ MemoryEngine (facade)          │ │
│  │ Sessionizer  │  │ Redact → LLM │  │ embed → vector store           │ │
│  │ event fold   │  │ → Activity   │  │         → graph store          │ │
│  │ (pure, sync) │  │ (async,batch)│  │         → RRF/hybrid fusion    │ │
│  └──────┬───────┘  └──────┬───────┘  └───────────────┬────────────────┘ │
│         │                 │                          │                   │
├─────────┴─────────────────┴──────────────────────────┴───────────────────┤
│                        CAPTURE LAYER (daemon-owned)                      │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │ daemon: OS threads → mpsc channel → single writer task → SQLite   │  │
│  │ triggers: interval tick + window-change event + global hotkey     │  │
│  │ screenshots → PNG files on disk (sharded by date), rows reference │  │
│  └──────────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────────┤
│                        CORE / PERSISTENCE LAYER                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                   │
│  │ event ledger │  │ memory index │  │ graph store  │                   │
│  │ SQLite + WAL │  │ chunks + FTS5│  │ nodes + edges│                   │
│  │ + migrations │  │ + vec0 (ANN) │  │ (SQLite)     │                   │
│  └──────────────┘  └──────────────┘  └──────────────┘                   │
│  + screenshots/ dir, daemon.sock, daemon.pid, config.toml               │
└─────────────────────────────────────────────────────────────────────────┘
  egress only: outbound HTTPS to LLM provider (redacted text only)
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| Capture sensors | Turn OS state into typed events; nothing else | 1–2 OS threads (keyboard tap, focus poll), `xcap` screenshots, `active-win-pos-rs` window info; non-macOS = stub returning `UnsupportedPlatform` |
| Scheduler | Decide *when* to capture a screenshot | Interval timer (default 5 min) + focus-change hook + global hotkey listener; each trigger tags the event with its source |
| Event ledger | Append-only durable log of everything captured | SQLite WAL: `events`, `segments`, `screenshots` tables; refinery-style embedded migrations; single writer |
| Segmenter / sessionizer | Fold raw event stream into time-bounded sessions | Pure state machine (`on_event → Option<Segment>` + `flush()`); deterministic, no I/O, unit-testable (rustwatch `SegmentGrouper`; 2ndm1nd `sessionize`; activity-frames Tier-1 compiler) |
| Privacy gate | Scrub before anything leaves the device | Regex redaction → `[REDACTED]`, byte-safe truncation, per-app exclusion list; consulted by *both* capture (skip) and analysis (scrub) |
| Classifier provider seam | One interface, N vendors | `#[async_trait] trait ActivityClassifier` + `build_provider(config) -> Box<dyn …>` factory keyed on config string; OpenAI + Anthropic native, Ollama/llama.cpp via OpenAI-compatible base-URL override |
| Analysis pipeline | Batch: unanalyzed segments → redact → classify → persist activities → ingest into memory | CLI/daemon-triggered job, ≤50 segments per batch, spinner/progress; constructs provider via factory (injectable for tests) |
| Embedder | Text → fixed-dim vector, with graceful degradation | Trait with two impls: local model (`fastembed` ONNX / Ollama `nomic-embed-text`) + deterministic hash fallback; lexical (FTS5) search must work when the embedder is unavailable |
| Vector store | Chunk persistence + semantic KNN | `memory_chunks` (embedding as f32 BLOB) + `vec0` virtual table (`sqlite-vec`) when available; brute-force cosine scan is acceptable <10k chunks, index beyond |
| Lexical index | Exact-term search that never breaks | FTS5 virtual table (`unicode61` tokenizer) mirrored on every write via trigger or dual-insert; external-content or content-sync |
| Graph store | Entity co-occurrence links across memories | `graph_nodes` + `graph_edges` in SQLite; `activity→used_app→app`, `activity→about_topic→topic`; expansion = 1-hop walk with score discount (~0.85) |
| Fusion (GraphRag) | Merge vector + lexical (+graph) rankings | Reciprocal Rank Fusion `score = Σ 1/(60 + rank)` — rank-based, needs no score calibration between BM25 and cosine; optional `+0.2` substring boost and recency decay |
| Daemon + IPC | Own capture threads and the single writer; expose control plane | `rustwatchd` with Unix-socket JSON IPC (`DaemonCommand`/`DaemonReply`), length-prefix or newline-delimited framing, pid file, launchd plist; CLI verbs are thin RPC clients |
| CLI | One function per subcommand, opens stores, formats output | `status`, `tail`, `analyze`, `chart`, `memory search`, `ask`, `export`; open `Store` once in `main`, pass `&Store` down |
| TUI | Read-only dashboard polling the stores | ratatui event-loop template (`Tui` struct + `EventHandler` + tick/render intervals); re-read store on tick (250 ms–1 s); timeline, time-by-app, score, search/ask box |
| MCP server | stdio JSON-RPC read surface for agents | Hand-rolled or `rmcp` dispatch (`initialize`/`tools/list`/`tools/call`); **tracing to stderr only**; 5–7 tools: search, ask, timeline/stats, capture status, annotate |

## Recommended Project Structure

The existing 7-crate workspace already matches the domain consensus. Keep it; fix the
seams inside it. (The 8th crate, `rustwatch-memory-backends`, is an orphan nothing depends
on — either give it a trait to plug into or delete it; see Anti-Patterns.)

```
crates/
├── rustwatch-core/        # vocabulary + ledger + IPC — depends on nothing internal
│   ├── events.rs          # CaptureEvent / CaptureEventKind (pipeline currency)
│   ├── segment.rs         # SegmentGrouper (pure fold, no I/O)
│   ├── db.rs              # Store — sole gateway to rustwatch.db (+refinery migrations)
│   ├── paths.rs           # DataPaths — SINGLE authority for all on-disk paths
│   ├── config.rs          # Config — every field must have a reader (or be deleted)
│   ├── ipc.rs             # DaemonCommand / DaemonReply / DaemonClient
│   └── error.rs           # thiserror typed errors; services use anyhow
├── rustwatch-capture/     # OS sensors only — depends on core only, never touches DB
│   ├── lib.rs             # CaptureHandle facade (start/pause/screenshot/permissions)
│   └── platform/
│       ├── macos.rs       # keytap thread + focus-poll thread + xcap + hotkey
│       └── stub.rs        # UnsupportedPlatform for non-macOS
├── rustwatch-daemon/      # binary: owns capture threads + writer task + socket loop
│   └── main.rs            # pid file, Arc<Mutex<Store>>, atomic counters, IPC dispatch
├── rustwatch-analyze/     # batch cognition — takes &Store/&Config, opens nothing
│   ├── classifier.rs      # ActivityClassifier trait + build_classifier factory
│   ├── providers/         # openai.rs, anthropic.rs, local.rs (OpenAI-compat base URL)
│   ├── redact.rs          # Redactor — privacy boundary (char-boundary-safe truncation)
│   └── chart.rs           # pure day-rendering (terminal / JSON / HTML)
├── rustwatch-memory/      # memory subsystem behind MemoryEngine facade
│   ├── lib.rs             # MemoryEngine: open / ingest / rebuild / search
│   ├── embedder.rs        # Embedder trait: fastembed impl + hash fallback
│   ├── sqlite_store.rs    # chunks + FTS5 mirror + vec0 (sqlite-vec) KNN
│   ├── graph.rs           # GraphStore: nodes/edges, 1-hop expansion
│   └── rag.rs             # RRF fusion (+keyword boost, recency decay)
├── rustwatch-cli/         # binary: clap dispatch + commands.rs + tui.rs
│   ├── main.rs            # open Store ONCE, pass &Store into commands
│   ├── commands.rs        # one pub fn per subcommand
│   └── tui.rs             # ratatui dashboard (read-only, tick-poll)
└── rustwatch-mcp/         # binary: stdio JSON-RPC, Store + MemoryEngine readers
    └── main.rs            # route initialize/tools-list/tools-call; stderr logging
```

### Structure Rationale

- **Crates are layers, not features.** `core` is depended on by all, depends on none —
  the DAG root. Every other crate depends only downward. This is exactly how Retrace
  (`SecondMindKit` library + thin `2ndm1nd`/`brain` binaries) and Mnemosyne (Go watcher →
  Redis buffer → Python brain → SQLite archive) separate capture from cognition.
- **Capture never knows the database exists.** It emits `CaptureEvent`s onto an mpsc
  channel; the daemon writer task persists them. This is the "no LLM in the capture path"
  rule generalized: capture does no I/O, no network, no inference — so it stays fast,
  testable, and private. 2ndm1nd enforces the same rule structurally (capture agent vs.
  brain agent are separate launchd jobs).
- **Binaries are thin; logic lives in libs.** CLI/daemon/MCP parse input, open stores,
  format output. Everything testable (segmenter, redactor, fusion, chart) is a pure
  function or a trait-impl in a library crate — directly importable by integration tests.
- **One facade per subsystem** (`CaptureHandle`, `MemoryEngine`, `build_classifier`)
  so frontends never touch stores directly and new providers/backends plug in at one seam.

## Architectural Patterns

### Pattern 1: Capture → ledger → batch-cognition pipeline

**What:** Three stages connected by durable SQLite, not by function calls. Capture appends
events; a pure fold groups them into segments; an asynchronous batch job classifies
segments into activities and ingests them into memory. Each stage runs even if the next
never does (capture is useful with the brain disabled — 2ndm1nd proves this).
**When to use:** Always in this domain — it is the consensus shape (Rewind, Retrace,
2ndm1nd, Mnemosyne, rustwatch all do this).
**Trade-offs:** Pro: crash-safe (ledger survives), backfillable (`rebuild_from_store`),
each stage independently testable. Con: eventual consistency — search lags capture by one
batch cycle; must surface "pending analysis" counts so the user trusts the system.

**Example:**
```rust
// daemon writer task: the ONLY writer; everything downstream reads the ledger
while let Some(event) = rx.recv().await {
    let guard = store.lock().expect("store poisoned"); // never try_lock + drop
    if let Err(e) = guard.insert_event(&event) { error!(?e, "event write failed"); }
    if let Some(seg) = grouper.on_event(&event) { guard.insert_segment(&seg)?; }
}
```

### Pattern 2: Provider-seam trait + string-keyed factory (+ injectable pipeline)

**What:** `#[async_trait] trait ActivityClassifier` with one impl per vendor; a
`build_classifier(&config) -> Box<dyn ActivityClassifier>` factory; and — critically —
the pipeline takes `&dyn ActivityClassifier` instead of constructing its own. Local models
(Ollama, llama.cpp) need no new code: they are the OpenAI impl with an overridden
`base_url` (the `async-openai` + custom `base_url` idiom; `llm-trait`/`llm-unified` crates
generalize this to protocol adapters + model registry).
**When to use:** For the classifier (required: 3 providers) and the embedder (required:
fastembed + fallback). Do NOT build the same seam for vector/graph backends until a
second backend actually ships — a trait with one impl is speculative generality.
**Trade-offs:** Pro: adding Anthropic/local = new file, no call-site edits; tests inject a
mock provider, no HTTP, no env mutation. Con: `Box<dyn>` + `async_trait` boxing overhead
— irrelevant at batch cadence (one HTTP round trip per ≤50 segments dominates).

**Example:**
```rust
#[async_trait]
pub trait ActivityClassifier: Send + Sync {
    async fn classify(&self, input: &ClassifyInput) -> Result<Vec<ActivityLabel>>;
}
// pipeline is injectable — tests pass a stub, production passes build_classifier(&cfg)
pub async fn analyze_pending(store: &Store, classifier: &dyn ActivityClassifier) -> Result<usize>
```

### Pattern 3: Hybrid vector + FTS5 search fused with RRF, lexical always live

**What:** Every ingest writes both a vector row (chunk + embedding) and an FTS5 row. Every
query runs KNN + BM25 independently and fuses with Reciprocal Rank Fusion
(`1/(60 + rank)` per list, summed) — rank-based, so BM25 scores and cosine distances never
need calibration. If the embedder is unavailable, search degrades to lexical-only with an
explicit warning and never errors (sqlite-rag-mcp's graceful-degradation contract).
**When to use:** Always — pure-vector misses exact names/IDs, pure-FTS misses paraphrase;
RRF beats either alone and beats score-normalization hacks (vstash BEIR evals, Supabase RRF
docs, Alex Garcia's sqlite-vec hybrid guide all agree).
**Trade-offs:** Pro: single SQLite file, zero services, `cp` backup; experiments are one
`SELECT`. Con: brute-force cosine scan is O(n) — fine <10k chunks, then `vec0` index or
LanceDB; FTS5 has no metadata filtering, so keep filters (date/app) as SQL `WHERE` on the
content table, not in the FTS query.

**Example:**
```sql
-- vec0 KNN + FTS5 BM25, fused in Rust with RRF (k=60)
SELECT rowid, rank FROM memory_fts WHERE memory_fts MATCH '"quarterly" OR plan*';
SELECT rowid, distance FROM vec_chunks WHERE embedding MATCH ? AND k = 20;
-- fuse: score(d) = 1/(60+rank_fts(d)) + 1/(60+rank_vec(d)), +0.2 substring boost
```

### Pattern 4: Daemon owns the writer; frontends are read-only WAL clients

**What:** One long-running process holds the single writable `Store` (behind `Mutex`,
locked only around synchronous rusqlite calls, never across `.await`); CLI/TUI/MCP open
their own read handles and rely on WAL's N-readers + 1-writer property (`journal_mode=WAL`,
`busy_timeout=5000`, `synchronous=NORMAL`). Control goes over a Unix socket
(newline-delimited or length-prefixed JSON); screenshot PNGs live on disk sharded by date,
DB holds only paths. Permission-gated capture degrades to "daemon down / capture paused"
rather than crashing.
**When to use:** Any local-first tracker with a menu-bar/launchd daemon + multiple readers.
**Trade-offs:** Pro: no lock service, no broker, Time-Machine-friendly single files. Con:
no cross-process writer coordination — so there must be exactly one writer by construction;
readers must set `busy_timeout` (rustwatch currently doesn't — CONCERNS.md flag) and never
`try_lock`-and-drop on the write path.

### Pattern 5: Read-only TUI tick loop + stdio MCP with stderr logging

**What:** The ratatui dashboard holds no domain state — it re-reads the stores every tick
(250 ms–1 s) using the standard `Tui` + `EventHandler` template (tokio `select!` over
crossterm `EventStream`, tick interval, render interval, `CancellationToken`). The MCP
server is a separate stdio process speaking JSON-RPC (`initialize`/`tools/list`/`tools/call`)
with all tracing routed to stderr so stdout stays a clean protocol channel.
**When to use:** TUI for the daily dashboard (timeline, shares, score, Q&A); MCP so agents
(Cursor/Claude Desktop) can query memory without touching SQLite files.
**Trade-offs:** Pro: TUI never shows stale state; MCP crash can't take down the daemon
(separate process). Con: full re-query per tick — keep dashboard queries indexed
(`ORDER BY ts`, covering indexes on date/app) or the tick will jank past ~100k events.

## Data Flow

### Request Flow

```
[macOS OS event: keypress / focus change / timer tick / hotkey]
    ↓ (OS threads, std::thread — never tokio, keytap is blocking)
[PlatformCapture] → mpsc::UnboundedSender<CaptureEvent>
    ↓
[daemon writer task] → SegmentGrouper::on_event → Store::insert_* (single writer)
    ↓ (WAL-persisted ledger: events, segments, screenshots rows + PNG files)
[analyze_pending — batch, on demand or scheduled]
    ↓ Redactor::scrub (regex + exclusions + char-boundary-safe truncate)
[ActivityClassifier provider] → HTTPS → OpenAI / Anthropic / Ollama-local
    ↓ ActivityRecords persisted → MemoryEngine::ingest_activities
[embed → SqliteMemoryStore::upsert (chunks + FTS mirror + vec0) → GraphStore::upsert]
    ↓
[query: CLI memory search / ask · TUI search box · MCP search tool]
    ↓ embed query → vec KNN + FTS5 BM25 → RRF fuse → graph expand → top-k
[rendered answer + citations (segment ids, timestamps, app)]
```

### State Management

No application state container anywhere in this domain — state lives in exactly three
places (rustwatch already does this; keep it):

1. **SQLite files** shared by all processes (the only cross-process state).
2. **Daemon-local `Arc`s**: `Arc<Mutex<Store>>` (writer), `Arc<AtomicBool> paused`
   (shared with every `CaptureHandle` so Pause reaches OS threads), `Arc<AtomicU64>`
   counters surfaced in `DaemonState` (+ add `write_errors` so silent drops are visible).
3. **`SegmentGrouper`'s `Option<ActiveSegment>`** — the sole in-memory fold state,
   flushed at shutdown.

### Key Data Flows

1. **Capture → disk (hot path, must never lose data):** OS thread → mpsc → writer task →
   `insert_event` + segment close + screenshot row. Blocking `send`, real `Mutex` (not
   `try_lock`), log-and-count every write error. Target <50 ms per event; SQLite WAL
   writes are 1–5 ms.
2. **Segments → activities (batch cognition):** fetch ≤50 unanalyzed segments → drop
   excluded apps → scrub → one LLM round trip → persist labels → ingest into memory.
   Runs on demand (`analyze` verb) or on a schedule; reports pending counts.
3. **Ingest → dual index:** render summary text → embed → `upsert` writes chunk + FTS row
   + vec row in one transaction → graph fan-out (activity node + app/topic nodes + edges).
4. **Query → fused answer:** embed query → parallel KNN + FTS → RRF → graph 1-hop expand
   (×0.85) → optional recency decay → top-k with provenance. `ask` adds one LLM call over
   the fused context; MCP `search` returns chunks, never raw keystroke streams.
5. **Control plane (IPC):** CLI → `DaemonClient::send(cmd)` → Unix socket → daemon
   dispatch → `DaemonReply`. `tail` is the one streaming verb (server-push until
   disconnect); everything else is request/response.

## Scaling Considerations

Personal single-user local tracker: scale means *data volume over years*, not users.

| Scale | Architecture Adjustments |
|-------|--------------------------|
| 0–50k events (~months) | Current shape is fine: brute-force cosine scan, full re-query TUI tick, one SQLite file per store |
| 50k–1M events (~1–2 years) | Add `vec0` index (sqlite-vec) or promote LanceDB backend; covering indexes on `(ts)`, `(app)`, `(day)`; TUI tick queries must stay <50 ms (EXPLAIN QUERY PLAN check); VACUUM/prune policy + retention config |
| 1M+ events / multi-year | Partition by month (separate DB files or `ATTACH`), embedding cache on disk (never re-embed on query — the #1 perf trap in the DEV.to Rewind retrospective: 60 s → 2 s), background `rebuild_from_store` as daemon-owned task via `spawn_blocking` |

### Scaling Priorities

1. **First bottleneck:** full-table embedding scan in `SqliteMemoryStore::search` (loads
   every embedding, scores in Rust). Fix with `vec0` KNN inside the same schema — no new
   service, no migration of data model.
2. **Second bottleneck:** single-file DB churn vs. Time Machine/backup + TUI tick cost.
   Fix with monthly partitions + retention pruning + `busy_timeout` on every reader.

## Anti-Patterns

### Anti-Pattern 1: Model in the capture path

**What people do:** Call an LLM/VLM per screenshot or per keystroke batch inline with capture.
**Why it's wrong:** Capture must be <50 ms/event and work offline; model latency, cost, and
outages then break the ledger — the one thing that must never break. Privacy review also
becomes per-call instead of structural.
**Do this instead:** Capture writes raw events to SQLite and nothing else; cognition runs as
a separate batch step (separate process/job ideally — 2ndm1nd's two-launchd-agent split;
Retrace's capture-then-caption pipeline that deletes raw frames every cycle).

### Anti-Pattern 2: Trait seam nothing can inject through / backend nobody selects

**What people do:** Declare `ActivityClassifier` (or vector/graph backend traits) while the
pipeline constructs its own provider internally and config keys (`vector_backend`,
`batch_interval`, `send_screenshots_to_llm`) have no readers.
**Why it's wrong:** Reads as pluggable, tests as untestable; dead config fields lie to users
about privacy controls that don't exist.
**Do this instead:** Pipeline takes `&dyn Trait` as a parameter; factory lives at the call
site; every config field gets its reader in the same change or is deleted. (rustwatch
CONCERNS.md already flags all nine dead fields.)

### Anti-Pattern 3: Async signatures over blocking SQLite work shared across tasks

**What people do:** `async fn search/ingest` doing synchronous rusqlite I/O, then share the
engine across `tokio::spawn` tasks.
**Why it's wrong:** Advertises non-blocking behavior it doesn't deliver; a full-scan cosine
query blocks a runtime worker and can starve the daemon's IPC loop on a shared runtime.
**Do this instead:** Keep store fns sync and honest; wrap in `tokio::task::spawn_blocking`,
or move the scan behind `vec0`/LanceDB. Never hold a DB mutex across `.await`.

### Anti-Pattern 4: `try_lock`-and-drop on the persistence path

**What people do:** `if let Ok(guard) = store.try_lock()` in the writer, silently skipping
events (and `let _ = insert_…` discarding write errors) when contended.
**Why it's wrong:** `SQLITE_BUSY`/disk-full become invisible; `status` reports healthy with
an empty database — the exact "silent data loss" failure the project's Core Value forbids.
**Do this instead:** Blocking `lock()` + per-error logging + `write_errors` counter in
`DaemonState`; contention at this write rate means a bug elsewhere, not a case to optimize.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| OpenAI chat completions | `async-openai` client, JSON `ActivityBatchResponse`, key from `OPENAI_API_KEY` env | Only redacted+truncated segment text is sent; never keystrokes, screenshots, or secrets |
| Anthropic messages | Raw `reqwest` (`x-api-key` header, `anthropic-version`), separate response parser | Different wire format — this is why the provider trait exists; max_tokens required |
| Ollama / llama.cpp local | Same OpenAI-compat client with `base_url=http://localhost:11434/v1`, no key | Zero new provider code; also serves `nomic-embed-text` for local embeddings |
| fastembed (ONNX, in-process) | Feature-gated dep, 384-dim (`bge-small` family), ~700 chunks/s CPU | Behind `Embedder` trait; hash fallback when feature off — lexical search covers the gap |
| launchd | `com.rustwatch.plist` with substituted daemon path, `~/Library/LaunchAgents/` | Re-run `install` after any plist change (it's `include_str!` — compile-time) |
| MCP hosts (Cursor, Claude Desktop) | Spawn `rustwatch-mcp`, newline JSON-RPC over stdio | Read-only tools; validate args against published `inputSchema` (currently `unwrap_or(default)` — fix) |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| capture → daemon | `mpsc::UnboundedSender<CaptureEvent>`, OS threads | Blocking send (zero loss); `let _ = tx.send(…)` never `.await` |
| CLI/MCP → daemon | Unix socket, length-prefixed JSON (`DaemonCommand`/`DaemonReply`) | Connect failure = `DaemonNotRunning` → CLI falls back (e.g. in-process screenshot); socket perms 0600, no auth (local-only by design) |
| analyze → memory | Direct `MemoryEngine::ingest_activities` call after classify | Same-process today; keep the call explicit so a future daemon-owned background task can reuse it |
| memory internals | `MemoryEngine` composes embedder + vector store + graph store | Facade owns `open/ingest/rebuild/search`; add `memory`/`memory-graph` paths to `DataPaths` instead of hardcoding joins |
| frontends → stores | Each binary opens its own `Store` + `MemoryEngine` read handles | Set `busy_timeout` everywhere; open once per process invocation and pass `&Store` down |

## Suggested Build Order (dependency implications for roadmap)

1. **Ledger + paths + config honesty first** — `DataPaths` as single path authority,
   `busy_timeout` on every connection, delete-or-wire every dead config field. Everything
   else reads config and paths; nothing is trustworthy until this is.
2. **Capture hardening second** — char-boundary-safe truncation (ends 3 known panics),
   hotkey + scheduler triggers, sleep survival, real permissions probing. Capture is the
   hot path and the Core Value; it must not lose data before anything downstream matters.
3. **Privacy gate third** — redactor fixes + per-app exclusions enforced at both capture
   and analysis. Must land before any new data flows to a cloud LLM (blocks provider work).
4. **Provider seam + local endpoint fourth** — injectable `&dyn ActivityClassifier`,
   Anthropic impl, Ollama/llama.cpp base-URL impl. Depends on (3) for the scrub contract.
5. **Real embeddings + hybrid search fifth** — fastembed/Ollama embedder behind the trait,
   `vec0` KNN, RRF fusion replacing ad-hoc `+0.2`/max-score merge, FTS actually populated.
   Depends on (4) only for ingest text quality, not structurally — can parallelize.
6. **Graph expansion sixth** — fix `hops`-as-LIMIT to real traversal, entity extraction
   at ingest. Pure retrieval-quality work; independent of (4)–(5) except sharing ingest.
7. **Frontends last** — CLI `ask`, TUI dashboard/search box, MCP tools. They are thin
   readers over (1)–(6); building them first bakes in whatever the stores happen to return
   today. TUI tick polling and MCP stderr discipline are the only structural risks.

## Sources

- Rewind.ai app teardown (Kevin Chen, kevinchen.co) — ScreenCaptureKit + Vision OCR + SQLite FTS + H.264 chunks schema; segment/frame/node/search tables — HIGH
- Retrace repo (mihai-satmarean/retrace) — Swift helpers + Python core, capture→caption→embed→persist pipeline, FTS5+semantic+hybrid search modes, read-only MCP — HIGH
- 2ndm1nd-runtime (bayraak) — capture/brain process split, SQLite+FTS5 ledger schema, provider-seam docs, no-LLM-in-capture-path — HIGH
- Mnemosyne (vel5id) — Go 5Hz watcher → Redis Stream → Python/Ollama brain → SQLite WAL archive; tiered capture/ingestion/cognition/storage — MEDIUM
- activity-frames (nossa-y) — measured-vs-inferred two-tier contract, deterministic compiler over capture DB, MCP tools — MEDIUM
- Mem0 docs (how-it-works, graph-memory, memory-evaluation) — add/search loop, 3-store split (SQL/vector/entity), multi-signal retrieval + entity boost, ADD-only extraction — HIGH
- sqlite-vec hybrid guides (Alex Garcia; Jez's blog) — FTS5 + vec0 + RRF in pure SQL, three fusion methods, RRF k=60 constant — HIGH
- sqlite-rag-mcp (giuseppeferretti) — stdio MCP + RRF + Ollama embeddings + lexical fallback contract — MEDIUM
- vstash paper (arXiv 2604.15484) — adaptive RRF + single-file SQLite local-first retrieval, BEIR evals — MEDIUM
- llm-trait/llm-unified crates + base14 Rust LLM observability guide — provider-trait + factory + OpenAI-compat base-URL pattern for Ollama/local — MEDIUM
- Ratatui official docs (async event stream, Tui template, event-driven template) — Tui/EventHandler/tick-render loop pattern — HIGH
- Daemon/IPC precedents: mosaico daemon-design (single-writer UDS daemon + thin RPC clients), synwire-daemon (stdio↔UDS MCP proxy), toki DESIGN (writer-thread owns DB, broadcast sink) — MEDIUM
- rustwatch own codebase map (`.planning/codebase/ARCHITECTURE.md`, CONCERNS.md) — current Store/SegmentGrouper/MemoryEngine/IPC ground truth — HIGH

---
*Architecture research for: rustwatch (local activity memory, macOS, Rust)*
*Researched: 2026-10-02*
