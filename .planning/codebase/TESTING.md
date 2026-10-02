# Testing Patterns

**Analysis Date:** 2026-10-02

## Current State: There Are No Tests

This is the single most important fact in this document.

| Check | Result |
|---|---|
| `#[cfg(test)]` modules in `crates/` | **0** across 30 `.rs` files (`rg -c "cfg\(test\)"` → 0 matches, 38 files searched) |
| `#[test]` / `#[tokio::test]` attributes | **0** |
| `tests/` directories in any crate | **0** |
| `[dev-dependencies]` in any `Cargo.toml` | **0** across 8 manifests |
| `.config/nextest.toml`, `.github/workflows/*.yml` | do not exist |
| Coverage config (tarpaulin, llvm-cov, cargo-llvm-cov.toml) | does not exist |

**`cargo test --workspace` exits 0** and prints ten times:
```
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
The build reports success while asserting nothing. Any pipeline that treats a zero exit code as "tests pass" is green on an empty suite. Until at least one test exists, the only meaningful gate is `cargo clippy` (which does emit real warnings — see `CONVENTIONS.md`).

`crates/rustwatch-memory-backends` has no tests either, and is additionally excluded from `--workspace`, so it is never compiled or tested by any command in `README.md`.

## Test Framework

**Runner:**
- Rust's built-in `libtest`, invoked only through `cargo test`. Not configured, not installed as a separate tool.
- Nothing else is wired in.

**Assertion Library:**
- None. No `assert_cmd`, `predicates`, `insta`, `similar`, or hand-rolled helpers.

**Available run commands (all currently vacuous):**
```bash
cargo test                          # run everything (0 tests)
cargo test -p rustwatch-core        # per-crate; lib target only, no test target exists
cargo test --workspace -- --nocapture
cargo nextest run --workspace       # NOT INSTALLED — required by docs/TESTING_PLAN.md:112
cargo llvm-cov --workspace          # NOT INSTALLED — required by docs/TESTING_PLAN.md:115
```

## The Test Plan That Already Exists

`docs/TESTING_PLAN.md` (127 lines, last touched 2026-08-24) is a complete, unimplemented six-phase design. **Read it before writing tests — it names the target functions and the intended libraries precisely.** It has never been started: none of the dev-dependencies, `.config/nextest.toml`, `.github/workflows/test.yml`, or `tests/` files it specifies exist.

**Strategy table (`docs/TESTING_PLAN.md:5-12`) — four layers:**

| Layer | Scope | Tooling |
|---|---|---|
| Unit | Pure functions & types, one module at a time | `#[cfg(test)]`, `proptest` for edge cases |
| Behavior | Given/When/Then across a component's public API | `#[cfg(test)]`, scenario-named tests |
| Integration | Real SQLite / LanceDB / SurrealDB / Unix sockets / stdio JSON-RPC | `tests/` dirs + `tempfile` |
| HTTP contract | OpenAI/Anthropic classifier request/response behavior | `wiremock` |

**Planned dependency set (`docs/TESTING_PLAN.md:16-29`) — none of this is installed:**
```toml
# [workspace.dependencies]
tempfile = "3"
proptest = "1"
```
| Crate | Extra dev-dependencies |
|---|---|
| `rustwatch-analyze` | `wiremock = "0.6"`, `tokio-test` |
| `rustwatch-mcp` | `assert_cmd = "2"`, `predicates = "3"` |
| `rustwatch-cli` | `assert_cmd`, `predicates` |
| `rustwatch-daemon` | none — requires a lib/bin split first (Phase 4) |
| `rustwatch-memory-backends` | none — requires workspace membership first |

**Planned CI gate (`docs/TESTING_PLAN.md:109-115`):**
```bash
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```
Two jobs: macOS runner for real capture paths, Ubuntu runner to validate the stub-platform path. Coverage via llvm-cov with a **≥80% target on the core / analyze / memory libraries**. Both halves of that gate currently fail: `cargo fmt --check` reports 45 diff hunks (see `CONVENTIONS.md`), and `-D warnings` would fail on the 3 existing clippy warnings.

## Testable Seams That Already Exist

These are functions with no filesystem, network, or platform dependency. They are the natural first targets — `docs/TESTING_PLAN.md:31-66` enumerates nearly all of them.

**Pure logic, reachable today from a `#[cfg(test)] mod tests` in the same file:**

