---
last_mapped_commit: 14fdad5e6d149c3edee1c2cdfd75d2d964b763c6
last_mapped_at: 2026-10-02
---
# Test Plan — Automated Capture-Fidelity Verification

**Date:** 2026-10-02
**Status:** Phase 1 complete — 51 tests passing, 0 failing
**Constraint:** Every test must run headless. No Accessibility grant, no physical
keyboard, no screen recording permission, no user interaction of any kind.

---

## Why This Plan Exists

`docs/TESTING_PLAN.md` describes a six-phase plan that was never started. This
document covers the subset that is achievable today, for the three bugs fixed in
this phase, plus the structural work required to make everything else testable.

The central constraint drove the architecture: `run_keyboard_loop` originally
called `keytap::Tap::new()` inline, which requires an Accessibility grant and a
real keyboard. Nothing about it could be tested. The fix was to extract the pure
translation logic into `rustwatch-core::keystroke` and inject the event source,
so the entire pipeline is drivable from a `Vec` of scripted key events.

---

## Layer 1 — Pure Unit Tests (no I/O, no permissions)

Location: `crates/rustwatch-core/src/keystroke.rs` — 18 tests

These are the tests that would have caught all three original bugs. They are
pure functions over `LogicalKey` and `Modifiers`.

| Group | Tests | Covers |
|---|---|---|
| Letter case | 4 | shift uppercase, caps-lock, shift+caps inversion |
| Punctuation | 2 | unshifted and shifted symbol rows |
| Digits | 1 | number row and shifted symbol row |
| Whitespace | 1 | space, enter, tab, backspace, escape |
| Round-trip | 2 | full sentence `"Fix the bug, please?"`, full URL |
| Invariants | 1 | every text key yields exactly one char |
| Exclusion | 7 | default config, substring, case, blanks, empty list |

**Key regression assertions:**

- `shift_uppercases_letters` — old table returned `"h"` regardless of shift
- `unshifted_punctuation_produces_text` — old table returned `None`, so
  `TextDelta` was never emitted for `.` `,` `/` `'`
- `blank_entries_do_not_exclude_everything` — a stray `""` in config must not
  silently disable all capture

## Layer 2 — State Machine Tests (no I/O)

Location: `crates/rustwatch-core/src/segment.rs` — 11 tests

`SegmentGrouper` consumes events and emits segments. No I/O required.

**Key regression assertions:**

- `text_before_any_focus_change_is_not_lost` — the focus loop *polls*, so text
  arriving before the first `FocusChange` used to be dropped silently
- `keystrokes_after_suppressed_focus_change_are_attributed` — an excluded app
  produces no `FocusChange`, so its keystrokes need a segment opened on demand
- `multibyte_trim_does_not_panic_and_stays_valid_utf8` — the byte-offset
  `replace_range` panicked on multi-byte input, killing the daemon's only
  writer task
- `mixed_ascii_and_multibyte_trim_is_safe` — interleaved scripts

## Layer 3 — Pipeline Tests (scripted events, real translation path)

Location: `crates/rustwatch-capture/src/platform/macos.rs` — 22 tests

These drive `translate_key_events` with a scripted key sequence and an injected
app resolver and clipboard. This is the layer that proves the wiring is correct,
not just the pure functions.

**Key regression assertions:**

- `excluded_app_produces_no_events_at_all` — the headline. 1Password
  keystrokes must yield *zero* events, not merely zero `TextDelta`
- `versioned_excluded_name_still_matches` — `"1Password 8"`, `"1Password –
  Browser"` all excluded by substring
- `exclusion_is_case_insensitive_in_the_pipeline` — `"1PASSWORD"` excluded by
  config `"1password"`
- `every_text_event_carries_app_attribution` — app attribution survives the
  translation step
- `meta_v_is_not_treated_as_a_letter` — `Cmd+V` emits `Paste`, not the letter
  `"v"` (a regression introduced *and caught* during this work)
- `shift_release_restores_lowercase` — `"Aa"` proves modifier state resets

**Platform note:** `macos.rs` is `#[cfg(target_os = "macos")]`, so Layer 3 runs
only on macOS. Layers 1 and 2 are cross-platform. CI must include a macOS runner
or Layer 3 silently disappears.

---

## What Is Still Untestable, and Why

These require either refactoring or infrastructure that does not exist yet.

| Area | Blocker | Unblocking work |
|---|---|---|
| `keytap::Tap` itself | Needs Accessibility grant + physical keyboard | Inject `KeyEventSource` into `run_keyboard_loop` (trait already added; production wiring done, tap untested) |
| `current_app_context` | Needs a real frontmost app | Inject a trait; tests already use a closure for this |
| `capture_to_disk` / `xcap` | Needs Screen Recording permission | Return fixture images; test only the path/hash logic |
| `read_clipboard_text` | Needs a real pasteboard | Injected as a closure — done |
| `Store` / SQLite | No `tempfile` dev-dependency | Add `tempfile` to `[dev-dependencies]` of `rustwatch-core` |
| `rustwatch-cli` / `-daemon` / `-mcp` | Bin-only crates, no `lib.rs` — integration tests cannot import | Add `src/lib.rs` and declare `[lib]` |
| `rustwatch-memory-backends` | Not in `workspace.members`; `cargo metadata` errors | Add to members or add `[workspace]` + `exclude` |
| `classifier.rs` | Builds its own classifier internally | Accept `&dyn ActivityClassifier` (the seam `docs/TESTING_PLAN.md:78` already specifies) |
| `build_prompt` / `render_*` | Private helpers | Make `pub(crate)` |
| `Config` | No test constructor | Add `Config::for_test()` |
| CI | None | Add `.github/workflows/ci.yml` |

---

## Testability Inventory — Usable Today, No Refactor

These functions are already pure and private; promoting them to `pub(crate)`
unlocks tests immediately:

- 14 pure private functions across `rustwatch-core`
- `SegmentGrouper` — now fully covered (Layer 2)
- `GraphRag::merge`
- `Redactor::scrub` — worth testing immediately; it has a UTF-8 panic at
  `redact.rs:31` (`out.truncate` at a byte offset)
- `key_to_text` — now covered (Layer 1)

---

## Definition of Done for This Phase

- [x] `key_to_text` respects shift, caps-lock, and the full punctuation row
- [x] `exclude_apps` reaches `run_keyboard_loop` and is enforced before any
      text is emitted
- [x] `SegmentGrouper` opens a segment on first text event
- [x] `append_text` cannot panic on multi-byte UTF-8
- [x] Every test runs headless — no permissions, no keyboard, no user action
- [x] `cargo test --workspace` passes: 51 passed, 0 failed
- [x] `cargo build --workspace` clean (1 pre-existing dead-code warning)

## Definition of Done for Full Autonomy

To reach the goal of "test everything without user action":

- [ ] Add `tempfile` dev-dependency and test `Store` against real temp SQLite
- [ ] Add `src/lib.rs` to the three binary crates so integration tests can import
- [ ] Bring `rustwatch-memory-backends` into the workspace or exclude it
- [ ] Inject `ActivityClassifier` so classification is testable with a stub
- [ ] Extract a `PlatformCapture` trait so macOS APIs sit behind a fake
- [ ] Add CI running `cargo test --workspace` on macOS
- [ ] Fix `redact.rs:31` and `commands.rs:241` byte-slicing panics, with tests