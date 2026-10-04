---
phase: 01-reliable-capture-system-foundation
plan: 02
subsystem: capture
tags: [screenshots, interval, throttle, hotkey, idle, sqlite-migration, cgeventsource, xcap, annotate]

# Dependency graph
requires: [01-01-ledger]
provides:
  - Interval scheduler (same-channel Screenshot events, Skip backlog, wake single-shot)
  - D-09 throttled focus-loop trigger + honest screenshot scope + segment-linked shots
  - Hardware-idle pipeline (CGEventSource probe, hysteresis, idle:true segments, V2 schema)
  - Chord hotkey (Ctrl+Shift+Space) with shoot-first annotate + D-20 idle suppression
affects: [01-03-permissions-doctor, status-TUI-banner, phase-5-reports-exclude-idle, phase-5-digest-notes]

# Actuals (#2632) — pairs with the plan's `estimate` to calibrate future estimates.
actuals:
  tokens: 22264
  tasks: 3
  commits: 3
  plan_head_before: 480c9278fc7d9848d0e5740c82a555c228485420

# Tech tracking
tech-stack:
  added: []
  patterns: [pure-helpers-over-injected-values (detect_wake/poll_idle/should_shoot/NoteEditor), synthetic-idle-signals-on-shared-channel, serialize-driver-calls-behind-static-mutex, lossy-notify-plus-authoritative-flag]
key-files:
  created:
    - crates/rustwatch-core/migrations/V2__notes_idle.sql
  modified:
    - crates/rustwatch-daemon/src/main.rs
    - crates/rustwatch-capture/src/platform/macos.rs
    - crates/rustwatch-capture/src/platform/stub.rs
    - crates/rustwatch-capture/src/lib.rs
    - crates/rustwatch-core/src/events.rs
    - crates/rustwatch-core/src/segment.rs
    - crates/rustwatch-core/src/db.rs
    - crates/rustwatch-cli/src/commands.rs
    - crates/rustwatch-cli/src/main.rs
    - crates/rustwatch-cli/Cargo.toml
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "Interval ticks route through the same mpsc channel as capture events (uniform retry/loss accounting) — scheduler owns tick + idle poll + wake-gap in one task, always spawned so idle works with interval=0"
  - "Hotkey chord Ctrl+Shift+Space via hotkey_enabled/hotkey_chord knobs; chord-on-tap (not keytap ChordMatcher) so the existing exclusion check covers it with zero new threads/taps"
  - "core-graphics 0.23.2 has no secondsSinceLastEventType binding (vendored sources checked) — 10-line extern C fallback per RESEARCH.md, HIDSystemState + kCGAnyInputEventType"
  - "Idle close marks the segment idle:true and resume closes the idle segment, so post-resume work never appends to an idle row"
  - "Annotate is CLI-side (daemon is headless): raw-mode prompt, Enter saves redacted note, Esc discards"

patterns-established:
  - "Hardware idle via CGEventSourceSecondsSinceLastEventType; broken probe reads as active (never idle on a broken probe)"
  - "Every screenshot row carries its true scope (Screen fallback labeled Screen) and the open segment id"
  - "SIGTERM-to-exit survives slow captures via shutdown-flag + select!-abandon, not Notify alone"

requirements-completed: [CAPT-02, CAPT-03, CAPT-04, CAPT-07]