| Function | File:line | Notes |
|---|---|---|
| `SegmentGrouper::on_event` / `flush` | `crates/rustwatch-core/src/segment.rs:28,63` | Full state machine over `CaptureEvent`; no I/O. Plan covers focus open/close, append order, snapshot-replace, `MAX_BUFFER_CHARS` eviction |
| `hash_embedding` | `crates/rustwatch-memory/src/embedder.rs:46` | Private; deterministic, 384-dim, L2-normalized. Plan wants determinism + normalization properties |
| `GraphRag::merge` | `crates/rustwatch-memory/src/rag.rs:6` | `pub`, static method on a unit struct. Keyword boost `+0.2` at `:12`, max-score dedup by `chunk_id` at `:17`, desc sort at `:29` |
| `Redactor::scrub` / `is_excluded_app` | `crates/rustwatch-analyze/src/redact.rs:25,36` | `pub`; pure once a `Redactor` exists. `Redactor::new` only compiles regexes |
| `key_to_text`, `active_modifiers`, `scope_label`, `hash_content` | `crates/rustwatch-capture/src/platform/macos.rs:252,165,348,355` | Private, macOS-only module, no platform handles. `hash_content` is currently dead code |
| `Cosine`, `bytes_to_f32` | `crates/rustwatch-memory/src/sqlite_store.rs:131,124` | Private; plan wants round-trip, known cosine values, zero-vector safety |
| `slug` | `crates/rustwatch-memory/src/graph.rs:112` and `crates/rustwatch-memory-backends/src/surreal.rs:113` | **Duplicated verbatim across two crates.** Plan names the `graph.rs` copy |
| `expand_tilde` | `crates/rustwatch-core/src/paths.rs:58` | Pure string logic over `HOME` |
| `parse_ts` | `crates/rustwatch-core/src/db.rs:252` | Private; plan wants the bad-input regression test (`:119`) |

**Seams needing only `tempfile` (a real path argument is already the API):**
- `Store::open(&Path)` — `crates/rustwatch-core/src/db.rs:17`. Runs refinery migrations automatically on open, so a temp DB is immediately schema-complete.
- `SqliteMemoryStore::open(&Path)` — `crates/rustwatch-memory/src/sqlite_store.rs:27`
- `GraphStore::open(&Path)` — `crates/rustwatch-memory/src/graph.rs:10`
- `LanceMemoryStore::open(&Path)` — `crates/rustwatch-memory-backends/src/lance.rs:19`
- `SurrealGraphStore::open(&Path)` — `crates/rustwatch-memory-backends/src/surreal.rs:13` (in-memory `Mem` engine, so no filesystem actually needed despite the signature)

**Seams needing only a `UnixStream` pair:**
```rust
// crates/rustwatch-core/src/ipc.rs:70-73 — the handler is injected, so no daemon required
pub async fn handle_connection(
    mut stream: UnixStream,
    handler: impl Fn(DaemonCommand) -> DaemonReply,
) -> Result<()>
```
Pair it with `DaemonClient::send` (`crates/rustwatch-core/src/ipc.rs:51`) for a full round-trip of every `DaemonCommand`/`DaemonReply` variant with no process spawn. `docs/TESTING_PLAN.md:84-86` calls for exactly this plus a malformed-frame case.

**Seams that work on any OS:**
- `PlatformCapture::permissions()` on the stub returns a populated report with zero I/O (`crates/rustwatch-capture/src/platform/stub.rs:29-34`). Every fallible stub method returns `Error::UnsupportedPlatform` (`:23-27`, `:44-47`, `:54-58`), which is directly assertable — this is what the planned Ubuntu CI job validates (`docs/TESTING_PLAN.md:114`).

**Seams that need `wiremock`:**
- `OpenAiClassifier::classify` — `crates/rustwatch-analyze/src/classifier.rs:88`. Request shape at `:90-97`, endpoint and bearer auth at `:99-104`, response extraction at `:110-114`
- `AnthropicClassifier::classify` — `:138`. Request shape at `:140-146`, `x-api-key` + `anthropic-version: 2023-06-01` headers at `:151-152`, `content[0].text` extraction at `:160`
- Plan wants 500 / timeout / rate-limit propagation (`docs/TESTING_PLAN.md:89`). `error_for_status()?` at `:106` and `:156` means non-2xx surfaces as `reqwest::Error`.

