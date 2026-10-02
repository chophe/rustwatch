---
last_mapped_commit: 14fdad5e6d149c3edee1c2cdfd75d2d964b763c6
last_mapped_at: 2026-10-02
---
# Codebase Structure

**Analysis Date:** 2026-10-02

## Directory Layout

```text
rustwatch/
├── Cargo.toml                  # Workspace manifest: 7 members, shared dep versions, release LTO profile
├── Cargo.lock
├── README.md                   # Features, build, quick start, data layout, macOS permissions
├── LICENSE
├── crates/                     # All Rust source — one crate per layer
│   ├── rustwatch-core/         # Domain types, Config, DataPaths, Store, SegmentGrouper, IPC codec
│   │   ├── src/
│   │   │   ├── lib.rs          # Module declarations + flat re-exports
│   │   │   ├── config.rs       # Config tree + Default (writes config.toml shape)
│   │   │   ├── db.rs           # Store — SQLite access + embedded migrations
│   │   │   ├── error.rs        # thiserror Error enum + crate Result alias
│   │   │   ├── events.rs       # CaptureEvent, CaptureEventKind, SessionSegment, ActivityRecord, wire envelopes
│   │   │   ├── ipc.rs          # DaemonCommand/DaemonReply/DaemonState, DaemonClient, handle_connection
│   │   │   ├── paths.rs        # DataPaths, expand_tilde, load_or_create_config
│   │   │   └── segment.rs      # SegmentGrouper — event stream → SessionSegment fold
│   │   ├── migrations/
│   │   │   └── V1__initial.sql # events, segments, screenshots, activities (embedded at compile time)
│   │   └── rustwatch-capture/  # ⚠ EMPTY leftover scaffold — no files, safe to delete
│   │       └── src/platform/
│   ├── rustwatch-capture/      # Platform capture abstraction + per-OS impls
│   │   ├── src/
│   │   │   ├── lib.rs          # CaptureHandle facade + re-exports
│   │   │   └── platform/
│   │   │       ├── mod.rs      # cfg dispatch: macos.rs on macOS, stub.rs elsewhere
│   │   │       ├── macos.rs    # keytap keyboard loop, focus poll loop, xcap screenshots
│   │   │       └── stub.rs     # Returns Error::UnsupportedPlatform everywhere
│   │   └── Cargo.toml          # macOS-only deps gated: active-win-pos-rs, keytap, xcap
│   ├── rustwatch-daemon/       # Binary: rustwatchd
│   │   └── src/main.rs         # Whole daemon: writer task + IPC accept loop (164 lines, no lib)
│   ├── rustwatch-cli/          # Binary: rustwatch
│   │   ├── src/
│   │   │   ├── main.rs         # clap grammar + dispatch match
│   │   │   ├── commands.rs     # One fn per subcommand (292 lines, largest file in the repo)
│   │   │   └── tui.rs          # ratatui activity dashboard + key handling
│   │   └── Cargo.toml          # [[bin]] name = "rustwatch"
│   ├── rustwatch-analyze/      # Library: LLM classification, redaction, charts
│   │   └── src/
│   │       ├── lib.rs          # Re-exports classifier, chart, redact
│   │       ├── classifier.rs   # ActivityClassifier trait, build_classifier, analyze_pending, vendor impls
│   │       ├── chart.rs        # render_chart → terminal | json | html
│   │       └── redact.rs       # Redactor — privacy gate
│   ├── rustwatch-memory/       # Library: vector + graph memory
│   │   ├── src/
│   │   │   ├── lib.rs          # MemoryEngine facade
│   │   │   ├── sqlite_store.rs # MemoryChunk/ScoredChunk DTOs, vector store, FTS5, cosine
│   │   │   ├── graph.rs        # GraphStore — graph_nodes/graph_edges
│   │   │   ├── embedder.rs     # Embedder (fastembed feature / hash fallback)
│   │   │   └── rag.rs          # GraphRag::merge — hybrid score fusion, SearchHit
│   │   └── Cargo.toml          # [features] fastembed (optional heavy dep)
│   ├── rustwatch-mcp/          # Binary: rustwatch-mcp
│   │   ├── src/main.rs         # Hand-rolled JSON-RPC 2.0 over stdio
│   │   └── Cargo.toml          # [[bin]] name = "rustwatch-mcp"; optional rmcp dep (unused)
│   └── rustwatch-memory-backends/  # ⚠ NOT a workspace member — builds only via its own manifest
│       ├── src/
│       │   ├── lib.rs          # Re-exports both stores
│       │   ├── lance.rs         # LanceMemoryStore (Arrow RecordBatch, FixedSizeList 384)
│       │   └── surreal.rs      # SurrealGraphStore (surrealdb kv-mem)
│       └── Cargo.toml          # Own version/deps (not workspace-inherited); heavy: lancedb, surrealdb, arrow
├── deploy/
│   └── macos/
│       └── com.rustwatch.plist # launchd agent; {{RUSTWATCHD_PATH}} substituted by `rustwatch install`
├── docs/
│   └── TESTING_PLAN.md         # Five-phase test plan; source of the known-risk list
├── .planning/                  # GSD planning artifacts (not part of the build)
│   └── codebase/               # STACK / INTEGRATIONS / ARCHITECTURE / STRUCTURE / CONVENTIONS / TESTING
├── .crush/                     # Local agent-tooling state (crush.db, logs/) — not in .gitignore, not in the build
└── target/                     # Cargo build output — generated, gitignored
```

