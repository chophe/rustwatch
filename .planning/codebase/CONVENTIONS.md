---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
# Coding Conventions

**Analysis Date:** 2026-10-02

Observed across 29 Rust files / ~3,200 LOC in 8 packages. Where the codebase is internally inconsistent, the section says so explicitly and names the majority rule to follow.

## Naming Patterns

**Files:**

- `snake_case.rs`, one concern per file, named after its primary exported type or verb:
  - Type-named: `config.rs` → `Config`, `db.rs` → `Store`, `error.rs` → `Error`, `segment.rs` → `SegmentGrouper`, `redact.rs` → `Redactor`, `embedder.rs` → `Embedder`, `rag.rs` → `GraphRag`, `graph.rs` → `GraphStore`, `ipc.rs` → `DaemonClient`
  - Verb-named: `chart.rs` → `render_chart` / `ChartFormat`, `classifier.rs` → `analyze_pending` / `build_classifier`
- Put a new concern in its own file rather than appending to an existing one. `crates/rustwatch-core/src/` is a flat module directory — no subfolders.
- Platform-split code uses a `platform/` directory with one file per platform plus `mod.rs`: `crates/rustwatch-capture/src/platform/{mod.rs,macos.rs,stub.rs}`. Follow this shape for any new platform variant.

**Crates:**

- Package name is `rustwatch-<domain>`, kebab-case, and always equals the directory name: `crates/rustwatch-core` → `rustwatch-core`.
- Domain is a noun: `core`, `capture`, `analyze`, `memory`, `cli`, `daemon`, `mcp`, `memory-backends`.
- Binary name differs from the package and is declared explicitly with `[[bin]]` + `name`/`path`: `rustwatch-cli` → binary `rustwatch`, `rustwatch-daemon` → binary `rustwatchd`, `rustwatch-mcp` → binary `rustwatch-mcp` (`crates/rustwatch-cli/Cargo.toml:11-13`).
- Do not add a `[[bin]]` name that shadows the package name unless there is only one binary.

**Functions:**

- `snake_case` with a verb first. Public API is uniformly verb-object:
  - Write: `insert_event`, `insert_segment`, `insert_activity`, `insert_screenshot` (`crates/rustwatch-core/src/db.rs:29,48,67,86`)
  - Read: `list_events_since`, `list_segments_between`, `list_unanalyzed_segments`, `list_activities_for_date`, `get_segment` (`crates/rustwatch-core/src/db.rs:101,132,157,182,205`)
  - Derive: `build_classifier`, `build_prompt`, `render_chart`, `expand_around_apps`, `rebuild_from_store`
  - Lifecycle: `start`, `flush`, `open`, `ensure_dirs`, `send`, `handle_connection`
- Private free functions take no `_test_` prefix and no `pub(crate)`: `parse_ts`, `map_event_row`, `append_text`, `slug`, `cosine`, `hash_embedding`, `bytes_to_f32`, `build_prompt`, `key_to_text`, `scope_label`, `escape`, `flag`, `parse_opt_ts`.
- Prefix unused parameters with `_`, never `unused_`: `fn new(_exclude_apps: Vec<String>)`, `fn start(&self, _tx: …, _poll_focus_ms: u64, …)` (`crates/rustwatch-capture/src/platform/stub.rs:23,36-42`). This is how the stub signals "not implemented on this platform".

**Variables:**

- `snake_case` for locals and bindings: `exclude_apps`, `writer_store`, `query_lower`, `date_offset`.
- Leading-underscore locals for deliberate discards are written `let _ = expr;` rather than `let _binding = expr;`. See `crates/rustwatch-cli/src/commands.rs:71-72`.

**Types:**

- `PascalCase`. Acronyms are title-cased, not shouted: `Error::Db` (not `Error::DB`) at `crates/rustwatch-core/src/error.rs:10`.
- Suffix conveys the role — reuse these when naming new types:
  - `*Record` — a persistence row: `ActivityRecord`, `ScreenshotRecord` (`crates/rustwatch-core/src/events.rs:82,95`)
  - `*Config` — a config section: `CaptureConfig`, `AnalyzeConfig`, `PrivacyConfig` (`crates/rustwatch-core/src/config.rs`)
  - `*Command` / `*Reply` / `*State` — IPC: `DaemonCommand`, `DaemonReply`, `DaemonState` (`crates/rustwatch-core/src/ipc.rs`)
  - `*Store` — a persistence handle: `Store`, `SqliteMemoryStore`, `GraphStore`, `LanceMemoryStore`, `SurrealGraphStore`
  - `*Kind` — a tagged event/payload variant set: `CaptureEventKind`
  - `*Scope` — a closed set of modes: `ScreenshotScope`
  - `*Engine`, `*Grouper`, `*Classifier`, `*Redactor`, `*Embedder`, `*Report`, `*Handle` — behavioral roles: `MemoryEngine`, `SegmentGrouper`, `ActivityClassifier`, `Redactor`, `Embedder`, `PermissionsReport`, `CaptureHandle`
