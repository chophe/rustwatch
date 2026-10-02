# Project Research Summary

**Project:** rustwatch — local-first activity memory / personal productivity tracker (macOS, Rust, brownfield hardening)
**Domain:** Automatic time-tracking + LLM-classified activity memory + on-device vector/graph recall (CLI/TUI/MCP)
**Researched:** 2026-10-02
**Confidence:** HIGH (direction) / MEDIUM (two major version jumps need phase-level verification)

## Executive Summary

rustwatch is a local-first personal activity-memory tool: a macOS daemon captures keyboard context, window focus, clipboard, and screenshots into SQLite; a batch pipeline classifies segments via LLM (OpenAI/Anthropic/local) and ingests them into an on-device vector + FTS5 + graph memory; CLI, TUI dashboard, and MCP server expose search, Ask/Q&A, scoring, and digest. Every comparable system (Rewind, Retrace, 2ndm1nd, Mnemosyne, sqlite-rag MCP servers) converges on the same shape — dumb-fast capture → append-only ledger → batch cognition → dual-index memory → thin frontends — and rustwatch's existing 7-crate workspace already matches it. This is a hardening milestone, not a rewrite: keep the skeleton, fix the seams inside it.

The recommended approach is dependency-ordered hardening: ledger/paths/config honesty first, then capture reliability, then privacy enforcement, then classification robustness + local-provider fallback, then real embeddings + hybrid search, then thin surfaces (Ask, TUI dashboard, MCP) last. The single highest-leverage engineering is making memory real — today search is fake (hash "embeddings" with no semantics, FTS table written but never queried, graph `hops` used as SQL LIMIT), so every intelligence feature (Ask, score, digest, MCP) is presentation over a hollow retrieval layer. Frontends built before retrieval is fixed will bake in whatever the stores happen to return today.

The key risks are trust-destroying, not cosmetic: (1) privacy controls that don't cover the highest-risk paths — `exclude_apps` never checked on the keyboard loop, raw keystroke buffers handed to MCP/export/TUI unredacted, single-regex egress-only redaction leaving secrets at rest; (2) silent capture data loss — `try_lock`-and-drop writer plus swallowed write errors plus Unicode panics plus no graceful shutdown losing every session tail; (3) corrupted analysis mapping — classifier fan-out attaching all 50 segments to every activity, unvalidated LLM timestamps accepted as fact. All three are ship-blockers for a keystroke+screenshot recorder and must land in the first phases with automated regression tests, not manual verification. See `PITFALLS.md` for file:line evidence and `ARCHITECTURE.md` §Suggested Build Order for the dependency chain.

## Key Findings

### Recommended Stack

Keep the proven core (tokio 1.53, reqwest 0.12 + rustls, ratatui 0.29, keytap 0.4, xcap 0.9.8) and make five surgical changes: rusqlite 0.32→0.40 `bundled` (FTS5 + 5s busy_timeout kills the daemon-vs-CLI `database is locked` class), fastembed 4.9→7 **default-on** (384-dim BGE-small matches schema; kills the silent `DefaultHasher` fallback lie), sqlite-vec 0.1.9 + zerocopy 0.8 (ANN `vec0` KNN in the existing `memory.db`, no new service), refinery 0.8→0.9 extended to all three DBs (versioned migrations end schema drift), and rmcp 0.3→3.5 adopted for real (official SDK `tool_router` replaces the die-on-any-malformed-line hand-rolled JSON-RPC loop). Add fs2 0.4 (single-instance advisory lock), pin the toolchain in `rust-toolchain.toml`, unify on crossterm 0.28 (defer ratatui 0.30 migration — zero dashboard benefit), and delete or `workspace.exclude` the unbuildable `rustwatch-memory-backends` orphan (LanceDB/SurrealDB weight for a second vector project hiding inside hardening). Full version matrix and per-crate upgrade commands in `STACK.md`.