## Directory Purposes

**`crates/`:**

- Purpose: every line of Rust in the repo. One crate per architectural layer.
- Contains: `src/` (library modules) and/or `src/main.rs` (binary), plus `Cargo.toml` and, for core, `migrations/`.
- Key files: `crates/rustwatch-core/src/lib.rs`, `crates/rustwatch-cli/src/commands.rs`, `crates/rustwatch-capture/src/platform/macos.rs`

**`crates/rustwatch-core/`:**

- Purpose: the foundation every other crate builds on. No sibling-crate dependencies.
- Contains: domain types, config, paths, error, SQLite store, segmentation fold, IPC wire format, SQL migrations.
- Key files: `crates/rustwatch-core/src/db.rs`, `crates/rustwatch-core/src/events.rs`, `crates/rustwatch-core/migrations/V1__initial.sql`
- Note: the empty `rustwatch-capture/src/platform/` tree nested inside this crate is dead scaffolding — no files, no manifest, no references.

**`crates/rustwatch-capture/`:**

- Purpose: macOS OS-state → `CaptureEvent` conversion, hidden behind `PlatformCapture`.
- Contains: one facade module and a `platform/` directory with one file per OS.
- Key files: `crates/rustwatch-capture/src/lib.rs`, `crates/rustwatch-capture/src/platform/macos.rs`
- Note: heavy macOS-only crates (`keytap`, `xcap`, `active-win-pos-rs`) are declared under `[target.'cfg(target_os = "macos")'.dependencies]`, so the crate still builds elsewhere via `stub.rs`.

**`crates/rustwatch-analyze/`:**

- Purpose: turn raw segments into labelled activities, plus rendering.
- Contains: the one trait seam in the codebase (`ActivityClassifier`), two vendor impls, redaction, chart rendering.
- Key files: `crates/rustwatch-analyze/src/classifier.rs`, `crates/rustwatch-analyze/src/redact.rs`

**`crates/rustwatch-memory/`:**

- Purpose: retrieval over captured content — vectors, graph, embeddings, hybrid fusion.
- Contains: `MemoryEngine` facade over four collaborators; schemas created inline in `open()` rather than in migration files.
- Key files: `crates/rustwatch-memory/src/lib.rs`, `crates/rustwatch-memory/src/sqlite_store.rs`
- Note: this is the only crate whose SQL lives in Rust string literals (`sqlite_store.rs:33-51`, `graph.rs:16-29`), not in `migrations/`.

**`crates/rustwatch-memory-backends/`:**

- Purpose: alternative LanceDB + SurrealDB implementations of the memory stores.
- Contains: two store impls mirroring `SqliteMemoryStore` / `GraphStore` method-for-method.
- Key files: `crates/rustwatch-memory-backends/src/lance.rs`, `crates/rustwatch-memory-backends/src/surreal.rs`
- Note: excluded from `[workspace] members` (`Cargo.toml:3-11`), so `cargo build/test/clippy` at the root never compiles it. Build it explicitly with `--manifest-path crates/rustwatch-memory-backends/Cargo.toml`. `docs/TESTING_PLAN.md:29` lists adding it as a workspace member as Phase 0 work.

**`deploy/`:**

- Purpose: non-Rust artifacts required at runtime.
- Contains: the launchd plist template.
- Key files: `deploy/macos/com.rustwatch.plist`
- Note: `include_str!`-embedded into the CLI at `crates/rustwatch-cli/src/commands.rs:29`, so it is a compile-time input, not just an installer asset.

