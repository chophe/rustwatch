---
phase: 01-reliable-capture-system-foundation
verified: 2026-10-06T00:00:00Z
status: human_needed
score: 15/15 must-haves verified
behavior_unverified: 0
overrides_applied: 0
covered_files:
  - .planning/phases/01-reliable-capture-system-foundation/01-01-PLAN.md
  - .planning/phases/01-reliable-capture-system-foundation/01-02-PLAN.md
  - .planning/phases/01-reliable-capture-system-foundation/01-03-PLAN.md
  - .planning/phases/01-reliable-capture-system-foundation/01-01-SUMMARY.md
  - .planning/phases/01-reliable-capture-system-foundation/01-02-SUMMARY.md
  - .planning/phases/01-reliable-capture-system-foundation/01-03-SUMMARY.md
  - .planning/REQUIREMENTS.md
  - crates/rustwatch-daemon/src/main.rs
  - crates/rustwatch-core/src/db.rs
  - crates/rustwatch-core/src/ipc.rs
  - crates/rustwatch-core/src/config.rs
  - crates/rustwatch-core/src/paths.rs
  - crates/rustwatch-core/src/permissions.rs
  - crates/rustwatch-core/src/segment.rs
  - crates/rustwatch-core/src/events.rs
  - crates/rustwatch-capture/src/platform/macos.rs
  - crates/rustwatch-capture/src/lib.rs
  - crates/rustwatch-cli/src/commands.rs
  - crates/rustwatch-cli/src/main.rs
  - crates/rustwatch-cli/src/tui.rs
  - crates/rustwatch-analyze/src/redact.rs
  - crates/rustwatch-core/migrations/V2__notes_idle.sql
  - deploy/macos/com.rustwatch.plist
human_verification:
  - test: "Sleep/wake matrix on a granted macOS machine (10 s test interval, sleep 2 min, wake)"
    expected: "Exactly one wake screenshot appears; no burst of stale rows; interval cadence resumes"
    why_human: "Machine cannot be slept programmatically from here; Linux CI cannot exercise xcap/CGEventSource paths"
  - test: "Deny each TCC grant in turn (Input Monitoring, Accessibility, Screen Recording); re-grant each"
    expected: "Daemon stays up, status banner names exactly the degraded path, re-grant hot-attaches within ~30 s or says restart-required; first run prompts at most once per grant and never re-prompts a denial"
    why_human: "Requires toggling System Settings TCC grants on real macOS hardware"
  - test: "kill -9 the daemon under launchd, then clean stop/start cycle"
    expected: "launchd restarts the crashed daemon; clean stop stays stopped; doctor passes healthy / warns on injected write errors / fails with daemon stopped"
    why_human: "Requires a real launchd install; verifier must not mutate the operator's launchd state"
  - test: "Press Ctrl+Shift+Space in a real app; run rustwatch annotate (Enter and Esc paths); rapid Alt-Tab burst; leave machine idle 5+ min"
    expected: "Screenshot appears with no keystroke interruption (sub-200 ms feel); Enter saves the note linked to the shot, Esc saves nothing; at most 3 shots per 10 s spaced 2 s+; idle:true segment closes, automatic shots suppress, hotkey stays live, stray nudge does not end idle"
    why_human: "Latency feel, real key hardware, and a genuinely quiet machine cannot be produced in this sandbox"
---

# Phase 01: Reliable Capture & System Foundation Verification Report