coverage:
  - id: D1
    description: "Interval screenshots land on cadence during active use; wake takes exactly one shot, never a backlog burst"
    requirement: "CAPT-02"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-daemon#skip_behavior_produces_no_backlog + detect_wake boundaries + interval_zero_disables"
        status: pass
      - kind: other
        ref: "live: 75 s run, 10 s interval — 4 completed captures, 4 files + 4 rows (file<=>row invariant holds); 5th capture abandoned mid-flight at SIGTERM with no partial row"
        status: pass
    human_judgment: true
    rationale: "Unit tests prove gap/Skip/disable logic; sleep/wake single-shot on real hardware needs a human (machine cannot be slept from here)."
  - id: D2
    description: "Rapid window switching yields at most 3 shots per 10 s spaced 2 s+ apart"
    requirement: "CAPT-03"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-capture#cooldown_blocks_rapid_refire + burst_cap_allows_three_per_ten_seconds + burst_window_slides"
        status: pass
    human_judgment: true
    rationale: "Pure-helper proof is airtight; Alt-Tab spam on a granted machine is the plan's human-check."
  - id: D3
    description: "Hotkey chord screenshots without disturbing typing; annotate saves the note with screenshot linkage, Esc saves nothing"
    requirement: "CAPT-04"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-capture#full_chord_fires_one_screenshot + partial/repeat/exclusion/disabled + cargo test -p rustwatch-cli#annotate_enter/esc/backspace"
        status: pass
      - kind: other
        ref: "live over pty: Enter saved 'fix login bug' linked to shot-2; Esc left the notes table unchanged"
        status: pass
    human_judgment: true
    rationale: "Chord latency (<200 ms) and no keystroke interruption need a human pressing Ctrl+Shift+Space in a real app."
  - id: D4
    description: "5 min idle closes an idle:true segment while capture continues; sub-30 s nudges never end idle; automatic shots suppress during idle"
    requirement: "CAPT-07"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-daemon#hysteresis_needs_thirty_sustained_seconds + stray_nudge_does_not_end_idle + segment mark_idle/mark_active fold + V2 fresh/upgrade migration tests"
        status: pass
      - kind: other
        ref: "live: Quartz probe links and reads sane values (19.6 s / 0.1 s); V2 migration applied in production log; active machine correctly stayed non-idle (no false positives across 4 live runs)"
        status: pass
    human_judgment: true
    rationale: "Positive end-to-end (real 5-min idle → idle:true row, suppression observed) needs a quiet machine; this box's console user was continuously active."

# Metrics
duration: 48min
completed: 2026-10-04
status: complete
---

# Phase 01 Plan 02: Screenshot Triggers + Idle Detection Summary

**Scheduler, throttled focus trigger, chord hotkey with annotate, and hardware-idle marking — all capture triggers live on the 01-01 ledger with uniform loss accounting.**

## Performance

- **Duration:** ~48 min (2026-10-04T14:34:46Z → 15:23:11Z)
- **Tasks:** 3 of 3 (1 tracer, 2 auto)
- **Files modified:** 12 (1 created, 11 modified)
- **Tests:** 103 passing workspace-wide (28 capture + 47 core + 14 daemon + 5 CLI + 9 pre-existing elsewhere), 0 failed

## Accomplishments

- Interval scheduler task: `screenshot_interval_secs` tick (0 disables) on the shared mpsc channel, `MissedTickBehavior::Skip`, pure `detect_wake` (interval + 5 s slack), one shot per tick via `spawn_blocking`
- Focus-loop D-09 throttle (2 s cooldown + 3-per-10 s burst, pure `should_shoot`), honest scope propagation (`(PathBuf, ScreenshotScope)`, Screen fallback labeled Screen), segment-linked screenshot rows
- Idle pipeline: `extern "C"` CGEventSource probe, 5 s scheduler poll, pure `poll_idle` hysteresis (300 s start / 30 s sustained end), synthetic `IdleStart`/`ActivityResumed` events, `mark_idle`/`mark_active` fold, `SessionSegment.idle`, V2 migration (notes table + idle column)
- Hotkey: `Ctrl+Shift+Space` chord-on-tap with parser + knobs, repeat suppression, exclusion-honoring, trigger consumed; `rustwatch annotate` raw-mode prompt (Enter saves redacted note, Esc discards)
- D-20 suppression: shared idle flag gates interval + focus shots (hotkey live), interval timer resets at idle end

## Task Commits

Each task was committed atomically:

1. **Task tracer: end-to-end interval screenshot** — `e546c09` (feat)
2. **Task auto: throttle, scope, segment-link, idle** — `164bd1e` (feat)
3. **Task auto: hotkey, annotate, suppression** — `d2ee7af` (feat)

## Files Created/Modified