**`docs/`:**

- Purpose: prose about the project that is not the README.
- Contains: `TESTING_PLAN.md` — the authoritative list of known risks and required refactors. Cite it before planning changes to `SegmentGrouper`, `Redactor`, `expand_around_apps`, or `list_unanalyzed_segments`.

**`.planning/`:**

- Purpose: GSD planning and codebase-map artifacts. Not compiled, not shipped.
- Contains: `codebase/` maps (STACK, INTEGRATIONS, ARCHITECTURE, STRUCTURE, CONVENTIONS, TESTING, CONCERNS).
- Note: read `ARCHITECTURE.md`, `STRUCTURE.md`, and `CONVENTIONS.md` before writing code; `CONCERNS.md` lists the debt a new change should not deepen.

## Key File Locations

**Entry Points:**

- `crates/rustwatch-cli/src/main.rs:74` — `rustwatch` binary `main()`; clap grammar starts at `:18`, dispatch match at `:88`
- `crates/rustwatch-daemon/src/main.rs:18` — `rustwatchd` binary `main()`; `run_daemon` at `:30`, writer task at `:58`, accept loop at `:89`
- `crates/rustwatch-mcp/src/main.rs:9` — `rustwatch-mcp` binary `main()`; request loop at `:25`, tool dispatch at `:38`
- `crates/rustwatch-cli/src/tui.rs:12` — `tui::run`, alternate-screen entry point
- `crates/rustwatch-core/src/lib.rs:1` — library root for core (the crate everything depends on)
- `crates/rustwatch-capture/src/lib.rs:1`, `crates/rustwatch-analyze/src/lib.rs:1`, `crates/rustwatch-memory/src/lib.rs:1`, `crates/rustwatch-memory-backends/src/lib.rs:5` — other library roots

**Configuration:**

- `Cargo.toml` — workspace members, `[workspace.dependencies]` (all versions centralized here), release profile (`lto = true`, `codegen-units = 1`)
- `crates/rustwatch-core/src/config.rs:61` — `impl Default for Config`; **this is the schema of `~/.rustwatch/config.toml`**
- `crates/rustwatch-core/src/paths.rs:81` — `load_or_create_config`: read-or-write-default
- `crates/rustwatch-core/migrations/V1__initial.sql` — capture schema, embedded at `crates/rustwatch-core/src/db.rs:10`
- `crates/rustwatch-memory/src/sqlite_store.rs:33` — vector schema (inline `execute_batch`)
- `crates/rustwatch-memory/src/graph.rs:16` — graph schema (inline `execute_batch`)
- `deploy/macos/com.rustwatch.plist` — launchd template, rendered by `crates/rustwatch-cli/src/commands.rs:18`
- `.gitignore` — ignores `target/`, `debug`, `*.rs.bk`. Does **not** ignore `.crush/`

**Core Logic:**

- `crates/rustwatch-core/src/db.rs` — `Store`: every read/write of `rustwatch.db`; migrations at `:23`, unanalyzed-segment query at `:157`
- `crates/rustwatch-core/src/segment.rs:28` — `SegmentGrouper::on_event`, the segmentation state machine
- `crates/rustwatch-core/src/events.rs:16` — `CaptureEventKind`, the pipeline's tagged union
- `crates/rustwatch-core/src/ipc.rs:51,70` — IPC codec (client send / server handle)
- `crates/rustwatch-capture/src/platform/macos.rs:101,173` — keyboard loop, focus loop
- `crates/rustwatch-analyze/src/classifier.rs:21` — `analyze_pending` pipeline; `:13` provider factory; `:166` prompt builder
- `crates/rustwatch-analyze/src/redact.rs:25` — `Redactor::scrub`, the only privacy boundary
- `crates/rustwatch-memory/src/lib.rs:31,56,89,104` — ingest segments / activities, rebuild, search
- `crates/rustwatch-memory/src/rag.rs:6` — hybrid fusion scoring
- `crates/rustwatch-cli/src/commands.rs` — one function per CLI subcommand; largest file at 292 lines

**Testing:**

- `docs/TESTING_PLAN.md` — the plan; no tests exist yet
- `crates/*/tests/` — **do not exist**; no `#[cfg(test)]` module exists anywhere in the repo
- To add a unit test: put `#[cfg(test)] mod tests` at the bottom of the source file it covers (the pattern the plan prescribes)
- To add an integration test: create `crates/<crate>/tests/<name>_integration.rs`. Note that `rustwatch-cli`, `rustwatch-daemon`, and `rustwatch-mcp` expose no library target, so only binaries can be tested externally — `docs/TESTING_PLAN.md:99-104` requires splitting lib/bin first

