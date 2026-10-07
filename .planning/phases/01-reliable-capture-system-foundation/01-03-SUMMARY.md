---
phase: 01-reliable-capture-system-foundation
plan: 03
subsystem: permissions
tags: [tcc, doctor, launchd, permissions, degradation, onboarding, tui, plist]

# Dependency graph
requires:
  - phase: 01-01-ledger
    provides: [DaemonState counters/banner wire format, parked-not-dead capture threads, "[permissions] prompted_*" schema, safe start/stop]
  - phase: 01-02-triggers
    provides: [scheduler task (interval/idle), focus-loop throttle, hotkey chord, segment-linked screenshots]
provides:
  - True three-state permission reporting (granted/denied/undetermined) from TCC probes to banner
  - Per-path degradation with 30 s check-only re-probe (HotAttach / restart-required / detach)
  - First-run preflight (request-only-undetermined-once, persisted flags, Settings fix for denials)
  - "rustwatch doctor" with D-08 exit semantics + hardened launchd plist + TUI banner parity
affects: [phase-5-reports-permission-status, phase-2-exclusion-hardening, ship-gate-doctor]

# Actuals (#2632) — pairs with the plan's `estimate` to calibrate future estimates.
actuals:
  tokens: 25000
  tasks: 3
  commits: 3
  plan_head_before: 21e50c5

# Tech tracking
tech-stack:
  added: []
  patterns: [grant-as-data (Grant enum owns names/panes/attach semantics), pure-transition-fns-over-injected-states, check-only-reprobe-plus-request-once-preflight, merge-preserving prompted-flag save, split-note-ownership (thread vs re-probe)]

key-files:
  created:
    - .planning/phases/01-reliable-capture-system-foundation/01-03-SUMMARY.md
  modified:
    - crates/rustwatch-capture/src/platform/macos.rs
    - crates/rustwatch-capture/src/platform/stub.rs
    - crates/rustwatch-capture/src/lib.rs
    - crates/rustwatch-core/src/permissions.rs
    - crates/rustwatch-core/src/ipc.rs
    - crates/rustwatch-core/src/config.rs
    - crates/rustwatch-core/src/paths.rs
    - crates/rustwatch-core/src/lib.rs
    - crates/rustwatch-daemon/src/main.rs
    - crates/rustwatch-cli/src/commands.rs
    - crates/rustwatch-cli/src/main.rs
    - crates/rustwatch-cli/src/tui.rs
    - deploy/macos/com.rustwatch.plist

key-decisions:
  - "No new crates: Accessibility probe/request declared as 10-line extern C fallback (ApplicationServices has no binding in tree) — tracer STOP rule honored, T-03-SC clean"
  - "Preflight runs AFTER the single-instance lock so only the real daemon ever triggers a native dialog"
  - "Screen Recording attach is restart-required (latched); tap paths hot-attach in-process — encoded once in Grant::restart_required_on_attach, consumed by daemon + doctor alike"
  - "Denied/undetermined never re-prompts: prompted flags persist via toml::Value merge (unknown/future keys survive), the 30 s loop only CHECKS"
  - "doctor prints the open command but never executes it — no command-injection surface from static pane URLs"

patterns-established:
  - "Permission logic lives in pure fns over injected states (classify_grant, grants_to_request, grant_transition, diff_permissions, paths_live, permission_fix_lines, doctor_exit_code, tui_status_line) — every behavior unit-tested without TCC/hardware/terminal"
  - "A dead path is daemon-state fact (capture_note), never a lone log warning — keyboard thread owns keyboard_note, re-probe owns notice, Status joins them"
  - "Doctor FAILS only on live breakage; every degraded-but-running state warns and exits 0"

requirements-completed: [CAPT-06, SYS-02]

