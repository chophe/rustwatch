---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
# Codebase Structure

**Analysis Date:** 2026-10-02

## Directory Layout

```
rustwatch/
├── Cargo.toml                      # Workspace root: members, shared deps, release profile
├── Cargo.lock                      # 648 resolved packages (committed)
├── README.md                       # Build, binaries, data layout, permissions
├── LICENSE                         # MIT
├── .gitignore                      # target/, debug, *.rs.bk, *.pdb
│
├── crates/
│   ├── rustwatch-core/             # Domain kernel — no internal deps
│   │   ├── Cargo.toml
│   │   ├── migrations/
│   │   │   └── V1__initial.sql     # events/segments/screenshots/activities + 3 indexes
│   │   └── src/
│   │       ├── lib.rs              # 15 lines: module decls + 6 re-exports
│   │       ├── events.rs           # CaptureEvent, CaptureEventKind, *Record types
│   │       ├── segment.rs          # SegmentGrouper (stateful fold)
│   │       ├── db.rs               # Store (SQLite) + embed_migrations! + row mappers
│   │       ├── config.rs           # Config (6 sections) + Default impl
│   │       ├── paths.rs            # DataPaths, expand_tilde, load_or_create_config
│   │       ├── ipc.rs              # DaemonCommand/Reply/State, DaemonClient, handle_connection
│   │       ├── error.rs            # thiserror Error + Result alias
│   │       └── rustwatch-capture/  # ⚠ STRAY EMPTY DIRS — delete (see below)
│   │           └── src/platform/
│   │
│   ├── rustwatch-capture/          # OS input capture (macOS-only at runtime)
│   │   ├── Cargo.toml              # [target.'cfg(target_os="macos")'.dependencies]
│   │   └── src/
│   │       ├── lib.rs              # CaptureHandle newtype over PlatformCapture
│   │       └── platform/
│   │           ├── mod.rs          # cfg selector: macos.rs | stub.rs
│   │           ├── macos.rs        # keytap loop, focus poll loop, xcap screenshots (359 lines)
│   │           └── stub.rs         # every method → Error::UnsupportedPlatform
│   │
│   ├── rustwatch-daemon/           # Binary `rustwatchd` — writer task + socket server
│   │   ├── Cargo.toml              # [[bin]] name = "rustwatchd", path = "src/main.rs"
│   │   └── src/main.rs             # 164 lines: run_daemon, IPC handler match
│   │
│   ├── rustwatch-cli/              # Binary `rustwatch` — user-facing commands
│   │   ├── Cargo.toml              # [[bin]] name = "rustwatch", path = "src/main.rs"
│   │   └── src/
│   │       ├── main.rs             # 116 lines: clap grammar + subcommand dispatch
│   │       ├── commands.rs         # 292 lines: one fn per subcommand
│   │       └── tui.rs              # 127 lines: ratatui dashboard, 250ms poll loop
│   │
│   ├── rustwatch-analyze/          # LLM classification + chart rendering
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs              # 7 lines: mod decls + re-exports
│   │       ├── classifier.rs       # ActivityClassifier trait, 2 impls, analyze_pending (180)
│   │       ├── redact.rs           # Redactor: regex scrub + app exclusion (41)
│   │       └── chart.rs            # render_chart → terminal/json/html (58)
│   │
│   ├── rustwatch-memory/           # Retrieval: vector + graph + embedder
│   │   ├── Cargo.toml              # [features] fastembed (optional dep)
│   │   └── src/
│   │       ├── lib.rs              # MemoryEngine facade (110)
│   │       ├── sqlite_store.rs     # MemoryChunk, ScoredChunk, SqliteMemoryStore (149)
│   │       ├── graph.rs            # GraphStore, slug() (123)
│   │       ├── embedder.rs         # Embedder + hash_embedding fallback (61)
│   │       └── rag.rs              # GraphRag::merge, SearchHit (40)
│   │
│   ├── rustwatch-mcp/              # Binary `rustwatch-mcp` — stdio JSON-RPC
│   │   ├── Cargo.toml              # [features] rmcp (optional dep, UNUSED in code)
│   │   └── src/main.rs             # 122 lines: line loop + 5 tool handlers
│   │
│   └── rustwatch-memory-backends/  # ⚠ NOT A WORKSPACE MEMBER — dead code
│       ├── Cargo.toml              # Pinned versions, version="0.1.0" hard-coded
│       └── src/
│           ├── lib.rs              # Re-exports only
│           ├── lance.rs            # LanceMemoryStore (134)
│           └── surreal.rs          # SurrealGraphStore (124)
│
├── deploy/
│   └── macos/
│       └── com.rustwatch.plist     # launchd agent template, {{RUSTWATCHD_PATH}} token
│
├── docs/
│   └── TESTING_PLAN.md             # 4-layer test plan — zero tests implemented
│
├── .planning/
│   └── codebase/                   # GSD analysis artifacts (this directory)
│
└── .crush/                         # Crushed-code tool state; crush.db is generated
```