## Naming Conventions

**Files:**

- `snake_case.rs`, one module per file, named after the primary type or concept it holds: `db.rs`→`Store`, `segment.rs`→`SegmentGrouper`, `graph.rs`→`GraphStore`, `paths.rs`→`DataPaths`
- Platform implementations are named after the OS: `platform/macos.rs`, `platform/stub.rs`
- Two-file exception: `platform/mod.rs` performs `cfg` dispatch and re-exports (`crates/rustwatch-capture/src/platform/mod.rs:1-9`). All other directories use flat modules declared in `lib.rs`
- Migrations use refinery's convention: `V<n>__<description>.sql` (`crates/rustwatch-core/migrations/V1__initial.sql`)
- Test files should use `*_integration.rs` for `tests/` (see `docs/TESTING_PLAN.md:82-93`)

**Types — use the suffix that matches the role:**

| Suffix | Meaning | Examples |
|--------|---------|----------|
| `Record` | a persisted row being written | `ActivityRecord` (`events.rs:83`), `ScreenshotRecord` (`events.rs:96`) |
| `Store` | owns a database connection | `Store` (`db.rs:12`), `SqliteMemoryStore`, `GraphStore`, `LanceMemoryStore`, `SurrealGraphStore` |
| `Config` | a settings sub-tree, deserialized from TOML | `Config`, `CaptureConfig`, `MemoryConfig`, `PrivacyConfig` |
| `Kind` / `Scope` | small enums carried inside a record | `CaptureEventKind`, `ScreenshotScope` |
| `Command` / `Reply` / `State` | IPC wire types | `DaemonCommand`, `DaemonReply`, `DaemonState` |
| `Request` / `Response` / `Batch` | LLM envelope types | `SegmentBatch`, `ActivityBatchResponse` |
| `Handle` / `Client` / `Engine` / `Grouper` / `Classifier` / `Redactor` | behavioral services | `CaptureHandle`, `DaemonClient`, `MemoryEngine`, `SegmentGrouper`, `ActivityClassifier`, `Redactor` |

- Structs: `PascalCase`. Enums: `PascalCase` variants, all snake_case after serde renaming.
- Serde-tagged enums always use `rename_all = "snake_case"` and a lowercase tag: `#[serde(tag = "type", ...)]` (`events.rs:15`), `tag = "cmd"` (`ipc.rs:20`), `tag = "reply"` (`ipc.rs:31`).

**Functions and modules:**

- Functions: `snake_case`, verb-first — `insert_event`, `list_events_since`, `open`, `flush`, `scrub`, `expand_tilde`, `ensure_parent`, `load_or_create_config`, `build_prompt`, `handle_connection`, `run_daemon`
- Predicate/query prefix convention on `Store`: `insert_*` for writes, `list_*` for reads, `get_*` for single-row reads
- Private module-level helpers are plain `fn` at the bottom of their file, unexported: `parse_ts` (`db.rs:252`), `map_event_row` (`db.rs:258`), `append_text` (`segment.rs:106`), `build_prompt` (`classifier.rs:166`), `cosine` (`sqlite_store.rs:131`), `slug` (`graph.rs:112`), `scope_label` (`macos.rs:348`)

**Crates and binaries:**

- Library crates: `rustwatch-<layer>` — `rustwatch-core`, `rustwatch-capture`, `rustwatch-analyze`, `rustwatch-memory`, `rustwatch-memory-backends`
- Binary crates keep the `-cli` / `-daemon` suffix but rename the binary: `rustwatch-cli` → `rustwatch`, `rustwatch-daemon` → `rustwatchd` (no hyphen, matching `launchd`), `rustwatch-mcp` → `rustwatch-mcp`
- Binaries declare an explicit `[[bin]]` block with `name` and `path` (`crates/rustwatch-cli/Cargo.toml:9`)

**Data naming:**

