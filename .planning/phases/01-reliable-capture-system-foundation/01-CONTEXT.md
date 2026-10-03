# Phase 1: Reliable Capture & System Foundation - Context

**Gathered:** 2026-10-03
**Status:** Ready for planning

## Phase Boundary

The daemon captures keyboard context, app/window state, and screenshots continuously without silent loss, survives sleep/wake, reports macOS permission states honestly with onboarding guidance, and every `config.toml` knob is either wired or removed.

Covers requirements **CAPT-01 … CAPT-07, SYS-01, SYS-02** (see `.planning/REQUIREMENTS.md`). This phase is about making the existing engine trustworthy — not adding capabilities. Privacy enforcement (Phase 2), classification (Phase 3), search/export/retention (Phase 4), and Ask/dashboard/MCP surfaces (Phase 5) are out of scope.

Plan slots defined in ROADMAP.md: `01-01` ledger + daemon hardening, `01-02` screenshot triggers + idle, `01-03` permissions + config honesty + launchd doctor.

## Implementation Decisions

### Loss surfacing

- **D-01:** Failed writes are surfaced in two places: `error!` logs (launchd stderr log) **and** counters in `rustwatch status` / the TUI. Log-only was explicitly rejected. — **Reversibility:** reversible
- **D-02:** Root fix for event loss is removing the `try_lock()` path in the writer task (`crates/rustwatch-daemon/src/main.rs:62`) — the writer awaits the lock instead of dropping events under IPC contention.
- **D-03:** Beyond surfacing, use a **bounded in-memory retry queue**. A pure blocking writer (no queue) and a disk spool/replay ledger were both considered; the queue is the chosen middle. — **Reversibility:** costly — it becomes the daemon's persistence contract once plans depend on its semantics
- **D-04:** Queue semantics: retry only `SQLITE_BUSY` / IO errors on a timer (~500 ms); non-retryable errors (schema drift, constraint) are logged + counted immediately. Cap ~10k events; on overflow **drop the oldest** and increment `dropped_events` so loss stays visible. Never unbounded.
- **D-05:** Shutdown: on SIGTERM stop capture, flush `SegmentGrouper`, drain the retry queue with a **2 s deadline**, then count the remainder as dropped and exit cleanly. Long drains (30 s) and no-flush were rejected.
- **D-06:** **Any task death → the daemon process exits non-zero** and launchd `KeepAlive` restarts it. This applies to (a) a writer-task panic and (b) a capture-thread panic (keytap / focus poll) — the same rule, no partial-capture zombie state. A supervisor that restarts the writer in-process was explicitly rejected. — **Reversibility:** costly — launchd behavior and `stop`/`start` semantics are built around this
- **D-07:** `status`/TUI show a **healthy/degraded banner** (`capture: healthy` vs `capture: degraded (3 write errors, 12 queued)`), green/yellow/red; detailed counters print below **only when nonzero**. Always-print-everything was rejected.
- **D-08:** `doctor` **warns** on nonzero `write_errors`/`dropped` (history), and fails (exit 1) only on live breakage: launchd not installed, daemon not running, DB not writable. Counters permanently failing doctor was rejected.

### Screenshot triggers

- **D-09:** Window-change screenshots are throttled with a **2 s cooldown plus a burst cap** (≤3 shots per 10 s window), configurable as `min_interval_secs` under `[capture]`. Shooting every transition and a 3 s stability debounce were rejected. — **Reversibility:** reversible
- **D-10:** The hotkey annotation note is stored in a **new `notes` table** (id, ts, note, screenshot_id?) written by the daemon — queryable by date for later digest/status use. A `CaptureEventKind::Annotation` variant and a flat daily log file were rejected.
- **D-11:** Hotkey UX: **shoot first, prompt second.** The screenshot fires immediately (well under the 200 ms requirement), then a small terminal input appears — Enter saves the note, Esc closes with no note. Clipboard-as-note and marker-now/annotate-later were rejected. — **Reversibility:** reversible
- **D-12:** Across sleep/wake the interval timer **discards missed ticks** (no backlog of stale shots) and takes **one** screenshot immediately on wake. Backfilling up to 3 and doing nothing on wake were rejected.

### Permission onboarding

