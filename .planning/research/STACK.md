# Stack Research

**Domain:** Local-first activity capture + LLM classification + SQLite vector memory on macOS (Rust — rustwatch hardening milestone)
**Researched:** 2026-10-02
**Confidence:** MEDIUM (versions verified against crates.io registry + Context7 official docs; ecosystem judgments cross-checked across 2+ sources)

> Scope note: this is a **brownfield hardening** milestone. The engine already exists (capture → LLM → SQLite memory → CLI/TUI/MCP). This doc prescribes what to **keep, upgrade, add, and delete** — it does not re-research the existing architecture.

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended | Confidence |
|------------|---------|---------|-----------------|------------|
| tokio | 1.53.1 (keep — already latest) | Async runtime, Unix-socket IPC, timers, signal handling | Latest release per crates.io (Jul 2026); already pinned in lockfile. Gives `tokio::net::UnixListener`, `tokio::time::interval` (5-min screenshot tick), and `tokio::signal::unix` (SIGTERM/SIGINT graceful shutdown) with zero dependency change | HIGH |
| rusqlite | 0.40.x, `features = ["bundled"]` (upgrade from 0.32.1) | All three SQLite files (events, memory, graph) | Bundled build compiles SQLite from source (no system lib) **with FTS5 enabled by default**; 0.40 defaults new connections to a 5 s `busy_timeout`. Fixes the daemon-vs-CLI `database is locked` class of bugs when combined with explicit `busy_timeout` + WAL + `synchronous=NORMAL` on every connection | HIGH (version + FTS5/bundled facts verified; upgrade-churn risk is the only MEDIUM part) |
| sqlite-vec | 0.1.9 stable + zerocopy 0.8 | ANN vector index (`vec0` virtual table, cosine) inside the existing `memory.db` | The official sqlite-vec Rust crate statically links the C extension and registers via `sqlite3_auto_extension` — works with rusqlite+bundled, no server, no new DB file, no build of platform binaries. Replaces the O(N) full-table-scan + in-Rust cosine loop with `WHERE embedding MATCH ? ORDER BY distance LIMIT k`. This is what makes "real embeddings end-to-end" actually retrievable | MEDIUM |
| fastembed | 7.1 (upgrade from 4.9.1; **make default-on**) | Local embedding inference (ONNX) | Default model `BGE-small-en-v1.5` = 384-dim, matches the existing schema width. 4.9.1 is ~2 years stale; 7.x is current (Sep 2026). Must be default-on (or loud startup error naming the active embedder) — the silent `DefaultHasher` fallback is the single most misleading behavior in the memory path | MEDIUM |
| reqwest | 0.12 (keep lockfile 0.12.28; **do not chase 0.13 yet**) | HTTP transport for all three LLM providers | 0.12 + `rustls-tls` (no OpenSSL) is proven in-tree. What it lacks is not a version but configuration: no timeout (hangs forever on a hung provider), no retry on 429/5xx. Add `.timeout()` + `connect_timeout` + a small bounded retry/backoff loop. 0.13.5 exists but buys nothing for simple JSON POSTs — defer to avoid churn during hardening | MEDIUM |
| refinery | 0.9.x with `rusqlite` feature (upgrade from 0.8.16; **extend to all 3 DBs**) | Embedded SQL migrations | Already used for the main DB (`embed_migrations!`). The two memory DBs use unversioned `CREATE TABLE IF NOT EXISTS` string batches — any column/index backfill (vec0 table, embedding-model column, UNIQUE constraints) has no version tracking. One migration runner for all three files fixes schema drift permanently | MEDIUM |
| rmcp | 3.5, features `["server", "transport-io"]` (adopt for real; declared 0.3.2 is dead code) | MCP server (stdio) replacing the hand-rolled JSON-RPC loop | rmcp is now the **official** Rust MCP SDK (`modelcontextprotocol/rust-sdk`, 16.5M recent downloads) with `#[tool_router]`/`#[tool]` macros and `ServiceExt::serve`. The hand-rolled loop dies on any malformed line and never emits JSON-RPC `error` — a protocol-correctness liability with agent clients. 5 tools = bounded migration | MEDIUM |
| ratatui | 0.29 (keep; **defer 0.30**) + crossterm 0.28 (unify; remove 0.29 from lock tree) | TUI dashboard (timeline, shares, score, Q&A) | 0.30.2 is latest but modularized (`ratatui-core`/`ratatui-crossterm` split, breaking imports) — a migration with zero dashboard benefit. 0.29 already ships `ratatui::init()/restore()` single-path setup **with a panic hook that restores the terminal**, which is exactly the TUI cleanup fix needed. Unify on one crossterm to kill the dual-init hazard | MEDIUM |
| keytap | 0.4 (keep; **use the `chord` feature for hotkey-annotate**) | Keyboard capture + configurable annotate hotkey | Purpose-built rdev successor: CGEventTap without the Sonoma-crashing layout APIs, typed `PermissionDenied` via `IOHIDCheckAccess` (fixes silent no-capture), `Drop` shutdown. Its `ChordMatcher` implements "fire when this combo is held" on the **existing** system-wide tap — a hotkey-annotate trigger with zero new dependencies and zero new permissions | MEDIUM |
| xcap | 0.9.8 (keep, patch-bump from 0.9) | Window/monitor screenshots | Still maintained (macOS Spaces commit Mar 2026), already integrated, covers `Window` + `Monitor` capture. All three required triggers (5-min `tokio::interval`, focus-loop window-change, chord hotkey) are scheduling logic *around* xcap — no crate change needed | MEDIUM |
| active-win-pos-rs | 0.11 (keep — already latest) | Active window app/title/PID polling | Latest release (May 2026), tiny focused API, already integrated. Note: still no `bundle_id` — resolve bundle id from PID separately if identity-based app matching is wanted | HIGH (version), LOW (bundle-id workaround needs phase research) |
| arboard | 3.6 (keep, patch-bump from 3) | Clipboard read via NSPasteboard | 1Password-maintained, 3.6.1 current (Aug 2025). Text path is all rustwatch uses — no change | HIGH |
| fs2 | 0.4.3 (new, tiny) | Single-instance advisory lock on the pid file | Fixes "two daemons, one SQLite file". `File::lock_exclusive` (flock) held for the daemon lifetime; second instance exits with a clear message instead of stealing the socket. 87M downloads, frozen API — age is stability here, not rot | MEDIUM |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde / serde_json | 1 (keep) | Wire + storage formats everywhere | Already universal; rmcp 3.x also builds on them |
| chrono 0.4 + `serde` | keep | All timestamps as `DateTime<Utc>`, RFC 3339 text | Keep; normalize all writes through one helper (fixes lexicographic-ordering fragility) |
| clap 4 + `derive` | keep | CLI (`memory search`, `ask`, dashboard flags) | Keep; new commands (`ask`, retention/prune) are more subcommands, not a new framework |
| uuid 1 (`v4` + `serde`) | keep for event ids; **stop using for chunk ids** | Stable ids | Chunk ids must become deterministic (hash of segment-id + offset) so re-ingest replaces instead of duplicating |
| sha2 0.10 | keep, widen role | Deterministic hashing | Hash `segment_id + chunk offset` for chunk ids; replaces `DefaultHasher` (not stable across toolchains) for anything persisted |
| regex 1 | keep, widen pattern set | Secret redaction | Ship broader defaults (AWS keys, JWTs, PEM blocks, bearer tokens) and move redaction to the **write path** so storage/export/MCP are protected, not just LLM egress |
| directories 5 | keep | `~/.rustwatch` resolution | Keep; fix `Config::default()` tilde landmine by routing everything through `expand_tilde` |
| anyhow 1 / thiserror 2 | keep | Error handling (binaries vs `Error` enum) | Keep existing convention |
| tracing stack | keep | Logging + daemon diagnostics | Keep; add `write_errors` counter + capture-thread health to `DaemonState` so silent drops become visible |
| schemars | new (via rmcp) | JSON Schema for MCP tool params | Comes with rmcp `#[tool]` macros — no separate decision needed |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| rust-toolchain.toml (new file) | Pin the toolchain (currently unpinned, verified 1.97.1) | keytap 0.4 is edition-2024 / rust 1.85+; fastembed 7 + rmcp 3 pull modern deps. Pin `channel = "stable"` minimum, or exact 1.97.x for reproducible ONNX builds |
| cargo-nextest + `.config/nextest.toml` | Test runner (already specified in unimplemented `docs/TESTING_PLAN.md`) | Adopt as planned; hardening without regression tests reintroduces the same bugs |
| cargo clippy `--workspace --all-targets -- -D warnings` in CI | Lint gate (3 warnings today, no gate) | `.github/workflows/` does not exist — create it; also add `cargo machete` for the unused-dep drift |
| `cargo update -p <crate>` | Per-crate upgrades | Prefer over full `cargo update` — rusqlite 0.32→0.40 and fastembed 4→7 are the two bumps most likely to surface API breakage; isolate them |