- SQL tables: plural `snake_case` — `events`, `segments`, `screenshots`, `activities`, `memory_chunks`, `memory_fts`, `graph_nodes`, `graph_edges`
- JSON-in-TEXT columns take an `_json` suffix — `app_json`, `payload_json`, `apps_json`, `topics_json`, `segment_ids_json`, `props_json`
- Timestamps: RFC3339 `TEXT` columns; `DateTime<Utc>` in Rust; dates as `%Y-%m-%d` strings
- Screenshot files: `~/.rustwatch/screenshots/YYYY-MM-DD/<unix-millis>-<scope>.png`, where scope is `window` or `screen` (`crates/rustwatch-capture/src/platform/macos.rs:302-304,348`)
- Graph ids: lowercase slug with every non-alphanumeric character replaced by `_` (`crates/rustwatch-memory/src/graph.rs:112`; duplicated verbatim at `crates/rustwatch-memory-backends/src/surreal.rs:113`)

## Where to Add New Code

**New capture source or new macOS permission (keyboard, clipboard, AX text field):**

- Event variant: `crates/rustwatch-core/src/events.rs:16` (add to `CaptureEventKind`)
- Emitter: `crates/rustwatch-capture/src/platform/macos.rs` (emit from `run_keyboard_loop` or `run_focus_loop`)
- Segmentation behavior: `crates/rustwatch-core/src/segment.rs:29` (add a match arm in `on_event`)
- **Do this:** add the `match` arm at the same time. `on_event` is exhaustive; omitting it is a compile error, which is correct — do not add a `_ =>` catch-all.
- **Non-macOS only:** `crates/rustwatch-capture/src/platform/stub.rs` needs no change — it implements the same inherent methods.

**New operating system support:**

- Implementation: `crates/rustwatch-capture/src/platform/<os>.rs` with the same inherent methods as `macos.rs`: `new`, `permissions`, `start`, `capture_screenshot`, `set_paused`
- Dispatch: `crates/rustwatch-capture/src/platform/mod.rs:1-9`
- Dependencies: gate under `[target.'cfg(...)'.dependencies]` in `crates/rustwatch-capture/Cargo.toml`

**New database column, table, or index:**

- `crates/rustwatch-core/migrations/V2__<description>.sql` — **never edit `V1__initial.sql`**; refinery records applied versions, so editing a shipped migration desynchronizes existing databases
- Query method: `crates/rustwatch-core/src/db.rs` (write → `insert_*`, read → `list_*`/`get_*`)
- Memory-layer schema instead goes inline in the relevant `open()`: `crates/rustwatch-memory/src/sqlite_store.rs:33` or `crates/rustwatch-memory/src/graph.rs:16`

**New LLM provider:**

- Implement the trait: `crates/rustwatch-analyze/src/classifier.rs:9` (`#[async_trait]`, matching the existing `OpenAiClassifier` at `:86` / `AnthropicClassifier` at `:136`)
- Register it: `crates/rustwatch-analyze/src/classifier.rs:13` (`build_classifier` match arm on `config.analyze.provider`)
- Add the model name to `Config::default()` at `crates/rustwatch-core/src/config.rs:80-85`

**New redaction rule:**

- `crates/rustwatch-core/src/config.rs:99` (`privacy.redact_patterns`) — **no code change needed.** `Redactor::new` compiles every pattern at `crates/rustwatch-analyze/src/redact.rs:12-18`.

**New chart format:**

- `crates/rustwatch-analyze/src/chart.rs:5` — add a `ChartFormat` variant and a `render_*` fn
- `crates/rustwatch-cli/src/commands.rs:213` — add the `--format` string arm
- `crates/rustwatch-cli/src/main.rs:47` — add the `--format` clap default if it should not be free-form

**New CLI subcommand:**

- Grammar: `crates/rustwatch-cli/src/main.rs:18` (variant in `Commands`)
- Implementation: `crates/rustwatch-cli/src/commands.rs` — one `pub fn` / `pub async fn`, take `&DataPaths` (+ `&Config` if it needs settings), print directly, return `anyhow::Result<()>`
- Dispatch: `crates/rustwatch-cli/src/main.rs:88` (add the arm to the match)

**New daemon capability:**

- `crates/rustwatch-core/src/ipc.rs:21` — `DaemonCommand` variant
- `crates/rustwatch-core/src/ipc.rs:32` — matching `DaemonReply` variant
- `crates/rustwatch-daemon/src/main.rs:103` — arm in the handler closure
- `crates/rustwatch-core/src/ipc.rs:10` — extend `DaemonState` only if the capability has observable state (and increment a matching `Arc<AtomicU64>`)

**New MCP tool:**

- Schema: `crates/rustwatch-mcp/src/main.rs:44` (add to the `tools/list` array via the `tool()` helper at `:68`)
- Implementation: `crates/rustwatch-mcp/src/main.rs:83` (arm in `handle_tool`); return `json!({"content":[{"type":"text","text": ...}]})`
- **Never write to stdout** outside the response line at `:62`.

