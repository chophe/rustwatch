---
phase: 01-reliable-capture-system-foundation
plan: 01
subsystem: capture
tags: [sqlite, rusqlite, ipc, unix-socket, fs2, tokio, sigterm, unicode, config]

# Dependency graph
requires: []
provides:
  - Blocking single-writer ledger (Store + SegmentGrouper + bounded retry queue on its own thread)
  - DaemonState loss counters + health banner wire format (write_errors, dropped_events, queued, capture_health)
  - Honest config schema (dead fields deleted, capture knobs + [permissions] prompted_* for 01-02/01-03)
  - Single-instance fs2 lock, 0600 socket, 16 MiB frame cap, safe start/stop
affects: [01-02-triggers, 01-03-permissions-doctor, status-TUI-banner]

# Actuals (#2632) — pairs with the plan's `estimate` to calibrate future estimates.
actuals:
  tokens: 18369
  tasks: 4
  commits: 4
  plan_head_before: 5872363

# Tech tracking
tech-stack:
  added: [fs2 0.4 (daemon pid-file lock), core-graphics 0.23 (direct macOS dep of capture, idle probe lands in 01-02)]
  patterns: [single writer owns Store on its own std thread, recv_timeout doubles as retry timer, classify-by-error-kind never by string, parked-not-dead capture threads, lock-then-pid-then-bind]

key-files:
  created: []
  modified:
    - crates/rustwatch-daemon/src/main.rs
    - crates/rustwatch-core/src/db.rs
    - crates/rustwatch-core/src/ipc.rs
    - crates/rustwatch-core/src/config.rs
    - crates/rustwatch-core/src/paths.rs
    - crates/rustwatch-cli/src/commands.rs
    - crates/rustwatch-analyze/src/redact.rs
    - crates/rustwatch-capture/src/lib.rs
    - crates/rustwatch-capture/src/platform/macos.rs
    - crates/rustwatch-capture/src/platform/stub.rs
    - crates/rustwatch-daemon/Cargo.toml
    - crates/rustwatch-capture/Cargo.toml

key-decisions:
  - "Writer moved to its own std::thread on std::sync::mpsc (not tokio task): rusqlite is blocking, and the 500 ms recv_timeout doubles as the D-04 retry timer"
  - "Graceful capture-thread failure parks the thread (D-14 degraded) while panics still trip the D-06 death watch — otherwise the watch would crash-loop grant-less machines"
  - "Drain logic split into testable free functions (handle_event/drain_retry_queue); zero-deadline test proves the SIGTERM accounting without sleeping hardware"
  - "Dead config fields deleted (not wired): data.*, vision_model, batch_interval_minutes, vector/graph/surreal backends, ui.*, send_screenshots_to_llm, accessibility_poll_ms"

patterns-established:
  - "D-04 classification by error kind (sqlite_error_code/ffi::ErrorCode), never string matching"
  - "Every insert failure surfaces via error! AND a counter; no let _ on persistence paths"
  - "Unknown config keys warn (collected by unknown_config_keys, printed at startup), never fail"

requirements-completed: [CAPT-01, CAPT-05, SYS-01]