## Installation

```toml
# workspace Cargo.toml — changes from current pins
[workspace.dependencies]
tokio      = { version = "1", features = ["full"] }          # keep (1.53.1 latest)
rusqlite   = { version = "0.40", features = ["bundled"] }    # upgrade 0.32 -> 0.40
reqwest    = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }  # keep line, add timeouts in code
refinery   = { version = "0.9", features = ["rusqlite"] }    # upgrade 0.8 -> 0.9
ratatui    = "0.29"                                          # keep, defer 0.30
crossterm  = "0.28"                                          # unify (drop 0.29 from tree)
fastembed  = "7"                                             # upgrade 4 -> 7, enable by default
rmcp       = { version = "3.5", features = ["server", "transport-io"] }  # adopt for real
sqlite-vec = "0.1"                                           # new (0.1.9 stable)
zerocopy   = "0.8"                                           # new (for vec byte passing)
fs2        = "0.4"                                           # new (single-instance lock)
# capture (macos-gated) — keep lines, bump patches:
# keytap = "0.4", xcap = "0.9", active-win-pos-rs = "0.11", arboard = "3"
```

```bash
# apply upgrades one at a time so breakage is attributable
cargo update -p rusqlite        # then cargo check --workspace
cargo update -p refinery        # then cargo check --workspace
# fastembed 4 -> 7 and rmcp 0.3 -> 3.5 are major jumps: bump the
# manifest version, then `cargo check -p rustwatch-memory` / `-p rustwatch-mcp`
cargo add sqlite-vec zerocopy fs2   # run inside the milestone's first phase
```

