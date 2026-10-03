# Phase 1: Reliable Capture & System Foundation - Research

**Researched:** 2026-10-03
**Domain:** macOS daemon capture reliability (keyboard tap, screenshots, idle, permissions, SQLite ledger, launchd)
**Confidence:** HIGH (codebase-evidenced) / MEDIUM (macOS TCC/idle APIs — web-sourced, need compile-time verification)

## Summary

Phase 1 makes the existing capture engine trustworthy without adding capabilities. The codebase audit (CONCERNS.md, verified against live source this session) shows the hot path has five loss/crash defects that must die in plan 01-01: `try_lock`-and-drop event loss in the daemon writer (`crates/rustwatch-daemon/src/main.rs:62`), discarded insert errors (`:63,66,70`), three byte-slice Unicode panic sites (one already fixed in `segment.rs`, two remain), no SIGTERM flush, and no single-instance lock. Significant hardening already landed before this phase — keyboard-path `exclude_apps` enforcement, char-boundary-safe `append_text`, `ensure_segment` for pre-focus text, and 50+ unit tests in `keystroke.rs`/`segment.rs`/`macos.rs` — so plans build on tested ground rather than starting from zero.

The CONTEXT.md decisions (D-01…D-20) lock the semantics: blocking writer + bounded 10k retry queue (SQLITE_BUSY/IO only, 500 ms timer, drop-oldest with counter), any-task-death exits non-zero for launchd restart, health banner in status/TUI, `notes` table for hotkey annotations, shoot-first-prompt-second hotkey, CGEventSource idle (300 s start / 30 s sustained-activity end, capture continues, segments marked `idle: true`), per-path permission degradation with 30 s re-probe, and sleep/wake discarding missed ticks plus one wake shot.

**Primary recommendation:** Keep all new logic in the existing layers (writer task owns the retry queue on its own thread; `SegmentGrouper` owns idle marking as pure logic; `PlatformCapture` owns probes/triggers), add exactly two small dependencies (`fs2` for the pid-file lock, `core-graphics` already transitively at 0.23.2 for the idle probe), and cover every fix with a regression test — loss bugs that regress silently are the failure class this phase exists to kill.

## User Constraints (from CONTEXT.md)

> Copied from `.planning/phases/01-reliable-capture-system-foundation/01-CONTEXT.md`. The Implementation Decisions (D-01…D-20) are locked; the planner MUST honor them.

### Locked Decisions

- **D-01:** Failed writes surface in `error!` logs AND counters in `rustwatch status`/TUI. Log-only rejected.
- **D-02:** Root fix for event loss is removing the `try_lock()` path in the writer task (`crates/rustwatch-daemon/src/main.rs:62`) — writer awaits the lock.
- **D-03:** Bounded in-memory retry queue (middle ground; pure blocking writer and disk spool both rejected).
- **D-04:** Retry only `SQLITE_BUSY`/IO on ~500 ms timer; non-retryable errors logged + counted immediately. Cap ~10k events; overflow drops oldest + increments `dropped_events`. Never unbounded.
- **D-05:** SIGTERM: stop capture, flush `SegmentGrouper`, drain retry queue with 2 s deadline, count remainder as dropped, exit cleanly.
- **D-06:** Any task death → daemon exits non-zero, launchd `KeepAlive` restarts. Applies to writer-task panic AND capture-thread panic. In-process supervisor rejected.
- **D-07:** `status`/TUI healthy/degraded banner (`capture: healthy` vs `capture: degraded (3 write errors, 12 queued)`), green/yellow/red; detail counters only when nonzero.
- **D-08:** `doctor` warns on nonzero history counters, fails (exit 1) only on live breakage (launchd missing, daemon down, DB not writable).
- **D-09:** Window-change screenshots throttled: 2 s cooldown + burst cap ≤3/10 s, configurable as `min_interval_secs` under `[capture]`.
- **D-10:** Hotkey annotation in new `notes` table (id, ts, note, screenshot_id?) written by daemon. `CaptureEventKind::Annotation` variant and flat daily log rejected.
- **D-11:** Hotkey UX shoot-first-prompt-second; Enter saves, Esc closes with no note. Screenshot never delayed by typing.
- **D-12:** Sleep/wake: discard missed interval ticks, take one screenshot on wake.
- **D-13:** First run preflights all three grants, requests only undetermined ones (native TCC dialogs, once); previously-denied → open System Settings at correct pane + print URL.
- **D-14:** Per-path degradation, daemon stays up: no Input Monitoring → keyboard thread doesn't start (titles/screenshots continue); no Screen Recording → screenshot triggers skipped (app/title continues).
- **D-15:** Re-probe grants every ~30 s and on every `status`/`doctor`; hot-attach where API permits, else say restart is required.
- **D-16:** Permission state in health banner; `doctor` prints per-permission fix (Settings path, restart note, `open` command).
- **D-17:** Idle via `CGEventSource.secondsSinceLastEventType` (hardware idle), threshold from config default 300 s.
- **D-18:** During idle capture continues; `SegmentGrouper` closes active segment at idle start, segment carries `idle: true`; reports exclude idle segments.
- **D-19:** Idle starts after 300 s no-input, ends only after 30 s sustained activity.
- **D-20:** During idle: interval + window-change screenshots suppressed, hotkey stays live, interval timer restarts at idle end.

