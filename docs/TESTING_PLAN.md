# Complete Test Plan — rustwatch

## Strategy Overview

Four test layers, applied TDD-first where logic is pure:

| Layer | Scope | Tooling |
|---|---|---|
| **Unit** | Pure functions & types, one module at a time | `#[cfg(test)]`, `proptest` for edge cases |
| **Behavior** | Given/When/Then scenarios across a component's public API | `#[cfg(test)]` with scenario-named tests |
| **Integration** | Real SQLite/LanceDB/SurrealDB/Unix sockets/stdio JSON-RPC | `tests/` dirs + `tempfile` |
| **HTTP contract** | OpenAI/Anthropic classifier request/response behavior | `wiremock` |

## Phase 0 — Test Infrastructure

1. Add `[workspace.dependencies]` dev-deps; each member gets:
   ```toml
   [dev-dependencies]
   tempfile = "3"
   proptest = "1"
   ```
2. Per-crate extras:
   - `rustwatch-analyze`: `wiremock = "0.6"`, `tokio-test`
   - `rustwatch-mcp`: `assert_cmd = "2"`, `predicates = "3"`
   - `rustwatch-cli`: `assert_cmd`, `predicates`
   - `rustwatch-daemon`: split into lib+bin (see Phase 4)
3. Add `.config/nextest.toml` and CI workflow `.github/workflows/test.yml`
   (fmt → clippy → nextest on macOS runner + ubuntu stub run).
4. Enable `rustwatch-memory-backends` as a workspace member.

## Phase 1 — Unit Tests (TDD)

### rustwatch-core
- `segment.rs` `SegmentGrouper`: focus-change opens/closes segments;
  TextDelta/Paste append order; TextFieldSnapshot replaces buffer only if shorter;
  MAX_BUFFER_CHARS cap eviction; flush emits pending segment.
- `events.rs`: tagged serde round-trips for all `CaptureEventKind`; unknown-tag rejection.
- `ipc.rs`: `DaemonCommand`/`DaemonReply` round-trip all variants.
- `error.rs`: Display strings for every variant; From conversions.
- `config.rs`: defaults sanity; TOML round-trip; partial TOML merge onto defaults.
- `db.rs`: `new_activity_id` uniqueness format.

### rustwatch-analyze
- `redact.rs`: pattern replacement with `[REDACTED]`; multi-pattern; app exclusion;
  truncation to `chunk_max_chars`; empty input.
- `classifier.rs` (after small refactor): extract `build_prompt` → pub(crate);
  extract response parsing into pure fns; test valid/malformed payloads.
- `chart.rs`: publish render helpers; terminal output assertions; HTML markup; empty day.

### rustwatch-memory
- `embedder.rs`: hash_embedding determinism, 384-dim, L2-normalized.
- `sqlite_store.rs`: bytes↔f32 round-trip; cosine known values; zero-vector safety.
- `graph.rs`: slug() normalization.
- `rag.rs` `GraphRag::merge`: keyword boost (+0.2); max-score dedup by chunk_id;
  sort desc; empty inputs.

### rustwatch-capture
- macOS pure fns: `key_to_text`, `hash_content`, `scope_label`, `active_modifiers`.
- stub: UnsupportedPlatform errors; empty permissions report.

### Property-based tests (proptest)
- `Redactor::scrub`: never panics on arbitrary UTF-8 (char-boundary truncation).
- `SegmentGrouper.append_text`: buffer never exceeds cap under arbitrary interleavings.
- `hash_embedding` always normalized for arbitrary strings.
- IPC serde round-trips for arbitrary values.

## Phase 2 — Behavior Tests

Scenario-style tests through public APIs:

1. Capture-to-segment lifecycle: synthetic event streams → grouping semantics.
2. Analysis pipeline with MockClassifier: fetch pending, skip excluded apps,
   scrub before classify, persist activities, mark analyzed (idempotent second run).
3. Memory ingest→search: ranked retrieval; rebuild_from_store reproducibility.
4. GraphRag fusion: no duplicate chunks; graph-expanded apps surface related hits.
5. MCP tool contract: all 5 tools happy path + error paths.

*Prerequisite refactor:* `analyze_pending` accepts `&dyn ActivityClassifier`.

## Phase 3 — Integration Tests (`tests/` dirs)

1. `core/tests/db_integration.rs`: migrations ran; CRUD; ordering+limit; stats();
   regression test for LIKE-substring join in `list_unanalyzed_segments`.
2. `core/tests/ipc_integration.rs`: real Unix socket; server/client round-trips;
   malformed frame handling.
3. `memory/tests/*`: SqliteMemoryStore on temp files; FTS5; GraphStore persistence;
   MemoryEngine end-to-end on temp dirs.
4. `analyze/tests/classifier_http.rs` (wiremock): OpenAI + Anthropic request shape,
   auth headers; 500/timeout/rate-limit error propagation.
5. `memory-backends/tests/`: SurrealDB in-mem upsert+expand; LanceDB on-disk
   create/upsert/search.
6. `mcp/tests/jsonrpc_stdio.rs` (assert_cmd): initialize → tools/list → tools/call
   over stdio JSON-RPC.

## Phase 4 — Binary Coverage (CLI + daemon + TUI)

Refactors first:

1. **daemon**: split main.rs into lib+bin; extract command-handler closure into
   testable fn; integration: boot run_daemon with stub capture on temp dir.
2. **cli**: commands return structured results (printing in main dispatch);
   unit-test parse_opt_ts and clap grammar via try_parse_from.
3. **TUI**: extract state model + render(frame, state); TestBackend snapshot tests;
   leave crossterm event loop untested.
4. **CLI e2e smoke** (assert_cmd): permissions/export/chart against temp HOME/store.

## Phase 5 — CI & Quality Gates

```bash
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

- Ubuntu CI job validates stub-platform path.
- Coverage via llvm-cov, target ≥80% on core/analyze/memory libs.

## Known Risks (tests should pin/document)

- `parse_ts` silently falls back to `Utc::now()` on bad input → regression test.
- `String::truncate` char-boundary panic risk in Redactor → proptest will catch.
- `expand_around_apps(hops)` uses hops as row LIMIT, not BFS depth → document.
- `list_unanalyzed_segments` LIKE-substring matching can cross-match ids.

## Execution Order

Phase 0 → core units → analyze refactor+units → memory units → behavior tests →
integration → binaries/backends → CI polish.