coverage:
  - id: D1
    description: "Sustained typing concurrent with tail loses zero events; write_errors stays 0"
    requirement: "CAPT-01"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-daemon#periodic_drain_redelivers_through_real_store"
        status: pass
      - kind: unit
        ref: "cargo test -p rustwatch-core#drop_oldest_accounting_at_cap"
        status: pass
    human_judgment: true
    rationale: "Unit tests prove redelivery/accounting logic; true zero-loss under typing flood + concurrent tail needs a human on a granted macOS machine (plan human-check)."
  - id: D2
    description: "CJK/emoji/paste floods never panic writer, segmenter, redactor, or snippet path"
    requirement: "CAPT-05"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-analyze#emoji_flood_truncates_on_char_boundary"
        status: pass
      - kind: unit
        ref: "cargo test -p rustwatch-cli (rustwatch binary)#emoji_flood_snippet_is_char_safe"
        status: pass
    human_judgment: false
  - id: D3
    description: "SIGTERM exits 0 with tail segment flushed and retry queue drained inside 2 s"
    requirement: "CAPT-05"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-daemon#zero_deadline_shutdown_drain_drops_remainder"
        status: pass
      - kind: other
        ref: "live: SIGTERM to isolated-HOME daemon exited 0 (wait $DAEMON)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Second rustwatchd exits 2 with already-running message, first keeps its socket"
    requirement: "SYS-01"
    verification:
      - kind: other
        ref: "live: second instance exited 2, first stayed ALIVE (isolated HOME)"
        status: pass
    human_judgment: false
  - id: D5
    description: "config.toml with removed keys plus unknown key loads with warning; every remaining field has a reader"
    requirement: "SYS-01"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-core#removed_and_unknown_keys_warn_but_load"
        status: pass
      - kind: other
        ref: "dead-field grep: zero readers outside schema; new-knob readers land in 01-02/01-03 by plan"
        status: pass
    human_judgment: false
  - id: D6
    description: "Poison (constraint/drift) errors surface once and never loop the retry timer"
    requirement: "CAPT-05"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-daemon#fatal_errors_never_requeued"
        status: pass
      - kind: unit
        ref: "cargo test -p rustwatch-core#retryable_vs_fatal_classification"
        status: pass
    human_judgment: false

# Metrics
duration: 95min
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 01: Loss-Free Ledger + Lifecycle Hardening Summary

**Blocking writer with bounded retry queue, SIGTERM flush, fs2 single-instance lock, and an honest config schema — the durable foundation 01-02/01-03 build on.**

## Performance

- **Duration:** ~95 min (session spanning prior Wave-0 run + this execution)
- **Started:** 2026-10-04 (Wave-0 dependency approval)
- **Completed:** 2026-10-04
- **Tasks:** 4 of 4 (1 checkpoint approved, 1 tracer, 2 auto)
- **Files modified:** 14 (12 source + 2 Cargo.toml + Cargo.lock)

## Accomplishments

- End-to-end durable write path: writer owns Store on its own thread, `DaemonState` loss counters + `capture: healthy/degraded` banner in `status`
- Retry queue (10k, drop-oldest + counter, BUSY/IO-only retry on 500 ms timer) with SIGTERM drain (2 s deadline, remainder counted dropped) and nonzero-exit death watch
- Single-instance fs2 lock (exit 2), 0600 socket / 0700 data root, 16 MiB IPC frame cap, safe start/stop, Unicode-safe truncation everywhere, corrupt timestamps error
- Config honesty: 14 dead fields deleted, 8 wired knobs + `[permissions] prompted_*` added, unknown keys warn

## Task Commits

Each task was committed atomically:

1. **Task checkpoint: approve fs2 + core-graphics** — human approved; no commit (gate only)
2. **Task tracer: durable write path + counters** — `ba1bfcf` (feat) + `e8d7659` (feat, retry-queue core: prior run)
3. **Task auto: retry queue, shutdown, crash-restart** — `0f22d6b` (feat)
4. **Task auto: lock, socket, start/stop, text/config honesty** — `36665bb` (feat)

**Prior-run note:** the handoff described a clean tree with 0/4 tasks, but the tree held `ba1bfcf` (tracer) and `e8d7659` (RetryQueue core) plus uncommitted RetryQueue extensions. Both were verified green and adopted rather than redone.

## Files Created/Modified