coverage:
  - id: D1
    description: "permissions reports the true three-state value per grant, denied distinct from never-prompted"
    requirement: "CAPT-06"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-core#granted_probe_always_granted + unprompted_false_is_undetermined_prompted_false_is_denied + cargo test -p rustwatch-capture#injected_screen_probe_maps_to_three_states"
        status: pass
    human_judgment: true
    rationale: "Injected-probe mapping is proven; true on-hardware grant vs deny vs revoke flows need a human toggling System Settings (plan human-check)."
  - id: D2
    description: "A missing grant disables only its path; daemon stays up; re-probe hot-attaches, latches restart-required, or detaches"
    requirement: "CAPT-06"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-core#degradation_matrix_is_per_path + reprobe_transitions_route_attach_detach + snapshot_diff_collects_simultaneous_changes"
        status: pass
      - kind: other
        ref: "live: isolated-HOME daemon captured events + screenshot with all grants; status banner green with no note"
        status: pass
    human_judgment: true
    rationale: "Deny-each-grant-in-turn with daemon-up + named degradation + ~30 s re-grant attach needs a human on a grant-capable machine."
  - id: D3
    description: "First run prompts only undetermined grants once; denials lead to System Settings with a runnable open command, never re-prompt loops"
    requirement: "CAPT-06"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-core#preflight_requests_each_grant_exactly_once + prompt_gate_fires_exactly_once_per_grant + prompted_save_preserves_unknown_keys; cargo test -p rustwatch-cli#permission_fix_lines_name_each_gap"
        status: pass
    human_judgment: true
    rationale: "Single-dialog behavior and no-re-prompt-after-denial must be observed on real macOS TCC dialogs."
  - id: D4
    description: "doctor exits 1 only on live breakage and warns on nonzero history counters"
    requirement: "SYS-02"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-cli#doctor_exit_code_matrix (healthy 0, warn-only 0, daemon-down 1, db-unwritable 1)"
        status: pass
      - kind: other
        ref: "live: isolated-HOME doctor printed fail lines for plist + socket, pass for db/screenshots, warn for ownership/key — EXIT=1"
        status: pass
    human_judgment: true
    rationale: "Healthy-install pass, warn-after-injected-write-error, kill -9 restart vs clean-stop-stays-stopped need a real launchd install."
  - id: D5
    description: "launchd restarts crashes but clean stop stays stopped; logs outside /tmp; TUI banner agrees with status"
    requirement: "SYS-02"
    verification:
      - kind: unit
        ref: "cargo test -p rustwatch-cli#plist_template_renders_hardened + tui_status_line trio (healthy/degraded/unreachable-vs-not-running)"
        status: pass
    human_judgment: true
    rationale: "KeepAlive crash-restart vs stop-stays-stopped and visual TUI/status agreement must be confirmed on hardware under launchd."

# Metrics
duration: ~150min
completed: 2026-10-06
status: complete
---

# Phase 01 Plan 03: Honest Permissions + Doctor/launchd Summary

**True three-state TCC reporting with per-path degradation, prompt-once onboarding, a trustworthy doctor, and a hardened launchd plist — partial capture can never look like full capture.**

## Performance

- **Duration:** ~150 min across two sessions (tracer in prior session, tasks 2–3 this session)
- **Tasks:** 3 of 3 (1 tracer, 2 auto)
- **Files modified:** 13 (12 source + 1 plist)
- **Tests:** 124 passing workspace-wide (37 capture + 59 core + 14 daemon + 12 CLI + 2 pre-existing), 0 failed

## Accomplishments

- End-to-end true permission states: Screen Recording (`CGPreflightScreenCaptureAccess` / request variant), Accessibility (`AXIsProcessTrusted` / WithOptions prompt dict), Input Monitoring (tap-creation probe) — all through `DaemonState.permissions` to the `status` banner
- Per-path degradation with the daemon staying up: no Input Monitoring → keyboard never spawns (HotAttach on late grant); no Screen Recording → focus/interval/hotkey shots skip (restart-required latch on late grant); every transition lands in visible `capture_note` state
- First-run preflight: requests ONLY undetermined-never-prompted grants once (after the single-instance lock), persists `prompted_*` via merge-preserving save, denials log/print the Settings pane + runnable `open` command
- `rustwatch doctor`: launchd plist + `launchctl list`, socket Ping (not-running vs wedged), rolled-back DB write probe, socket/pid ownership note, per-grant fixes, screenshots-dir + disk note, history-counters warn, `OPENAI_API_KEY` warn — exit 1 only on live breakage
- Hardened plist (`~/Library/Logs/rustwatch/`, `KeepAlive{SuccessfulExit=false}`, `ThrottleInterval 30`) rendered with substituted paths by `install`; TUI status line at full banner parity with `status`