- Public data structs have all-`pub` snake_case fields, no accessors: `crates/rustwatch-core/src/events.rs` is the reference.
- Avoid type names starting with `Active` outside module-private scope — `ActiveSegment` (`crates/rustwatch-core/src/segment.rs:15`) is private to its module and that is the correct usage.

**Constants:**

- `SCREAMING_SNAKE_CASE` with underscores: `MAX_BUFFER_CHARS: usize = 16_384` (`crates/rustwatch-core/src/segment.rs:9`), `TABLE: &str`, `DIMS: i32 = 384` (`crates/rustwatch-memory-backends/src/lance.rs:11-12`).
- Numeric separators are used for magnitudes (`16_384`, `1000`).

## Code Style

**Formatting:**

- No `rustfmt.toml` exists — the project uses stock rustfmt (4-space indent, 100-col max width).
- **Formatting has not been applied consistently.** Several lines exceed 100 columns and stock `cargo fmt` would reflow them, e.g. `crates/rustwatch-core/src/db.rs:138`, `crates/rustwatch-core/src/config.rs:5`, `crates/rustwatch-capture/src/platform/macos.rs:47`, `crates/rustwatch-core/src/error.rs:1-21`. Run `cargo fmt` on files you touch; do not assume the tree is already formatted.
- No `rust-toolchain.toml` — the compiler version is unpinned. Do not rely on version-specific syntax beyond edition 2021.

**Linting:**

- No `clippy.toml`, no `[lints]` workspace table, and **no crate-level `#![deny(...)]` or `#![warn(...)]` attributes anywhere** (verified: zero `#![` lines in the tree).
- There is no `#[allow]` or `#[expect]` attribute anywhere. Do not add suppressions to silence a lint — fix or restructure instead.
- No CI and no lint gate exist. `cargo clippy --workspace --all-targets -- -D warnings` is not currently enforced; treat clippy cleanliness as a goal for files you touch.

**Crate attributes to match:**

- Serde enums that cross a wire or storage boundary use an explicit tag and `snake_case` renames:
  - `#[serde(tag = "type", rename_all = "snake_case")]` — `CaptureEventKind` (`crates/rustwatch-core/src/events.rs:15`)
  - `#[serde(tag = "cmd", rename_all = "snake_case")]` / `#[serde(tag = "reply", …)]` — `DaemonCommand` / `DaemonReply` (`crates/rustwatch-core/src/ipc.rs:20,31`)
  - Plain enums use only `#[serde(rename_all = "snake_case")]` — `ScreenshotScope` (`crates/rustwatch-core/src/events.rs:44`)
- `Copy` is derived only on small `enum`s: `ScreenshotScope` (`events.rs:43`), `ChartFormat` (`crates/rustwatch-analyze/src/chart.rs:4`).
- `PartialEq, Eq` is derived only on the value types used in comparisons or tests: `AppContext`, `CaptureEventKind`, `ScreenshotScope` (`events.rs:6,14,43`). Record types carrying `f32` confidence (`ActivityRecord`) deliberately omit `PartialEq`.
- Config structs derive `#[derive(Debug, Clone, Serialize, Deserialize)]` and provide defaults via a hand-written `impl Default for Config` (`crates/rustwatch-core/src/config.rs:5,61`). **No `#[serde(default)]` is used anywhere** — a partial `config.toml` therefore fails to deserialize. Do not add `#[serde(default)]` to new fields without also deciding whether the existing strict behavior should change.

## Import Organization

**Order:**
Three blank-line-separated groups, in this order:

1. `std` and external crates, alphabetically
2. workspace crates (`rustwatch_core`, …) — these are grouped *with* external crates, separated only by a blank line from `std`
3. `crate::` — always last

Reference: `crates/rustwatch-capture/src/platform/macos.rs:1-13`

