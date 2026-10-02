# Coding Conventions

**Analysis Date:** 2026-10-02

## Naming Patterns

**Crates:**
- `rustwatch-<noun>` — 8 packages: `rustwatch-core`, `rustwatch-capture`, `rustwatch-daemon`, `rustwatch-cli`, `rustwatch-analyze`, `rustwatch-memory`, `rustwatch-mcp`, `rustwatch-memory-backends` (`Cargo.toml:3-11`)
- One word, no version suffix. The `-backends` suffix marks the optional heavy-dependency crate.

**Files:**
- `snake_case.rs`, one concept per file: `segment.rs`, `redact.rs`, `embedder.rs`, `classifier.rs`, `sqlite_store.rs`, `graph.rs`, `rag.rs`
- The only nested module directory is `crates/rustwatch-capture/src/platform/` holding platform variants (`mod.rs`, `macos.rs`, `stub.rs`)
- Platform variants are named after the OS, not the abstraction: `macos.rs` / `stub.rs`, selected at `crates/rustwatch-capture/src/platform/mod.rs:1-9`

**Types (structs, enums, traits):** `PascalCase`
- `Store`, `SegmentGrouper`, `ActiveSegment`, `DataPaths`, `Config`
- `CaptureEvent`, `CaptureEventKind`, `ScreenshotScope`, `DaemonCommand`, `DaemonReply`, `DaemonState`
- `MemoryEngine`, `MemoryChunk`, `ScoredChunk`, `SearchHit`, `GraphRag`, `Embedder`
- `ActivityClassifier` (trait), `PlatformCapture`, `PermissionsReport`, `ChartFormat`

**Functions and methods:** `snake_case`
- Storage verbs read as SQL mirrors: `insert_event`, `insert_segment`, `insert_activity`, `insert_screenshot`, `list_events_since`, `list_segments_between`, `list_unanalyzed_segments`, `list_activities_for_date`, `get_segment`
- Predicates start with `is_`/`has_`: `is_excluded_app` (`crates/rustwatch-analyze/src/redact.rs:36`)
- Private helpers are plain verb phrases: `parse_ts`, `map_event_row`, `append_text`, `truncate`, `slug`, `cosine`, `bytes_to_f32`, `hash_embedding`, `flag`, `escape`, `scope_label`, `key_to_text`, `active_modifiers`

**Constructors — two distinct verbs, use the right one:**
- `new(...)` for in-memory value construction: `SegmentGrouper::new()` (`crates/rustwatch-core/src/segment.rs:24`), `Redactor::new(&Config)`, `Embedder::new(&str)`, `DaemonClient::new(impl AsRef<Path>)` (`crates/rustwatch-core/src/ipc.rs:45`), `CaptureHandle::new(Vec<String>)`, `DataPaths::new(Option<PathBuf>)`
- `open(path)` for anything that touches a file or database: `Store::open(&Path)` (`crates/rustwatch-core/src/db.rs:17`), `MemoryEngine::open(&DataPaths, &Config)`, `SqliteMemoryStore::open`, `GraphStore::open`, `LanceMemoryStore::open`, `SurrealGraphStore::open`
- Async openers keep the `open` name: `pub async fn open(...)` at `crates/rustwatch-memory/src/lib.rs:22`

**Conversions:**
- Consuming conversions use `into_`: `ActiveSegment::into_segment(self, ended_at)` (`crates/rustwatch-core/src/segment.rs:91`)
- Borrowing accessors use standard library names only: `as_ref()`, `as_str()`, `to_rfc3339()`, `display()`

**Constants:** `SCREAMING_SNAKE_CASE`, module-private
- `const MAX_BUFFER_CHARS: usize = 16_384;` (`crates/rustwatch-core/src/segment.rs:9`)
- `const TABLE: &str = "memory_chunks";` / `const DIMS: i32 = 384;` (`crates/rustwatch-memory-backends/src/lance.rs:11-12`)

**Command naming — enums mirror the CLI one-to-one:**
- `Commands` variants (`Install`, `Start`, `Status`, `Tail`, `Export`, `Analyze`, `Chart`, `Memory`, `Tui`) at `crates/rustwatch-cli/src/main.rs:18-55` map to same-named functions in `crates/rustwatch-cli/src/commands.rs` (`install`, `start`, `stop`, `status`, `permissions`, `tail`, `screenshot`, `export`, `analyze`, `chart`, `memory_search`, `memory_ingest`, `memory_graph`)
- `DaemonCommand` (`Ping`, `Status`, `Pause`, `Resume`, `Tail`, `Screenshot`) maps to `DaemonReply` (`Ok`, `Status`, `Events`, `Error`, `Screenshot`)
- **When adding a command, add the clap variant, the `commands::` function, the IPC command, and the reply variant together.**

