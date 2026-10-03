# Walking Skeleton — rustwatch Phase 1

**Phase:** 1 — Reliable Capture & System Foundation
**Generated:** 2026-10-03

## Capability Proven End-to-End

A synthetic keystroke event travels from a capture channel through the daemon's single blocking writer into `~/.rustwatch/rustwatch.db`, with write errors counted (never swallowed) and visible in the `rustwatch status` health banner.

## Architectural Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Runtime | Rust workspace, tokio (daemon/CLI IPC) + `std::thread` capture loops | keytap tap and xcap are blocking; event send must stay non-blocking |
| Data layer | rusqlite 0.32 `bundled`, WAL + `synchronous=NORMAL` + 5 s `busy_timeout`; refinery migrations (`V1` + new `V2__notes_idle`) | Single writer owns the Store; WAL alone does not cover writer contention |
| Loss contract | Blocking writer + bounded 10k in-memory retry queue (SQLITE_BUSY/IO only, 500 ms timer, drop-oldest + counter); any task death exits nonzero for launchd restart | Middle ground per D-03/D-04/D-06; pure-blocking and disk-spool rejected |
| Idle source | `CGEventSource.secondsSinceLastEventType` via core-graphics (macOS target dep) | Hardware idle; self-inference is blind exactly when it must work (D-17) |
| Hotkey | Chord-on-tap `Ctrl+Shift+Space` (config knob), shoot-first-prompt-second via `rustwatch annotate` | Zero new deps/permissions, sub-ms latency; daemon is headless so the prompt is CLI-side (D-11) |
| Single-instance | `fs2` advisory exclusive lock on pid file, lock-then-bind ordering | Crash-safe, race-free, clear second-instance message (exit 2) |
| Config | File-first `~/.rustwatch/config.toml`; `#[serde(default)]` everywhere, unknown keys warn; dead fields deleted (DataPaths is the sole path authority) | Strict parsing bricked installs; deleted keys stay backward compatible via warn |
| Permissions | Three-state probes (AXIsProcessTrusted / tap-NULL probe / CGPreflightScreenCaptureAccess), per-path degradation, prompt-undetermined-once | Daemon stays up degraded; re-prompt loops risk TCC throttling |
| Deployment target | launchd `com.rustwatch.plist` with `SuccessfulExit=false` + `ThrottleInterval 30`, logs under `~/Library/Logs/rustwatch/` | Pairs with the exit-nonzero crash contract; /tmp logs are a symlink risk |
| Directory layout | Existing 7-crate DAG rooted at rustwatch-core; no new crates (2 new deps only) | New logic lands in existing layers per the responsibility map |

## Stack Touched in Phase 1

- [x] Build — `cargo check/test --workspace` green on macOS (Linux CI cannot cover TCC/CGEventSource/xcap paths; manual human-checks cover them)
- [x] Capture — keyboard + focus + three screenshot triggers emitting typed events on one mpsc channel
- [x] Database — real reads AND writes: events/segments/screenshots/notes rows round-trip through the writer
- [x] CLI — `status` banner, `permissions`, `doctor`, `annotate` wired to daemon state
- [x] Deployment — launchd install + KeepAlive restart verified by kill -9 vs clean stop

## Planner Discretion Decisions (locked for executors)

- Hotkey chord: `Ctrl+Shift+Space`, knobs `hotkey_enabled` / `hotkey_chord`; `KeyState` gains ctrl tracking.
- Interval ticks route through the same mpsc channel as capture events (uniform loss accounting).
- Dead-field list: DELETE `data.*`, `analyze.vision_model`, `analyze.batch_interval_minutes`, `memory.vector_backend/graph_backend/surreal_engine`, `ui.tui_enabled/progress_bars`, `privacy.send_screenshots_to_llm`, `capture.accessibility_poll_ms` (+ `Config::paths()`); ADD `screenshot_interval_secs`, `min_interval_secs`, `idle_start_secs`, `idle_end_sustained_secs`, `hotkey_enabled`, `hotkey_chord`, `[permissions] prompted_*`.
- Both crate adds (`fs2`, `core-graphics`) happen in 01-01 behind one blocking human checkpoint; 01-02/01-03 fetch nothing.
- Roadmap adjustment: single-instance lock + full config audit live in 01-01 (not 01-03) so the schema/wire types land once and 01-02/01-03 only consume them; 01-03 keeps permissions + doctor/status + launchd.

## Out of Scope (Deferred to Later Slices)

- Privacy enforcement (Phase 2: write-path redaction, exclusion of screenshots, pause mode, screenshot egress policy)
- Classification providers and real embeddings (Phase 3)
- FTS5/hybrid search, export, screenshot retention (Phase 4)
- CLI Ask, TUI dashboard/score/digest, MCP tools (Phase 5)
- Windows/Linux capture backends, cloud sync, encryption at rest

## Subsequent Slice Plan

- Phase 2: secrets redacted at write path + every read boundary; exclusions enforced on screenshots; pause mode
- Phase 3: OpenAI/Anthropic/local classification via provider seam; default-on local embeddings with loud failure
- Phase 4: FTS5 + hybrid search that actually queries; export; bounded screenshot retention
- Phase 5: CLI Ask with confidence gating; TUI timeline/score/digest; scoped MCP tools