### Agent's Discretion

- Config honesty (SYS-01): which unread fields get wired vs deleted; unknown keys must warn at startup.
- Single-instance lock mechanism (roadmap mandates lock + clear message; CONCERNS.md suggests `fs2`/`fd-lock` on pid file).
- Exact TCC probe APIs per permission, incl. hot-attach vs restart-required.
- `doctor`/`status` exact check list and output layout beyond D-07/D-08/D-16.

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope. Phase 2+ work (privacy, classification, search/export/retention, Ask/dashboard/MCP) must not leak into this phase.

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CAPT-01 | Daemon captures keystroke context + app/window titles continuously on macOS | Writer hardening (§Architecture Pattern 1), per-path degradation (§Pattern 3), keytap 0.4.0 keep [VERIFIED: Cargo.lock] |
| CAPT-02 | Screenshots every 5 min (configurable interval) during active use | Interval tick in daemon/scheduler task, `screenshot_interval_secs` new knob (§Standard Stack config) |
| CAPT-03 | Screenshot on every window/app change | Focus-loop trigger + D-09 throttle (2 s cooldown, ≤3/10 s burst) |
| CAPT-04 | Global hotkey → screenshot + annotation note, <200 ms visible feedback | Chord detection on existing keytap stream, `notes` table migration, shoot-first-prompt-second (D-10/D-11) |
| CAPT-05 | Survives sleep/wake, no non-ASCII/CJK/emoji panics, never silently drops events | Instant-gap wake detection (D-12), remaining 2 Unicode sites (§Pitfalls), blocking writer + retry queue (D-02…D-04), SIGTERM flush (D-05), task-death exit (D-06) |
| CAPT-06 | Correct permission states + onboarding + per-grant graceful degradation | TCC probe APIs (§Code Examples), D-13…D-16 behaviors |
| CAPT-07 | Idle detected and excluded from active segments | CGEventSource probe (D-17), `idle: true` marking (D-18), 300 s/30 s hysteresis (D-19), screenshot suppression (D-20) |
| SYS-01 | Every config field wired or removed; single-instance lock with clear message | Dead-field audit table (§Architecture), fs2 pid-file lock [ASSUMED], `#[serde(default)]` + unknown-key warn |
| SYS-02 | launchd auto-start with doctor/status confirming installed + running + DB writable | Plist fixes (log paths, backoff), `install` + `doctor` checks (§Code Examples) |

## Project Constraints (from AGENTS.md)

The repo `AGENTS.md` contains only graft graph-navigation instructions (ask/grep/skeleton/callers before reading source). No coding conventions, forbidden patterns, or required tools are declared there. Planner should still follow `.planning/codebase/CONVENTIONS.md` and `TESTING.md` per CONTEXT.md canonical refs.

## Architectural Responsibility Map

Single-machine daemon; all capabilities live in-process or in the CLI. "Tiers" here are process/thread boundaries.

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Keyboard/focus/screenshot sensing | Capture threads (`std::thread` in `PlatformCapture`) | — | Must never block on DB or IPC; emit typed events only |
| Durable event ledger + retry queue | Daemon writer task (owns `Store`) | — | Single writer is the SQLite correctness contract; queue lives on the writer thread, never the runtime |
| Segment fold + idle marking | `SegmentGrouper` (pure, in writer task) | — | Deterministic `on_event → Option<Segment>`; unit-testable with zero I/O |
| Permission probes + trigger scheduling | `PlatformCapture` / daemon scheduler task | CLI `doctor` (read-only re-probe) | Probes need the process's TCC identity; CLI re-probe only reports, never changes daemon state |
| Health/counters IPC | Daemon (`DaemonState` extension) | CLI `status`/`doctor`/TUI (render) | Wire-format change shared by daemon + all readers; extend the three IPC types together |
| launchd lifecycle | `com.rustwatch.plist` + `commands::{start,stop,install}` | Daemon SIGTERM handler | D-06 makes exit-nonzero the restart signal; `stop` must distinguish intentional stop from crash |

## Standard Stack