## Directory Purposes

**`crates/`:**

- Purpose: the entire implementation. Seven workspace members plus one detached crate.
- Contains: one directory per crate, each with exactly one `Cargo.toml` and one `src/`.
- Key files: root `Cargo.toml:2-10` (`[workspace] members`), root `Cargo.toml:50-52` (`[profile.release] lto = true, codegen-units = 1`).
- **Layout rule:** every member is flat — `src/*.rs` directly, one level deep. Only `rustwatch-capture` nests, and only because of the `platform/` seam. No `src/bin/`, no `examples/`, no `benches/`, no `tests/`.

**`crates/rustwatch-core/src/`:**

- Purpose: the shared vocabulary. Every type crossing a crate boundary is declared or re-exported here (`crates/rustwatch-core/src/lib.rs:9-15`).
- Contains: 8 modules. `lib.rs` is a 15-line index of `pub mod` + `pub use` — the crate's public API is a deliberate, curated re-export surface, not the raw module tree.
- Key files: `crates/rustwatch-core/src/lib.rs` (the contract), `crates/rustwatch-core/src/db.rs` (the only writer to `rustwatch.db`), `crates/rustwatch-core/src/events.rs` (the cross-crate currency).

**`crates/rustwatch-core/migrations/`:**

- Purpose: versioned SQL schema for `rustwatch.db`, embedded at compile time.
- Contains: `V1__initial.sql` — 4 tables (`events`, `segments`, `screenshots`, `activities`) and 3 indexes, all `CREATE ... IF NOT EXISTS`.
- Compiled in by `embed_migrations!("migrations")` at `crates/rustwatch-core/src/db.rs:10`; run on every `Store::open` at `:23-25`. Naming convention is `V{n}__{snake_case_description}.sql` (refinery).
- The two memory SQLite files (`memory.db`, `memory-graph.db`) deliberately do **not** use refinery — they run inline `CREATE TABLE IF NOT EXISTS` DDL at open (`crates/rustwatch-memory/src/sqlite_store.rs:32-52`, `crates/rustwatch-memory/src/graph.rs:15-30`). If you change memory schema you must hand-edit those two files; there is no migration path.

**`crates/rustwatch-capture/src/platform/`:**

- Purpose: the OS-specific seam.
- Contains: `mod.rs` (9-line `cfg` selector), `macos.rs` (real implementation, macOS deps only), `stub.rs` (non-macOS; every method errors).
- **To add a platform:** create `crates/rustwatch-capture/src/platform/<os>.rs`, copy `stub.rs`'s exact signatures, then add two arms to `crates/rustwatch-capture/src/platform/mod.rs` and a `[target.'cfg(target_os = "<os>")'.dependencies]` block to `crates/rustwatch-capture/Cargo.toml`. The 6-method `PlatformCapture` surface must match exactly: `new`, `permissions`, `start`, `capture_screenshot`, `set_paused`, plus `PermissionsReport { input_monitoring, accessibility, screen_recording, notes }`.

**`deploy/`:**

