---
last_mapped_commit: c8ba2a9e65b063c9dbc60fc393d554bb99e9ca7a
last_mapped_at: 2026-10-02
---
# Testing Patterns

**Analysis Date:** 2026-10-02

## Current State: There Is No Test Suite

This codebase has **zero tests**. Nothing in this document is a pattern to copy — it is a record of what does not exist plus an inventory of what the code already exposes to make testing possible when it starts.

**Verified absences (all checked against `crates/`, 2026-10-02):**

| Thing | Status | How verified |
|---|---|---|
| `#[cfg(test)]` modules | **0** | `rg 'cfg\(test\)' --type rust` → no matches |
| `#[test]` / `#[tokio::test]` | **0** | `rg '#\[test\]\|#\[tokio::test\]' --type rust` → no matches |
| `mod tests` | **0** | `rg 'mod tests' --type rust` → no matches |
| `tests/`, `benches/`, `examples/` dirs | **0** | `fd -t d -g 'tests' -g 'benches' -g 'examples'` → none |
| `[dev-dependencies]` sections | **0** | `rg 'dev-dependencies' -g '*.toml'` → no matches in any of the 8 manifests |
| Test crates in `Cargo.lock` | **none** | 648 locked packages; `proptest`, `wiremock`, `assert_cmd`, `predicates`, `insta`, `rstest`, `mockall`, `criterion`, `quickcheck`, `serial_test` all absent. `tempfile` appears **only as a transitive dependency**, not as a dev-dep |
| CI workflow / quality gate | **none** | No `.github/`, no `.config/nextest.toml`, no `deny.toml` |

`cargo test --workspace` today compiles the workspace and runs **0 tests**. `docs/TESTING_PLAN.md` is a written plan with **no code behind it** — treat it as intent, not as an existing convention. (See "Planned, Not Implemented" at the end.)

## Test Framework

**Runner:**

- **None installed.** The built-in `libtest` harness would work with zero setup (`cargo test` discovers `#[test]` fns), but no test target exists.
- No `cargo-nextest` config. No `#[bench]` targets. No criterion setup.

**Assertion Library:**

- **None.** `assert!`, `assert_eq!`, and `pretty_assertions` are all absent from the tree. Stock `assert_eq!` is the zero-dependency default and matches the project's current "no dev-deps" posture.

**Assertion style when tests are added:**
Use stock `assert_eq!` / `assert!` from std. Introducing `pretty_assertions` would mean the first `[dev-dependencies]` block in the workspace.

**Run Commands:**

```bash
cargo test --workspace              # runs 0 tests today
cargo test --workspace -- --nocapture
cargo nextest run --workspace       # NOT installed; no .config/nextest.toml exists
```

## Test File Organization

**Location:**

- **No convention exists.** With zero tests, there is no established place to put them.
- The Rust-ecosystem default, and what the code structure supports, is co-located `#[cfg(test)] mod tests` at the bottom of each source file — appropriate here because most testable units are private free functions (see "Testability Inventory" below).
- Integration tests would go in `crates/<name>/tests/<subject>.rs`. None of these directories exist.

**Naming:**

- Nothing to match. If following the default: `tests/db_integration.rs`, `tests/ipc_integration.rs`, snake_case subject prefix.

**Structure:**

```
crates/
├── rustwatch-core/
│   ├── migrations/V1__initial.sql     # embedded via embed_migrations! at db.rs:10
│   ├── src/
│   │   ├── db.rs                      # Store — largest testable surface
│   │   ├── segment.rs                 # SegmentGrouper — pure state machine
│   │   ├── events.rs                  # serde-tagged wire types
│   │   ├── error.rs                   # 7-variant thiserror enum
│   │   ├── config.rs / paths.rs / ipc.rs
│   │   └── lib.rs
│   └── (no tests/ dir)
├── rustwatch-analyze/src/{chart,classifier,redact}.rs
├── rustwatch-memory/src/{embedder,graph,rag,sqlite_store}.rs
├── rustwatch-capture/src/{lib,platform/{mod,macos,stub}}.rs
├── rustwatch-memory-backends/src/{lance,surreal}.rs   # NOT a workspace member — unbuildable
└── rustwatch-{cli,daemon,mcp}/src/main.rs            # bin-only crates
```

## Test Structure

**Suite Organization:**
No examples exist. The structure that fits this codebase:

```rust
// Appended to the bottom of the file under test (e.g. crates/rustwatch-memory/src/rag.rs).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_applies_keyword_boost() {
        let hits = vec![ScoredChunk {
            chunk_id: "c1".into(),
            text: "App: Terminal\nTitle: rustwatch".into(),
            app_name: "Terminal".into(),
            score: 0.5,
        }];
        let out = GraphRag::merge("rustwatch", hits, Vec::new());
        assert_eq!(out.len(), 1);
        // query is a substring of the text → +0.2 boost per rag.rs:11-15
        assert!((out[0].score - 0.7).abs() < 1e-6, "got {}", out[0].score);
    }
}
```

**Patterns to follow:**

- `use super::*;` inside the module — matches the codebase's reliance on private free helpers (`cosine`, `bytes_to_f32`, `slug`, `key_to_text`) as the natural unit-test targets.
- Name tests `unit_under_test_behavior` — snake_case, descriptive of the assertion, no `test_` prefix (the codebase uses no prefixes anywhere).
- Prefer `assert_eq!` on observable outputs; use `assert!` with a format message for float comparisons, since `f32` scores appear throughout (`crates/rustwatch-memory/src/sqlite_store.rs:131`, `crates/rustwatch-memory/src/rag.rs:6`, `crates/rustwatch-memory/src/graph.rs:98`).
- No `setUp`/`tearDown` concept in Rust. Use `Result<(), E>` return from `#[test]` fns when the body is fallible — `fn t() -> Result<()>` and let `?` work.

**Setup / teardown:**

- No fixture or temp-dir machinery exists. The project has **no `tempfile` dev-dep**, so any test touching `Store::open`, `SqliteMemoryStore::open`, or `GraphStore::open` — all of which write to disk (`crates/rustwatch-core/src/db.rs:17-27`, `crates/rustwatch-memory/src/sqlite_store.rs:27-54`, `crates/rustwatch-memory/src/graph.rs:10-32`) — currently has no way to get an isolated database without either adding `tempfile` or hand-rolling `std::env::temp_dir().join(unique)`.
- **This is the single biggest blocker for starting the test suite.** Adding `tempfile` to `[workspace.dependencies]` plus a per-crate `[dev-dependencies]` block is the prerequisite for every DB-level test.

## Mocking

**Framework:** None. No `mockall`, `rstest`, or hand-rolled mock modules exist anywhere in the tree.

**What could be mocked (and the seams that exist):**

| Seam | Location | Notes |
|---|---|---|
| `impl Fn(DaemonCommand) -> DaemonReply` | `crates/rustwatch-core/src/ipc.rs:70-73` | `handle_connection` accepts an injected handler — the codebase's existing DI seam. A sync closure test drives it directly with a `tokio::net::UnixStream`. |
| `Box<dyn ActivityClassifier>` | `crates/rustwatch-analyze/src/classifier.rs:13-19`, `classifier.rs:8-11` | `build_classifier` returns a boxed trait object, so a mock classifier is the intended substitute. **Blocker:** `analyze_pending(store, config)` (`classifier.rs:21`) constructs the classifier internally from config — it must first gain a parameter accepting `&dyn ActivityClassifier` for injection to work. |
| `PlatformCapture` behind `cfg` | `crates/rustwatch-capture/src/platform/mod.rs` | `stub.rs` is already a non-functional stand-in returning `Error::UnsupportedPlatform` (`crates/rustwatch-capture/src/platform/stub.rs:23-27,44-47,54-57`). This is a natural test double — build/test on Linux to get it. |
| HTTP classifier responses | `crates/rustwatch-analyze/src/classifier.rs:99-108` (OpenAI), `:148-158` (Anthropic) | Hardcoded endpoint URLs with `reqwest::Client::new()` built inline in each `::new` (`classifier.rs:79,129`). No base-URL or client injection → requires a real network stub (`wiremock`) plus refactoring the endpoint to be configurable before HTTP contract tests are possible. |

**What NOT to mock:**

- Pure functions — `cosine` (`sqlite_store.rs:131`), `bytes_to_f32` (`sqlite_store.rs:124`), `hash_embedding` (`embedder.rs:46`), `slug` (`graph.rs:112` and `surreal.rs:113`), `build_prompt` (`classifier.rs:166`), `append_text`/`truncate` (`segment.rs:106,116`), `expand_tilde` (`paths.rs:58`), `key_to_text`/`scope_label`/`active_modifiers` (`macos.rs:252,348,165`). Test these directly with real inputs; mocking them tests nothing.
- SQLite. `rusqlite` is bundled and in-process; use a real temp-file database rather than a mocked `Store`.

## Fixtures and Factories

**Test Data:** None exists. There are no `fixtures/`, `testdata/`, builder types, or helper modules in any crate.

**Location:** Would go in `crates/<name>/tests/common/mod.rs` for shared integration fixtures, or inline in the `#[cfg(test)] mod tests` block for unit tests. Neither pattern is established.