**Enum serialization is explicit, never inferred:**
```rust
// crates/rustwatch-core/src/events.rs:15
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureEventKind { ... }

// crates/rustwatch-core/src/ipc.rs:20
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DaemonCommand { ... }

// crates/rustwatch-core/src/ipc.rs:31
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum DaemonReply { ... }
```
Unit-only enums get value renaming without a tag: `#[serde(rename_all = "snake_case")]` on `ScreenshotScope` (`crates/rustwatch-core/src/events.rs:44`).

## Code Style

**Formatting:**
- Tool: `rustfmt` (default configuration — **no `rustfmt.toml` exists**, so defaults apply: 4-space indent, `max_width = 100`, edition 2021 trailing-comma behavior)
- **The tree is not rustfmt-clean. `cargo fmt --check` reports 45 diff hunks across 17 of 30 `.rs` files.** `cargo fmt` has never been run on this codebase. Worst offenders: `crates/rustwatch-core/src/db.rs` (8 hunks), `crates/rustwatch-capture/src/platform/macos.rs` (8), `crates/rustwatch-cli/src/commands.rs` (7)
- The failures are all mechanical: over-long signatures the author wrote on one line (`crates/rustwatch-analyze/src/classifier.rs:21`), method chains collapsed below the width limit (`crates/rustwatch-analyze/src/redact.rs:37-39`), unsorted `pub use` (`crates/rustwatch-capture/src/lib.rs:3-4`)
- Line length is otherwise respected: only 25 of 3,185 lines (0.8%) exceed 100 columns, and nearly all of those are unbreakable string literals — SQL in `crates/rustwatch-core/src/db.rs:50` (144 cols), the HTML template in `crates/rustwatch-analyze/src/chart.rs:56` (277 cols), the JSON system prompt in `crates/rustwatch-analyze/src/classifier.rs:93` (184 cols)
- **Do this:** run `cargo fmt` before committing. Do not hand-tune line breaks — let rustfmt decide, except inside string literals.

**Linting:**
- Tool: `clippy` (default lint groups — **no `clippy.toml`, no `[lints]` table in any `Cargo.toml`**, no `#[allow(...)]` anywhere in the workspace, no `#[deny(...)]`)
- **Current state: `cargo clippy --workspace --all-targets` yields exactly 3 warnings, 0 errors** (verified on clippy 1.97.0):
  - `dead_code` — `pub fn hash_content` at `crates/rustwatch-capture/src/platform/macos.rs:355` is never called. It is `pub` but lives in a private `mod macos`, so `pub` does not exempt it.
  - `clippy::vec_init_then_push` — `crates/rustwatch-capture/src/platform/macos.rs:46-48` builds `Vec::new()` then immediately pushes twice
  - `clippy::collapsible_match` — `crates/rustwatch-cli/src/tui.rs:115-120` has `if` inside a `match` arm