**Seams that need `assert_cmd` + `predicates`:**
- MCP stdio JSON-RPC: `initialize` → `tools/list` → `tools/call` (`crates/rustwatch-mcp/src/main.rs:38-59`, `handle_tool` at `:76-112`). All 5 tools are declared at `:46-50`. Plan: `mcp/tests/jsonrpc_stdio.rs` (`docs/TESTING_PLAN.md:92`)
- CLI smoke: `permissions`, `export`, `chart` against a temp `HOME` (`docs/TESTING_PLAN.md:105`)

**Seam needing `proptest`** (`docs/TESTING_PLAN.md:61-65`):
- `Redactor::scrub` never panics on arbitrary UTF-8 (char-boundary truncation)
- `SegmentGrouper` buffer never exceeds `MAX_BUFFER_CHARS` under arbitrary event interleavings
- `hash_embedding` always normalized for arbitrary strings
- `DaemonCommand`/`DaemonReply` serde round-trips for arbitrary values

## Blocked Seams — Why Nothing Is Testable Yet

Seven structural blockers. `docs/TESTING_PLAN.md` addresses most of them in Phase 4; doing that work first is what unblocks the test suite.

1. **No `lib.rs` in any binary crate.** `crates/rustwatch-cli/src/main.rs`, `crates/rustwatch-daemon/src/main.rs`, `crates/rustwatch-mcp/src/main.rs` have no library target, so nothing in `tests/` can import them. **Fix:** split each into `lib.rs` + thin `main.rs`. The daemon is explicitly called out (`docs/TESTING_PLAN.md:99-100`).
   > `crates/rustwatch-daemon` → `src/lib.rs` exporting `pub async fn run_daemon(paths: DataPaths, config: Config)`; `main.rs` keeps only tracing init and the call. The function already exists at `crates/rustwatch-daemon/src/main.rs:30` and just needs `pub` + relocation.

2. **CLI commands print and return `()`.** All 13 functions in `crates/rustwatch-cli/src/commands.rs` return `anyhow::Result<()>` and write directly to stdout via `println!` (21 calls). There is nothing to assert on. **Fix (`docs/TESTING_PLAN.md:101-102`):** return a structured value and let `main` do the printing.

3. **The TUI has no state/render split.** `run_loop` (`crates/rustwatch-cli/src/tui.rs:23-127`) owns terminal state, the `Store` query, the daemon round-trip, widget construction, and the crossterm event loop in one function. **Fix (`docs/TESTING_PLAN.md:103-104`):** extract a `State` struct and `fn render(frame: &Frame, state: &State)`; test with ratatui's `TestBackend`. The plan explicitly leaves the crossterm event loop untested.

4. **`analyze_pending` cannot be given a fake classifier.** The trait exists —
   ```rust
   // crates/rustwatch-analyze/src/classifier.rs:8-11
   #[async_trait]
   pub trait ActivityClassifier: Send + Sync {
       async fn classify(&self, batch: SegmentBatch) -> anyhow::Result<Vec<ActivityLabel>>;
   }
   ```
   — but `analyze_pending` constructs its own via `build_classifier(config)` at `:41`, which requires `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` in the environment (`:76`, `:126`). **Fix (`docs/TESTING_PLAN.md:78`, named as the Phase 2 prerequisite):** change the signature to accept `&dyn ActivityClassifier`. The `Send + Sync` bound is already there for exactly this reason.

5. **`build_prompt` and the response parsers are private.** `build_prompt` (`crates/rustwatch-analyze/src/classifier.rs:166`) and the JSON extraction at `:110-114` / `:160-163` cannot be reached from a test. **Fix (`docs/TESTING_PLAN.md:46-47`):** mark `pub(crate)` and extract the response parsing into pure functions.

6. **`render_terminal` / `render_html` are private and only reachable through `render_chart(store, ...)`.** `crates/rustwatch-analyze/src/chart.rs:20,42` take `&[ActivityRecord]` and are pure, but `render_chart` (`:11`) demands a `Store`. **Fix (`docs/TESTING_PLAN.md:48`):** publish the render helpers.

7. **`Config` cannot be partially constructed.** No `#[serde(default)]` on any field (`crates/rustwatch-core/src/config.rs:5-59`), and `DataConfig` requires all four path fields. A test must start from `Config::default()` and overwrite nested fields, which also means a test `Config` picks up `~/.rustwatch` defaults it did not ask for. **Fix:** add a `Config::for_test(root: PathBuf)` constructor or `#[serde(default)]`.