- `crates/rustwatch-daemon/src/main.rs` — own-thread writer, retry drain, SIGTERM/SIGINT, fs2 lock-then-pid-then-bind, 0600 socket, death watch, shutdown join
- `crates/rustwatch-core/src/db.rs` — `RetryQueue`, `is_retryable_{db,store}_error`, pragmas (tracer), `parse_ts` now errors
- `crates/rustwatch-core/src/ipc.rs` — counters + health banner (tracer), `TAIL_MAX_LIMIT`, 16 MiB frame cap, 30 s IPC timeouts
- `crates/rustwatch-core/src/config.rs` — schema rewrite: dead deleted, knobs + `[permissions]` added, serde defaults
- `crates/rustwatch-core/src/paths.rs` — `unknown_config_keys`, 0700 data root
- `crates/rustwatch-cli/src/commands.rs` — D-07 banner (tracer), Ping-poll start + log tail, verified stop, char-safe snippet
- `crates/rustwatch-analyze/src/redact.rs` — char-boundary truncation
- `crates/rustwatch-capture/src/{lib,platform/{macos,stub}}.rs` — std channel, thread registry + `threads_alive`, dead param removed
- `crates/rustwatch-{daemon,capture}/Cargo.toml` + `Cargo.lock` — fs2 0.4, core-graphics 0.23 (approved)

## Decisions Made

- Writer on `std::thread` + `std::sync::mpsc` (not a tokio task): rusqlite calls are blocking, and `recv_timeout(500ms)` doubles as the D-04 retry timer — Pitfall 1 avoided structurally.
- Parked-not-dead capture threads (Rule 2): a graceful `Err` (missing grant) parks the thread forever instead of finishing it, so the D-06 watch only fires on real panics and the daemon stays up degraded per D-14.
- Drain as pure-ish free functions over `&Store` so the SIGTERM deadline is unit-testable with a zero-deadline synthetic burst.
- Wire-vs-delete audit: deleted everything with zero readers; new knobs have schema + defaults + warn-list entries with readers landing in 01-02/01-03 exactly as PLAN.md scopes.

## Wire-vs-Delete Audit (SYS-01, recorded verbatim per plan)

**DELETED (zero readers outside the schema itself):** `data.*` (4 fields; `Config::paths()` removed with them — DataPaths is sole authority) · `analyze.vision_model` · `analyze.batch_interval_minutes` · `memory.vector_backend` · `memory.graph_backend` · `memory.surreal_engine` · `ui.tui_enabled` · `ui.progress_bars` · `privacy.send_screenshots_to_llm` · `capture.accessibility_poll_ms` (plumbing removed from daemon + capture lib/macos/stub).

**KEPT (reader in parentheses):** `capture.poll_focus_ms` / `exclude_apps` / `screenshot_on_focus_change` (daemon→capture) · `analyze.provider` (status, classifier) · `analyze.model` (classifier) · `memory.embedding_model` / `graph_expand_hops` (memory) · `memory.chunk_max_chars` (redactor) · `privacy.redact_patterns` (redactor).

**ADDED (planned consumer):** `screenshot_interval_secs`, `min_interval_secs` (01-02 scheduler) · `idle_start_secs`, `idle_end_sustained_secs` (01-02 idle) · `hotkey_enabled`, `hotkey_chord` (01-02 hotkey) · `[permissions] prompted_{input_monitoring,accessibility,screen_recording}` (01-03 preflight).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] rusqlite 0.32 `SqliteFailure` test constructor**
- **Found during:** Task 3 (retry-queue tests)
- **Issue:** Test helper passed `ErrorCode` where 0.32 takes `ffi::Error{code, extended_code}` — compile error, and it confirmed the plan's A4 assumption resolves to struct-literal construction.
- **Fix:** `ffi::Error { code, extended_code: 0 }` in the helper; no production change.
- **Files modified:** `crates/rustwatch-core/src/db.rs`
- **Verification:** `cargo test -p rustwatch-core` green
- **Committed in:** `0f22d6b` (part of task commit)