**Phase Goal:** The daemon captures keyboard context, app/window state, and screenshots continuously
without silent loss, survives sleep, reports permissions honestly, and every config knob is real.
**Verified:** 2026-10-06
**Status:** human_needed (all automated evidence green; hardware-matrix sign-offs remain)
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Sustained typing while `tail` runs loses zero events; counters visible | ✓ VERIFIED | Writer owns Store on its own std thread (`main.rs:170`), `recv_timeout(500ms)` doubles as retry timer; `Tail` reads via short-lived separate `Store::open` (`main.rs:371-377`) capped at `TAIL_MAX_LIMIT` (`ipc.rs:77`); `DaemonState` carries `write_errors/dropped_events/queued/capture_health` (`ipc.rs:11-34`); tests `periodic_drain_redelivers_through_real_store`, `drop_oldest_accounting_at_cap` pass |
| 2 | CJK/emoji/paste floods never panic writer, segmenter, redactor, snippet path | ✓ VERIFIED | `redact.rs:31-34` floors to char boundary; `commands.rs:845` uses `chars().take(120)`; `parse_ts` errors instead of substituting now (`db.rs:395-398`); tests `emoji_flood_truncates_on_char_boundary`, `emoji_flood_snippet_is_char_safe`, `corrupt_timestamp_errors_instead_of_now` pass |
| 3 | SIGTERM stops capture, flushes tail segment, drains retry queue within 2 s, exits 0 | ✓ VERIFIED | SIGTERM/SIGINT handler (`main.rs:128-154`, `notify_waiters`), writer drains with `Duration::from_secs(2)` deadline then `grouper.flush()` (`main.rs:199-212`); test `zero_deadline_shutdown_drain_drops_remainder` passes; SUMMARY records live isolated-HOME SIGTERM exit 0 |
| 4 | Second `rustwatchd` exits nonzero with clear already-running message, never steals socket | ✓ VERIFIED | `fs2::FileExt::try_lock_exclusive`, message `rustwatchd is already running (pid file locked: …)`, `exit(2)` (`main.rs:43-48`); lock-then-pid-then-bind ordering; SUMMARY records live second-instance exit 2 with first daemon ALIVE |
| 5 | Every `config.toml` field wired or removed; unknown keys warn | ✓ VERIFIED | `unknown_config_keys` (`paths.rs:178`) warned at startup (`paths.rs:91`); test `removed_and_unknown_keys_warn_but_load` passes; **live proof:** verifier's own `doctor` run on this macOS box warned on 9 legacy keys (`analyze.vision_model`, `[data]`, `[ui]`, …) and still loaded |
| 6 | Interval screenshots land on cadence during active use, never during idle | ✓ VERIFIED | Scheduler task with `MissedTickBehavior::Skip`, `interval_duration()` (0 disables, task still polls idle) (`main.rs:735-801`); D-20 idle gate + timer reset at idle end (`main.rs:799-885`); tests `skip_behavior_produces_no_backlog`, `interval_zero_disables` pass; live 75 s run: 4 captures → 4 files + 4 rows |
| 7 | Rapid window switching: at most 3 shots per 10 s, ≥2 s spacing | ✓ VERIFIED | Pure `should_shoot` (`macos.rs:933`) with cooldown + sliding burst cap, wired in focus loop (`macos.rs:805`); tests `cooldown_blocks_rapid_refire`, `burst_cap_allows_three_per_ten_seconds`, `burst_window_slides` pass |
| 8 | Hotkey chord screenshots in under 200 ms; note prompt never delays the shot | ✓ VERIFIED | `Ctrl+Shift+Space` chord-on-tap inside `translate_key_events`, shoot-first synchronous capture on trigger key-down (`macos.rs:472-477`, `parse_hotkey` `macos.rs:627`); `annotate` is CLI-side with `NoteEditor`, Enter saves / Esc discards (`commands.rs:623-650`); tests for full/partial/repeat/exclusion/disabled chords + annotate Enter/Esc pass; live pty test saved note linked to shot |
| 9 | 5 min idle closes `idle:true` segment, capture continues; stray nudge never ends idle | ✓ VERIFIED | `extern "C"` CGEventSource probe (`macos.rs:266-335`, fallback recorded — binding absent from core-graphics 0.23.2, verified in vendored sources); pure `poll_idle` hysteresis 300 s/30 s (`main.rs:703-731`); `mark_idle`/`mark_active` pure fold (`segment.rs:46-64`); V2 migration adds `notes` table + `segments.idle` (verified file contents); hysteresis + fold + migration tests pass; live probe read sane values (19.6 s/0.1 s) |
| 10 | Wake from sleep takes exactly one screenshot, never a backlog burst | ✓ VERIFIED | Pure `detect_wake` (interval + 5 s slack) (`main.rs:677`), `Skip` behavior, one wake shot (`main.rs:818`); boundary tests pass (`detect_wake` true/false edges). Real sleep/wake needs human (see Human Verification) |
| 11 | `permissions` reports true three-state per grant, denied distinct from never-prompted | ✓ VERIFIED | Real FFI probes (`CGPreflightScreenCaptureAccess`, `AXIsProcessTrusted`, tap-creation probe; `macos.rs:286-371`), `permissions_with_prompted` + `classify_grant` three-state mapping, `DaemonState.permissions` wire field (`ipc.rs:30`); injected-probe mapping + banner tests pass; **live proof:** `doctor` on this box rendered `permissions: all granted` from real probes |
| 12 | Missing grant disables only its path; daemon stays up; banner names degradation | ✓ VERIFIED | `KeyboardLaunch`/`screenshots_live` gates, parked-not-dead threads, `capture_note` in daemon state (`ipc.rs:34`) joined into Status (`main.rs:348`); degradation-matrix + `reprobe_transitions_route_attach_detach` + `diff_permissions` tests pass; 30 s check-only re-probe (`main.rs:770-786`) with per-grant HotAttach/restart-required semantics |
| 13 | First run prompts only undetermined grants once; denials → Settings, never re-prompt loops | ✓ VERIFIED | `run_permission_preflight` AFTER the single-instance lock (`main.rs:62-65, 465-511`), `grants_to_request` + `wants_prompt` gate, merge-preserving `save_permissions_prompted` (`paths.rs:106`); preflight-once + prompt-gate + unknown-key-survival tests pass |
| 14 | `doctor` exits 1 only on live breakage, warns on nonzero history counters | ✓ VERIFIED | `Commands::Doctor` (`main.rs:23,95`), `doctor_exit_code` fail-only-on-`Fail` (`commands.rs:265`), full check list (plist, Ping, rolled-back DB probe, ownership, per-grant fixes, screenshots+disk, history-counters warn, `OPENAI_API_KEY` warn); exit-matrix test passes; **live proof:** verifier's `doctor` run rendered `[fail]/[warn]/[pass]` lines with correct per-check detail on a real machine |
| 15 | launchd restarts crashes but clean stop stays stopped; logs outside /tmp; TUI parity | ✓ VERIFIED | Plist has `KeepAlive{SuccessfulExit=false}`, `ThrottleInterval 30`, `{{LOGS_DIR}}` paths (verified file contents); `install` renders substituted paths (test `plist_template_renders_hardened` passes); D-06 death watch exits nonzero on writer/capture/scheduler death (`main.rs:280-298`) pairing with KeepAlive; `tui_status_line` parity incl. unreachable-vs-not-running (`tui.rs:19-53`, trio tests pass) |