```rust
use std::path::PathBuf;              // group 1: std
use std::sync::{ … };

use rustwatch_core::{ …, Result };  // group 2: workspace crate
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use crate::redact::Redactor;         // group 3: crate-local
```

Second reference: `crates/rustwatch-core/src/db.rs:1-8`

```rust
use chrono::{DateTime, Utc};
use refinery::embed_migrations;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::{ActivityRecord, CaptureEvent, Result, ScreenshotRecord, SessionSegment};
```

**Specifics:**

- Import the concrete names, never a glob, for both crates and `crate::` — `use crate::{ActivityRecord, CaptureEvent, Result, ScreenshotRecord, SessionSegment};`. The only glob in the tree is the facade re-export `pub use events::*;` at `crates/rustwatch-core/src/lib.rs:12`, which is deliberate (that module holds only public wire types).
- Prefer importing a path over writing it inline. Inline full paths do occur for one-off items (`rustwatch_core::ScreenshotRecord` at `crates/rustwatch-daemon/src/main.rs:69`; `rustwatch_core::ScreenshotScope::Window` at `main.rs:141-143`), so this is a soft rule — add the import when the path is used more than once.
- Known deviations: `crates/rustwatch-cli/src/commands.rs:11-15` has two separate `rustwatch_core::{…}` blocks, and `crates/rustwatch-cli/src/tui.rs:10` lists `DataPaths, Store, Config` out of alphabetical order. Do not copy these; keep a single sorted block.

**Path Aliases:**

- None. No `use` renaming and no `paths`/`[aliases]` table. Inside `rustwatch-core`, self-references use `crate::` (`crate::Error::from` at `crates/rustwatch-core/src/paths.rs:39`); outside it, `rustwatch_core::`.

## Error Handling

**Strategy: two tiers, split by crate.**

1. **`rustwatch-core` owns a typed error** — `crates/rustwatch-core/src/error.rs`:
   ```rust
   pub type Result<T> = std::result::Result<T, Error>;

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
   Rules for extending it:
   - Add `#[from]` for any foreign error that should convert automatically; add a plain `String` payload for context-carrying errors (`Config`, `Other`).
   - Write `#[error("…")]` messages in lowercase with **no trailing period** (`"daemon not running"`).
   - `Error::Other` is the escape hatch for third-party errors that have no natural variant (`migration failed: {e}` at `crates/rustwatch-core/src/db.rs:23-25`).
2. **Every other crate uses `anyhow::Result`** — 60+ occurrences across `rustwatch-analyze`, `rustwatch-memory`, `rustwatch-capture` internals, `rustwatch-cli`, `rustwatch-daemon`, `rustwatch-mcp`, `rustwatch-memory-backends`. There is no per-crate `thiserror` enum outside `rustwatch-core`; `thiserror` is declared as a dependency in `rustwatch-analyze/Cargo.toml` and `rustwatch-capture/Cargo.toml` but never used there. Do not introduce a new `thiserror` enum without a concrete reason.
- Cross the `rustwatch-core` boundary as needed: inside core, `?` converts `std::io::Error` and `rusqlite::Error` into `crate::Error` via `#[from]`; from other crates, `rustwatch_core::Result` is the signature (`crates/rustwatch-capture/src/platform/macos.rs:10,57,92`).

**Patterns:**

- `?` is the default. Apply it whenever `Error` (core) or `anyhow::Error` handles the conversion.
- `map_err(crate::Error::from)` when the `?` chain needs an explicit target type — `crates/rustwatch-core/src/paths.rs:39-42`.
- `map_err(Into::into)` at the end of a rusqlite row-collection, converting `rusqlite::Error` into `crate::Error` without naming it — `crates/rustwatch-core/src/db.rs:154,179,202`.
- `anyhow::Context` adds operator context to an error message — used in exactly four places: `crates/rustwatch-core/src/paths.rs:83` (`read {path}`), `crates/rustwatch-cli/src/commands.rs:27,48` (`locate rustwatchd binary`), `commands.rs:54` (`spawn rustwatchd`), `commands.rs:67` (`parse pid`). Use it for I/O and process operations where the raw error text is uninformative.
- `anyhow::bail!` for unrecoverable branches: unsupported provider (`crates/rustwatch-analyze/src/classifier.rs:17`), unexpected IPC reply (`crates/rustwatch-cli/src/commands.rs:141-142`).
- Deliberate discard is `let _ = …;` and is used for cleanup and fire-and-forget sends: removing stale pid/socket files (`crates/rustwatch-cli/src/commands.rs:71-72`), sending events onto the capture channel (`crates/rustwatch-capture/src/platform/macos.rs:134,143,153,203,215,224`), and **inserting rows in the daemon writer loop** (`crates/rustwatch-daemon/src/main.rs:63,65,81`). The last of these silently drops persisted events on failure — when adding new writes to that loop, log with `tracing::error!` rather than adding another bare `let _ =`.