- Purpose: OS packaging artifacts only.
- Contains: `macos/com.rustwatch.plist` — launchd user agent with `RunAtLoad: true`, `KeepAlive: true`, logs to `/tmp/rustwatchd.{out,err}.log`.
- **Templating:** the literal string `{{RUSTWATCHD_PATH}}` is replaced at install time by `.replace(...)` in `crates/rustwatch-cli/src/commands.rs:29-30`, then written to `~/Library/LaunchAgents/com.rustwatch.plist`. The plist is compiled into the CLI binary via `include_str!` (`crates/rustwatch-cli/src/commands.rs:29`) — **edit the file and rebuild**, it is not read at runtime.

**`docs/`:**

- Purpose: single planning document, not generated API docs.
- Contains: `TESTING_PLAN.md` — a 4-layer plan (unit / behavior / integration / HTTP contract) with named proptest, wiremock, assert_cmd, tempfile, nextest targets. **None of it is implemented**; treat it as the intended test layout, not current reality.

**`.planning/codebase/`:**

- Purpose: GSD codebase-analysis artifacts consumed by `/gsd-plan-phase`.
- Contains: `STACK.md`, `INTEGRATIONS.md`, `ARCHITECTURE.md`, `STRUCTURE.md`, plus `CONCERNS.md` / `TESTING.md` / `CONVENTIONS.md` when mapped.
- Committed to git. Do not put source-of-truth information here — it will drift.

## Key File Locations

**Entry Points:**

- `crates/rustwatch-cli/src/main.rs:74` — `#[tokio::main] main()` for `rustwatch`; bootstrap at `:83-86`, dispatch match at `:88-113`.
- `crates/rustwatch-daemon/src/main.rs:18` — `main()`; hands off to `run_daemon` at `:30`.
- `crates/rustwatch-mcp/src/main.rs:9` — `main()`; stdin loop at `:25-64`.
- `crates/rustwatch-capture/src/platform/macos.rs:70` and `:76` — the two `std::thread::spawn` calls that originate all captured data.

**Configuration:**

- `Cargo.toml:13-17` — `[workspace.package]` (version, edition, license, authors). Per-crate manifests use `version.workspace = true` etc.
- `Cargo.toml:20-49` — `[workspace.dependencies]`. **Always add third-party deps here, not to individual crates** — every workspace member references them as `anyhow.workspace = true`.
- `Cargo.toml:50-52` — `[profile.release]`.
- `crates/rustwatch-core/src/config.rs:5-59` — the runtime `Config` struct (6 sections: `data`, `capture`, `analyze`, `memory`, `ui`, `privacy`).
- `crates/rustwatch-core/src/config.rs:61-104` — `impl Default for Config`; the single source of truth for defaults.
- `crates/rustwatch-core/src/paths.rs:8-49` — `DataPaths`, the resolved location of every file.
- `deploy/macos/com.rustwatch.plist` — launchd template.

**Core Logic:**

- `crates/rustwatch-core/src/segment.rs:28-61` — `SegmentGrouper::on_event`, the focus-change fold. Pure logic, fully unit-testable with no I/O. Highest-value first test target.
- `crates/rustwatch-core/src/segment.rs:9` — `MAX_BUFFER_CHARS = 16_384`, the eviction policy.
- `crates/rustwatch-core/src/db.rs:29-99` — all four `insert_*` methods; `:101-225` all five query methods.
- `crates/rustwatch-core/src/db.rs:157-180` — `list_unanalyzed_segments`, the `LIKE`-on-JSON query flagged in `ARCHITECTURE.md`.
- `crates/rustwatch-core/src/ipc.rs:51-67` and `:70-86` — the symmetric client/server frame codec.
- `crates/rustwatch-analyze/src/classifier.rs:8-19` — the `ActivityClassifier` trait and its factory.
- `crates/rustwatch-analyze/src/classifier.rs:21-66` — `analyze_pending`: select → redact → classify → persist.
- `crates/rustwatch-analyze/src/redact.rs:25-40` — the privacy boundary.
- `crates/rustwatch-memory/src/lib.rs:104-109` — `MemoryEngine::search`, the hybrid retrieval pipeline.
- `crates/rustwatch-memory/src/rag.rs:6-31` — `GraphRag::merge`: `+0.2` keyword boost, dedupe-by-max, sort desc. Pure, deterministic, ideal first test.
- `crates/rustwatch-memory/src/sqlite_store.rs:86-115` — full-table cosine scan; `cosine()` at `:131-149` is pure and trivially testable.
- `crates/rustwatch-memory/src/embedder.rs:46-61` — `hash_embedding`, pure and deterministic.
- `crates/rustwatch-capture/src/platform/macos.rs:252-296` — `key_to_text`, a pure 37-arm match. Testable without macOS permission.
- `crates/rustwatch-capture/src/platform/macos.rs:355-359` — `hash_content` (SHA-256), pure.
- `crates/rustwatch-capture/src/platform/macos.rs:348-353` — `scope_label`, pure.
- `crates/rustwatch-daemon/src/main.rs:58-84` — the single writer task; the write path for all captures.
- `crates/rustwatch-daemon/src/main.rs:101-162` — the per-connection IPC handler.