- `crates/rustwatch-daemon/src/main.rs` — scheduler (interval + wake + idle poll + suppression publish), `open_segment_id` fill, new `start` args, idle flag, death-watch + shutdown join
- `crates/rustwatch-capture/src/platform/macos.rs` — throttle, honest `capture_to_disk`, `CAPTURE_LOCK`, idle FFI probe, hotkey chord + parser, keyboard/focus loop wiring
- `crates/rustwatch-capture/src/platform/stub.rs` + `src/lib.rs` — signature parity (`min_interval`, hotkey, idle flag, tuple shots, `None` idle probe)
- `crates/rustwatch-core/src/events.rs` — `IdleStart`/`ActivityResumed` variants, `SessionSegment.idle`, `Note`
- `crates/rustwatch-core/src/segment.rs` — `mark_idle`/`mark_active`, idle-born segments, `open_segment_id`
- `crates/rustwatch-core/src/db.rs` — idle column in all segment writes/reads, notes fns, first screenshots reader
- `crates/rustwatch-core/migrations/V2__notes_idle.sql` — notes table + `segments.idle` (new file)
- `crates/rustwatch-cli/src/commands.rs` + `src/main.rs` + `Cargo.toml` — `annotate` command, `NoteEditor`, uuid dep
- `Cargo.toml` — tokio `test-util` feature (paused-time Skip test; no new crates)
- `Cargo.lock` — uuid edge for CLI only

## Decisions Made