**Core technologies:**
- tokio 1.53 (keep) — async runtime, Unix-socket IPC, 5-min screenshot tick, SIGTERM handling; zero change
- rusqlite 0.40 `bundled` (upgrade) — all three SQLite files; FTS5 + busy_timeout + WAL + synchronous=NORMAL on every connection via one shared `open_*` helper
- sqlite-vec 0.1.9 + zerocopy 0.8 (new) — cosine ANN `vec0` table inside `memory.db`; replaces O(N) full-scan + in-Rust cosine loop
- fastembed 7 default-on (upgrade + behavior change) — real local ONNX embeddings; loud startup error when no embedder initializes, never silent hash fallback
- reqwest 0.12 (keep, configure) — add `.timeout(60s)` + `connect_timeout(10s)` + bounded 429/5xx retry; do NOT chase 0.13 during hardening
- refinery 0.9 (upgrade + extend) — one migration runner per DB file; backfills vec0 tables, embedding-model column, UNIQUE constraints
- rmcp 3.5 `server + transport-io` (adopt) — 5-tool MCP migration; JSON-RPC `error` objects, server survives bad input
- ratatui 0.29 + crossterm 0.28 (keep/unify) — `init()/restore()` panic-hook terminal cleanup; defer 0.30 modular-import migration
- keytap 0.4 `chord` (keep, use) — hotkey-annotate trigger on the existing tap, zero new deps/permissions
- fs2 0.4 (new) — single-instance flock on pid file; second daemon exits with a clear message

### Expected Features