**Schema:**

- `crates/rustwatch-core/migrations/V1__initial.sql` — `rustwatch.db` (4 tables).
- `crates/rustwatch-memory/src/sqlite_store.rs:32-52` — `memory.db`: `memory_chunks` + `memory_fts` (FTS5 virtual table).
- `crates/rustwatch-memory/src/graph.rs:15-30` — `memory-graph.db`: `graph_nodes` + `graph_edges`.
- `crates/rustwatch-memory-backends/src/lance.rs:26-48` — LanceDB Arrow schema, created on first open.

**Testing:**

- None. Zero `#[cfg(test)]` modules, zero `#[test]` / `#[tokio::test]` functions, zero `tests/` directories, zero `[dev-dependencies]` in any manifest across the workspace.
- `docs/TESTING_PLAN.md` — the intended structure. Its Phase 0 prescribes adding `[workspace.dependencies]` dev-deps and per-crate `[dev-dependencies]` blocks, `.config/nextest.toml`, and `.github/workflows/test.yml`.
- No CI at all: no `.github/`, no `.gitlab-ci.yml`, no `.circleci/`.

**Tooling / Lint Config:**

- None present. No `rustfmt.toml`, no `clippy.toml`, no `.cargo/config.toml`, no `rust-toolchain.toml`, no `justfile`, no `Makefile`, no `build.rs` anywhere. Formatting is whatever `cargo fmt` defaults produce; the toolchain version floats with the installed stable.

## Naming Conventions

**Crates:**

- Pattern: `rustwatch-<role>` — `rustwatch-core`, `-capture`, `-daemon`, `-cli`, `-analyze`, `-memory`, `-memory-backends`, `-mcp`.
- Use a role suffix, never a numbered or capitalized variant. Adding a crate means picking a new role noun.

**Binaries:**

- Pattern: three binaries, named from the package with an explicit `[[bin]]` override.
- `crates/rustwatch-cli/Cargo.toml` → binary `rustwatch` (package name differs from binary name).
- `crates/rustwatch-daemon/Cargo.toml` → binary `rustwatchd` (contract suffix `-d`).
- `crates/rustwatch-mcp/Cargo.toml` → binary `rustwatch-mcp` (same as package name).
- **The package name is not the binary name for two of three crates.** `std::env::current_exe()?.parent()?.parent()?.join("rustwatchd")` is the idiom for locating a sibling binary (`crates/rustwatch-cli/src/commands.rs:23-27`, `:44-48`).

**Files:**

- `snake_case.rs`, one noun per file, matching the primary type it declares: `events.rs`→`CaptureEvent`, `segment.rs`→`SegmentGrouper`, `db.rs`→`Store`, `paths.rs`→`DataPaths`, `ipc.rs`→IPC types, `error.rs`→`Error`, `config.rs`→`Config`, `chart.rs`→`render_chart`, `redact.rs`→`Redactor`, `rag.rs`→`GraphRag`, `graph.rs`→`GraphStore`, `embedder.rs`→`Embedder`.
- Feature-named files use a verb: `classifier.rs` (`classify`), `sqlite_store.rs` (`SqliteMemoryStore`), `lance.rs`, `surreal.rs`.
- `main.rs` is `main.rs` everywhere; there are no `lib.rs` files in any binary crate.
- Migrations: `V{n}__{snake_case}.sql` — refinery convention. Only `V1__initial.sql` exists.