- No CI enforces either tool (see `docs/TESTING_PLAN.md:107-115` for the intended gate: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`)

## Import Organization

**Order:** `std` → external crates → `rustwatch-*` → `crate::`. Blank line between groups.

```rust
// crates/rustwatch-core/src/paths.rs:1-6  — canonical example
use std::path::{Path, PathBuf};

use anyhow::Context;
use directories::ProjectDirs;

use crate::Result;
```

```rust
// crates/rustwatch-core/src/db.rs:1-8
use chrono::{DateTime, Utc};
use refinery::embed_migrations;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::{
    ActivityRecord, CaptureEvent, Result, ScreenshotRecord, SessionSegment,
};
```

Exceptions you will find, and whether to follow them:

| Case | Location | Follow it? |
|---|---|---|
| No blank line before `rustwatch_core::` — merged into the external group | `crates/rustwatch-cli/src/tui.rs:10` | **No.** Keep the blank line |
| Workspace crates in the same group as third-party crates | `crates/rustwatch-analyze/src/classifier.rs:1-2`, `crates/rustwatch-memory-backends/src/lance.rs:4-9` | **No.** Separate them |
| `use` statements after `pub use` / `mod` declarations | `crates/rustwatch-capture/src/lib.rs:1-8` | **No.** Put all `use` first |
| Function-scoped imports for platform-gated deps | `crates/rustwatch-capture/src/platform/macos.rs:105` (`use keytap::{EventKind, Key};`), `:299-300` (`use chrono::Utc; use xcap::{Monitor, Window};`) | **Yes** when the dependency is only reachable on one platform |
| Fully-qualified `serde::Serialize` in a derive instead of an import | `crates/rustwatch-memory/src/rag.rs:34` | **No.** Import `Serialize` like every other file does |

**Do this:** imports at the top of the file, `std` first, blank line between each group, no `use` after a `mod` or `pub use` declaration.

## Error Handling

**Two-tier split by crate boundary — this is the defining convention:**

1. **`rustwatch_core::Error` (thiserror) + the `pub type Result<T>` alias** — used by `rustwatch-core` and `rustwatch-capture` only. 25 signatures.
2. **`anyhow::Result<T>`** — used by every other crate and by all three binaries. 52 signatures.

The single error enum, `crates/rustwatch-core/src/error.rs:5-21`:
```rust
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("daemon not running")]
    DaemonNotRunning,
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error("{0}")]
    Other(String),
}
```
- **Messages are lowercase and carry no trailing period.** `"io error: {0}"`, `"daemon not running"`. Follow this when adding a variant.
- Three variants use `#[from]` (`Io`, `Db`, `Serde`) so `?` converts automatically. The other four are built by hand.
- `Other(String)` is the catch-all for foreign errors.

**Converting foreign errors:**
- std/filesystem into the enum: `.map_err(crate::Error::from)?` — `crates/rustwatch-core/src/db.rs:19`, `crates/rustwatch-core/src/paths.rs:39-42`
- Third-party SDK errors into `Other`: `.map_err(|e| rustwatch_core::Error::Other(e.to_string()))?` — 9 sites in `crates/rustwatch-capture/src/platform/macos.rs:309-339`
- Migration failures: `crate::Error::Other(format!("migration failed: {e}"))` — `crates/rustwatch-core/src/db.rs:23-25`
- `anyhow` adds context with `.context("literal")` or `.with_context(|| format!(...))` — `crates/rustwatch-core/src/paths.rs:83`, `crates/rustwatch-cli/src/commands.rs:27,48,54,67`, `crates/rustwatch-daemon/src/main.rs:86`
- Ad-hoc failures: `anyhow::bail!("unsupported provider: {other}")` (`crates/rustwatch-analyze/src/classifier.rs:17`), `anyhow::anyhow!("OPENAI_API_KEY not set")` (`:77`, `:127`)
- Errors that must cross the IPC boundary are stringified: `DaemonReply::Error { message: err.to_string() }` — `crates/rustwatch-daemon/src/main.rs:130,136,152`

**Propagation:** plain `?` everywhere. 199 `?;` sites across the workspace; there is not a single hand-written match that re-wraps an error.

**Panics:** only 4 `.unwrap()` calls exist, and zero `.expect()`:
- `crates/rustwatch-core/src/db.rs:183-184` — `and_hms_opt(0, 0, 0).unwrap()` / `and_hms_opt(23, 59, 59).unwrap()`, infallible for valid clock times
- `crates/rustwatch-cli/src/commands.rs:192` — constant `ProgressStyle::with_template` string
- `crates/rustwatch-memory-backends/src/lance.rs:23` — `path.to_str().unwrap()`, **will panic on a non-UTF-8 path** (and that crate is not even a workspace member, so it has never been compiled)

**Do this:** never add `.unwrap()` on anything fallible. Use `?`, or an explicit fallback.

### Silent fallbacks — the most important convention to understand

The codebase degrades instead of failing in 17 places. Some are deliberate; two are not.