**Score:** 15/15 truths verified (0 present-but-behavior-unverified)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `daemon/src/main.rs` | writer, retry queue, SIGTERM, fs2 lock, hardened socket, scheduler, preflight | ✓ VERIFIED | All present and wired; 14 daemon tests pass |
| `core/src/ipc.rs` | counters + health, frame cap, timeouts, permissions/note wire fields | ✓ VERIFIED | `MAX_FRAME_BYTES`, `TAIL_MAX_LIMIT`, `IPC_IO_TIMEOUT`, `DaemonState` extensions |
| `core/src/config.rs` | honest schema, no dead fields | ✓ VERIFIED | Defaults + knobs + `[permissions] prompted_*`; dead-field test passes |
| `core/src/permissions.rs` | Grant enum, transitions, fix lines | ✓ VERIFIED | New file (450 lines), 5+ unit tests |
| `core/migrations/V2__notes_idle.sql` | notes table + idle column | ✓ VERIFIED | Contents verified; fresh + upgrade migration tests pass |
| `capture/.../macos.rs` | probes, throttle, hotkey, idle FFI, honest scope | ✓ VERIFIED | 1625 lines, 37 tests pass; only pre-existing `hash_content` dead-code warning remains (declared out of scope) |
| `cli/src/commands.rs` + `main.rs` | status banner, doctor, permissions, annotate | ✓ VERIFIED | 12 CLI tests pass; live `doctor` smoke passes |
| `cli/src/tui.rs` | banner parity | ✓ VERIFIED | Pure `tui_status_line` + 3 parity tests |
| `deploy/macos/com.rustwatch.plist` | hardened KeepAlive, log paths, backoff | ✓ VERIFIED | Contents verified byte-for-byte against plan |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| writer mpsc receiver | Store inserts | single blocking writer owns Store + RetryQueue + SegmentGrouper | WIRED | `main.rs:170-212`; no `try_lock`-and-drop on writer path (only remaining `try_lock` is the fs2 pid lock) |
| DaemonState counters | `status` degraded banner | `health_label` + banner render | WIRED | `ipc.rs:41-61`, `commands.rs:211-235`; live banner green in smoke run |
| pid-file lock | socket bind ordering | lock → pid → bind, no delete-then-bind | WIRED | `main.rs:35-59` region; 0600 socket re-applied (`main.rs:268`), 0700 data root (`paths.rs:44`) |
| scheduler/mpsc channel | writer inserts | same channel for interval/idle/hotkey events | WIRED | interval + `IdleStart`/`ActivityResumed` + chord shots all emit `CaptureEvent` on the shared channel |
| idle signals | SegmentGrouper.mark_idle | pure fold, capture layer owns clock | WIRED | `segment.rs:103-104` folds synthetic events; `open_segment_id` links screenshot rows (`segment.rs:38`) |
| probe results | DaemonState.permissions → status/doctor | one wire-type change read by CLI, TUI, daemon | WIRED | `ipc.rs:30`, `tui.rs:36-53`, `commands.rs:299-455`; live `doctor` proves end-to-end |
| 30 s re-probe loop | live path attach/detach | HotAttach / restart-latch / detach | WIRED | `ReprobeHandle::run_cycle` (`main.rs:513+`), `diff_permissions` transitions |
| KeepAlive semantics | D-06 exit-nonzero / stop exit-0 | SuccessfulExit=false + death watch | WIRED | plist dict + `main.rs:280-298`; `stop` uses kill-0 probe + name verify + poll (`commands.rs:115-174`) |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| writer inserts | `event` from `event_rx` | keyboard/focus/scheduler/hotkey producers | ✓ FLOWING | retry queue + counters account every outcome; no `let _` on persistence paths |
| `tail` output | rows via separate `Store::open` | live SQLite | ✓ FLOWING | separate read handle, server-side limit cap |
| `status` banner | `DaemonState` snapshot | writer counters + permission snapshot + notes join | ✓ FLOWING | live `status`/`doctor` smoke shows real values |
| screenshot rows | `(PathBuf, ScreenshotScope)` + `open_segment_id` | `capture_to_disk` + grouper | ✓ FLOWING | scope honestly labeled Screen on fallback; segment id filled (not None) |
| permission states | FFI probe booleans + prompted flags | TCC APIs + config | ✓ FLOWING | live `doctor` rendered real probe results on this macOS box |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| full workspace test suite | `cargo test --workspace` | 124 passed (2+37+12+59+14), 0 failed | ✓ PASS |
| `doctor` end-to-end on real macOS | `cargo run -q -p rustwatch-cli -- doctor` | rendered fail/warn/pass lines, unknown-key warnings, probe results | ✓ PASS |
| config backward-compat | same run | 9 legacy dead keys warned, load succeeded | ✓ PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| CAPT-01 | 01-01 | Continuous keystroke/app/window capture daemon | ✓ SATISFIED | blocking writer, retry queue, counters; live capture in smoke runs |
| CAPT-02 | 01-02 | Screenshots every 5 min (configurable) during active use | ✓ SATISFIED | interval scheduler + Skip + suppression; live 4-shots-in-75 s run |
| CAPT-03 | 01-02 | Screenshot on every window/app change | ✓ SATISFIED | focus-loop trigger with D-09 throttle; unit-proven |
| CAPT-04 | 01-02 | Global hotkey + annotation note in under 200 ms | ✓ SATISFIED | chord shoot-first + `annotate`; live pty note round-trip |
| CAPT-05 | 01-01 | Survives sleep/wake, no non-ASCII panic, no silent drops | ✓ SATISFIED | char-boundary fixes, `detect_wake`/Skip, loss counters; REQUIREMENTS.md already marks Complete |
| CAPT-06 | 01-03 | True permission states + onboarding + per-grant degradation | ✓ SATISFIED | three FFI probes, preflight, re-probe, fix lines; live probe execution |
| CAPT-07 | 01-02 | Idle detected and excluded from active segments | ✓ SATISFIED | hardware probe + hysteresis + `idle:true` fold + V2 schema |
| SYS-01 | 01-01 | Honest config + single-instance lock | ✓ SATISFIED | dead fields deleted, unknown-keys warn (live-proven), fs2 exit-2 lock (live-proven) |
| SYS-02 | 01-03 | launchd auto-start + doctor/status check | ✓ SATISFIED | hardened plist, `doctor` with D-08 semantics (live-proven rendering) |