The MVP is *reliable memory you can interrogate*, not a full tracker suite. Table stakes (every credible tracker ships these): silent background capture, idle detection (currently missing — scores are fiction without it), sleep/wake survival, timeline view as the TUI default, time-by-app/category aggregation, keyword search (wire up the existing-but-unused FTS5), visible local-first UX, capture-time per-app exclusions, pause/private mode, macOS TCC onboarding with per-grant degradation, launchd auto-start + `doctor` check, JSON/CSV export, and retention/storage budget (screenshots grow unboundedly today — disk-full is the predictable #1 support ticket). Differentiators that justify the project: natural-language Ask/Q&A gated behind retrieval-confidence thresholds, LLM auto-classification with local-provider fallback (converts fragility into the headline advantage), multi-provider trait (not flag) design, real hybrid + graph memory search, screenshot *recall* (not just capture), hotkey-annotate journaling, transparent user-weighted productivity score, terminal-native digest, scoped MCP agent tools, and verified secret-redaction as a marketable trust property. Full competitor matrix (ActivityWatch/RescueTime/Timing/Qbserve/Pieces/Screenpipe) and the Ask←hybrid←embeddings←classification←capture dependency chain in `FEATURES.md`.

**Must have (table stakes):**
- Reliable capture + sleep survival + non-ASCII panic fixes — without this nothing else matters (Core Value)
- Permissions detection + onboarding — else installs silently capture nothing
- Idle detection (CGEventSource) — else all downstream numbers are wrong
- Redaction gate + capture-time exclusions + pause — else the tool is unsafe to run
- Segment folding hardened (exists — pure `SegmentGrouper`, keep it deterministic)
- Classification working (one cloud provider + local Ollama fallback)
- Keyword search via existing FTS5 wired up — minimum viable recall
- Timeline + time-by-app/category TUI dashboard as default view
- Config honesty (every field wired or removed) — dead knobs destroy trust in a privacy tool
- Export (JSON/CSV) incl. memory scope

**Should have (competitive):**
- Real embeddings + hybrid (vector + FTS5 + RRF) search — highest-leverage engineering in the project
- Ask Q&A (CLI → TUI → MCP in that order, gated on retrieval eval set)
- Productivity score + daily digest (after 2+ weeks stable classification; transparent multi-signal composite, never LLM-category-only)
- Hotkey-annotate to current segment (<200ms to visible)
- MCP hardening + scoped permissions (agents shouldn't read banking titles either)
- Retention pruning policy (age + size cap surfaced in `status`)

**Defer (v2+):**
- True multi-hop graph traversal — needs proven single-hop relevance first
- Rule-based project tagging (Timing-style keyword rules capture 80% when demand is validated)
- Auto-billing/invoicing, cross-device/sync, web dashboard, teams surveillance, distraction blocking — out of scope or anti-features (see `FEATURES.md` anti-feature table: cloud screenshots by default, verbatim keystroke retention, manual timers, sub-second polling)

### Architecture Approach

Keep the 7-crate workspace — crates are layers, not features: `core` (vocabulary + ledger + IPC, depends on nothing) → `capture` (OS sensors, emits events, never touches DB) → `daemon` (owns threads + single writer + socket loop) → `analyze` (batch cognition, takes `&Store`, opens nothing) → `memory` (`MemoryEngine` facade: embed → vector + FTS5 + graph + RRF fusion) → `cli` / `mcp` (thin binaries, open stores once, read-only handles). Five consensus patterns from 5+ independent implementations: capture→ledger→batch-cognition pipeline (LLM never in hot path); provider-seam trait + string-keyed factory with injectable `&dyn` pipeline (Ollama = OpenAI-compat base-URL override, zero new code); hybrid vector+FTS5 fused with RRF k=60 with lexical-always-live degradation; daemon-owns-writer with WAL N-readers + `busy_timeout` everywhere; read-only TUI tick loop + stdio MCP with stderr-only logging. State lives in exactly three places (SQLite files, daemon-local Arcs + `write_errors` counter, `SegmentGrouper` fold state) — no app state container. Full diagram, data flows, scaling table (brute-force fine <10k chunks → vec0 → monthly partitions), and anti-patterns in `ARCHITECTURE.md`.

**Major components:**
1. Capture sensors + scheduler (keytap threads, focus poll, xcap; interval + window-change + hotkey triggers) — typed events onto mpsc, no I/O/network/inference
2. Event ledger (`Store`, refinery migrations, WAL, single writer) — append-only durability; the only cross-process state
3. Segmenter / sessionizer (pure `on_event → Option<Segment>` + `flush()`) — deterministic fold, unit-testable, flushed at shutdown
4. Privacy gate (`Redactor` in core, consulted by capture AND analysis) — regex + exclusions + char-boundary-safe truncation at the write path
5. Classifier provider seam (`ActivityClassifier` trait + `build_provider` factory) — OpenAI + Anthropic native, Ollama via base-URL; pipeline injectable for mock tests
6. MemoryEngine (embedder trait + chunks/FTS5/vec0 + graph + RRF fusion) — `open/ingest/rebuild/search` facade; hybrid query with provenance
7. Thin frontends (CLI verbs, ratatui tick-poll dashboard, stdio MCP) — read-only WAL clients; never own the DB

### Critical Pitfalls

Twelve pitfalls with file:line evidence (HIGH confidence — firsthand audit of all ~3,185 lines; domain-general claims capped at MEDIUM, no external corroboration available). Top 5 ship-blockers, all Phase 1–2:

1. **Exclusion bypass on keyboard path** — `exclude_apps` checked only in focus loop (`macos.rs:191`), keyboard loop never consults it; password-manager keystrokes stored plaintext. Fix: thread exclusions into `run_keyboard_loop`, per-path regression tests, prefer bundle-id matching.
2. **Raw keystroke buffers to MCP/export/TUI** — `rustwatch-mcp/main.rs:90-107` serializes full `text_buffer` with zero redaction; same in `export`. Fix: move `Redactor` to core, enforce at every external boundary, raw-off-by-default for MCP, `0700/0600` file modes.
3. **Single-regex egress-only redaction** — one `sk-` pattern, applied only at classify time after plaintext persistence. Fix: broad defaults (AWS/JWT/PEM/bearer/high-entropy), redact at write path, decouple char cap from `memory.chunk_max_chars`, secret-corpus fixture test.
4. **Fake vector search presented with scores** — `DefaultHasher` bag-of-words (not toolchain-stable, no version marker) with silent fastembed fallback; CLI "Score" column is theater. Fix: fastembed default-on, loud absence signal, `embedding_model` version column, related-vs-unrelated score-separation test.
5. **Classifier fan-out corrupts segment↔activity mapping** — whole 50-segment batch attached to every activity (`classifier.rs:60`); cascades into ingest picking arbitrary `segment_ids.first()`. Fix: model returns segment ids per activity, union-coverage validation, fail-retryable batches, real `activity_segments` join table replacing `LIKE`-on-JSON.

Plus: daemon `try_lock`-and-drop + `let _ =` write errors with pre-incremented success counter (soak-test with `tail` running); unvalidated LLM output as time-series fact (per-label salvage, timestamp clamping, timeouts, 429/5xx backoff); prompt injection via window titles (sentinel delimiters + output range validation); premature scoring measuring classifiability (multi-signal transparent composite, user weights, neutral bucket — Phase 4 only); screenshot retention/scope/linkage rot (prune policy, actual-scope propagation, `segment_id` wiring, kill-or-wire `send_screenshots_to_llm`); 13 dead config fields that lie (wire-or-remove gate + `#[serde(default)]` + CI dead-field audit); Unicode byte-slice panics + unreachable shutdown flush (char-boundary truncation, SIGTERM handler, single-instance lock). Full "Looks Done But Isn't" 14-item checklist and recovery table in `PITFALLS.md`.

## Implications for Roadmap

Suggested 5-phase structure follows the dependency chain in `ARCHITECTURE.md` §Suggested Build Order and the pitfall-to-phase map in `PITFALLS.md`. Rule: nothing downstream is trustworthy until the layer below is.

### Phase 1: Capture reliability + daemon hardening
**Rationale:** Capture is the hot path and the stated Core Value — if it silently drops data, nothing else matters, and every later phase builds on the ledger.
**Delivers:** Loss-free writer (blocking `lock()`, no `let _ =`, `write_errors` counter), `busy_timeout(5s)` + WAL + `synchronous=NORMAL` on all three DBs via shared helper, char-boundary-safe truncation (ends 3 known panics), SIGTERM/SIGINT graceful shutdown (flush grouper, checkpoint WAL, remove pid), fs2 single-instance lock, Unix-socket hardening (0600, peer-UID check, 16 MiB frame cap, timeouts), 3 screenshot triggers (5-min interval + focus-change + hotkey chord) with retention prune (age + size cap in `status`) and honest scope labels, real TCC permission probing + capture-thread health in `DaemonState`, sleep/wake gap-tolerant segmentation, launchd plist review (backoff, log paths, API-key env note).
**Addresses:** Reliable capture, sleep survival, permissions onboarding, screenshot triggers, retention, auto-start `doctor` (FEATURES.md P1).
**Avoids:** Pitfalls 6 (dropped events), 12 (Unicode/shutdown), 10a/b (retention/scope), socket/PID security mistakes.

### Phase 2: Privacy enforcement + config honesty gate
**Rationale:** Must land before any new data flows to cloud LLMs or new agents read the stores — blocks provider and surface work; also gates every future config flag from rotting the same way.
**Delivers:** `exclude_apps` enforced on keyboard + focus paths (bundle-id matching, per-path regression tests), `Redactor` moved to core with broadened defaults applied at write path AND every read boundary (MCP/export/TUI), `purge --before/--app` primitives + documented threat model (encryption-at-rest decision recorded), wire-or-remove all 13 dead fields + `#[serde(default)]` + CI dead-field audit + startup unknown-key warnings, `0700` data root, export permission tightening, privacy panel UX (sources on/off, verified exclusions, egress log).
**Addresses:** Exclusions, pause/private mode, redaction gate, export trust, config honesty (FEATURES.md P1).
**Avoids:** Pitfalls 1 (exclusion bypass), 2 (raw buffers to MCP/export), 3 (narrow egress-only redaction), 11 (dead config lies).

### Phase 3: Classification robustness + multi-provider
**Rationale:** Classification is the fragility point (cost/latency/offline) and feeds everything downstream — score, digest, memory ingest quality all inherit its errors.
**Delivers:** Injectable `&dyn ActivityClassifier` pipeline + `build_provider` factory, Anthropic impl (own wire format, version header review), Ollama/llama.cpp OpenAI-compat preset (longer timeouts, no `response_format` assumption), per-provider default models + startup model↔provider validation, `timeout` + bounded retry/backoff, per-label defensive parsing (salvage good, quarantine bad — never all-or-nothing 50-segment loss), timestamp clamping to batch range + confidence [0,1] validation, prompt-injection sentinels (untrusted title/buffer delimited as data-only) + output range checks, segment↔activity mapping fix (model-returned ids, subset/union assertions) + `activity_segments` join-table migration replacing `LIKE`-on-JSON, idle detection (CGEventSource, fold idle out of segments).
**Addresses:** Classification (cloud + local fallback), idle detection (FEATURES.md P1 — required before scoring means anything).
**Avoids:** Pitfalls 5 (fan-out), 7 (unvalidated LLM output), 8 (prompt injection), provider/timeout integration gotchas.
**Uses:** reqwest configured client, provider-seam pattern (STACK.md / ARCHITECTURE.md Pattern 2).

### Phase 4: Real memory — embeddings + hybrid + graph
**Rationale:** The highest-leverage engineering: turns hollow retrieval (hash vectors, unused FTS, LIMIT-as-hops) into the foundation Ask/score/MCP all present. Structurally independent of Phase 3 except for ingest text quality — can overlap once Phase 2 privacy is in.
**Delivers:** fastembed 7 default-on (model prefetch/cache under `~/.rustwatch/models`, progress + offline story, active-embedder surfaced in `status`), `embedding_model` + version column + deterministic chunk ids (hash of segment_id + offset) + UNIQUE constraints via refinery migrations, sqlite-vec `vec0` KNN + FTS5 actually queried + RRF k=60 fusion replacing ad-hoc `+0.2`/max-score merge (lexical-always-live degradation with explicit warning), ingest dedupe (high-water mark replacing 7-day re-scan) + shadow-build-then-swap `--rebuild`, real graph BFS to depth `hops` + entity extraction at ingest, latency benchmarks at 10k/50k chunks, related-vs-unrelated score-separation eval.
**Addresses:** Real embeddings + hybrid search, graph traversal single-hop (FEATURES.md P2; multi-hop stays v2+).
**Avoids:** Pitfalls 4 (fake embeddings), full-scan/append-duplicate/rebuild performance traps, `expand_around_apps` correctness bug.
**Uses:** sqlite-vec + zerocopy + fastembed 7 + refinery-for-3-DBs (STACK.md); RRF hybrid pattern (ARCHITECTURE.md Pattern 3).

### Phase 5: Surfaces — Ask, dashboard, MCP, score + digest
**Rationale:** Frontends are thin readers over Phases 1–4; building them last means they present trustworthy data. Only structural risks are TUI tick cost and MCP protocol correctness.
**Delivers:** rmcp 3.5 migration (5 tools, typed `#[tool]` params, JSON-RPC `error` objects, malformed-input conformance tests, stderr-only logging, scoped permissions + raw-off-by-default), CLI `ask` → TUI search/ask box → MCP search order with retrieval-confidence gating, ratatui read-only dashboard (timeline default, time-by-app/category rollup with graceful unknown-category state, indexed tick queries <50 ms, working `[r]`, surfaced errors), productivity score as transparent multi-signal composite (typing rate, fragmentation, annotations, topic continuity + user-tunable weights + neutral unclassified bucket + prompt-reword stability check), terminal-native digest (CLI + TUI card), hotkey-annotate pinned to current segment, screenshot `segment_id` linkage for visual timeline recall.
**Addresses:** Timeline dashboard, Ask Q&A, score + digest, hotkey-annotate, MCP agent tools (FEATURES.md P2).
**Avoids:** Pitfalls 9 (score measures classifiability), 10c (segment linkage), 2 (MCP redaction boundary), debt-table MCP die-on-input, TUI polling/dead-key UX traps.
**Uses:** rmcp 3.5 + ratatui 0.29 + keytap chord (STACK.md); read-only TUI + stdio MCP pattern (ARCHITECTURE.md Pattern 5).

### Phase Ordering Rationale

- **Ledger → capture → privacy → cognition → memory → surfaces** is the dependency order: each stage's correctness is assumed by the next (Ask needs hybrid needs embeddings needs classification needs capture; score needs idle + trustworthy classification; timeline needs segment linkage).
- **Privacy (Phase 2) blocks providers (Phase 3) and surfaces (Phase 5)** — no new egress or read path before the redaction/exclusion choke points exist; this is a trust-contract constraint, not a preference.
- **Config honesty gates everything after Phase 2** — new flags for embeddings/scoring/retention rot the same way unless the wire-or-remove discipline + CI audit lands first.
- **Score (Phase 5) waits for classification (Phase 3)** — building the headline metric on unfixed fan-out/hallucinated-timestamp/injection errors bakes them into the number users judge the product by.
- **Memory (Phase 4) can overlap Phase 3** once Phase 2 is done — only ingest text quality couples them, not structure; respectful parallelization opportunity for two workstreams.
- **Anti-scope excluded throughout:** cloud screenshot upload, verbatim keystroke retention, blocking/focus enforcement, team surveillance, manual timers, web dashboard, Windows/Linux, billing suite — each has a documented cheaper/trust-safer alternative in FEATURES.md.

### Research Flags

Phases likely needing deeper research during planning (`/gsd-plan-phase --research-phase <N>`):
- **Phase 1:** macOS TCC permission-probing APIs (real per-grant detection replaces hardcoded all-false; exact CGEventSource idle-time + power-notification APIs; bundle-id-from-PID resolution — STACK.md flags active-win-pos-rs gap as LOW-confidence workaround).
- **Phase 3:** Ollama/llama.cpp OpenAI-compat divergences (timeout tuning, `response_format` support matrix per local server) + Anthropic version-header/prompt-format differences.
- **Phase 4:** fastembed 4→7 breaking API migration + sqlite-vec `sqlite3_auto_extension` registration order with rusqlite `bundled` (both compile SQLite from source — load-bearing pairing) + first-run model-download offline story.
- **Phase 5:** rmcp 0.3→3.5 migration (3 major versions; `tool_router`/`serve` patterns, schemars version pairing) + scoring-rubric design (prospective, no codebase evidence — validate against hand-labeled days).

Phases with standard patterns (skip research-phase):
- **Phase 2 (mostly):** redaction-at-write-path, exclusion enforcement, `#[serde(default)]` + migration discipline, Unix file-mode hardening — all well-documented Rust/SQLite idioms.
- **Phase 5 (TUI/CLI portions):** ratatui `Tui`+`EventHandler` tick loop, clap subcommand-per-verb, RRF k=60 fusion constant — consensus patterns with official docs.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | MEDIUM | Version numbers are registry facts (re-verifiable via `cargo search`); API-behavior claims from Context7 official docs; ecosystem judgments (keytap-vs-rdev, xcap-vs-screencapturekit, refinery-vs-sqlx) cross-checked but web-corroboration limited. Two jumps need phase verification: fastembed 4→7 and rmcp 0.3→3.5 (breaking APIs). |
| Features | HIGH | Cross-checked across ActivityWatch/RescueTime/Rize/Timing/Qbserve/Pieces/Screenpipe sources + PROJECT.md active requirements; MVP cut grounded in actual codebase state; competitor matrix agrees on table-stakes set. |
| Architecture | HIGH | Patterns confirmed across 5+ independent implementations (Rewind teardown, Retrace, 2ndm1nd, Mnemosyne, sqlite-rag MCP servers) + rustwatch's own codebase map; suggested build order matches both consensus and in-tree dependency reality. |
| Pitfalls | HIGH (codebase-evidenced) / MEDIUM (domain-general) | Pitfalls 1–8, 10–12 are firsthand reads of all ~3,185 lines with exact file:line citations in `.planning/codebase/CONCERNS.md`; independently corroborated by author's `docs/TESTING_PLAN.md` Known Risks. Pitfall 9 (scoring) is prospective — no implementation exists yet. No external post-mortems consulted (web search unavailable). |

**Overall confidence:** HIGH for direction and phase ordering; MEDIUM for the two major version migrations and TCC/bundle-id macOS API details flagged above.

### Gaps to Address

- **fastembed 4→7 + rmcp 0.3→3.5 breaking changes:** isolate with `cargo update -p <crate>` + `cargo check -p <crate>` per STACK.md; budget spike time in Phase 3/4/5 plans; fallback pins documented (stay on rusqlite 0.32 + still add busy_timeout/WAL if 0.40 bites).
- **macOS API specifics (TCC probing, idle time, bundle-id from PID):** resolve during Phase 1 planning with `--research-phase`; real probing replaces hardcoded `permissions()` before daemon hardening is declared done.
- **Encryption-at-rest decision:** SQLCipher vs. file-level vs. documented-no-encryption threat model — decide in Phase 2 (currently plaintext DBs + screenshots; affects storage design, so don't defer past privacy phase).
- **Scoring rubric validation:** no ground truth exists; plan Phase 5 scoring around hand-labeled-day agreement + prompt-reword stability checks, transparent composite with user weights — never a black-box ratio.
- **Vision/screenshots-to-LLM:** explicitly out of this milestone (privacy threat model hasn't approved the egress path); `send_screenshots_to_llm` stays dead→deleted or explicit-opt-in; revisit with screencapturekit-rs only if a future phase approves vision.
- **1M+ chunk scaling:** vec0 covers the milestone; monthly partitioning + embedding-cache-on-disk are documented next steps, not this-milestone work.

## Sources

### Primary (HIGH confidence)
- rustwatch own codebase map: `.planning/codebase/ARCHITECTURE.md`, `CONCERNS.md`, `INTEGRATIONS.md` (2026-10-02, every `.rs` file read, build/clippy attempts, dead-code greps) — ground truth for pitfalls, integration surfaces, current Store/SegmentGrouper/MemoryEngine/IPC
- PROJECT.md active requirements + author's `docs/TESTING_PLAN.md` Known Risks (independently corroborates byte-slice panics, hops-as-LIMIT, timestamp fallback, LIKE-join)
- Timing exclusions/idle/preferences docs (timingapp.com/help) — exclusions tab, idle handling shape
- Ratatui official docs (async event stream, Tui template) + Rewind.ai teardown (ScreenCaptureKit + FTS + chunk schema) + Retrace repo (capture→caption→embed→persist, FTS5+hybrid, read-only MCP) + 2ndm1nd (capture/brain split, no-LLM-in-capture) + Mem0 docs (3-store split, multi-signal retrieval) — architecture consensus
- crates.io registry API (exact versions: tokio 1.53.1, rusqlite 0.40.2, fastembed 7.1.0, sqlite-vec 0.1.9, rmcp 3.5.0, ratatui 0.30.2 / keep 0.29, keytap 0.4.0, xcap 0.9.8, refinery 0.9.2, zerocopy 0.8.59, fs2 0.4.3)

### Secondary (MEDIUM confidence)
- Context7 official-docs lookups: `/anush008/fastembed-rs`, `/rusqlite/rusqlite`, `/seanmonstar/reqwest`, `/tokio-rs/tokio`, `/ratatui/ratatui`, `/websites/rs_rmcp_rmcp` — API surfaces for the six load-bearing deps
- ActivityWatch vs RescueTime/ManicTime comparison, RescueTime/Rize scoring coverage, Qbserve Mac docs, Pieces LTM/MCP coverage, Screenpipe vs Rewind positioning (Rewind shutdown Dec 2025 corroborated) — feature landscape
- sqlite-vec hybrid guides (Alex Garcia; Jez's blog; RRF k=60), sqlite-rag-mcp (lexical-fallback contract), vstash paper (BEIR RRF evals), llm-trait/llm-unified + Ollama base-URL idiom — retrieval + provider-seam patterns
- Daemon/IPC precedents (mosaico, synwire-daemon, toki), MUSE.md-scale capture analyses — single-writer UDS daemon shape

### Tertiary (LOW confidence, needs validation)
- xcap maintenance freshness + screencapturekit-rs 11 alternative; keytap-vs-rdev Sonoma specifics; async-openai/rig-vs-hand-rolled judgment; global-hotkey main-thread constraint; local-LLM `response_format` support matrix — web-sourced, cross-checked where possible, flagged for phase-level verification

---
*Research completed: 2026-10-02*
*Ready for roadmap: yes*