**Secondary blockers:**
- `crates/rustwatch-memory-backends` is not a workspace member (`Cargo.toml:3-11`), so `--workspace` never compiles or tests it. Plan Phase 0 item 4 (`docs/TESTING_PLAN.md:29`).
- `MemoryChunk`, `ScoredChunk`, and `SearchHit` lack `PartialEq` (`crates/rustwatch-memory/src/sqlite_store.rs:1,14`; `crates/rustwatch-memory/src/rag.rs:34`), so store round-trips must be asserted field-by-field rather than with `assert_eq!`. Add the derive.
- `CaptureEvent::new` stamps `timestamp: Utc::now()` internally (`crates/rustwatch-core/src/events.rs:59-66`) with no `with_timestamp` alternative, so `SegmentGrouper` tests cannot construct deterministic timestamps. Add a constructor that accepts one.

## Test File Organization

**Current:** none. **Target (per `docs/TESTING_PLAN.md`):** co-located unit tests, separate `tests/` directory for integration.

```
crates/<crate>/
├── src/
│   └── <module>.rs          # #[cfg(test)] mod tests at the bottom
└── tests/
    ├── <name>_integration.rs
    └── common/
        └── mod.rs           # shared helpers (does not exist yet)
```

Planned file names (`docs/TESTING_PLAN.md:80-93`):
| Path | Covers |
|---|---|
| `crates/rustwatch-core/tests/db_integration.rs` | migrations ran, CRUD, ordering + limit, `stats()`, `list_unanalyzed_segments` LIKE-substring regression |
| `crates/rustwatch-core/tests/ipc_integration.rs` | real Unix socket, server/client round-trips, malformed frame |
| `crates/rustwatch-memory/tests/*` | `SqliteMemoryStore` on temp files, FTS5, `GraphStore` persistence, `MemoryEngine` end-to-end |
| `crates/rustwatch-analyze/tests/classifier_http.rs` | OpenAI + Anthropic request shape, auth headers, 500/timeout/rate-limit |
| `crates/rustwatch-memory-backends/tests/` | SurrealDB in-mem upsert + expand, LanceDB on-disk create/upsert/search |
| `crates/rustwatch-mcp/tests/jsonrpc_stdio.rs` | `initialize` → `tools/list` → `tools/call` over stdio |

**Do this:** pure function → `#[cfg(test)] mod tests` at the bottom of its own file. Anything needing a temp dir, a socket, an HTTP stub, or a spawned binary → `crates/<crate>/tests/`. Do not put a `#[cfg(test)]` module in a file you do not own, and do not reach into another module's privates — make them `pub(crate)` instead.

## Test Structure

**Suite organization (from the plan's intent, `docs/TESTING_PLAN.md:9-10`):** unit tests are named per behavior, not per function. The plan lists them as behavior phrases — "focus-change opens/closes segments", "TextDelta/Paste append order", "TextFieldSnapshot replaces buffer only if shorter", "bytes↔f32 round-trip", "hash_embedding determinism, 384-dim, L2-normalized" — rather than `test_focus_change_1`.

**Do this:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn focus_event(app_name: &str) -> CaptureEvent {
        CaptureEvent::new(
            CaptureEventKind::FocusChange {
                from: None,
                to: AppContext {
                    app_name: app_name.to_string(),
                    window_title: "title".into(),
                    process_id: 1,
                    bundle_id: None,
                },
            },
            None,
        )
    }

    #[test]
    fn focus_change_closes_previous_segment_and_opens_a_new_one() {
        let mut grouper = SegmentGrouper::new();
        assert!(grouper.on_event(&focus_event("Safari")).is_none());

        let closed = grouper
            .on_event(&focus_event("Terminal"))
            .expect("focus change should close the open segment");
        assert_eq!(closed.app_name, "Safari");
    }
}
```
Use `use super::*;` — the existing `lib.rs` files make everything in the module reachable that way, and the plan's per-module unit-test phase assumes it.

**Behavior-layer tests** are scenario-named with explicit Given/When/Then intent, per `docs/TESTING_PLAN.md:69-77`. The five suites the plan names:
1. Capture-to-segment lifecycle — synthetic event streams → grouping semantics
2. Analysis pipeline with a `MockClassifier` — fetch pending, skip excluded apps, scrub before classify, persist activities, mark analyzed (idempotent on a second run)
3. Memory ingest→search — ranked retrieval; `rebuild_from_store` reproducibility
4. GraphRag fusion — no duplicate chunks; graph-expanded apps surface related hits
5. MCP tool contract — all 5 tools, happy path and error paths

## Mocking

**Framework:** none installed. The plan names `proptest` for properties, `wiremock` for HTTP, and a hand-written `MockClassifier` for the trait (`docs/TESTING_PLAN.md:72`). No `mockall`, `mockito`, or `wiremock`-alternative.

**The one existing trait seam** is `ActivityClassifier`. Once `analyze_pending` accepts `&dyn ActivityClassifier` (`docs/TESTING_PLAN.md:78`), a mock is three lines:
```rust
struct MockClassifier {
    calls: std::sync::Mutex<Vec<SegmentBatch>>,
    reply: Vec<ActivityLabel>,
}