**Construction cost to be aware of:**
Building a `CaptureEvent` is cheap — `CaptureEvent::new(kind, app)` generates the UUID and timestamp (`crates/rustwatch-core/src/events.rs:59-66`). But that makes `timestamp` non-deterministic, which matters for `SegmentGrouper` (segment boundaries are derived from `event.timestamp` at `crates/rustwatch-core/src/segment.rs:30,72`). Tests over grouping logic will want to construct the struct literal directly with a fixed `DateTime<Utc>` rather than call `new()`.

Note that `CaptureEvent` has no `Default`, and `AppContext` requires four fields (`app_name`, `window_title`, `process_id`, `bundle_id` — `events.rs:6-12`), so a small test-local helper is worth writing rather than repeating the literal.

## Coverage

**Requirements:** None enforced. No `cargo-llvm-cov` config, no `tarpaulin`, no threshold, no badge.

**View Coverage:**

```bash

# Not installed — would require `cargo install cargo-llvm-cov` first.

cargo llvm-cov --workspace --html
```

There is no coverage measurement of the current code, and no CI to publish one.

## Test Types

All four categories are absent.

**Unit Tests:** 0. The highest-value untested targets are the pure functions listed in "Mocking" above plus `SegmentGrouper` (`crates/rustwatch-core/src/segment.rs:23-82`), `Redactor` (`crates/rustwatch-analyze/src/redact.rs`), `GraphRag::merge` (`crates/rustwatch-memory/src/rag.rs`), and `Store::new_activity_id` (`db.rs:247`).

**Integration Tests:** 0. Uncovered seams that need real resources: SQLite migrations + CRUD via `Store::open` (`crates/rustwatch-core/src/db.rs:10,17`); the `refinery` embedded migration runner; real Unix socket round-trip through `DaemonClient::send` / `handle_connection` (`crates/rustwatch-core/src/ipc.rs:51,70`); FTS5 table creation in `SqliteMemoryStore::open` (`crates/rustwatch-memory/src/sqlite_store.rs:45`).

**E2E Tests:** None. No `assert_cmd` harness exists for the three binaries.

**Property-based tests:** None. No `proptest` or `quickcheck`.

## Blocker: Bin-Only Crates

`rustwatch-cli`, `rustwatch-daemon`, and `rustwatch-mcp` have **no lib target** — only `[[bin]] name/path = "src/main.rs"` (`crates/rustwatch-cli/Cargo.toml:11-13`, `crates/rustwatch-daemon/Cargo.toml:11-13`, `crates/rustwatch-mcp/Cargo.toml:11-13`). Everything lives in `main.rs`, including non-trivial private helpers:

- `parse_opt_ts` — `crates/rustwatch-cli/src/commands.rs:287-292`
- `flag` — `crates/rustwatch-cli/src/commands.rs:120-126`
- `handle_tool` — `crates/rustwatch-mcp/src/main.rs:76-112`
- `tool` — `crates/rustwatch-mcp/src/main.rs:68-74`
- `daemon_text` — `crates/rustwatch-mcp/src/main.rs:114-122`
- `run_daemon` — `crates/rustwatch-daemon/src/main.rs:30`
- `run_loop` (TUI) — `crates/rustwatch-cli/src/tui.rs:23`

A `#[cfg(test)] mod tests` **can** live inside a `main.rs` and will run, so unit tests there are possible today. Integration tests (`tests/`) are **not** — they cannot import from a bin-only crate. Adding a `src/lib.rs` to these crates is a prerequisite for any integration or E2E test.

## Blocker: `rustwatch-memory-backends` Is Unbuildable

`crates/rustwatch-memory-backends` is **not** listed in `[workspace] members` in the root `Cargo.toml:3-11`, and it is not in `workspace.exclude` either. Building it fails outright:

```
$ cd crates/rustwatch-memory-backends && cargo metadata --no-deps
error: current package believes it's in a workspace when it's not
```

Its deps (`lancedb`, `surrealdb`, `arrow-array`, `arrow-schema`, `futures`) are absent from `Cargo.lock` and have never been resolved or built. `cargo test` cannot reach `crates/rustwatch-memory-backends/src/lance.rs` or `surreal.rs` until the crate is added to `workspace.members` or given an empty `[workspace]` table. The `--manifest-path` build command in `README.md` does not work as written.

## Known Risky Code With No Test Protection

These are the spots where a bug is currently invisible. Each is untested *and* unlogged.