## Alternatives Considered

| Recommended | Alternative | When to Use Alternative |
|-------------|-------------|-------------------------|
| Hand-rolled `Provider` trait over reqwest | async-openai 0.42 | Only if rustwatch ever needs OpenAI-only advanced surface (streaming, files, audio). It tracks OpenAI API churn release-to-release and does nothing for Anthropic/Ollama — three providers need three clients anyway, and Ollama/llama.cpp are OpenAI-compatible, so one Chat-Completions-shaped client + per-provider base-URL/auth covers all |
| Hand-rolled `Provider` trait over reqwest | rig / ai-lib / agent frameworks | If the product grows agents/RAG pipelines/tool-calling. Today classification is one JSON-in/JSON-out call — a framework is 10 dependencies for zero leverage |
| sqlite-vec 0.1.9 in `memory.db` | LanceDB / SurrealDB (`rustwatch-memory-backends`) | Effectively never for v1: the backends crate is unbuildable (orphan, never resolved), pulls `arrow` + `surrealdb` weight, and its Lance schema silently drops metadata the SQLite store keeps. Revisit only if memory passes ~1M chunks with latency SLOs sqlite-vec can't hit |
| Keep xcap 0.9.8 | screencapturekit-rs 11 | If screenshots need HDR fidelity, occluded-window capture, or macOS 26 support. Costs: macOS 13+ floor, Swift-bridge build dep, new permission surface. xcap covers PNG screenshots today |
| keytap `ChordMatcher` | global-hotkey 0.8 (Tauri) | If a hotkey must fire when the daemon's tap is *off* (it never is — capture is the daemon's job) or without Input Monitoring (Carbon hotkeys don't need it). Costs: manager must live on the main thread with a running CFRunLoop — fights the tokio daemon design |
| refinery 0.9 for all 3 DBs | sqlx (migrate + query) | If the project ever needs async Postgres. For local SQLite, sqlx means replacing rusqlite wholesale — unjustified churn |
| ratatui 0.29 | ratatui 0.30.2 | Next milestone, as its own migration (modular imports touch every TUI file). Nothing in dashboard/score/Q&A needs 0.30 |
| rusqlite 0.40 | stay on 0.32.1 | If `cargo update -p rusqlite` surfaces real API breakage mid-milestone: pin 0.32, still add `busy_timeout`/WAL (available in 0.32), retry the bump after hardening |
| fs2 0.4.3 | fd-lock / lock-file polling | fd-lock is the maintained cousin; either is fine — the requirement is "advisory exclusive lock held for process lifetime", not a specific crate |

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `rustwatch-memory-backends` (lancedb 0.17 / surrealdb 2 / arrow 53) | Unbuildable orphan crate (not in workspace members, `cargo check` fails); Surreal backend ignores its path and stores in RAM (total data loss on exit); Lance schema drops `segment_id`/timestamps; `hops`-as-LIMIT-grade rot throughout. Reviving it is a second vector/GDBMS project hiding inside a hardening milestone | sqlite-vec 0.1.9 + FTS5 hybrid in the existing `memory.db`; delete or `workspace.exclude` the crate |
| `DefaultHasher` hash "embeddings" (and silent fastembed fallback) | Bag-of-words hash with no semantics, biased-positive cosine, whitespace-only tokenization, **not stable across Rust toolchains** — persisted vectors silently become garbage after a toolchain bump. The silent fallback makes fake search indistinguishable from working search | fastembed 7 default-on; store `embedding_model` + version in `memory_chunks`; fail loud when no real embedder initializes |
| rmcp 0.3.2 declaration | 3 major versions behind a fast-moving official SDK; the feature flag wires to nothing (hand-rolled loop always runs). Keeping the pin signals "MCP done" while the real server rots | rmcp 3.5 with `tool_router`, or delete the dep until the migration is scheduled — never a dead dep that lies |
| rdev (or any new keyboard crate) | Unmaintained since 2023; Sonoma main-thread crash (`TSMGetInputSourceProperty`); silent no-events on permission denial — every failure mode keytap 0.4 was written to fix | keytap 0.4 (already in tree) |
| async-openai / rig for classification | OpenAI-only churn tracker / heavy orchestration for a single classify call; neither natively unifies Anthropic's `x-api-key` + version headers and Ollama's no-auth local URL | Thin `LlmProvider` trait (OpenAI + Anthropic + Ollama-compatible) over the existing reqwest client; keep prompt/response validation + timestamp clamping work where it is |
| Global `reqwest::Client::new()` with no timeout | Default timeout is **none** — a hung provider hangs `analyze` forever; also no 429/5xx handling | `Client::builder().timeout(60s).connect_timeout(10s)` + bounded retry with backoff + `tokio::time::timeout` at the call site |
| `mpsc::unbounded_channel` + `try_lock` writer | Unbounded memory growth on DB stalls + **silent event drops** during IPC lock contention (the CRITICAL data-loss bug) | Bounded `mpsc::channel(N)` with drop-counter + `lock().await` in the writer; never drop capture data on contention |
| rusqlite connections without pragmas | No `busy_timeout` anywhere → instant `database is locked` across daemon/CLI/MCP processes sharing files | `busy_timeout(5s)` + `journal_mode=WAL` + `synchronous=NORMAL` (+ `foreign_keys=ON` where apt) on **all three** DBs, set in one shared `open_*` helper |
| Ad-hoc `CREATE TABLE IF NOT EXISTS` for new memory/graph schema | No version tracking = users silently keep stale schemas; vec0 tables and UNIQUE constraints can't be backfilled reliably | refinery migrations for all three DBs (V-next files), same runner pattern as the main DB |
| `#[serde(tag)]` hand-rolled MCP + `?` on every line | One malformed stdin line kills the server; tool errors kill the server; failures returned as `result` (clients can't tell error from success) | rmcp 3.5 `serve` + typed tools; every tool returns JSON-RPC `error` on failure, server survives |

## Stack Patterns by Variant

**If screenshots must stay lightweight PNGs on a 5-min cadence:**
- Use xcap + `tokio::time::interval` + existing focus loop + keytap chord, with retention pruning (age + size cap surfaced in `status`)
- Because capture volume is the scaling risk (hundreds of MB/day for heavy Alt-Tab users), not capture quality

**If memory search quality is the milestone (it is):**
- Use fastembed 7 (BGE-small-en-v1.5, 384-dim) → sqlite-vec `vec0(distance_metric=cosine)` + FTS5 `MATCH` prefilter → reciprocal-rank merge honoring `k`
- Because pure-vector misses exact tokens (app names, error strings) and pure-FTS misses semantics; the hybrid is the standard local-RAG pattern and both indexes live in the same file

**If the daemon must survive sleep + crash-loops:**
- Use `tokio::signal` shutdown (flush grouper, checkpoint WAL, remove pid file) + launchd `KeepAlive` kept + fs2 single-instance guard + `ThrottleInterval` awareness in plist review
- Because every ungraceful death currently loses the in-progress segment and leaves a stale pid file that blocks the next `start`

**If a future phase needs vision (screenshots to LLM):**
- Revisit `analyze.vision_model` + screencapturekit-rs then — not in this stack
- Because screenshots stay on-device by default per the privacy constraint; wiring vision now pre-builds an egress path the threat model hasn't approved

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| rusqlite 0.40 `bundled` | sqlite-vec 0.1.9 static link | Both compile SQLite from C source; register via `sqlite3_auto_extension` **before** opening connections. `bundled` is load-bearing — system libsqlite3 would be a different SQLite instance than the extension registered against |
| sqlite-vec 0.1.9 | zerocopy 0.8 | `Vec<f32>.as_bytes()` zero-copy passing is the documented pattern; pin zerocopy 0.8 (Sep 2026) |
| ratatui 0.29 | crossterm 0.28 only | Do not let 0.29 into the tree (today both resolve). The 0.30 upgrade pairs with crossterm 0.29 via `ratatui-crossterm` — keep that pairing for the future migration, not mixed into 0.29 |
| rmcp 3.5 | tokio 1 + serde 1 + schemars 0.8 | Check schemars version rmcp 3.5 expects before adding derives; MCP `#[tool]` params need `JsonSchema` |
| fastembed 7 | ort (ONNX Runtime) + first-run model download | Build-time cost (ONNX) + runtime model fetch from HF hub. Set `cache_dir` under `~/.rustwatch/models`, `show_download_progress`, and record model id in DB. Offline/zero-download installs need a prefetch step — flag for the phase plan |
| keytap 0.4 (edition 2024, rust 1.85+) | toolchain ≥ 1.85 (have 1.97.1) | Fine, but another reason to add `rust-toolchain.toml` so CI doesn't drift below it |
| refinery 0.9 `rusqlite` feature | rusqlite 0.40 | Upgrade together; `embed_migrations!` path per DB file (`migrations/`, `memory_migrations/`, `graph_migrations/`) |
| reqwest 0.12 `rustls-tls`, `default-features = false` | tokio 1 (timer for `connect_timeout`) | `connect_timeout` requires a Tokio timer context — already have it in all three binaries |

## Sources

- Context7 `/anush008/fastembed-rs` (MEDIUM) — `TextEmbedding::try_new`, `TextInitOptions`, BGE-small default, 384-dim
- Context7 `/rusqlite/rusqlite` (MEDIUM) — `bundled` + FTS5 flag, `busy_timeout`/`busy_handler`/`pragma_update` API, 0.40.x line
- Context7 `/seanmonstar/reqwest` (MEDIUM) — `ClientBuilder` timeout/retry API surface
- Context7 `/tokio-rs/tokio` (MEDIUM) — `UnixListener`, `signal::unix` SIGTERM pattern
- Context7 `/ratatui/ratatui` (MEDIUM) — 0.29→0.30 modularization, `init`/`restore` + panic hook, `ratatui-crossterm` version pairing
- Context7 `/websites/rs_rmcp_rmcp` (MEDIUM) — `serve_server`, `ServiceExt::serve`, `tool_router`/`tool`, stdio transport
- crates.io registry API (version numbers: tokio 1.53.1, reqwest 0.13.5, rusqlite 0.40.2, fastembed 7.1.0, sqlite-vec 0.1.9 stable, rmcp 3.5.0, ratatui 0.30.2, crossterm 0.29.0, keytap 0.4.0, global-hotkey 0.8.0, xcap 0.9.8, active-win-pos-rs 0.11.0, arboard 3.6.1, refinery 0.9.2, zerocopy 0.8.59, fs2 0.4.3)
- Web research (LOW, cross-checked where possible): xcap maintenance + screencapturekit-rs 11 alternative; keytap-vs-rdev comparison + chord feature; sqlite-vec + rusqlite integration pattern; async-openai/rig vs hand-rolled assessment; refinery-vs-sqlx threads; global-hotkey main-thread constraint

---
*Stack research for: rustwatch hardening milestone (capture reliability + screenshot triggers + multi-provider LLM + local embeddings + dashboard/Q&A)*
*Researched: 2026-10-02*
*Note on confidence: version numbers are registry facts re-verifiable with `cargo search`/`cargo update --dry-run`; MEDIUM marks API-behavior claims from docs; LOW marks ecosystem-judgment claims from web sources. The two version jumps needing phase-level verification are fastembed 4→7 and rmcp 0.3→3.5 (both span breaking API changes).*