#[async_trait::async_trait]
impl ActivityClassifier for MockClassifier {
    async fn classify(&self, batch: SegmentBatch) -> anyhow::Result<Vec<ActivityLabel>> {
        self.calls.lock().unwrap().push(batch);
        Ok(self.reply.clone())
    }
}
```
The `Send + Sync` supertrait bound on the trait (`crates/rustwatch-analyze/src/classifier.rs:9`) is what makes a `Mutex`-based recorder legal.

**The second seam** needs no mock at all — `handle_connection` takes the handler as a parameter (`crates/rustwatch-core/src/ipc.rs:72`), so a test supplies a `|command| DaemonReply::…` closure directly.

**What to mock:**
- Only the LLM HTTP boundary, via `wiremock` (`docs/TESTING_PLAN.md:88-89`)
- Only the classifier trait, via a hand-written `MockClassifier`, once the injection refactor lands
- The filesystem, via `tempfile::TempDir` passed into the existing `open(path)` constructors — **not** by mocking `std::fs`
- The daemon, via a real `UnixListener` + `handle_connection` in-process — **not** by mocking `DaemonClient`

**What NOT to mock:**
- **SQLite.** Every store takes a path and `rusqlite` is bundled. Use a temp file and exercise the real schema and the real migrations — `docs/TESTING_PLAN.md:82-87` calls for this explicitly.
- **The platform layer.** Do not abstract `PlatformCapture` behind a trait for tests. On non-macOS the `stub.rs` implementation already returns `Error::UnsupportedPlatform`, which is a directly assertable contract (`docs/TESTING_PLAN.md:59`).
- **Time.** No clock abstraction exists. `Utc::now()` is called in 12 places (`crates/rustwatch-core/src/segment.rs:64`, `crates/rustwatch-core/src/events.rs:62`, `crates/rustwatch-core/src/db.rs:255`, `crates/rustwatch-memory/src/lib.rs:92-97`, …). Do not add a time-travel crate to fix this — instead construct values with explicit timestamps and assert on ranges or on the non-time fields. Note that `parse_ts`'s `Utc::now()` fallback (`crates/rustwatch-core/src/db.rs:255`) makes any bad-timestamp test nondeterministic by design; that is the bug the plan wants pinned (`docs/TESTING_PLAN.md:119`).

## Fixtures and Factories

**Current:** none exist. No `tests/common/mod.rs`, no builder types, no `#[cfg(test)] mod fixtures`.

**Test data builders are the missing piece.** Every test that touches storage needs a `CaptureEvent`, a `SessionSegment`, and an `ActivityRecord` with plausible timestamps, and none of those types has a constructor — all three are structs with public fields and no `new` (`crates/rustwatch-core/src/events.rs:70,83`). Only `CaptureEvent::new(kind, app)` exists (`:59`), and it stamps `Utc::now()` internally.

**Do this:** add a small fixture module. Put shared builders in `crates/rustwatch-core/src/lib.rs` behind `#[cfg(any(test, feature = "test-util"))]` so every crate's `tests/` can import them, or duplicate a `tests/common/mod.rs` per crate until a `test-util` feature is worth the surface. The plan does not specify a location; pick one and use it consistently.

