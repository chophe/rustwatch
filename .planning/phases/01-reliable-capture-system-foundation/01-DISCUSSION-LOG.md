# Phase 1: Reliable Capture & System Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-10-03
**Phase:** 1-reliable-capture-system-foundation
**Areas discussed:** Loss surfacing, Screenshot triggers, Permission onboarding, Idle semantics

**Prior context applied (not re-asked):** PROJECT.md Key Decisions (capture = keyboard + screen; screenshots = 5-min interval + window-change + hotkey annotating daily logs; macOS-only local-first), REQUIREMENTS.md CAPT-01…07 / SYS-01…02, ROADMAP.md Phase 1 success criteria, `.planning/codebase/` maps. No prior phase CONTEXT.md exists (Phase 1 is first).

---

## Loss surfacing

| Option | Description | Selected |
|--------|-------------|----------|
| status counter + error log | error! to launchd log AND counters in status/TUI | ✓ |
| Log only | error! only, nothing in status/TUI | |
| Counter + non-zero exit | status counter + log + nonzero write_errors fails doctor/status | |

**User's choice:** status counter + error log
**Notes:** doctor exit-code semantics were settled separately (see below) — counters warn there, they do not fail it.

| Option | Description | Selected |
|--------|-------------|----------|
| Blocking writer, no queue | lock().await + write, log+count on failure, event dropped | |
| In-memory retry queue | failed writes retried on a timer; lost on restart | ✓ |
| Disk spool / replay ledger | JSONL spool under ~/.rustwatch replayed on start | |

**User's choice:** In-memory retry queue
**Notes:** The `try_lock()` root cause is fixed regardless of this choice.

| Option | Description | Selected |
|--------|-------------|----------|
| Bounded, retryable-only | Retry SQLITE_BUSY/IO every ~500ms; non-retryable logged+counted immediately; cap ~10k, drop oldest, increment dropped_events | ✓ |
| Unbounded, retry everything | Retry all failures forever with backoff | |
| Bounded, drop newest | On overflow drop the new event instead of the oldest | |

**User's choice:** Bounded, retryable-only

| Option | Description | Selected |
|--------|-------------|----------|
| 2s flush deadline, then drop | stop capture, flush SegmentGrouper, drain up to 2s, count remainder dropped, exit clean | ✓ |
| Long drain (30s) | drain until empty or 30s | |
| No flush, count as dropped | drop whatever is queued at SIGTERM | |

**User's choice:** 2s flush deadline, then drop

| Option | Description | Selected |
|--------|-------------|----------|
| Writer panic → daemon exits | writer task is the liveness criterion; launchd KeepAlive restarts | ✓ |
| Supervisor restarts writer | restart writer task in-process, re-create connection | |
| Leave as-is | writer panic ends task silently | |

**User's choice:** Writer panic → daemon exits

| Option | Description | Selected |
|--------|-------------|----------|
| Healthy/degraded banner + details if bad | `capture: healthy` / `capture: degraded (3 write errors, 12 queued)`; detail lines only when nonzero | ✓ |
| Always print all counters | write_errors/queued/dropped always shown | |
| Health detail only in doctor | status stays terse, doctor owns detail | |

**User's choice:** Healthy/degraded banner + details if bad

| Option | Description | Selected |
|--------|-------------|----------|
| Same rule: any task death → exit | keytap/focus thread panic also exits non-zero | ✓ |
| Isolate and restart capture threads | catch_unwind per thread, restart in place | |
| Fix known panics only | patch the 3 known panic sites, otherwise trust threads | |

**User's choice:** Same rule: any task death → exit
**Notes:** Chosen together with the writer-panic decision — one liveness rule for every task.

| Option | Description | Selected |
|--------|-------------|----------|
| Warn on counters, fail only on live breakage | exit 1 on install/running/DB-writable failure; counters print as a warning line | ✓ |
| Counters fail doctor | any nonzero write_errors/dropped → exit 1 | |
| doctor ignores capture health | doctor checks install/running/DB only | |

**User's choice:** Warn on counters, fail only on live breakage

---

## Screenshot triggers

| Option | Description | Selected |
|--------|-------------|----------|
| 2s cooldown + burst cap | ignore focus change <2s, ≤3 shots per 10s window, config min_interval_secs | ✓ |
| Every transition, no throttle | one shot per focus change regardless of rate | |
| Defer: shoot after 3s of stability | debounce until focus settles | |

**User's choice:** 2s cooldown + burst cap