### Core (keep — no changes)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.x (`"1"`) [VERIFIED: Cargo.toml:37] | Async runtime, Unix-socket IPC, timers | Existing; screenshot interval tick + 30 s permission re-probe + 500 ms retry timer are `tokio::time::interval` |
| rusqlite | 0.32.1 `bundled` [VERIFIED: Cargo.lock] | All SQLite access via `Store` | Keep 0.32 for this phase — the 0.40 upgrade (FTS5 needs are Phase 4) must not ride along with reliability work [ASSUMED] |
| keytap | 0.4.0 [VERIFIED: Cargo.lock] | Global key tap (macOS CGEventTap) | Observe-only tap already integrated; hotkey chord detection reuses this stream — zero new deps/permissions [ASSUMED] |
| xcap | 0.9.8 [VERIFIED: Cargo.lock] | Window/monitor screenshots | Existing `capture_to_disk`; fix scope-mislabel fallback, don't replace backend |
| active-win-pos-rs | 0.11 [VERIFIED: Cargo.lock] | Focused app/window polling | Existing; `bundle_id` stays `None` this phase (bundle-id-from-PID is Phase 2 exclusion-hardening scope) |
| refinery | 0.8.16 [VERIFIED: Cargo.lock] | `rustwatch.db` migrations | `notes` table (D-10) ships as a new migration; compile-time embedded, no new infra |
| arboard | 3 [VERIFIED: crates/rustwatch-capture/Cargo.toml] | Clipboard read for Cmd+V paste events | Existing; unchanged |