**Directories:**

- `crates/<crate-name>/` — kebab-case, matching the package name exactly.
- `src/platform/` — the only nested module directory in the workspace. Use `cfg`-selected sibling files inside it, never nested subdirs per platform.
- `migrations/` — directly under the crate root (not under `src/`), because `embed_migrations!("migrations")` resolves relative to `CARGO_MANIFEST_DIR` (`crates/rustwatch-core/src/db.rs:10`).

**Types:**

- `PascalCase` structs and enums, no prefix — `Store`, `SegmentGrouper`, `CaptureEvent`, `DataPaths`, `MemoryEngine`, `Redactor`, `GraphRag`.
- Traits get the `-or` / role noun: `ActivityClassifier`, `PlatformCapture`, `PermissionsReport`.
- Suffix `Store` for anything wrapping a persistence handle: `Store`, `SqliteMemoryStore`, `GraphStore`, `LanceMemoryStore`, `SurrealGraphStore`.
- Suffix `Handle` for a shared, cheap-clone wrapper around a resource: `CaptureHandle` (implements `Clone` via `Arc<AtomicBool>`, `crates/rustwatch-capture/src/lib.rs:14-20`).
- Suffix `Record` for rows read back from SQLite: `SessionSegment` is the exception (it has no `Record` suffix despite being a table row); `ActivityRecord` and `ScreenshotRecord` follow it.

**Fields and identifiers:**

- `snake_case` for fields, locals, functions, modules. Structs are always `pub` with `pub` fields and `#[derive(Debug, Clone, Serialize, Deserialize)]` (`crates/rustwatch-core/src/events.rs:6-13`, `:50-56`).
- IDs are `String` holding a UUID v4 `to_string()`, never a `Uuid` struct — `CaptureEvent::new` (`crates/rustwatch-core/src/events.rs:61`), `SegmentGrouper::on_focus` (`crates/rustwatch-core/src/segment.rs:74`), `Store::new_activity_id` (`crates/rustwatch-core/src/db.rs:247-249`).
- Timestamps are `chrono::DateTime<Utc>` in memory, persisted as RFC3339 `String` via `.to_rfc3339()`.
- Serde enums are **internally tagged** with `#[serde(tag = "...", rename_all = "snake_case")]`: `"type"` for `CaptureEventKind` (`crates/rustwatch-core/src/events.rs:15`), `"cmd"` for `DaemonCommand` (`crates/rustwatch-core/src/ipc.rs:20`), `"reply"` for `DaemonReply` (`:31`). Follow this for any new protocol enum.
- Private module helpers are `snake_case` free functions at the bottom of the file, not `impl` blocks: `parse_ts` / `map_event_row` (`crates/rustwatch-core/src/db.rs:252`, `:258`), `append_text` / `truncate` (`crates/rustwatch-core/src/segment.rs:106`, `:116`), `slug` (`crates/rustwatch-memory/src/graph.rs:112`), `cosine` / `bytes_to_f32` (`crates/rustwatch-memory/src/sqlite_store.rs:124`, `:131`), `key_to_text` / `capture_to_disk` (`crates/rustwatch-capture/src/platform/macos.rs:252`, `:298`), `build_prompt` (`crates/rustwatch-analyze/src/classifier.rs:166`).

**SQL:**

- Tables and columns: `snake_case`, plural table names (`events`, `segments`, `activities`, `screenshots`, `memory_chunks`, `graph_nodes`, `graph_edges`).
- Indexes: `idx_<table>_<column>` — `idx_events_timestamp`, `idx_segments_started_at`, `idx_activities_started_at` (`crates/rustwatch-core/migrations/V1__initial.sql:8`, `:19`, `:38`).
- Positional parameters `?1..?N` with `rusqlite::params![]` in `Store` (`crates/rustwatch-core/src/db.rs:38`); anonymous `?` in the memory stores (`crates/rustwatch-memory/src/sqlite_store.rs:66`).
- Idempotent writes use `INSERT OR REPLACE` for derived/projection tables (`segments`, `activities`, `memory_chunks`, `graph_nodes`) and plain `INSERT` for the append-only source-of-truth tables (`events`, `screenshots`, `graph_edges`).