- Scheduler always runs even with interval 0 (idle must not depend on the screenshot knob); disabled interval parks its select arm via a never-ready future
- Chord detection inline in `translate_key_events` (not keytap's `ChordMatcher`) so the one exclusion check covers the hotkey with no second tap/thread
- Extra modifiers held beyond the chord do not fire (exact match); key-repeat suppressed via held-set; trigger Space consumed (no phantom text)
- Resume closes the idle segment (post-resume work starts fresh); segments born during idle are idle (stray nudges stay excludable)
- `DataPaths::new(Some(root)).screenshot_dir_for_date(...)` reuses the sole path authority with zero signature churn

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `notify_one` → `notify_waiters`**
- **Found during:** Task 1 (scheduler is a second shutdown waiter)
- **Issue:** The SIGTERM handler's `notify_one` could wake the scheduler and leave the accept loop parked forever (or vice versa).
- **Fix:** `notify_waiters()` with a comment.
- **Committed in:** `e546c09`

**2. [Rule 1 - Bug] Concurrent xcap captures stall 12–22 s**
- **Found during:** Task 1 live verification (file mtime vs name arithmetic; sequential captures are same-second)
- **Issue:** Overlapping interval + focus captures stalled each other; evidence pointed at driver-level concurrency.
- **Fix:** Static `CAPTURE_LOCK` serializing all `capture_to_disk` calls — a slow capture delays, never deadlocks.
- **Committed in:** `e546c09`

**3. [Rule 2 - Missing critical] Scheduler shutdown races**
- **Found during:** Task 1 (TERM arriving mid-capture loses the Notify wait)
- **Issue:** Notify is lossy — SIGTERM during `spawn_blocking` missed the waiter, and nothing stopped a post-TERM shot.
- **Fix:** Authoritative `shutdown_flag` checks (break, never shoot post-TERM) + `select!`-abandon of in-flight captures so SIGTERM-to-exit stays in milliseconds.
- **Committed in:** `e546c09`

**4. [Rule 2 - Missing critical] Scheduler must always spawn**
- **Found during:** Task 2 (idle poll lives in the scheduler task)
- **Issue:** Gating spawn on interval > 0 would kill idle detection when interval shots are disabled.
- **Fix:** Always spawn; `period: Option<Duration>` with a parked select arm when `None`.
- **Committed in:** `164bd1e`

**5. [Rule 1 - Bug] Idle suppression `continue` skipped `last` update**
- **Found during:** Task 3 (reviewing the focus-loop edit)
- **Issue:** `continue` inside the change block skipped `last = Some(current)`, duplicating FocusChange events every poll while idle.
- **Fix:** Gate only the shot (`screenshot_on_focus_change && !idle`), events and bookkeeping untouched.
- **Committed in:** `d2ee7af`

**6. [Rule 1 - Bug] Burst-cap test asserted the capped case as allowed**
- **Found during:** Task 2 (`burst_cap_allows_three_per_ten_seconds` failed)
- **Issue:** 3 in-window shots means the cap is reached; the test expected a 4th to fire.
- **Fix:** Test now asserts 2-in-window fires, 3-in-window caps.
- **Committed in:** `164bd1e`

**7. [Rule 3 - Blocking] tokio `test-util` feature for the Skip test**
- **Found during:** Task 1 (`pause`/`advance` gated behind `test-util`, not in `full`)
- **Issue:** The no-backlog regression test needs paused time.
- **Fix:** Added `"test-util"` to workspace tokio features — same vendored crate, no new fetch, no new supply-chain surface.
- **Committed in:** `e546c09`

**8. [Rule 3 - Blocking] uuid for CLI note ids**
- **Found during:** Task 3 (CLI had no uuid dep)
- **Issue:** `Note.id` needs a v4 id; hand-rolling ids is worse.
- **Fix:** `uuid.workspace = true` in CLI Cargo.toml (already in Cargo.lock; lock diff is one edge).
- **Committed in:** `d2ee7af`

**Compile-time verifications (per plan):** keytap 0.4 variants confirmed in vendored sources (`ControlLeft`/`ControlRight`, `Space`, `Alt*`, `Meta*`); core-graphics 0.23.2 has NO `secondsSinceLastEventType` binding → the documented `extern "C"` fallback is used (noted in code).

---

**Total deviations:** 8 auto-fixed (3 Rule-1, 2 Rule-2, 3 Rule-3-blocking). No architectural changes, no new crate fetches, no scope creep.

## Issues Encountered

- Shared box under load average ~45: xcap captures take ~12 s (vs same-second when unloaded). Scheduler self-throttles (cadence = max(period, capture time), no pile-up); live cadence proofs used longer windows.
- `timeout N` reports 124 whenever a timeout occurs even if the daemon then exits 0 on TERM ~1 s later — 124 alone does not mean unclean shutdown; verified via exit-path logs and post-TERM row accounting.
- First live run wrote its config to the wrong path (`DataPaths` resolves under `Library/Application Support/com.chophe.rustwatch`, not the HOME root) — isolated-HOME runs must pre-seed `$HOME/Library/Application Support/com.chophe.rustwatch/config.toml`.

## Live Verification (isolated HOMEs)

- 75 s run, 10 s interval: 4 completed captures → 4 files + 4 screenshot rows (file⟺row invariant holds); 5th capture correctly abandoned mid-flight at SIGTERM (file completes, no partial row, no error)
- Quartz probe live readings: 19.6 s and 0.1 s since last HID input (sane, tracks real console activity)
- V2 migration applied cleanly in production log on a fresh DB; V1→V2 upgrade covered by unit test
- Active machine correctly stayed non-idle across 4 runs (zero false-positive idle signals)
- `annotate` over a real pty: Enter saved "fix login bug" linked to the newest unnoted shot; Esc left the notes table unchanged

## User Setup Required

None — no external service configuration required.

## On-Machine Human Checks (from plan `<human-check>`)

Automated parts are done and green. On a granted, quiet macOS machine, please run:

1. **Interval + wake:** daemon with a 10 s test interval — screenshots land roughly on cadence; sleep 2 min and wake: exactly one wake shot, no burst of stale rows
2. **Throttle:** rapid Alt-Tab switching — at most 3 shots per 10 s spaced 2 s+ (check `screenshots` rows/file mtimes)
3. **Hotkey:** press Ctrl+Shift+Space in any app — screenshot file appears within 200 ms with no keystroke interruption; `rustwatch annotate` prompts, Enter saves (visible via `SELECT note FROM notes`), Esc saves nothing
4. **Idle:** leave the machine 5+ min untouched — an `idle: true` segment closes while capture continues; a stray nudge does not end idle; during forced idle, interval/window shots stop but the hotkey still fires

## Next Phase Readiness

- 01-03 consumes: `[permissions] prompted_*` (untouched), `threads_alive`/parked-thread model (extended: scheduler covered by D-06 watch), `doctor`/`status` extension points, `notes`/`idle` schema already in place for banner/report work
- Known gap (pre-existing, out of scope): `hash_content` dead-code warning in `macos.rs` still untouched
- Retention caps (T-02-03 follow-up) remain Phase 4 scope

## Self-Check: PASSED

- SUMMARY file exists at `.planning/phases/01-reliable-capture-system-foundation/01-02-SUMMARY.md`
- Commits `e546c09`, `164bd1e`, `d2ee7af` all present in `git log`
- `cargo test --workspace`: 103 passed, 0 failed · `cargo check --workspace`: green (1 pre-existing warning: `hash_content` never used)
- Stub scan on the plan diff: no TODO/FIXME/placeholder/unimplemented markers
- Threat scan: all new surface (scheduler, hotkey, idle probe, annotate) is inside the plan's threat register (T-02-01/02/03 mitigated, T-02-04 accepted, T-02-SC honored — no new crate fetches); no new flags

---
*Phase: 01-reliable-capture-system-foundation*
*Completed: 2026-10-04*