## Task Commits

Each task was committed atomically:

1. **Task tracer: end-to-end true screen-recording state from TCC probe to banner** — `61dd616` (feat; prior session, verified present, adopted as-is)
2. **Task auto: all three probes, per-path degradation, first-run onboarding** — `7b381e5` (feat; 9 files — resumed from uncommitted worktree progress, completed the missing daemon half, fixed one test bug)
3. **Task auto: doctor, status parity, launchd hardening, TUI banner** — `4aae99f` (feat; 4 files)

## Files Created/Modified

- `crates/rustwatch-capture/src/platform/macos.rs` — three probes + request variants, `KeyboardLaunch`/`start_keyboard` HotAttach guard, `screenshots_live` gates on focus/interval/chord paths, visible keyboard-note ownership
- `crates/rustwatch-capture/src/lib.rs` + `platform/stub.rs` — `KeyboardLaunch`, `probe_all_check_only`, `request_grant`, gated `start` signature (stub parity)
- `crates/rustwatch-core/src/permissions.rs` — `Grant` enum (names, Settings URLs, restart semantics), `paths_live`, `grants_to_request`, `grant_transition`/`diff_permissions` + 5 new unit tests
- `crates/rustwatch-core/src/ipc.rs` — `DaemonState.capture_note` (serde-defaulted wire addition)
- `crates/rustwatch-core/src/paths.rs` — merge-preserving `save_permissions_prompted` + unknown-key survival test
- `crates/rustwatch-core/src/config.rs` — `PermissionsConfig: Copy` (re-probe handle ergonomics)
- `crates/rustwatch-daemon/src/main.rs` — `run_permission_preflight`, `PermissionSnapshot`/`ReprobeHandle` + 30 s re-probe scheduler arm, interval-shot screenshot gate, Status serving snapshot + joined note
- `crates/rustwatch-cli/src/commands.rs` — `doctor` + `doctor_exit_code` + `permission_fix_lines` + `render_plist`, hardened `install`, `capture_note` in `status` + 3 new tests
- `crates/rustwatch-cli/src/main.rs` — `Commands::Doctor` + dispatch
- `crates/rustwatch-cli/src/tui.rs` — pure `tui_status_line` (parity, nonzero-only counters, unreachable-vs-not-running) + 3 tests, loop wired to it
- `deploy/macos/com.rustwatch.plist` — Logs dir, KeepAlive dict, ThrottleInterval

## Decisions Made

- No new crates (tracer STOP rule honored): the Accessibility pair is a 10-line `extern "C"` fallback in the documented RESEARCH.md style — `CGRequestScreenCaptureAccess` likewise declared directly.
- Preflight placement after the single-instance lock: a second instance must never fire native dialogs; only the lock-holder prompts.
- Restart-required is a per-grant datum (`Grant::restart_required_on_attach`, true only for Screen Recording), consumed identically by the daemon latch and the doctor fix lines — one source, no drift.
- `prompted_*` persistence merges through `toml::Value` instead of rewriting the struct, preserving the 01-01 SYS-01 forward-compat promise (old `[data]`/`[ui]` remnants, future keys).
- Doctor prints the `open 'x-apple…'` fix command but never executes it — static pane URLs, no subprocess, no injection surface.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Degradation-test probe vector granted the control grant**
- **Found during:** Task 2 verification (`cargo test -p rustwatch-capture` red)
- **Issue:** The resumed worktree's `injected_screen_probe_maps_to_three_states` passed `true` for the accessibility probe while asserting it stays `Undetermined` — with all three probes wired, a `true` probe is `Granted`, so the assertion failed (`left: Granted, right: Undetermined`).
- **Fix:** One-arg change to `build_report(false, false, false, &asked)` so the "prompting one grant never flips the others" assertion actually isolates.
- **Files modified:** `crates/rustwatch-capture/src/platform/macos.rs` (test only)
- **Verification:** `cargo test -p rustwatch-capture` green (37/37)
- **Committed in:** `7b381e5` (part of task commit)

