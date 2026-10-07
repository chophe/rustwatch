---
status: testing
phase: 01-reliable-capture-system-foundation
source: [01-VERIFICATION.md]
started: 2026-10-06T00:00:00Z
updated: 2026-10-06T00:00:00Z
---

## Current Test

number: 1
name: Sleep/wake — 10 s interval, sleep 2 min, expect exactly one wake shot, no burst
expected: |
  With screenshot interval set to 10 s, sleep the Mac for 2 minutes.
  On wake, exactly one catch-up shot is taken (no burst of backlogged shots).
  Daemon stays up; no events dropped.
awaiting: user response

## Tests

### 1. Sleep/wake capture behavior
expected: 10 s interval, sleep 2 min, exactly one wake shot, no burst, daemon healthy
result: [pending]

### 2. TCC deny matrix
expected: Deny each grant (Input Monitoring, Accessibility, Screen Recording) in turn — daemon stays up, status banner names exactly the degraded path, re-grant attaches within ~30 s or says restart-required; first-run prompts fire at most once per grant, denials show System Settings fix with no re-prompt
result: [pending]

### 3. launchd restart vs stop + doctor states
expected: kill -9 the daemon → launchd restarts it; clean `stop` → stays stopped; `doctor` passes healthy, warns after injecting a write error, fails with daemon stopped; logs live in ~/Library/Logs/rustwatch/
result: [pending]

### 4. Hotkey/annotate/throttle/idle feel
expected: Ctrl+Shift+Space takes an immediate shot; annotate dialog Enter attaches/Esc cancels; Alt-Tab bursts capped by throttle; 5-min idle suppresses screenshots and excludes idle from reports; TUI banner matches `status` banner
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