`let _ = expr;` — 17 sites, discarding a `Result`:
| Site | Intent |
|---|---|
| `crates/rustwatch-capture/src/platform/macos.rs:134,143,153,203,215,224` (6×) | **Deliberate** — `UnboundedSender::send` on a channel that is never closed; a dropped keystroke must not stop the tap |
| `crates/rustwatch-capture/src/platform/macos.rs:71-73,84-86` | **Deliberate** — thread-loop death is reported via `warn!(?err, ...)` |
| `crates/rustwatch-cli/src/commands.rs:71-72` | **Deliberate** — best-effort pid/socket cleanup |
| `crates/rustwatch-cli/src/tui.rs:112,118` | **Deliberate** — the TUI must not exit because the daemon is down |
| `crates/rustwatch-memory/src/embedder.rs:28` (`let _ = model_name;`) | **Deliberate** — silences the unused parameter when the `fastembed` feature is off |
| `crates/rustwatch-daemon/src/main.rs:63,65,70,81` | **NOT deliberate — silent data loss.** `let _ = store.insert_event(&event);` inside the writer loop; a full or corrupt database drops captured events with no log line and no counter |

Fallbacks that convert failure into a wrong-but-quiet value:
```rust
// crates/rustwatch-core/src/db.rs:252-256  — corrupt timestamp becomes NOW
fn parse_ts(raw: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
```
- `crates/rustwatch-core/src/db.rs:197-199` — `.unwrap_or_default()` on three JSON columns; corrupt JSON becomes an empty `Vec`
- `crates/rustwatch-analyze/src/classifier.rs:112,160` — `.unwrap_or("{}")` on LLM response content; a malformed model reply becomes "no activities" rather than an error
- `crates/rustwatch-capture/src/platform/macos.rs:322` — `.unwrap_or(false)` on `w.is_focused()`
- `crates/rustwatch-daemon/src/main.rs:62,80` — `try_lock()` contention means the event is silently dropped. The same condition is handled explicitly at `:126-137` (replies `"store locked"`), so the two call sites disagree.

**Do this:** when you add a write path, do not add another `let _ =`. Either propagate the error, log it with `tracing`, or make the fallback visible in the return value. `docs/TESTING_PLAN.md:119-121` already flags `parse_ts` and the `Redactor` truncation as bugs that tests should pin.

## Logging

**Two channels, split by audience:**

| Channel | Used by | Pattern |
|---|---|---|
| `tracing` structured | daemon, capture | 4 call sites total |
| `println!` + color | CLI user output | 21 calls, all in `crates/rustwatch-cli/src/commands.rs` |

**`tracing` — message last, structured fields first:**
```rust
// crates/rustwatch-capture/src/platform/macos.rs:72
warn!(?err, "keyboard capture stopped");
// crates/rustwatch-daemon/src/main.rs:87
info!(socket = %paths.socket.display(), "rustwatchd listening");
// crates/rustwatch-capture/src/platform/macos.rs:344
debug!(path = %path.display(), "screenshot saved");
// crates/rustwatch-daemon/src/main.rs:160
error!(?err, "daemon connection failed");
```
Use `?` for `Debug`, `%` for `Display`. Import the level macros at the top: `use tracing::{debug, warn};`, `use tracing::{error, info};`.

**`tracing` appears in only 2 of 30 files.** `rustwatch-core`, `rustwatch-analyze`, `rustwatch-memory`, and `rustwatch-memory-backends` each declare `tracing` as a dependency and never emit a single event — there is no observability into the analysis or memory pipeline. When adding library code, add `tracing` calls for the same class of milestone the daemon logs.

**Subscriber initialization** happens once per binary, always with `EnvFilter::from_default_env()` (reads `RUST_LOG`):
- CLI — registry with an `IndicatifLayer` so progress spinners and logs interleave (`crates/rustwatch-cli/src/main.rs:76-81`)
- Daemon — plain `tracing_subscriber::fmt()` (`crates/rustwatch-daemon/src/main.rs:20-22`)
- MCP — `.with_writer(io::stderr)` because **stdout is the JSON-RPC channel**; writing a log line to stdout would corrupt the protocol (`crates/rustwatch-mcp/src/main.rs:11-14`)

**User-facing output conventions in `crates/rustwatch-cli/src/commands.rs`:**
- Status messages colored via `console::style(...)`: `.green()` for success (`:32,57,73`), `.yellow()` for warnings (`:40,63,103`), `.bold()` for headings (`:110`)
- The `OwoColorize` trait is mixed in for `.yellow()` (`:40,63,103`) alongside `console::style` — pick one library and stay with it when editing that file
- Tabular data via `comfy-table` `Table`/`Cell` with `set_header` (`:81-92`, `:237-251`)
- Long-running work via `indicatif` `ProgressBar::new_spinner()` with a custom template and Unicode tick strings (`:189-194`)
- Status glyphs come from one helper, `fn flag(ok: bool) -> String` (`:120-126`) — extend it rather than inlining a new conditional