### New (add — 2 crates)

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| fs2 `0.4` [ASSUMED] | latest 0.4.x at plan time (`cargo add fs2`) | Advisory exclusive lock on pid file for single-instance guard (SYS-01) | Daemon startup: `try_lock_exclusive`, second instance prints message + exits nonzero. Well-known crate (danburkert/fs2, `FileExt::lock_exclusive`) [CITED: https://tikv.github.io/doc/fs2/trait.FileExt.html] |
| core-graphics `0.23` [ASSUMED] | 0.23.2 already in lock file transitively [VERIFIED: Cargo.lock] | `CGEventSource.secondsSinceLastEventType` idle probe (D-17) | Promote to direct dep of `rustwatch-capture` (macOS target). Apple API: `secondsSinceLastEventType(_:_:)` returns elapsed time since last event for a Quartz event source [CITED: https://developer.apple.com/documentation/coregraphics/cgeventsource] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| fs2 pid-file lock | `fslock` crate, or socket-bind exclusivity | `fslock` is fine too; socket-bind alone can't print a clear "already running (pid N)" message. Either is acceptable — planner picks one, not both |
| Instant-gap wake detection | `nsworkspace-rs` NSWorkspace notifications, or IOKit `IORegisterForSystemPower` | NSWorkspace needs a runloop/AppKit presence awkward in a headless daemon [CITED: https://github.com/mishamyrt/nsworkspace-rs]; IOKit power notifications are the daemon-correct API [CITED: https://developer.apple.com/documentation/iokit/1557114-ioregisterforsystempower] but add objc/unsafe surface. Monotonic-clock gap detection (`tokio::time::Instant` jump > interval + slack ⇒ wake) needs zero deps, is fully unit-testable, and satisfies D-12 exactly — use it, defer IOKit |
| Chord-on-tap hotkey | `tauri-apps/global-hotkey` (Carbon RegisterEventHotKey) | A registered system hotkey works when the tap is starved, but adds a new dep + a second permission-adjacent surface. `KeyState` already tracks modifiers in-process; chord detection (e.g. Ctrl+Shift+M — exact chord is planner's call, must avoid colliding with app shortcuts) fires in <1 ms on the existing stream, well under the 200 ms budget. Use chord-on-tap; global-hotkey only if chord proves unreliable |

**Installation:**
```bash
cargo add -p rustwatch-daemon fs2 --vers 0.4
# core-graphics: add under [target.'cfg(target_os = "macos")'.dependencies] of rustwatch-capture
cargo add -p rustwatch-capture core-graphics --vers 0.23
cargo check --workspace   # confirm resolution before writing code
```

**Config changes (new real knobs under `[capture]`, all wired — SYS-01):**
```toml
[capture]
screenshot_interval_secs = 300      # CAPT-02, default 5 min; 0 disables interval shots
screenshot_on_focus_change = true   # existing, keep
min_interval_secs = 2               # D-09 window-change cooldown
idle_start_secs = 300               # D-17/D-19
idle_end_sustained_secs = 30        # D-19
```

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| fs2 | crates.io | ~9 yrs (est.) | high (est.) | github.com/danburkert/fs2 | OK-equivalent [ASSUMED] | Approved pending `cargo add` resolution — planner adds `checkpoint:human-verify` before first network fetch |
| core-graphics | crates.io | ~9 yrs (servo) | high | github.com/servo/core-graphics | OK-equivalent [ASSUMED] | Approved — already in Cargo.lock transitively at 0.23.2 [VERIFIED: Cargo.lock], promoting to direct dep adds no new supply-chain surface |

**Packages removed due to SLOP verdict:** none (no network verification run in this session; both crates are long-established, not typosquat-shaped).
**Packages flagged as suspicious:** none.
*`gsd_run query package-legitimacy` was not runnable from this session; planner must gate each `cargo add` behind a `checkpoint:human-verify` task per protocol.*

## Architecture Patterns

### System Architecture Diagram

```
keytap tap ──TextDelta/Key/Paste──┐
focus poll ──FocusChange───────────┤
interval tick ──Screenshot────────┼──▶ mpsc ──▶ WRITER TASK (owns Store + SegmentGrouper + retry queue)
window-change ──Screenshot────────┤         │          │          ├── insert_event / insert_segment / insert_screenshot
hotkey chord ──Screenshot+note────┘         │          │          └── SQLITE_BUSY/IO → retry queue (500 ms) → 10k cap, drop-oldest + counter
                                            │          └── SIGTERM: stop capture → flush grouper → drain 2 s → exit
idle probe ──▶ SegmentGrouper.mark_idle() ─┘
permission probes ──▶ DaemonState.health ──▶ Unix socket ──▶ status / doctor / TUI banner
launchd KeepAlive ──▶ restart on nonzero exit (D-06)
```

Reader trace: keystroke → channel → writer inserts event + folds segment → SIGTERM flushes tail. Permission loss on one path degrades only that path (D-14); writer never drops on IPC contention (D-02).

### Recommended Project Structure

No new crates. New/changed code lands in existing files:

```
crates/rustwatch-core/src/
├── db.rs            # busy_timeout + synchronous=NORMAL; notes table fns; parse_ts error propagation
├── ipc.rs           # DaemonState += write_errors, dropped_events, queued, capture_health, permissions
├── config.rs        # new [capture] knobs; #[serde(default)] everywhere; delete dead fields
├── segment.rs       # idle: bool marking (mark_idle/close-at-idle); already char-safe
└── migrations/V2__notes.sql  # notes table (D-10)
crates/rustwatch-daemon/src/main.rs  # blocking writer, retry queue, SIGTERM, fs2 lock, socket 0600 + frame cap + timeouts
crates/rustwatch-capture/src/platform/macos.rs  # probes, idle, triggers, hotkey chord, scope fix
crates/rustwatch-cli/src/commands.rs # start/stop/doctor/status/banner
```

### Pattern 1: Single writer owns everything persistent
**What:** The writer task is the only code that touches `Store`, `SegmentGrouper`, and the retry queue. Capture threads and IPC handlers never lock the store directly — IPC `Tail` reads via a short-lived separate `Store::open` (read-only handle) instead of sharing the writer's `Mutex`.
**When to use:** Always in this daemon. Sharing one `Arc<Mutex<Store>>` between writer and IPC is the root cause of the `try_lock` drops.
**Example:** replace `if let Ok(store) = writer_store.try_lock()` with `let store = writer_store.lock().await;` (D-02), and move `Tail` to `Store::open(&paths.sqlite)` + `busy_timeout` so a long scan can never stall the writer.

### Pattern 2: Pure fold, impure edges
**What:** `SegmentGrouper` stays pure (`on_event`, `flush`, `mark_idle_for(timestamp)`) — no clock, no I/O inside. Idle detection (poll `secondsSinceLastEventType`, compare against thresholds + 30 s sustained-activity hysteresis) lives in the capture/scheduler layer and injects synthetic `IdleStart`/`ActivityResumed` signals.
**When to use:** Idle marking, wake-gap segmentation, any future time-based fold.

### Pattern 3: Probe-then-degrade per capture path
**What:** At startup each path (keyboard tap, focus poll, screenshots) probes its grant independently; a failed probe disables only that path and records `path_status` in `DaemonState`. The 30 s re-probe loop re-attaches paths whose grants appeared (D-15); paths requiring restart print that explicitly.
**When to use:** All three TCC grants (D-14).

### Anti-Patterns to Avoid
- **`let _ =` on any persistence-adjacent call:** that token is the bug class being removed. Every insert/send gets error handling or a counter.
- **Sharing the writer's mutex with IPC:** causes the exact contention drops D-02 removes.
- **Timestamp substitution on parse failure:** `parse_ts` returning `Utc::now()` corrupts the time series; propagate the error like `map_event_row` already does [VERIFIED: crates/rustwatch-core/src/db.rs:252-256].
- **Widening screenshot scope silently:** Window-fallback-to-fullscreen must propagate actual scope or fail, never mislabel (privacy-sensitive).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Single-instance guard | PID-file existence check | `fs2::FileExt::try_lock_exclusive` on the pid file | Existence checks race and go stale (daemon never removes pid on crash [VERIFIED: daemon/main.rs:42, no removal]); advisory locks release on process death automatically |
| SQLite lock waits | Retry loops with sleep | `busy_timeout(5 s)` on every connection + WAL + `synchronous=NORMAL` | One pragma call covers daemon-vs-CLI contention; hand-rolled retry misclassifies errors |
| Char-boundary truncation | Byte-index slicing helpers | `str::is_char_boundary` loops (already landed in `segment.rs:143,158` [VERIFIED]) + `chars().take(n).collect()` | Byte slicing panics on CJK/emoji; the fixed pattern is already in-tree — copy it to the 2 remaining sites |
| Hotkey registration | Carbon event-hotkey plumbing | Chord detection on the existing keytap stream | Zero new deps, zero new permissions, sub-ms latency |
| Idle time source | Last-event-timestamp inference | `CGEventSource.secondsSinceLastEventType` | Self-inference is blind during the exact failure it must detect (D-17 rationale); hardware idle is one FFI call |
| Config forward-compat | Strict `toml::from_str` | `#[serde(default)]` on all config structs + warn-on-unknown-keys | Strict parsing bricks every existing `config.toml` on the next added key |

**Key insight:** Every hand-rolled primitive this phase could build (locking, retry, truncation, idle inference) already has a one-call standard replacement; the daemon's bugs came from hand-rolling them badly. The only genuinely new code is thin glue: retry-queue policy, probe wiring, trigger scheduling.

## Common Pitfalls

### Pitfall 1: Writer blocks the tokio runtime on SQLite
**What goes wrong:** `lock().await` + synchronous rusqlite inside an async task stalls the executor; IPC handlers starve.
**Why it happens:** `Store` is sync (`rusqlite::Connection` is `!Send`-safe only via mutex) and the writer is `tokio::spawn`ed.
**How to avoid:** Keep the writer as a dedicated task that does blocking work in small units, or move inserts to `spawn_blocking`; never hold the mutex across `.await` points other than the lock itself. IPC `Tail` uses its own short-lived connection.
**Warning signs:** `status` hangs while typing heavily; clippy/thread-block warnings.

### Pitfall 2: Retry queue retries the non-retryable
**What goes wrong:** Schema-drift or constraint errors loop every 500 ms forever, filling logs and masking the real breakage.
**Why it happens:** Matching on error string instead of error kind.
**How to avoid:** D-04 classification at enqueue time: retry only `rusqlite::Error::SqliteFailure` with `SQLITE_BUSY` code + IO errors; everything else → `error!` + `write_errors` counter immediately. Classify with `err.sqlite_error_code()`, not string contains [ASSUMED — rusqlite API, verify at compile time].

### Pitfall 3: Two remaining Unicode panic sites
**What goes wrong:** Daemon or CLI panics on CJK/emoji input.
**Why it happens:** `crates/rustwatch-cli/src/commands.rs:241` — `&hit.text[..120]` byte-slices a table cell [VERIFIED]; `crates/rustwatch-analyze/src/redact.rs:31` — `out.truncate(self.max_chars)` panics off-char-boundary [VERIFIED via CONCERNS.md:138; redact.rs not re-read this session — planner must confirm exact line]. (`segment.rs` site is already fixed [VERIFIED: segment.rs:137-165].)
**How to avoid:** Copy the in-tree pattern: `hit.text.chars().take(120).collect::<String>()`; floor-then-round-up-to-boundary truncation for `redact.rs`.
**Warning signs:** Any `..N]` slice or `.truncate(` on a `String` in new code — grep for both in review.

### Pitfall 4: `stop` kills the wrong process; `start` lies
**What goes wrong:** Stale pid file → SIGTERM to a recycled PID (unrelated process); `start` reports success while daemon exits instantly.
**Why it happens:** `commands.rs:61-75` sends SIGTERM to an unvalidated PID and deletes files without waiting [VERIFIED]; `start` at `:38-59` sleeps 500 ms and prints unconditionally [VERIFIED].
**How to avoid:** `stop`: `kill(pid, 0)` probe + process-name verify (via `ps`/`sysctl` or `/proc`), then poll for exit with timeout before removing socket/pid. `start`: poll socket `Ping` with deadline instead of fixed sleep; report failure with log tail.
**Warning signs:** Manual testing only on happy path; require a stale-pid test and a crashing-binary test.

### Pitfall 5: Interval timer fires a backlog after wake
**What goes wrong:** `tokio::time::interval` (with default `Burst` behavior it catches up) fires N stale screenshots at wake.
**Why it happens:** Missed ticks queue during sleep.
**How to avoid:** D-12: `set_missed_tick_behavior(MissedTickBehavior::Skip)` + discard logic + exactly one wake shot. Detect wake via `Instant` gap (elapsed >> interval ⇒ slept).
**Warning signs:** Screenshot burst rows with identical timestamps after lid-open.

### Pitfall 6: Permission re-probe spams TCC dialogs
**What goes wrong:** Re-requesting an already-denied grant every 30 s throws system dialogs at the user (or gets the app throttled by TCC).
**Why it happens:** Treating "denied" and "undetermined" as the same state.
**How to avoid:** D-13: request ONLY `undetermined` grants, exactly once (persist a `prompted_*` flag in config or state file); denied → Settings URL + `open` command, never re-prompt. The 30 s loop only *checks*, never *requests*.

### Pitfall 7: Socket hardening breaks the CLI contract
**What goes wrong:** Frame caps or timeouts turn large `Tail` replies into errors; `0600` socket breaks existing installs.
**Why it happens:** 16 MiB frame cap + `Tail { limit }` unbounded growth; permission change on an existing socket file.
**How to avoid:** Cap `Tail` limit server-side (e.g. 10k) and document it; `chmod 0600` after every bind (covers pre-existing sockets); add `tokio::time::timeout` on both ends with a clear "daemon busy" error, not a hang.

## Code Examples

### TCC permission probes (macOS, D-13/D-15)

```rust
// Source pattern: Apple TCC APIs; exact crate bindings verified at compile time [ASSUMED — needs `cargo check` on macOS target]
// - Accessibility: AXIsProcessTrustedWithOptions (prompt variant shows native dialog once)
// - Screen Recording: CGPreflightScreenCaptureAccess (10.15+; true = granted, and calling it REQUESTS if undetermined)
// - Input Monitoring: NO public TCC API exists — probe by attempting CGEventTapCreate;
//   a NULL tap with no other error means denied. (Standard approach; keytap::Tap::new()
//   failing at macos.rs:257 is today's accidental version of this probe — make it explicit
//   and record which path failed instead of `warn!` + silent death.)

// Request-once bookkeeping: persist `prompted_input_monitoring = true` etc. in config
// after the first request so the 30 s loop never re-prompts (Pitfall 6).
```

### Idle probe via core-graphics (D-17)

```rust
// Source: Apple docs [CITED: https://developer.apple.com/documentation/coregraphics/cgeventsource]
// `secondsSinceLastEventType(_:_:)` — "Returns the elapsed time since the last event
// for a Quartz event source." Poll every ~5 s from the scheduler task:
//   idle_secs = CGEventSource::secondsSinceLastEventType(combined_state, .anyInput)
// idle_start when idle_secs >= idle_start_secs (default 300);
// idle_end only after idle_secs < threshold continuously for idle_end_sustained_secs (30).
// core-graphics 0.23.2 already in Cargo.lock transitively [VERIFIED: Cargo.lock] —
// confirm the exact Rust binding name (`event_source_get_seconds_since_last_event_type` or
// associated fn) via docs.rs during planning; if the binding is missing, fall back to a
// 10-line `extern "C" fn CGEventSourceSecondsSinceLastEventType` declaration.
```

### Single-instance lock via fs2 (SYS-01)

```rust
// Source: fs2 docs [CITED: https://tikv.github.io/doc/fs2/trait.FileExt.html]
// Open (not create-truncate) the pid file, try_lock_exclusive, hold the File
// handle for the daemon's lifetime — the OS releases it on any death, crash included.
use fs2::FileExt;
let pid_file = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&paths.pid_file)?;
if pid_file.try_lock_exclusive().is_err() {
    eprintln!("rustwatchd is already running (pid file locked: {})", paths.pid_file.display());
    std::process::exit(2);
}
// THEN write pid, THEN bind socket. Never delete-then-bind (current race at daemon/main.rs:32-34 [VERIFIED]).
```

### SQLite pragmas on every connection (all three DBs)

```rust
// Add to Store::open after Connection::open (db.rs:21-22 currently sets only WAL [VERIFIED]):
conn.pragma_update(None, "journal_mode", "WAL")?;
conn.pragma_update(None, "synchronous", "NORMAL")?;   // safe under WAL; fixes fsync-bound writer
conn.busy_timeout(std::time::Duration::from_secs(5))?;
```

### Char-boundary truncation (copy the in-tree fix)

```rust
// Source: already-fixed crates/rustwatch-core/src/segment.rs:137-165 [VERIFIED] —
// apply the same two idioms to commands.rs:241 and redact.rs:31:
//   snippet: hit.text.chars().take(120).collect::<String>()
//   floor: while start < s.len() && !s.is_char_boundary(start) { start += 1; }
```

### launchd plist fixes (SYS-02)

```xml
<!-- deploy/macos/com.rustwatch.plist changes [VERIFIED current content: 20 lines, /tmp logs, bare KeepAlive] -->
<!-- 1. StandardOutPath/StandardErrorPath → ~/Library/Logs/rustwatch/ (symlink-safe, not world-readable) -->
<!-- 2. KeepAlive → dict with SuccessfulExit=false so clean `stop` (exit 0) stays stopped but crashes restart (D-06) -->
<!-- 3. Add ThrottleInterval (e.g. 30) to bound KeepAlive restart storms -->
<!-- 4. Document OPENAI_API_KEY absence under launchd (no EnvironmentVariables today) — print a warning in doctor -->
```

### `doctor` check list (SYS-02 + D-08/D-16)

```
launchd plist installed? (~/Library/LaunchAgents/com.rustwatch.plist exists + loaded via launchctl list)
daemon socket Ping within timeout? (distinguishes "not running" from "wedged")
DB writable? (open + `PRAGMA quick_check` or probe write in a transaction + rollback)
socket/pid file ownership sane? (0600/0700 note)
per-permission state + fix (Settings path + `open` URL + restart-needed note)
WARN (not fail) on nonzero write_errors/dropped_events history
screenshots dir writable + disk-space note (retention itself is Phase 4)
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `try_lock`-and-drop writer | Blocking `lock().await` + bounded retry queue | This phase (D-02…D-04) | Loss becomes visible/countable instead of silent |
| Hardcoded `permissions()` all-false | Real TCC probes per grant | This phase (CAPT-06) | `permissions`/`doctor` become trustworthy |
| Notify-when-prompted sleep handling | Instant-gap detection + Skip-tick + one wake shot | This phase (D-12) | No new deps, fully testable without sleeping hardware |
| PID-existence single-instance | fd advisory lock held for process lifetime | This phase (SYS-01) | Crash-safe, race-free, clear second-instance message |

**Deprecated/outdated:**
- `Config::default()` literal `"~/.rustwatch"` path (`config.rs:63` [VERIFIED]) — must not be read directly once `data.*` fields are wired; route through `expand_tilde`/`DataPaths`.
- Deleting the socket file unconditionally at startup (`daemon/main.rs:32-34` [VERIFIED]) — replaced by lock-then-bind ordering.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | fs2 0.4 API is `FileExt::try_lock_exclusive` and macOS-prompt behavior as described | Standard Stack, Code Examples | Low — compile check catches it; fallback `fslock` or socket-bind |
| A2 | No public TCC API for Input Monitoring; CGEventTapCreate-NULL probe is the standard approach | Code Examples | Medium — if a better API exists, probe code changes but D-13/D-14 behavior doesn't |
| A3 | core-graphics 0.23 exposes the idle-seconds binding (else 10-line `extern "C"` fallback) | Code Examples | Low — fallback is trivial and documented inline |
| A4 | `rusqlite` 0.32 exposes `sqlite_error_code()` / `Error::SqliteFailure` for retry classification | Pitfalls | Low — compile check catches it; string-match fallback documented |
| A5 | redact.rs:31 is still the `truncate` panic site (not re-read this session) | Pitfalls | Low — planner greps to confirm before planning 01-01 |
| A6 | Chord-on-tap hotkey meets the 200 ms budget and survives typical use | Standard Stack | Medium — if tap starvation delays the chord, fall back to `global-hotkey` crate in 01-02 |
| A7 | `MissedTickBehavior::Skip` exists on the tokio version in lock file | Pitfalls | Low — compile check; manual last-tick comparison fallback |
| A8 | Version numbers for fs2/fslock recency, download counts | Package Audit | Low — `cargo add` resolves; human-verify checkpoint gates it |

## Open Questions

1. **Exact hotkey chord**
   - What we know: Must be a chord detectable on the keytap stream, unlikely to collide with app shortcuts, and recordable without Accessibility beyond what keytap already needs.
   - What's unclear: Which chord (Ctrl+Shift+M? Fn-based?) — user wasn't asked.
   - Recommendation: Planner picks a default (e.g. Ctrl+Shift+Space — verify against common bindings), makes it a config knob from day one.
2. **Screenshot interval tick placement**
   - What we know: Needs wake-gap + idle-suppression awareness; daemon scheduler task is the natural home.
   - What's unclear: Whether interval shots route through the same mpsc as capture events or write directly.
   - Recommendation: Same channel (uniform retry/loss accounting); planner decides.
3. **Which dead config fields get wired vs deleted**
   - What we know: Explicitly the agent's discretion; SYS-01 rule is wire-or-remove + unknown-key warn.
   - What's unclear: 9+ fields listed in CONTEXT.md discretion (note: `accessibility_poll_ms` IS forwarded today via `start()` [VERIFIED: macos.rs:57-64] thoughapor prefixed `_accessibility_poll_ms` unused at `:61` — planner to verify each field against current source, since CONCERNS.md predates recent fixes).
   - Recommendation: Planner runs a fresh dead-field grep (CONCERNS.md is dated 2026-10-02 and already stale on exclusions) and applies wire-or-remove per field.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo/rustc | All plans (build + test) | ✓ | 1.97.1 | — |
| macOS (darwin target) | Capture backends, TCC probes, xcap/keytap | ✓ (platform is darwin) | — | Non-macOS builds use stub backend (out of scope for v1) |
| Input Monitoring grant | CAPT-01 keyboard capture (manual verify) | ? (unknown for this shell) | — | D-14 degradation; `doctor` reports |
| Screen Recording grant | Screenshots (manual verify) | ? | — | D-14 degradation; CLI-direct screenshot fallback exists |
| crates.io network | `cargo add` fs2 / core-graphics direct | ? (not probed) | — | Vendor or pin from lock file; human-verify checkpoint |

**Missing dependencies with no fallback:** none for planning; TCC grants are user-environment state handled by D-13/D-14 at runtime.
**Missing dependencies with fallback:** crates.io reachability — planner's Wave 0 `cargo check` determines whether new deps resolve.

## Security Domain

> security_enforcement is enabled; ASVS level 1, block on high.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Single-user local tool; no auth surface. Socket peer restriction below covers the adjacent risk |
| V3 Session Management | No | No sessions |
| V4 Access Control | Yes | Socket `0600` + data-root `0700` + (recommended) peer-UID check before dispatching `Tail`/`Screenshot` (full keystroke/screen history readable over the socket today) |
| V5 Input Validation | Yes | IPC frame cap 16 MiB (4 GiB alloc DoS today [VERIFIED: ipc.rs:76-77]); timeouts on both IPC ends; `Tail.limit` server-side cap; char-boundary truncation |
| V6 Cryptography | No new crypto | No encryption-at-rest this phase (documented gap, Phase 2 threat-model decision) |
| V14 Configuration | Yes | SYS-01 wire-or-remove + unknown-key warn; `#[serde(default)]`; no secrets in config (env-only keys already the pattern) |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Unbounded IPC frame → 4 GiB alloc OOM | Denial of service | 16 MiB cap, reject with error reply |
| Stale pid → SIGTERM to unrelated process | Tampering / Elevation-adjacent | `kill(pid,0)` + name verify + poll-for-exit before file removal |
| Socket readable by other local users → full capture history leak | Information disclosure | `0700` root, `0600` socket, peer-UID check |
| LaunchAgent `/tmp` log symlink attack | Tampering | Move logs to `~/Library/Logs/rustwatch/` |
| Crash-loop KeepAlive storm on poison input | Denial of service | `SuccessfulExit=false` + `ThrottleInterval`, loud error log |

Out-of-phase (do NOT implement now, planner must not schedule): MCP/export/TUI redaction boundaries, write-path redaction, exclusion enforcement gaps — all Phase 2.

## Sources

### Primary (HIGH confidence — read this session)
- `crates/rustwatch-daemon/src/main.rs:1-164` — writer task, IPC handler, socket/pid lifecycle [VERIFIED]
- `crates/rustwatch-capture/src/platform/macos.rs:1-786` — exclusion enforcement present, char-state, focus loop, screenshot fallback [VERIFIED]
- `crates/rustwatch-core/src/{segment,keystroke,db,ipc,paths,config}.rs` — fold safety, pure translation/exclusion, store pragmas, wire types, path authority, dead-field definitions [VERIFIED]
- `crates/rustwatch-cli/src/commands.rs:1-292` — start/stop/status/permissions reality [VERIFIED]
- `.planning/phases/01-reliable-capture-system-foundation/01-CONTEXT.md` — D-01…D-20 locked decisions [VERIFIED]
- `.planning/{PROJECT,ROADMAP,REQUIREMENTS}.md`, `research/SUMMARY.md`, `codebase/CONCERNS.md` — scope, ordering, defect catalog [VERIFIED]

### Secondary (MEDIUM confidence — websearch, official/primary sources)
- Apple Developer Documentation, CGEventSource — idle-seconds API exists [CITED: https://developer.apple.com/documentation/coregraphics/cgeventsource]
- fs2 FileExt docs (tikv.github.io) — whole-file exclusive locks [CITED: https://tikv.github.io/doc/fs2/trait.FileExt.html]
- IOKit IORegisterForSystemPower docs — daemon-correct sleep/wake API (deferred) [CITED: https://developer.apple.com/documentation/iokit/1557114-ioregisterforsystempower]
- nsworkspace-rs repo — NSWorkspace listener crate (deferred, needs runloop) [CITED: https://github.com/mishamyrt/nsworkspace-rs]
- crates.io keytap page + docs.rs keytap — observe-only tap, 0.4 current [CITED: https://crates.io/crates/keytap]

### Tertiary (LOW confidence — unverified, flagged in Assumptions Log)
- TCC Input-Monitoring probe idiom (no official doc found this session); exact core-graphics Rust binding name; AX/ScreenRecording prompt-once semantics across macOS versions — all need compile-time + hardware verification in 01-03.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all keep-decisions verified in Cargo.toml/lock; two additions are established crates with documented fallbacks.
- Architecture: HIGH — patterns derive from locked CONTEXT.md decisions + verified current code; prior fixes (exclusion, truncation, ensure_segment) confirmed in-source.
- Pitfalls: HIGH for codebase-evidenced items (re-read this session); MEDIUM for macOS-API items (web-sourced, compile verification pending).

**Research date:** 2026-10-03
**Valid until:** 2026-11-03 (stable domain; macOS API details re-verify at plan time via `cargo check` on the macOS target)