- **D-13:** First run **preflights** all three grants and **requests only the undetermined ones** (native TCC dialogs, once). If a permission was previously denied, the request is a no-op — so instead we **open System Settings at the correct pane** and print the URL. Never-prompt and always-request were rejected. — **Reversibility:** reversible
- **D-14:** A missing grant causes **per-path degradation while the daemon stays up**: no Input Monitoring → keyboard thread doesn't start (window titles/screenshots still work); no Screen Recording → screenshot triggers skipped, app/title still recorded. Failing hard on any missing grant was rejected — CAPT-06 requires degrading instead of capturing nothing silently.
- **D-15:** Grants are **re-probed every ~30 s and on every `status`/`doctor` call.** A newly granted path is attached live where the API permits; a lost grant stops that path; if macOS requires a process restart we say so explicitly. Startup-only checking was rejected. — **Reversibility:** reversible
- **D-16:** Guidance placement: permission state lives in the **health banner** (D-07) in `status`/TUI; `doctor` prints the per-permission fix — exact System Settings path, restart-needed note, and the `open` command. A full onboarding block on every `status` was rejected as noise.

### Idle semantics

- **D-17:** Idle is defined by **`CGEventSource.secondsSinceLastEventType`** (user + system combined) — true hardware idle, so a missing Input Monitoring grant cannot fake idleness. Threshold comes from config, default 300 s. Inferring idle from our own last-event timestamp was rejected.
- **D-18:** During idle, **capture continues normally — nothing is dropped.** The idle period is marked instead: `SegmentGrouper` closes the active segment at idle start and the resulting segment carries `idle: true`, and reports exclude idle segments rather than data. Pausing capture and inferring idle at read time were both rejected.
- **D-19:** Idle **starts after 5 min (300 s)** of no input and **ends only after 30 s of sustained activity** — a stray mouse nudge does not split the session. End-on-first-input was rejected.
- **D-20:** During idle: **automatic screenshots suppressed** (no interval, no window-change), the **global hotkey stays live**, and the interval timer restarts when idle ends. Suppressing the hotkey too, or continuing interval shots, were rejected.

### the agent's Discretion

Areas surfaced but not discussed by choice — researcher/planner decide, requirements still bind them:

- **Config honesty (SYS-01):** which of the 9 unread config fields get wired vs deleted (`analyze.vision_model`, `analyze.batch_interval_minutes`, `memory.vector_backend`, `memory.graph_backend`, `memory.surreal_engine`, `data.lance_path`, `data.surreal_path`, `ui.tui_enabled`, `ui.progress_bars`, `privacy.send_screenshots_to_llm`, plus `accessibility_poll_ms` discarded at the leaf). Rule from SYS-01: a field either works or is removed; unknown keys must warn at startup.
- **Single-instance lock mechanism (SYS-01):** the roadmap mandates a lock with a clear second-instance message; CONCERNS.md suggests an advisory file lock (`fs2`/`fd-lock`) on the pid file. Mechanism is the agent's call.
- **Exact TCC probe APIs** for the three permissions (Input Monitoring / Accessibility / Screen Recording), including which can hot-attach vs require restart — D-13/D-15 fix the *behavior*, not the API.
- **`doctor`/`status` exact check list and output layout** beyond D-07/D-08/D-16.

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase requirements & scope
- `.planning/REQUIREMENTS.md` — CAPT-01…CAPT-07, SYS-01, SYS-02 are the binding requirements for this phase; `## Out of Scope` must not be crossed
- `.planning/ROADMAP.md` § Phase 1 — goal, 5 success criteria, and the three plan slots (01-01 / 01-02 / 01-03)
- `.planning/PROJECT.md` — locked product decisions (capture = keyboard + screen; 5-min interval + window-change + hotkey annotating daily logs; local-first; macOS-only) and constraints

### Codebase evidence (bug targets this phase must fix)
- `.planning/codebase/CONCERNS.md` § Known Bugs — the concrete defect list: `try_lock` event drops, discarded insert errors, three non-ASCII panic sites, `append_text` never truncating, `stop` SIGTERM to an unvalidated PID, `start` not verifying launch, two daemons running simultaneously, screenshot scope mislabelled on fallback, screenshots never linked to segments
- `.planning/codebase/ARCHITECTURE.md` — § Anti-Patterns (swallowing errors on the persistence path, config fields with no consumer), § Architectural Constraints (sync `Store` in async tasks, cross-process SQLite with WAL only, pid file is a liveness hint not a lock)
- `.planning/codebase/STACK.md` — dependency inventory: `keytap`, `active-win-pos-rs`, `xcap`, `arboard`, launchd plist at `deploy/macos/com.rustwatch.plist`, no `[dev-dependencies]`, no tests, no CI
- `.planning/codebase/CONVENTIONS.md`, `.planning/codebase/TESTING.md` — style and testing conventions before writing new code
- `docs/TESTING_PLAN.md` — unimplemented five-phase test plan referenced by the maps (proptest, tempfile, assert_cmd); useful precedent, not yet adopted