**Do this:** library and daemon code logs with `tracing`; CLI code prints with `println!` and color. Never log to stdout in the MCP server.

## Comments

**There are no doc comments in this codebase.** Zero `///` across all 30 `.rs` files. Zero `//!` module docs, with one exception: `crates/rustwatch-memory-backends/src/lib.rs:1-3`.

```rust
//! Optional LanceDB vector store and SurrealDB graph store for rustwatch.
//!
//! Build with: `cargo build -p rustwatch-memory-backends`
```

**Exactly one inline comment exists** in the entire tree:
```rust
// crates/rustwatch-capture/src/platform/macos.rs:247-250
fn read_focused_text_snapshot() -> Option<String> {
    // Best-effort placeholder: full AX integration can be expanded later.
    None
}
```

**Zero `TODO`, `FIXME`, `HACK`, `XXX`, `todo!`, or `unimplemented!` markers.**

Intent that would normally be a comment is instead carried by naming and by `Default`/`Result` shape — e.g. `read_focused_text_snapshot` returning `Option` says "may be unavailable"; `PlatformCapture::permissions()` returning a hardcoded all-false `PermissionsReport` (`crates/rustwatch-capture/src/platform/macos.rs:45-55`) says "not yet implemented" without a marker.

**Do this:** match the existing density — a comment only where the code cannot express the intent, and prefer a self-describing name over a comment. When you add a genuinely public or non-obvious API, add the crate's first `///` doc comment; that is a net improvement, not a deviation.

## Function Design

**Size:** no function exceeds ~105 lines. The longest is `run_loop` in `crates/rustwatch-cli/src/tui.rs:23-127` (~105 lines, and it mixes three concerns). Everything else is under 60. The largest non-TUI function is `run_focus_loop` (`crates/rustwatch-capture/src/platform/macos.rs:173-230`, ~58 lines).

**Layout:** free functions live at the bottom of the module, after the `impl` block they support. Examples: `parse_ts`/`map_event_row` after `impl Store` (`crates/rustwatch-core/src/db.rs:252-278`), `append_text`/`truncate` after `impl SegmentGrouper` (`crates/rustwatch-core/src/segment.rs:106-118`), `hash_embedding` after `impl Embedder` (`crates/rustwatch-memory/src/embedder.rs:46-61`), `slug` after `impl GraphStore` (`crates/rustwatch-memory/src/graph.rs:112-123`), `parse_opt_ts` at the end of `commands.rs` (`:287-292`).

**Receive configuration and paths as arguments; never store them.**
```rust
// crates/rustwatch-analyze/src/classifier.rs:21
pub async fn analyze_pending(store: &Store, config: &Config) -> anyhow::Result<Vec<ActivityRecord>>

// crates/rustwatch-cli/src/commands.rs:77
pub async fn status(paths: &DataPaths, config: &Config) -> anyhow::Result<()>
```
This is why the same functions are callable from the CLI, the daemon, and the MCP server with different paths.

**Receivers:**
- `&self` for everything read-only
- `&self` for all database writes, because `rusqlite::Connection` uses interior mutability — `Store::insert_event(&self, ...)` (`crates/rustwatch-core/src/db.rs:29`), `SqliteMemoryStore::upsert(&self, ...)` (`crates/rustwatch-memory/src/sqlite_store.rs:56`)
- `&mut self` only for genuinely stateful machines: `SegmentGrouper::on_event` / `flush` (`crates/rustwatch-core/src/segment.rs:28,63`)

**Return values:** concrete domain types, not `bool`/`String` sentinels. `Result<Vec<SessionSegment>>`, `Result<Option<SessionSegment>>` for maybe-missing (`get_segment`, `:205`), `Result<(u64, u64, u64, u64)>` for a stat tuple (`stats`, `:227`).

**Manual `Clone` impls appear only where a type holds non-`Clone` state:**
- `PlatformCapture` holds `Arc<AtomicBool>` (`crates/rustwatch-capture/src/platform/macos.rs:28-35`)
- `CaptureHandle` wraps it (`crates/rustwatch-capture/src/lib.rs:14-20`)
- `PlatformCapture` (stub) is a unit struct cloned as `Self` (`crates/rustwatch-capture/src/platform/stub.rs:16-20`)