| Option | Description | Selected |
|--------|-------------|----------|
| New notes table | (id, ts, note, screenshot_id?) written by the daemon | ✓ |
| New CaptureEvent variant | Annotation as a CaptureEventKind | |
| Append to a daily log file | ~/.rustwatch/notes-YYYY-MM-DD.log | |

**User's choice:** New notes table

| Option | Description | Selected |
|--------|-------------|----------|
| Shoot now, prompt for note | screenshot fires instantly, terminal input follows; Enter saves, Esc skips | ✓ |
| Note = clipboard text | note comes from clipboard at hotkey time | |
| Marker now, annotate later via CLI | marker+shot now, `rustwatch note "..."` later | |

**User's choice:** Shoot now, prompt for note

| Option | Description | Selected |
|--------|-------------|----------|
| Discard missed, one shot on wake | no backlog; one immediate shot after wake | ✓ |
| Backfill up to 3 on wake | cover the gap with up to 3 shots | |
| No special handling | timer free-runs | |

**User's choice:** Discard missed, one shot on wake

---

## Permission onboarding

| Option | Description | Selected |
|--------|-------------|----------|
| Preflight, request-if-undetermined, else guide | request only undetermined grants once; if previously denied, open System Settings pane + print URL | ✓ |
| Report-only, never prompt | print instructions, never trigger TCC dialogs | |
| Always request | call request APIs every doctor run | |

**User's choice:** Preflight, request-if-undetermined, else guide

| Option | Description | Selected |
|--------|-------------|----------|
| Per-path degradation, daemon stays up | each path checks its own grant; status shows which one is missing | ✓ |
| Run, let calls fail and log | run everything, permission errors surface per call | |
| Fail hard on any missing grant | refuse to start until all three granted | |

**User's choice:** Per-path degradation, daemon stays up

| Option | Description | Selected |
|--------|-------------|----------|
| Periodic re-probe, hot-attach where possible | ~30s + every status/doctor; attach/detach paths live, say "restart required" when the API demands it | ✓ |
| Startup only, say restart | check once at start, print restart guidance | |
| Probe only on status/doctor | no background probing | |

**User's choice:** Periodic re-probe, hot-attach where possible

| Option | Description | Selected |
|--------|-------------|----------|
| Banner in status, fix steps in doctor | permission state in the health banner; doctor prints System Settings path + open command | ✓ |
| Full block until granted | full onboarding block on every status until green | |
| doctor only | status just says `capture: degraded` with no reason | |

**User's choice:** Banner in status, fix steps in doctor

---

## Idle semantics

| Option | Description | Selected |
|--------|-------------|----------|
| CGEventSource, 5-min default | secondsSinceLastEventType, config threshold, default 5 min | ✓ |
| Our own last-event timestamp | track last event rustwatch received | |
| Both, cross-validated | CGEventSource signal validated against our own timestamp | |

**User's choice:** CGEventSource, 5-min default

| Option | Description | Selected |
|--------|-------------|----------|
| Capture continues, segment marked idle | nothing dropped; segment carries `idle: true`; reports exclude idle segments | ✓ |
| Pause capture while idle | stop keyboard/focus loops and screenshots during idle | |
| Infer idle at read time | no write-time marking; heuristics at report time | |

**User's choice:** Capture continues, segment marked idle

| Option | Description | Selected |
|--------|-------------|----------|
| Start 5min / end after 30s sustained | idle starts at 300s no-input, ends after 30s sustained activity | ✓ |
| End on first input | single keystroke/click ends idle | |
| Both as config knobs (300/30) | same behavior but both exposed as config | |

**User's choice:** Start 5min / end after 30s sustained
**Notes:** CONTEXT.md records that idle thresholds are expected to be real config knobs (SYS-01); key naming left to the planner.

| Option | Description | Selected |
|--------|-------------|----------|
| Suppress auto shots, hotkey stays live | no interval/window-change shots during idle; hotkey works; timer restarts on idle end | ✓ |
| Keep interval shots during idle | continuous timeline coverage while away | |
| Suppress all, including hotkey | leaving idle requires explicit resume | |

**User's choice:** Suppress auto shots, hotkey stays live

---

## the agent's Discretion

Surfaced but not discussed by user choice (still bound by SYS-01/SYS-02):
- Config honesty: wire vs delete the 9+ unread config fields
- Single-instance lock mechanism (advisory file lock vs other)
- Exact TCC probe APIs and which grants can hot-attach
- `doctor`/`status` exact check list and output layout beyond the agreed banner behavior

## Deferred Ideas

None — discussion stayed within phase scope.