No external specs/ADRs — no `Canonical refs:` line exists for Phase 1 in ROADMAP.md, and the discussion referenced no external documents.

## Existing Code Insights

### Reusable Assets
- `Store` (`crates/rustwatch-core/src/db.rs`) — sole gateway to `rustwatch.db`; every write-error and retry-queue decision lands here and in the daemon writer task
- `SegmentGrouper` (`crates/rustwatch-core/src/segment.rs`) — pure deterministic fold, `on_event` / `flush()`; idle marking (D-18) plugs in here and is unit-testable with no I/O
- `DaemonState` + `DaemonCommand`/`DaemonReply` (`crates/rustwatch-core/src/ipc.rs`) — the existing counter snapshot struct; `write_errors`, `dropped_events`, queue depth, and the health banner extend these three types together
- `CaptureHandle` / `PlatformCapture` (macos.rs) — facade + `cfg`-gated platform impl; per-path degradation (D-14), idle probe (D-17), and the 30 s re-probe loop (D-15) live here
- `DataPaths` (`crates/rustwatch-core/src/paths.rs`) — single path authority; the `notes` table's DB is already covered, but fix `MemoryEngine::open`'s hardcoded paths if touched
- `Config` (`crates/rustwatch-core/src/config.rs`) — where `[capture]` cooldown/idle knobs and the dead-field cleanup (SYS-01) happen
- `load_or_create_config` + first-run path (`paths.rs:81-92`) — the natural hook for permission onboarding (D-13)
- `deploy/macos/com.rustwatch.plist` + `commands::install` — launchd install/render; `KeepAlive` is what makes D-06 (exit on task death) actually restart the daemon
- `Redactor` (`crates/rustwatch-analyze/src/redact.rs`) — note text from the hotkey prompt (D-11) should pass through it, not bypass it

### Established Patterns
- Layered DAG workspace rooted at `rustwatch-core`; capture knows nothing about the DB — keep new capture logic (idle, permissions) out of persistence code
- Capture runs on plain `std::thread`s, not tokio; the event send must stay non-blocking (`let _ = ...` on a sender is *not* acceptable for loss — that is the bug class being removed)
- `tracing` with three different subscriber initializations (daemon plain, MCP stderr-only, CLI with indicatif) — new logs must respect MCP's stdout-is-protocol rule
- Sync `rusqlite` inside `async fn` — a retry queue must not block a runtime worker; use `spawn_blocking` or keep it on the writer task's own thread
- Length-prefixed JSON IPC over the Unix socket; any new status field is a wire-format change shared by CLI, MCP, and daemon

### Integration Points
- Daemon writer task (`crates/rustwatch-daemon/src/main.rs:58-84`) — retry queue, error counters, writer-liveness rule
- IPC handler closure (`crates/rustwatch-daemon/src/main.rs:102-162`) — health/permission reporting
- `PlatformCapture::permissions()` (`macos.rs:45-55`) — currently a hardcoded all-false placeholder; replaced by real probing
- Focus poll loop (`macos.rs:182-229`) — window-change throttle (D-09) and idle interaction (D-20)
- `rustwatch status` / `doctor` / TUI (`crates/rustwatch-cli/src/commands.rs`, `tui.rs`) — banner and detail rendering (D-07, D-08, D-16)
- `crates/rustwatch-core/migrations/` — a new migration is needed for the `notes` table (D-10); refinery is compile-time embedded

## Specific Ideas

- The health line reads as `capture: healthy` / `capture: degraded (3 write errors, 12 queued)` — user-approved phrasing.
- Screenshot cooldown expressed as a real config key `min_interval_secs` under `[capture]`.
- Idle thresholds are expected to be real config knobs (300 s start / 30 s end) — consistent with the "every knob is real" rule of this phase; the exact section/key naming is the planner's call.
- Annotation prompt: Enter saves, Esc closes with no note — the screenshot itself is never delayed by the typing.

## Deferred Ideas

None — discussion stayed within phase scope.

---

*Phase: 1-Reliable Capture & System Foundation*
*Context gathered: 2026-10-03*