**Panic discipline:**

- The entire codebase contains **4 `unwrap()` calls, 0 `expect()`, 0 `panic!`, 0 `todo!`, 0 `unimplemented!()`**:
  - `crates/rustwatch-core/src/db.rs:183-184` — `and_hms_opt(0,0,0).unwrap()` / `and_hms_opt(23,59,59).unwrap()` (constant-valid, safe)
  - `crates/rustwatch-cli/src/commands.rs:192` — `ProgressStyle::with_template(…).unwrap()` (literal template, safe)
  - `crates/rustwatch-memory-backends/src/lance.rs:23` — `path.to_str().unwrap()` (the one genuinely panicking `unwrap`)
- Rule: keep `unwrap()` for provably-total expressions (literals, fixed `and_hms_opt` arguments). Everything else returns `Result`.

**Anti-pattern to avoid — silent data-corruption fallbacks:**
`parse_ts` (`crates/rustwatch-core/src/db.rs:252-256`) returns `Utc::now()` when an RFC 3339 string fails to parse, silently rewriting history on read. `Redactor::scrub` (`crates/rustwatch-analyze/src/redact.rs:30-31`) calls `String::truncate`, which panics on a non-char-boundary index. Do not add more `unwrap_or_else(|_| …now())` style fallbacks in parsing or redaction paths.

## Logging

**Framework:** `tracing` 0.1, initialized per binary with `tracing-subscriber`.

- Use structured event macros (`info!`, `warn!`, `error!`, `debug!`) for anything diagnostic, and never `println!` for diagnostics. Structured-field syntax is the norm:
  ```rust
  info!(socket = %paths.socket.display(), "rustwatchd listening");   // daemon/src/main.rs:87
  error!(?err, "daemon connection failed");                            // daemon/src/main.rs:160
  warn!(?err, "keyboard capture stopped");                             // capture/platform/macos.rs:72
  debug!(path = %path.display(), "screenshot saved");                 // capture/platform/macos.rs:344
  ```
- Subscriber setup differs by binary, and that difference is deliberate:
  - CLI — `tracing_subscriber::registry()` + `EnvFilter::from_default_env()` + `IndicatifLayer` + `fmt::layer()`, so progress bars and logs interleave (`crates/rustwatch-cli/src/main.rs:76-81`)
  - Daemon — plain `fmt()` + `EnvFilter` (`crates/rustwatch-daemon/src/main.rs:20-22`)
  - MCP — plain `fmt()` + `EnvFilter`, **forced to stderr** via `.with_writer(io::stderr)` because stdout is the JSON-RPC channel (`crates/rustwatch-mcp/src/main.rs:11-14`). Never log to stdout in that binary.
- Coverage is uneven: only `rustwatch-daemon` and `rustwatch-capture/platform/macos.rs` use `tracing` macros. `rustwatch-core`, `rustwatch-memory`, `rustwatch-analyze`, and `rustwatch-cli` emit nothing. When adding logic to those crates, `tracing::debug!` is the appropriate new instrument.

**User-facing output:** `println!` with `owo-colors` (`.green()`, `.yellow()`, `.red()`, `.bold()`) or `console::style(…)` — never traced. See `crates/rustwatch-cli/src/commands.rs:32,40,63,73,103,110`. `cli` deliberately mixes both styles (`style("Installed launch agent").green()` at `commands.rs:32` vs the bare `"Daemon already appears to be running".yellow()` at `commands.rs:40`); standardize on the bare-string `.color()` form for short statuses and `style(…).color()` for multi-part messages.

## Comments

**Current state: the codebase is essentially uncommented.**

- **4 comment lines total.** `//!` crate docs in `crates/rustwatch-memory-backends/src/lib.rs:1-3`; one inline `//` at `crates/rustwatch-capture/src/platform/macos.rs:248` explaining a deliberate placeholder (`// Best-effort placeholder: full AX integration can be expanded later.`).
- **Zero `///` rustdoc comments** on any item, public or private, in any crate.