## Where to Add New Code

**New CLI command (subcommand):**

1. Add a variant to `Commands` in `crates/rustwatch-cli/src/main.rs:17-55` with `#[arg]` attributes.
2. Add a match arm in the dispatch at `crates/rustwatch-cli/src/main.rs:88-113` — the match is exhaustive, so the compiler enforces this.
3. Add the handler `pub async fn my_command(paths: &DataPaths, config: &Config) -> anyhow::Result<()>` to `crates/rustwatch-cli/src/commands.rs`. Signatures follow the two existing shapes: sync for local work (`status`, `export`, `chart`, `permissions` take no config or ignore it), async when touching `MemoryEngine` or `DaemonClient` (`analyze`, `tail`, `memory_*`). Import paths use `rustwatch_core::{...}` grouped at the top of the file (`crates/rustwatch-cli/src/commands.rs:11-15`).

**New CLI subcommand group (like `memory`):**

- Add a second `#[derive(Subcommand)] enum` next to `MemoryCommands` at `crates/rustwatch-cli/src/main.rs:57-72`, declare it as `#[command(subcommand)] command: ...` inside the parent variant (`:50-53`), and nest the match (`:101-111`).

**New OS platform for capture:**

1. Create `crates/rustwatch-capture/src/platform/<os>.rs` starting from `crates/rustwatch-capture/src/platform/stub.rs` (60 lines) so the signatures match exactly.
2. Add the two `cfg` arms in `crates/rustwatch-capture/src/platform/mod.rs:1-9`.
3. Add `[target.'cfg(target_os = "<os>")'.dependencies]` to `crates/rustwatch-capture/Cargo.toml` mirroring the macOS block.
4. Add any new capability in `crates/rustwatch-core/src/config.rs` **with a `Default` value in `crates/rustwatch-core/src/config.rs:61-104`**, and thread it through `CaptureHandle::start` (`crates/rustwatch-capture/src/lib.rs:29-44`).

**New LLM provider:**

1. Add a struct to `crates/rustwatch-analyze/src/classifier.rs` with the same three fields as `OpenAiClassifier` (`:68-72`): `reqwest::Client`, `model`, `api_key`.
2. `impl ActivityClassifier for YourClassifier` with `#[async_trait]`, returning `serde_json::from_str::<ActivityBatchResponse>(content)?.activities`.
3. Add one arm to `build_classifier` at `crates/rustwatch-analyze/src/classifier.rs:14-18`.
4. The response shape is fixed by `ActivityBatchResponse` / `ActivityLabel` in `crates/rustwatch-core/src/events.rs:104-123` — conform to it rather than defining new types.
5. Optionally gate the provider on a new `analyze.provider` string; no other wiring needed.

**New memory backend (vector or graph):**

1. Define the trait in `crates/rustwatch-memory/src/` (e.g. `vector_store.rs`) and re-export it from `crates/rustwatch-memory/src/lib.rs:6-10`.
2. Implement it against the existing `MemoryChunk` / `ScoredChunk` types (`crates/rustwatch-memory/src/sqlite_store.rs:1-20`) — do not define parallel chunk types, which is exactly the mistake `crates/rustwatch-memory-backends/src/` made.
3. Change the `MemoryEngine` fields from concrete to `Box<dyn ...>` (`crates/rustwatch-memory/src/lib.rs:15-16`) and branch on `config.memory.vector_backend` / `config.memory.graph_backend` in `MemoryEngine::open` (`crates/rustwatch-memory/src/lib.rs:22-29`) — those config fields already exist and are currently ignored.
4. Declare a single shared `EMBEDDING_DIMS` constant; do not re-hardcode `384` (duplicated at `crates/rustwatch-memory/src/embedder.rs:24`, `:29` and `crates/rustwatch-memory-backends/src/lance.rs:12`).

**New persistent field on an existing entity:**