**`Default` impls:** hand-written where the default is non-trivial and meaningful — `SegmentGrouper` (`:84-88`) and `Config` (`:61-104`, the single source of truth for every runtime default). Derived where the type is a flat bag of flags — `#[derive(Default)]` on `PermissionsReport` (`crates/rustwatch-capture/src/platform/stub.rs:6`).

## Module Design

**Private modules with explicit re-exports at the crate root.** Every `lib.rs` follows this:
```rust
// crates/rustwatch-analyze/src/lib.rs
mod chart;
mod classifier;
mod redact;

pub use chart::{render_chart, ChartFormat};
pub use classifier::{analyze_pending, build_classifier, ActivityClassifier};
pub use redact::Redactor;
```
```rust
// crates/rustwatch-memory/src/lib.rs:1-10
mod embedder;
mod graph;
mod rag;
mod sqlite_store;

pub use embedder::Embedder;
pub use graph::GraphStore;
pub use rag::GraphRag;
pub use rag::SearchHit;
pub use sqlite_store::{MemoryChunk, ScoredChunk, SqliteMemoryStore};
```
Consequences to respect:
- `pub` on an item inside a private module does **not** make it externally visible. `hash_content` at `crates/rustwatch-capture/src/platform/macos.rs:355` is `pub` but unreachable and dead.
- Re-export names, not modules. Consumers write `rustwatch_analyze::render_chart`, never `rustwatch_analyze::chart::render_chart`.
- Only one glob re-export in the workspace: `pub use events::*;` at `crates/rustwatch-core/src/lib.rs:12`.
- Binary crates (`rustwatch-cli`, `rustwatch-daemon`, `rustwatch-mcp`) declare private `mod commands;` / `mod tui;` in `main.rs` with no `lib.rs` and no re-exports.

**Cross-crate imports use the crate name at the top of the import list**, alphabetical within the braces:
```rust
// crates/rustwatch-cli/src/commands.rs:11-16
use rustwatch_core::{
    DaemonClient, DaemonCommand, DaemonReply, DataPaths, Store,
    Config,
};
```

## Concurrency Conventions

- **Blocking OS capture runs on `std::thread::spawn`, not tokio** — `crates/rustwatch-capture/src/platform/macos.rs:70,76`, with `std::thread::sleep` polling at `:183`. The keyboard loop (`run_keyboard_loop`) and focus loop (`run_focus_loop`) are separate OS threads that share only a `UnboundedSender` and an `Arc<AtomicBool>`.
- **`tokio::spawn` is used only in the daemon** for the event writer (`crates/rustwatch-daemon/src/main.rs:58`) and one task per accepted connection (`:101`).
- **Atomics always use `Ordering::Relaxed`** — `crates/rustwatch-daemon/src/main.rs:61,66,106,111,118,120,121`, `crates/rustwatch-capture/src/platform/macos.rs:97,112,184`. Never `SeqCst`. These are pure counters and a pause flag, so relaxed ordering is intentional.
- **Shared DB access is `Arc<Mutex<Store>>` with `try_lock`** — `crates/rustwatch-daemon/src/main.rs:36,62,80,126`. Two independent `tokio::sync::Mutex` guards would be needed for correctness across await points; `try_lock` avoids that but silently drops work on contention.
- **`tokio::sync::mpsc::unbounded_channel` is the only channel**, created once at `crates/rustwatch-daemon/src/main.rs:44` and passed by value into `CaptureHandle::start`.
- **Cloned handles before moving into closures** — `let writer_store = Arc::clone(&store);` etc. at `crates/rustwatch-daemon/src/main.rs:55-57,91-97`; `let paused_kb = Arc::clone(&self.paused);` at `crates/rustwatch-capture/src/platform/macos.rs:66-67`.

**Do not mark a function `async` unless it actually awaits.** `MemoryEngine::ingest_segments` (`crates/rustwatch-memory/src/lib.rs:31-54`), `ingest_activities` (`:56-87`), and `rebuild_from_store` (`:89-102`) are `async` but perform only synchronous `rusqlite` writes — blocking the tokio worker thread with no benefit. This is a real hazard for any future concurrent caller.

## SQL Conventions