**When to comment:**

- Do not narrate what the code does — the code says it.
- Comment *why* a non-obvious decision was made, matching the `macos.rs:248` precedent: a shortcut, a placeholder, or a deliberate limitation.
- Comment SQL strings that carry non-obvious semantics. `crates/rustwatch-core/src/db.rs:161` uses `LIKE '%' || s.id || '%'` to join segments to activities; that substring match is a known-fragile construction and warrants a comment.
- Comment the `-0.85` score decay in graph expansion (`crates/rustwatch-memory/src/graph.rs:98`, `crates/rustwatch-memory-backends/src/surreal.rs:96`) and the `+0.2` keyword boost (`crates/rustwatch-memory/src/rag.rs:12`) — these are magic numbers whose rationale is not visible at the call site.

**JSDoc/TSDoc (rustdoc):**

- Do not add `///` to self-evident items — that would break with the existing style.
- Do add `//!` module docs to `lib.rs` when introducing a new crate. `crates/rustwatch-memory-backends/src/lib.rs:1-3` is the template: one-line purpose, blank `//!`, then how to build it.
- Public items with non-obvious contracts (`Store::list_unanalyzed_segments`'s substring-join semantics, `GraphStore::expand_around_apps`'s use of `hops` as a row `LIMIT` rather than a BFS depth) are the exceptions worth documenting with `///`.

## Function Design

**Size:** Most functions are 5–40 lines. The single outlier is `key_to_text` (`crates/rustwatch-capture/src/platform/macos.rs:252-296`) — 45 lines of pure match arms, which is data rather than logic. Keep new functions under ~40 lines; extract a private free function (as `db.rs` does with `parse_ts`/`map_event_row`) when a body grows past that.

**Parameters:**

- Pass by reference for reads: `&self`, `&Path`, `&str`, `&Config`, `&SessionSegment`. Reference examples: `Store::insert_event(&self, event: &CaptureEvent)`, `DataPaths::new(root: Option<PathBuf>)`, `Redactor::new(config: &Config)`.
- Consuming constructors take `Vec<String>` by value: `PlatformCapture::new(exclude_apps: Vec<String>)` (`crates/rustwatch-capture/src/platform/macos.rs:38`), `CaptureHandle::new(exclude_apps: Vec<String>)` (`crates/rustwatch-capture/src/lib.rs:23`).
- Use `impl AsRef<Path>` for new path-accepting constructors — `DaemonClient::new(socket_path: impl AsRef<Path>)` (`crates/rustwatch-core/src/ipc.rs:45`) is the pattern; existing `&std::path::Path` signatures in `Store::open` and `SqliteMemoryStore::open` are the older style.
- Generic params over `Option` for "no value supplied": `DataPaths::new(root: Option<PathBuf>)`, `Store::list_events_since(since: Option<DateTime<Utc>>, limit: usize)`.
- Use `impl Fn` for injected single-operation dependencies: `handle_connection(mut stream: UnixStream, handler: impl Fn(DaemonCommand) -> DaemonReply)` (`crates/rustwatch-core/src/ipc.rs:70-73`). This is the codebase's one dependency-injection seam — reuse it rather than introducing a trait for single-method behavior.
- Bounds: `ActivityClassifier: Send + Sync` (`crates/rustwatch-analyze/src/classifier.rs:9`) because it is held as `Box<dyn ActivityClassifier>` across await points. Add `Send + Sync` to any trait boxed into a tokio task.

**Return values:**

- `anyhow::Result<T>` / `crate::Result<T>` for anything fallible; `Result<()>` for effect-only.
- `Result<Vec<T>>` or `Result<Option<T>>` for lookups. `Option` signals "not found" without an error: `SegmentGrouper::on_event(&mut self, …) -> Option<SessionSegment>` (`crates/rustwatch-core/src/segment.rs:28`), `Store::get_segment(&self, id: &str) -> Result<Option<SessionSegment>>` (`db.rs:205`).
- **`usize` is the return type for "how many things did I process"** — `ingest_segments`, `ingest_activities`, and `rebuild_from_store` all return `anyhow::Result<usize>` (`crates/rustwatch-memory/src/lib.rs:31,56,89`), and the CLI prints that count directly (`crates/rustwatch-cli/src/commands.rs:275`). Match this.
- Tuples for small fixed results are accepted: `Store::stats(&self) -> Result<(u64, u64, u64, u64)>` (`db.rs:227`), destructured at the call site (`crates/rustwatch-cli/src/commands.rs:79`).
- Avoid returning `bool` where a count would be more useful.