**2. [Rule 1 - Bug] `EventSender` alias arity**
- **Found during:** Task 3 (channel-type swap)
- **Issue:** Alias declared without a generic parameter but used as `EventSender<CaptureEvent>`.
- **Fix:** `pub type EventSender<T> = std::sync::mpsc::Sender<T>`.
- **Files modified:** `crates/rustwatch-capture/src/platform/macos.rs`
- **Verification:** `cargo check --workspace` green
- **Committed in:** `0f22d6b` (part of task commit)

**3. [Rule 2 - Missing critical] Parked capture threads (D-14 vs D-06)**
- **Found during:** Task 3 (death-watch review)
- **Issue:** As written, any finished capture thread (including graceful tap-init failure without a grant) would trip the D-06 watch and crash-loop the daemon — violating D-14 degradation on exactly the machines that need it.
- **Fix:** Graceful `Err` parks the keyboard thread (hour-long sleeps); only a real panic finishes a thread and trips the watch.
- **Files modified:** `crates/rustwatch-capture/src/platform/macos.rs`
- **Verification:** live daemon on a grant-less shell stays ALIVE; reasoned inspection for the panic path
- **Committed in:** `36665bb` (part of task commit)

---

**Total deviations:** 3 auto-fixed (2 Rule-1 compile, 1 Rule-2 correctness)
**Impact on plan:** All necessary for correctness; no scope creep. New-knob readers intentionally deferred to 01-02/01-03 per PLAN.md.

## Issues Encountered

- `cargo test` binary-tmp socket paths exceed SUN_LEN under `mktemp -d` homes: used `/tmp/rwtest*` short homes for live daemon runs. Environmental, not a code issue.
- First SIGTERM attempt hit the subshell wrapper instead of the daemon (orphaning it); re-ran with `$!` as the direct child and captured exit 0 via `wait`.

## Live Verification (isolated HOME, short path)

- Second `rustwatchd`: exit **2**, `rustwatchd is already running (pid file locked: …)`, first daemon kept socket and stayed ALIVE
- `kill -TERM`: daemon **exit 0** via `wait`
- Socket mode `srw-------` (0600), data root `drwx------` (0700), fresh `config.toml` shows new schema with zero dead sections

## User Setup Required

None - no external service configuration required.

## On-Machine Human Checks (from plan `<human-check>`)

Automated parts are done and green. On a granted macOS machine, please run:

1. `rustwatchd` under one Screen Recording denial + heavy typing + concurrent `tail`: confirm zero event loss, nonzero counters visible in `status`, no panic in logs
2. Typing flood concurrent with `tail`: `events_captured` should equal `events` row count, `write_errors` 0
3. CJK/emoji/paste flood through capture, search snippet, and redaction paths (unit-covered, needs hardware input to be certain)

## Next Phase Readiness

- 01-02 consumes: `DaemonState` counters/banner, `RetryQueue`/`Store` writer ownership, `screenshot_interval_secs`, `min_interval_secs`, `idle_*`, `hotkey_*` knobs — all landed with defaults.
- 01-03 consumes: `[permissions] prompted_*`, `threads_alive`/parked-thread model, `doctor`/`status` extension points, plist log-path note (daemon stderr now routable to `rustwatchd.log` via `start`).
- No blockers. Pre-existing `hash_content` dead-code warning in `macos.rs` left untouched (out of scope).

## Self-Check: PASSED

- SUMMARY file exists at `.planning/phases/01-reliable-capture-system-foundation/01-01-SUMMARY.md`
- Commits `ba1bfcf`, `e8d7659`, `0f22d6b`, `36665bb` all present in `git log`
- `cargo test --workspace`: 11 suites ok, 0 failed · `cargo check --workspace`: green
- Stub scan on the plan diff: no TODO/FIXME/placeholder/unimplemented markers
- Threat scan: all new surface (socket perms, frame cap, pid lock, stop/start) is inside the plan's threat register — no new flags

---
*Phase: 01-reliable-capture-system-foundation*
*Completed: 2026-10-04*