---

**Total deviations:** 1 auto-fixed test bug. No architectural changes, no new crate fetches (T-03-SC honored), no scope creep.

**Resume handling (not a deviation):** the handoff described commit `61dd616` plus uncommitted task-2 progress. The tracer commit was verified (`git show --stat`, permissions/banner code present) and adopted; the worktree diff was reviewed file-by-file, the missing daemon half (`run_permission_preflight`, `PermissionSnapshot`/`ReprobeHandle`, scheduler re-probe arm, Status wiring) was written to match the existing call sites, and everything was committed per-task.

## Issues Encountered

- The resumed worktree did not compile (call sites referenced `PermissionSnapshot`, `ReprobeHandle`, `run_permission_preflight`, and a 12-arg scheduler that did not exist yet). Resolved by implementing exactly those definitions per the plan's D-13/D-15 semantics — no redesign needed.
- `python3` used for two mechanical sed-class edits in commands.rs (unqualified test helper names, `&vec!` slice fix). Trivial, verified by green tests immediately after.

## Live Verification (isolated HOMEs, short paths)

- Daemon smoke (`/tmp/rwlive`): started clean under preflight, captured 2 events + 1 screenshot, `status` showed `capture: healthy` + `permissions: all granted` with no note — wire path proven end to end on this box.
- Doctor fail path (`/tmp/rwdoctor-test`): `[fail]` plist + socket, `[pass]` db + screenshots (+parsed `df` disk note), `[warn]` ownership (socket-absent wording) + `OPENAI_API_KEY` — `EXIT=1` exactly per D-08.
- No stray processes left (`pgrep` clean); temp HOMEs removed.

## User Setup Required

None — no external service configuration required.

## On-Machine Human Checks (from plan `<human-check>`)

Automated parts are done and green. On a grant-capable macOS machine, please run:

1. **Task 1:** with Screen Recording granted vs denied, `permissions` and `status` report the true state; revoke and re-grant flows reflect within one re-probe cycle.
2. **Task 2:** deny each grant in turn — daemon stays up, banner names exactly the degraded path, re-grant attaches within ~30 s or says restart-required; first run prompts at most once per grant and never re-prompts a denial.
3. **Task 3:** `doctor` passes on a healthy install, warns after injecting a write error, fails with the daemon stopped; `kill -9` the daemon and confirm launchd restarts it; `stop` then confirms it stays stopped; TUI shows the same banner as `status`.

## Next Phase Readiness

- Phase 1 is now complete (01-01 ledger, 01-02 triggers, 01-03 permissions/doctor): CAPT-01…CAPT-07, SYS-01, SYS-02 all have implementation + unit coverage; hardware-matrix items above are the remaining human sign-offs.
- Phase 2 consumes: `capture_note`/banner plumbing (privacy-degraded paths can ride the same state), `doctor` as the install-truth tool, daemon/CLI/TUI all reading one `DaemonState` wire format.
- Known gap (pre-existing, out of scope): `hash_content` dead-code warning in `macos.rs` still untouched.

## Self-Check: PASSED

- SUMMARY file exists at `.planning/phases/01-reliable-capture-system-foundation/01-03-SUMMARY.md`
- Commits `61dd616`, `7b381e5`, `4aae99f` all present in `git log`
- `cargo test --workspace`: 124 passed, 0 failed · `cargo check --workspace`: green (1 pre-existing warning: `hash_content` never used)
- Stub scan on the plan diff: no TODO/FIXME/placeholder/unimplemented markers (only the word "placeholder" inside a test assertion string)
- Threat scan: all new surface (check-only probes, prompt-once preflight, read-only doctor checks, plist log/KeepAlive changes) is inside the plan's threat register (T-03-01/T-03-02/T-03-05 mitigated, T-03-03 accepted, T-03-04 prior work, T-03-SC honored) — no new flags

---
*Phase: 01-reliable-capture-system-foundation*
*Completed: 2026-10-06*