**Receiver mutability:**

- `&self` for services backed by interior mutability — `Store::insert_event(&self, …)`, `SqliteMemoryStore::upsert(&self, …)`, `MemoryEngine::ingest_segments(&self, …)`. This is why `Store` is wrapped in `Arc<Mutex<…>>` in the daemon (`crates/rustwatch-daemon/src/main.rs:36`).
- `&mut self` only where the struct owns plain mutable state: `SegmentGrouper::{on_event, flush}` (`crates/rustwatch-core/src/segment.rs:28,63`).

**`new()` / `Default`:**

- `new()` is the constructor everywhere it exists: `SegmentGrouper::new`, `DataPaths::new`, `Store::open`, `MemoryEngine::open` (async), `SqliteMemoryStore::open`, `GraphStore::open`, `Embedder::new`, `Redactor::new`, `DaemonClient::new`, `CaptureEvent::new`.
- Add `impl Default` alongside `new()` when the type has an obvious empty state and the type is likely to be constructed in tests or by callers — `SegmentGrouper` does this (`crates/rustwatch-core/src/segment.rs:84-88`). `PermissionsReport` instead derives `Default` (`crates/rustwatch-capture/src/platform/stub.rs:6`).
- `Clone` is derived for plain data (`ActivityRecord`, `MemoryChunk`, `DaemonState`) and hand-implemented when the type wraps an `Arc` shared flag — `PlatformCapture` (`crates/rustwatch-capture/src/platform/macos.rs:28-35`) and `CaptureHandle` (`crates/rustwatch-capture/src/lib.rs:14-20`) both `Arc::clone` the `paused` atomic.

## Module Design

**Exports:**

- Library crates expose a **thin facade in `lib.rs`**: declare modules private, then re-export the public surface.
  ```rust
  // crates/rustwatch-analyze/src/lib.rs
  mod chart;
  mod classifier;
  mod redact;

  pub use chart::{render_chart, ChartFormat};
  pub use classifier::{analyze_pending, build_classifier, ActivityClassifier};
  pub use redact::Redactor;
  ```
  Same shape in `crates/rustwatch-memory/src/lib.rs` and `crates/rustwatch-memory-backends/src/lib.rs`. New library crates follow this: private `mod` + explicit `pub use`, one re-export line per module.
- `rustwatch-core` is the exception and uses **`pub mod` for every module** *plus* re-exports (`crates/rustwatch-core/src/lib.rs:1-15`). Both access paths work (`rustwatch_core::Store` and `rustwatch_core::db::Store`). When adding a module to `rustwatch-core`, declare it `pub` and add it to the re-export block.
- Consumers import from the crate root, not from submodules, with two exceptions: `rustwatch_core::paths::load_or_create_config` and `rustwatch_core::ipc::handle_connection` (`crates/rustwatch-daemon/src/main.rs:10,159`), and `rustwatch_core::events::{…}` inside capture (`crates/rustwatch-capture/src/platform/macos.rs:8`). Prefer root imports.
- Binary crates (`rustwatch-cli`, `rustwatch-daemon`, `rustwatch-mcp`) have **no lib target** — only `[[bin]] path = "src/main.rs"`. Nothing in them is importable from another crate or from an integration test. See TESTING.md before adding logic there.

**Barrel Files:**

- There are **no `mod.rs` barrels**. Only `crates/rustwatch-core/src/lib.rs` re-exports, plus `crates/rustwatch-capture/src/platform/mod.rs`, which is a conditional re-export shim, not a barrel:
  ```rust
  #[cfg(target_os = "macos")]
  mod macos;
  #[cfg(not(target_os = "macos"))]
  mod stub;

  #[cfg(target_os = "macos")]
  pub use macos::{PermissionsReport, PlatformCapture};
  #[cfg(not(target_os = "macos"))]
  pub use stub::{PermissionsReport, PlatformCapture};
  ```
  Both platforms must expose the *same* names with the *same* signatures — that parity is what makes the shim work and is why `stub.rs` accepts and ignores every parameter.

---

*Convention analysis: 2026-10-02*