- **Migrations via `refinery`, embedded at compile time** — `embed_migrations!("migrations");` at `crates/rustwatch-core/src/db.rs:10`, files in `crates/rustwatch-core/migrations/` named `V{n}__{description}.sql` (`V1__initial.sql`). `Store::open` runs them on every open (`:23-25`).
- **Migration SQL is idempotent**: `CREATE TABLE IF NOT EXISTS` + `CREATE INDEX IF NOT EXISTS`, 4-space indent, one leading comment line describing the file (`-- Initial schema for rustwatch capture pipeline`).
- **Positional parameters only** — `?1`..`?9` with `rusqlite::params![]`. Named `:name` parameters are used nowhere. See `crates/rustwatch-core/src/db.rs:36-44`.
- **Multi-column queries are written as aligned multi-line string literals** with column lists repeated in the `SELECT` and the row-mapping closure (`crates/rustwatch-core/src/db.rs:137-153`, `:158-178`, `:206-222`). The nine-column `segments` projection is copy-pasted three times; a new reader method should follow the same shape for consistency.
- **Timestamps are `TEXT` in RFC 3339** — written with `.to_rfc3339()`, read back through `parse_ts`. Never store epoch integers. Columns are typed `TEXT NOT NULL` with `idx_<table>_<column>` indexes (`crates/rustwatch-core/migrations/V1__initial.sql`).
- **Upserts are `INSERT OR REPLACE`** — `db.rs:50,69`, `sqlite_store.rs:63,79`, `graph.rs:36,51,63`.
- **Graph edge inserts are plain `INSERT`** so repeated ingestion accumulates duplicates rather than replacing (`crates/rustwatch-memory/src/graph.rs:55,67`).
- **Secondary databases do not use refinery** — inline `conn.execute_batch("CREATE TABLE IF NOT EXISTS ...")` in `crates/rustwatch-memory/src/sqlite_store.rs:32-52` (includes an FTS5 virtual table) and `crates/rustwatch-memory/src/graph.rs:15-30`.
- **Vectors are a `BLOB` of little-endian `f32`** — `chunk.embedding.iter().flat_map(|f| f.to_le_bytes()).collect()` at `crates/rustwatch-memory/src/sqlite_store.rs:57-61`, decoded by `bytes_to_f32` (`:124-129`).
- **SQL string literals are built with `format!` only where an identifier must be injected**, and then escaped — `crates/rustwatch-memory-backends/src/lance.rs:55` uses `format!("chunk_id = '{}'", escape(&chunk.chunk_id))` with `fn escape` doubling single quotes (`:132-134`). Row values always go through `?` parameters.

## Cross-Cutting Concerns

**IDs:** `Uuid::new_v4().to_string()` inline at 6 call sites — `crates/rustwatch-core/src/events.rs:61`, `crates/rustwatch-core/src/segment.rs:74`, `crates/rustwatch-memory/src/lib.rs:40,70`, `crates/rustwatch-daemon/src/main.rs:70`. The single wrapper is `Store::new_activity_id()` (`crates/rustwatch-core/src/db.rs:247-249`), needed because `crates/rustwatch-analyze/src/classifier.rs:50` calls it from another crate. Follow that wrapper pattern rather than adding a new free function.

**Privacy redaction:** `Redactor` (`crates/rustwatch-analyze/src/redact.rs`) compiles config-supplied regexes once in `new` and applies them in sequence with `replace_all(&out, "[REDACTED]")` (`:28`), then truncates to `memory.chunk_max_chars`. Redaction happens **before** anything reaches the LLM — `crates/rustwatch-analyze/src/classifier.rs:31-34`.

**Platform gating:** capabilities live behind `PlatformCapture` with `macos.rs`/`stub.rs` selected by `cfg(target_os = "macos")` at `crates/rustwatch-capture/src/platform/mod.rs:1-9`. The stub returns `Error::UnsupportedPlatform` from every fallible method, so non-macOS builds compile and fail loudly rather than at link time.

**Derives — the dominant pattern:** `#[derive(Debug, Clone, Serialize, Deserialize)]` appears 17 times and is the default for any type crossing a storage or wire boundary. Add `PartialEq, Eq` only for small comparable value types — done for exactly three: `AppContext`, `CaptureEventKind`, `ScreenshotScope` (`crates/rustwatch-core/src/events.rs:6,14,43`). Internal store structs skip serde: `#[derive(Debug, Clone)]` on `MemoryChunk`/`ScoredChunk` (`crates/rustwatch-memory/src/sqlite_store.rs:1,14`), `#[derive(Debug, Clone, Serialize)]` on `SearchHit` (`crates/rustwatch-memory/src/rag.rs:34`).

---

*Convention analysis: 2026-10-02*