**New memory backend (LanceDB / SurrealDB / anything else):**

- Implementation: `crates/rustwatch-memory-backends/src/<name>.rs`, mirroring the `open` / `upsert` / `search` / `expand_around_apps` shapes at `crates/rustwatch-memory-backends/src/lance.rs` and `surreal.rs`
- Export: `crates/rustwatch-memory-backends/src/lib.rs:5`
- To make it selectable at runtime you must also: add the crate to `[workspace] members` (`Cargo.toml:3-11`), and add a branch in `MemoryEngine::open` (`crates/rustwatch-memory/src/lib.rs:22-29`) keyed on `config.memory.vector_backend` / `graph_backend`. Consider introducing a shared trait first — `MemoryChunk` and `ScoredChunk` (`crates/rustwatch-memory/src/sqlite_store.rs:2,15`) already form a natural backend interface.
- Schema convention: `DIMS: i32 = 384` must match `Embedder::new` (`crates/rustwatch-memory/src/embedder.rs:29`)

**New config setting:**

- Field: `crates/rustwatch-core/src/config.rs` (struct + `Default`)
- **And** its consumer in the same change. A field with no reader is dead configuration; `docs/TESTING_PLAN.md` and `.planning/codebase/CONCERNS.md` both flag the current unread fields.

**Tests:**

- Unit test for a module → `#[cfg(test)] mod tests` at the bottom of that module's own file
- Integration test → `crates/<crate>/tests/<subject>_integration.rs`; needs `tempfile` in `[dev-dependencies]` for anything touching the filesystem
- Shared helper functions → `crates/rustwatch-core/src/<name>.rs`, declared in `crates/rustwatch-core/src/lib.rs` (it is the only crate every other crate depends on)

## Special Directories

**`crates/rustwatch-core/rustwatch-capture/`:**

- Purpose: none. Three nested empty directories (`rustwatch-capture/src/platform/`) left over from a misplaced scaffolding attempt.
- Generated: No
- Committed: Untracked by git — no file exists, so nothing is committed. Purely filesystem noise.
- Action: safe to `rm -rf`. It confuses every tool that walks the tree, including this map.

**`crates/rustwatch-memory-backends/`:**

- Purpose: alternative heavy memory backends.
- Generated: No
- Committed: Yes
- Special: excluded from `[workspace] members` (`Cargo.toml:3-11`), so it carries its own version pins and is invisible to root-level `cargo build`/`check`/`clippy`/`test`. Build with `cargo build --manifest-path crates/rustwatch-memory-backends/Cargo.toml`. Nothing in the workspace depends on it.

**`target/`:**

- Purpose: Cargo build output for all three binaries.
- Generated: Yes — never edit or commit
- Committed: No (`.gitignore:3`). Holds `target/release/rustwatch`, `target/release/rustwatchd`, `target/release/rustwatch-mcp`, which `rustwatch start` and `rustwatch install` locate by sibling path (`crates/rustwatch-cli/src/commands.rs:44-48`).

**`crates/rustwatch-core/migrations/`:**

- Purpose: versioned SQL schema for the capture database.
- Generated: No — hand-written
- Committed: Yes
- Special: compiled into the binary by `embed_migrations!` (`crates/rustwatch-core/src/db.rs:10`). Changes require a rebuild, and shipped migrations must not be edited.

**`deploy/macos/`:**

- Purpose: launchd LaunchAgent template.
- Generated: No — hand-written
- Committed: Yes
- Special: `include_str!`-embedded into the CLI (`crates/rustwatch-cli/src/commands.rs:29`). The installed copy at `~/Library/LaunchAgents/com.rustwatch.plist` is a *copy* — editing the template requires re-running `rustwatch install`.

**`.crush/`:**

- Purpose: local agent-tooling state (`crush.db`, `logs/crush.log`) from the Crush CLI.
- Generated: Yes
- Committed: **Not ignored** — `.gitignore` has no rule for it, so it will show up as untracked and can be committed by accident. Add a `.crush/` entry before the next commit.

**`.planning/`:**

- Purpose: GSD planning state and the codebase maps in `.planning/codebase/`.
- Generated: Partly — maps are regenerated by `/gsd-map-codebase`
- Committed: Yes
- Special: not in `.gitignore`; these documents are meant to be committed so future phases read them.

---

*Structure analysis: 2026-10-02*