- `rustwatch.db` → add `V2__<name>.sql` to `crates/rustwatch-core/migrations/`. It is picked up automatically by `embed_migrations!` (`crates/rustwatch-core/src/db.rs:10`) — no registration step. Also update the struct in `crates/rustwatch-core/src/events.rs` and every SELECT/INSERT in `crates/rustwatch-core/src/db.rs` (these are hand-written, not query-built — no codegen will catch a missed column).
- `memory.db` / `memory-graph.db` → there is no migration system. Edit the inline DDL in `crates/rustwatch-memory/src/sqlite_store.rs:32-52` or `crates/rustwatch-memory/src/graph.rs:15-30` **and** plan a backfill; existing user databases will not pick up new columns.

**New MCP tool:**

1. Add the descriptor to the `tools/list` array in `crates/rustwatch-mcp/src/main.rs:44-52` using the `tool(name, description, schema)` helper at `:68-74`.
2. Add a match arm in `handle_tool` at `crates/rustwatch-mcp/src/main.rs:83-111` returning `json!({"content":[{"type":"text","text": ...}]})`.
3. For anything touching the daemon, use `daemon_text(paths, DaemonCommand::…)` (`:114-122`) rather than opening the socket yourself.

**New chart / report format:**

- Add a variant to `ChartFormat` (`crates/rustwatch-analyze/src/chart.rs:4-9`), a `render_<format>` private fn, and an arm in the `match` at `:13-17`. Add the format string to the CLI's ad-hoc parser at `crates/rustwatch-cli/src/commands.rs:213-217`.

**Shared helper that more than one service crate needs:**

- `crates/rustwatch-core/src/` — but only if it is genuinely domain vocabulary (a type, config field, path, or protocol). Concrete per-service logic does not belong here; put it in a new module inside the service crate. `crates/rustwatch-core/src/lib.rs:9-15` is the re-export list — anything not listed there is crate-internal.

**Third-party dependency:**

- Add to `[workspace.dependencies]` in the root `Cargo.toml` (`:20-49`), then reference it as `crate_name.workspace = true` in each consuming crate's `[dependencies]`. Do not pin a version in a member manifest. Exception: `crates/rustwatch-memory-backends/` is outside the workspace and pins its own versions — if you bring it in, first convert it to `version.workspace = true` and add it to `members`.

## Special Directories

**`crates/rustwatch-core/rustwatch-capture/src/platform/`:**

- Purpose: **none.** This is a leftover from a bad path — a stray `rustwatch-capture` tree nested inside `rustwatch-core`. It contains three empty directories and zero files (verified with `find -type f`).
- Generated: No — it is dead scaffolding from an earlier layout.
- Committed: git does not track empty directories, so it exists only in the local working tree. It will not survive a fresh clone and is safe to `rm -rf`.

**`target/`:**

- Purpose: Cargo build output — three binaries at `target/release/{rustwatch,rustwatchd,rustwatch-mcp}`.
- Generated: Yes.
- Committed: No (`.gitignore:4`).

**`.crush/`:**

- Purpose: state for the Crushed tool (`crush.db`, `logs/crush.log`) — a third-party tool's own workspace, not part of the rustwatch build.
- Generated: Yes.
- Committed: `.crush/.gitignore` exists (it self-excludes), `.gitignore` at root does not mention it. Confirm with `git check-ignore -v .crush/crush.db` before committing anything under it.

**`.planning/`:**

- Purpose: GSD planning state and codebase analysis artifacts.
- Generated: Yes (by GSD agents).
- Committed: Yes — this is intentional; `README.md`, `docs/`, `deploy/` are likewise committed as durable project assets.

**`Cargo.lock`:**

- Purpose: pinned resolution of all 648 transitive packages, including `openssl`/`native-tls` reachable via the optional fastembed/lancedb trees even though default builds use rustls-only (`crates/rustwatch-analyze/Cargo.toml` sets `default-features = false`).
- Generated: Yes, by Cargo.
- Committed: **Yes** — deliberate for a workspace shipping binaries. Do not add to `.gitignore`. When adding an optional backend, expect a large lockfile diff.

---

*Structure analysis: 2026-10-02*