| Risk | Location | Why it matters |
|---|---|---|
| `parse_ts` silently falls back to `Utc::now()` | `crates/rustwatch-core/src/db.rs:252-256` | A malformed stored timestamp silently rewrites history to "now" on every read. No error, no log. |
| `String::truncate` on a non-char-boundary index | `crates/rustwatch-analyze/src/redact.rs:30-31` | Panics if `chunk_max_chars` lands mid-UTF-8-sequence. Only reachable when `len() > max_chars`. |
| `LIKE '%' \|\| s.id \|\| '%'` substring join | `crates/rustwatch-core/src/db.rs:161` | Segment id `abc` matches activity `xabcx`. Cross-matches silently, producing wrong "unanalyzed" results. |
| `expand_around_apps` uses `hops` as a SQL row `LIMIT` | `crates/rustwatch-memory/src/graph.rs:80-88` | Not a BFS depth. Named as if it were. |
| Silent event loss in the daemon writer loop | `crates/rustwatch-daemon/src/main.rs:63,65,81` | `let _ = store.insert_event(&event);` — a DB error drops captured user activity with no log line. |
| `try_lock` failure silently skips the write | `crates/rustwatch-daemon/src/main.rs:62,80,126` | Lock contention means dropped events; only the `Tail` path reports `"store locked"`. |
| `Redactor::scrub` never panics guarantee | `crates/rustwatch-analyze/src/redact.rs:25-34` | Not verifiable without property-based testing over arbitrary UTF-8. |
| `sqlite_store::search` is a full table scan | `crates/rustwatch-memory/src/sqlite_store.rs:86-115` | Loads every row and computes cosine in Rust. Correctness is easy to reason about; performance is untested at any scale. |

## Testability Inventory

Existing structure that makes a test suite tractable, with no refactor needed:

- **Pure private functions** (14 identified above) — directly testable from a co-located `#[cfg(test)] mod tests` via `use super::*`.
- **`SegmentGrouper`** (`crates/rustwatch-core/src/segment.rs`) — a self-contained state machine with no I/O. Only dependency is `Utc::now()` in `flush()` (`segment.rs:64`), which is avoidable by testing `on_event` and the private `on_focus`.
- **`GraphRag::merge`** (`crates/rustwatch-memory/src/rag.rs`) — pure function over two `Vec<ScoredChunk>`; a struct with no state (`pub struct GraphRag;`).
- **`Redactor`** (`crates/rustwatch-analyze/src/redact.rs`) — constructed from a `&Config`, no I/O. `Config::default()` at `crates/rustwatch-core/src/config.rs:61` supplies patterns, exclusions, and limits without touching the filesystem.
- **`Store`** (`crates/rustwatch-core/src/db.rs`) — takes an explicit `&Path` in `open()`, so a temp-dir path is all that is needed to test it.
- **Serde round-trips** — every wire type derives `Serialize + Deserialize` with explicit tags (`events.rs:15`, `ipc.rs:20,31`), so round-trip tests need no setup.
- **Conditional platform double** — `crates/rustwatch-capture/src/platform/stub.rs` provides a real, deterministic stand-in for `PlatformCapture`.

Requires refactoring before it is testable:

- `analyze_pending` constructs its own classifier from `Config` (`crates/rustwatch-analyze/src/classifier.rs:41`) instead of accepting one.
- `OpenAiClassifier` / `AnthropicClassifier` hardcode their endpoint URLs and construct `reqwest::Client::new()` internally (`classifier.rs:79,101,129,151`) — no base-URL or client injection.
- The TUI event loop (`crates/rustwatch-cli/src/tui.rs:23-126`) mixes state, rendering, and crossterm input in one function with no separable `render(frame, state)`.
- The daemon command handler is an inline closure inside the accept loop (`crates/rustwatch-daemon/src/main.rs:102-157`) rather than an extractable function.
- CLI `commands::*` functions print directly to stdout rather than returning structured values (`crates/rustwatch-cli/src/commands.rs` throughout).

## Planned, Not Implemented

`docs/TESTING_PLAN.md` (127 lines) proposes a four-layer strategy — unit, behavior, integration, HTTP contract — with `proptest`, `tempfile`, `wiremock`, `assert_cmd`, `predicates`, `cargo-nextest`, and `.github/workflows/test.yml`. Its Phase 0 checklist (dev-deps, nextest config, CI, adding `rustwatch-memory-backends` to the workspace) is **entirely unstarted**: none of those files, sections, or settings exist.

The plan's own "Known Risks" section already names four of the bugs listed above, and its Phase 4 correctly identifies the lib+bin split as a prerequisite for daemon/CLI/TUI testing.

**Do not treat this file as a convention to match — there is no code implementing any part of it.** When tests are first written, treat the plan as a candidate roadmap to be validated, not as a spec.

---

*Testing analysis: 2026-10-02*