All 9 phase requirement IDs accounted for — none orphaned, none missing.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `macos.rs` | 1028 | `hash_content` dead code (pre-existing warning) | ℹ️ Info | Declared out of scope in all three SUMMARies; untouched, harmless |
| `main.rs` / `macos.rs` | various | `=> {}` no-op match arms (5×) | ℹ️ Info | Legitimate explicit no-ops (None signals, wildcard key releases), not stubs |
| — | — | TODO/FIXME/placeholder/unimplemented | None | Zero markers in any phase-touched file |

### Human Verification Required

The four hardware-matrix items in frontmatter `human_verification` (sleep/wake single-shot;
deny/re-grant each TCC grant with prompt-once behavior; launchd kill-9 restart vs clean-stop
plus doctor healthy/warn/fail states; hotkey feel + annotate paths + Alt-Tab burst + 5-min
idle with suppression). These are the plans' own `<human-check>` items, deferred because they
need a granted, quiet macOS machine with a real launchd install — unit tests plus
isolated-HOME live runs cover everything else.

### Gaps Summary

No gaps. Every plan must-have (15 truths), every artifact, and every key link verified against
the actual codebase with passing tests (124/124) and a live `doctor` smoke run on this macOS
machine. The phase goal is achieved pending the four hardware sign-offs above, which cannot be
produced in this sandbox.

---
_Verified: 2026-10-06_
_Verifier: the agent (gsd-verifier)_