Minimum builders needed:
| Builder | Target type | Why |
|---|---|---|
| `focus_event(app)` | `CaptureEvent` | `SegmentGrouper` and capture-loop tests |
| `text_event(s)` | `CaptureEvent` | buffer append / cap eviction tests |
| `segment(app, started, ended)` | `SessionSegment` | `Store` and `MemoryEngine` tests |
| `activity(label, apps, topics)` | `ActivityRecord` | analyze, chart, graph, and MCP tests |
| `chunk(text)` | `MemoryChunk` | vector-store round-trip tests |
| `config_for(root)` | `Config` | every test needing filesystem layout — blocked until `Config::for_test` exists |

## Coverage

**Requirements:** none enforced. No `cargo-tarpaulin`, no `cargo-llvm-cov`, no threshold, no report artifact, no badge.

**Target (planned, `docs/TESTING_PLAN.md:115`):** llvm-cov with **≥80% on the `rustwatch-core`, `rustwatch-analyze`, and `rustwatch-memory` libraries**. Binaries (`cli`, `daemon`, `mcp`) are excluded from the threshold because their logic currently lives in untestable `main.rs` bodies.

**Where coverage will be worst on day one** — the untestable layers, by design:
- `crates/rustwatch-capture/src/platform/macos.rs` (359 lines) — macOS-only, needs Input Monitoring + Screen Recording
- `crates/rustwatch-cli/src/tui.rs` (127 lines) — needs the state/render split
- `crates/rustwatch-cli/src/commands.rs` (292 lines) — needs structured returns
- `crates/rustwatch-daemon/src/main.rs` (164 lines) — needs a lib target
- `crates/rustwatch-memory-backends/` (267 lines) — not a workspace member

**View coverage:**
```bash
# NOT INSTALLED — install first
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --html
```

## Known Risks Tests Should Pin

The plan names four (`docs/TESTING_PLAN.md:117-122`). Each is a real, verifiable defect in the current code — write a test for each before changing the code, so the test documents the bug and then confirms the fix.

1. **`parse_ts` silently falls back to `Utc::now()` on bad input** — `crates/rustwatch-core/src/db.rs:252-256`. A corrupt timestamp column becomes "now", which silently corrupts timeline queries instead of surfacing an error.
2. **`String::truncate` panics on a char boundary in `Redactor::scrub`** — `crates/rustwatch-analyze/src/redact.rs:30-32`. `out.truncate(self.max_chars)` slices at a byte offset and will panic whenever `max_chars` lands inside a multi-byte character. Reachable from any captured text containing non-ASCII. `proptest` over arbitrary UTF-8 will find it immediately (`docs/TESTING_PLAN.md:62`).
3. **`expand_around_apps(hops)` uses `hops` as a SQL `LIMIT`, not BFS depth** — `crates/rustwatch-memory/src/graph.rs:80,87` (`let limit = hops.max(1) as i64;` bound to `LIMIT ?2`). The config field is named `graph_expand_hops` (`crates/rustwatch-core/src/config.rs:46`), so the name and the behavior disagree. The plan says document it (`docs/TESTING_PLAN.md:121`); the same bug exists in the SurrealDB copy (`crates/rustwatch-memory-backends/src/surreal.rs:87`).
4. **`list_unanalyzed_segments` LIKE-substring matching can cross-match ids** — `crates/rustwatch-core/src/db.rs:161`, `a.segment_ids_json LIKE '%' || s.id || '%'`. A segment id that is a substring of a different id is treated as already analyzed. The plan wants a regression test specifically for this (`docs/TESTING_PLAN.md:83`).

Additional risks found while reading the code, not yet in the plan:
- **`Store::list_activities_for_date` unwraps twice** — `crates/rustwatch-core/src/db.rs:183-184`. Safe for valid clock times today, but it is an unguarded `unwrap` on the storage read path.
- **The daemon silently drops captured events on `try_lock` contention and on write failure** — `crates/rustwatch-daemon/src/main.rs:62-81`. `events_captured` increments before the write is attempted, so the counter reports captures that never reached disk.
- **`slug` is duplicated verbatim** between `crates/rustwatch-memory/src/graph.rs:112-123` and `crates/rustwatch-memory-backends/src/surreal.rs:113-124`. Test both or, better, extract one into `rustwatch-memory` and test it once.
- **`analyze_pending` attaches every filtered segment id to every returned label** — `crates/rustwatch-analyze/src/classifier.rs:60`. Every label claims all segments regardless of what the classifier matched.

---

*Testing analysis: 2026-10-02*
