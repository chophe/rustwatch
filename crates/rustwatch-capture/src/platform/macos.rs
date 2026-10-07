use std::collections::HashSet;
use std::ffi::c_void;
use std::path::PathBuf;use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use rustwatch_core::{
    classify_grant,
    events::{AppContext, CaptureEvent, CaptureEventKind, ScreenshotScope},
    is_excluded, key_to_text, Grant, LogicalKey, Modifiers, PermissionsConfig, PermissionState,
    Result,
};

use tracing::{debug, warn};

pub type EventSender<T> = std::sync::mpsc::Sender<T>;

#[derive(Debug, Clone, Default)]
pub struct PermissionsReport {
    pub input_monitoring: PermissionState,
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
    pub notes: Vec<String>,
}

pub struct PlatformCapture {
    exclude_apps: Vec<String>,
    paused: Arc<AtomicBool>,
    /// D-06: join handles for the capture threads. The daemon watches them
    /// and exits nonzero if any dies, so launchd KeepAlive restarts it.
    threads: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
    /// D-14 attach guard: the keyboard thread spawns at most once per
    /// process (startup spawn or one HotAttach), never duplicated.
    keyboard_started: Arc<AtomicBool>,
}

impl Clone for PlatformCapture {
    fn clone(&self) -> Self {
        Self {
            exclude_apps: self.exclude_apps.clone(),
            paused: Arc::clone(&self.paused),
            threads: Arc::clone(&self.threads),
            keyboard_started: Arc::clone(&self.keyboard_started),
        }
    }
}

impl PlatformCapture {
    pub fn new(exclude_apps: Vec<String>) -> Result<Self> {
        Ok(Self {
            exclude_apps,
            paused: Arc::new(AtomicBool::new(false)),
            threads: Arc::new(Mutex::new(Vec::new())),
            keyboard_started: Arc::new(AtomicBool::new(false)),
        })
    }

    /// D-06: true while every capture thread spawned by `start` is still
    /// running. Any finished thread (panic or early return) reads as dead —
    /// capture loops run forever by design.
    pub fn threads_alive(&self) -> bool {
        self.threads
            .lock()
            .map(|handles| handles.iter().all(|h| !h.is_finished()))
            .unwrap_or(false)
    }

    pub fn permissions() -> PermissionsReport {
        Self::permissions_with_prompted(&PermissionsConfig::default())
    }

    /// Full three-grant report. `prompted` is the `[permissions] prompted_*`
    /// bookkeeping: a `false` probe with no prior prompt is undetermined,
    /// with a prior prompt is denied (see `classify_grant`).
    pub fn permissions_with_prompted(prompted: &PermissionsConfig) -> PermissionsReport {
        let (input, accessibility, screen) = Self::probe_all_check_only();
        build_report(input, accessibility, screen, prompted)
    }

    /// D-15 check-only trio for the 30 s re-probe loop and `status`/`doctor`.
    /// NEVER prompts: Screen Recording uses the Preflight (check) variant,
    /// Accessibility uses `AXIsProcessTrusted` (check), and the Input
    /// Monitoring tap attempt is prompt-free once `prompted_*` is set — the
    /// preflight performs the single request before any of these run
    /// repeatedly (Pitfall 6).
    pub fn probe_all_check_only() -> (bool, bool, bool) {
        (
            probe_input_monitoring(),
            probe_accessibility(),
            probe_screen_recording(),
        )
    }

    /// D-13 first-run request: the REQUEST variant per grant, called at most
    /// once per undetermined grant (gated by `wants_prompt` + persisted
    /// flags). Returns the post-request probe state.
    pub fn request_grant(grant: Grant) -> bool {
        match grant {
            // No request API exists: the tap attempt IS the request — the
            // system prompts on first listen when undetermined.
            Grant::InputMonitoring => probe_input_monitoring(),
            Grant::Accessibility => request_accessibility(),
            Grant::ScreenRecording => request_screen_recording(),
        }
    }

    pub fn start(
        &self,
        tx: EventSender<CaptureEvent>,
        poll_focus_ms: u64,
        screenshot_on_focus_change: bool,
        min_interval_secs: u64,
        hotkey_enabled: bool,
        hotkey_chord: String,
        idle: Arc<AtomicBool>,
        screenshot_root: PathBuf,
        keyboard_enabled: bool,
        screenshots_live: Arc<AtomicBool>,
        keyboard_note: Arc<Mutex<String>>,
    ) -> Result<()> {
        let exclude = self.exclude_apps.clone();
        let paused_focus = Arc::clone(&self.paused);

        if keyboard_enabled {
            // D-14: no Input Monitoring means this never spawns (titles and
            // screenshots continue). A tap failure AFTER a granted probe
            // parks inside the thread with a visible note, never a lone
            // warning — and the daemon stays up.
            let launch = crate::KeyboardLaunch {
                tx: tx.clone(),
                exclude_apps: exclude.clone(),
                hotkey_enabled,
                hotkey_chord: hotkey_chord.clone(),
                screenshot_root: screenshot_root.clone(),
                paused: Arc::clone(&self.paused),
                screenshots_live: Arc::clone(&screenshots_live),
                keyboard_note: Arc::clone(&keyboard_note),
            };
            // Fresh handle: the guard is always free here; a HotAttach later
            // is the only other spawner.
            let _ = self.start_keyboard(launch)?;
        } else {
            *keyboard_note.lock().unwrap_or_else(|e| e.into_inner()) =
                "keyboard capture disabled: input monitoring not granted".to_string();
        }

        let tx_focus = tx.clone();
        let threads = Arc::clone(&self.threads);
        let focus = std::thread::Builder::new()
            .name("capture-focus".into())
            .spawn(move || {
                if let Err(err) = run_focus_loop(
                    tx_focus,
                    poll_focus_ms,
                    screenshot_on_focus_change,
                    min_interval_secs,
                    idle,
                    screenshot_root,
                    exclude,
                    paused_focus,
                    screenshots_live,
                ) {
                    warn!(?err, "focus capture stopped");
                }
            })
            .map_err(|e| rustwatch_core::Error::Other(format!("spawn focus thread: {e}")))?;

        threads
            .lock()
            .map_err(|_| rustwatch_core::Error::Other("capture thread registry poisoned".into()))?
            .extend([focus]);

        Ok(())
    }

    /// D-15 HotAttach: spawn the keyboard thread after a late grant. Guard
    /// makes it at-most-once per process — `Ok(false)` when already running.
    pub fn start_keyboard(&self, launch: crate::KeyboardLaunch) -> Result<bool> {
        if self
            .keyboard_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(false);
        }
        // Parse once: an unparsable chord disables the hotkey loudly
        // instead of failing every keypress.
        let hotkey = if launch.hotkey_enabled {
            match parse_hotkey(&launch.hotkey_chord) {
                Some(chord) => Some(chord),
                None => {
                    warn!(chord = %launch.hotkey_chord, "ignoring unparsable hotkey_chord; hotkey disabled");
                    None
                }
            }
        } else {
            None
        };

        let threads = Arc::clone(&self.threads);
        let keyboard = std::thread::Builder::new()
            .name("capture-keyboard".into())
            .spawn(move || {
                // D-14 park-retry: a graceful Err (grant flapped between
                // probe and tap) parks 30 s and retries with a visible note
                // instead of finishing the thread. A real panic still
                // finishes it and trips the daemon's D-06 death watch.
                loop {
                    let outcome = run_keyboard_loop(
                        &launch.tx,
                        &launch.paused,
                        &launch.exclude_apps,
                        hotkey.as_ref(),
                        &launch.screenshot_root,
                        &launch.screenshots_live,
                        &launch.keyboard_note,
                    );
                    match outcome {
                        Ok(()) => {
                            tracing::error!("keyboard tap stream ended unexpectedly; retrying parked");
                            set_keyboard_note(
                                &launch.keyboard_note,
                                "keyboard capture stream ended; retrying",
                            );
                        }
                        Err(err) => {
                            tracing::error!(?err, "keyboard capture unavailable; thread parked");
                            set_keyboard_note(
                                &launch.keyboard_note,
                                &format!("keyboard capture unavailable: {err}"),
                            );
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(30));
                }
            })
            .map_err(|e| rustwatch_core::Error::Other(format!("spawn keyboard thread: {e}")))?;

        threads
            .lock()
            .map_err(|_| rustwatch_core::Error::Other("capture thread registry poisoned".into()))?
            .push(keyboard);

        Ok(true)
    }

    pub fn capture_screenshot(
        &self,
        scope: ScreenshotScope,
        root: PathBuf,
    ) -> Result<(PathBuf, ScreenshotScope)> {
        capture_to_disk(scope, root)
    }

    /// D-17 hardware idle probe: seconds since the last HID input event,
    /// straight from Quartz. `None` when the value is unusable — the
    /// scheduler treats that as active (never idle on a broken probe).
    ///
    /// COMPILE-TIME verification (01-02): core-graphics 0.23.2 exposes no
    /// `secondsSinceLastEventType` binding (checked vendored sources), so
    /// this declares the 10-line `extern "C"` fallback from RESEARCH.md.
    /// `kCGEventSourceStateHIDSystemState` (1) + `kCGAnyInputEventType`
    /// (`~0u`) is the standard idle-time pairing.
    pub fn system_idle_seconds() -> Option<f64> {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
        }
        // SAFETY: pure Quartz getter; constant args are always valid.
        let secs = unsafe { CGEventSourceSecondsSinceLastEventType(1, u32::MAX) };
        if secs.is_finite() && secs >= 0.0 {
            Some(secs)
        } else {
            None
        }
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}

// COMPILE-TIME verification (01-03 tracer): neither core-graphics 0.23.2
// nor any other crate in the tree exposes `CGPreflightScreenCaptureAccess`
// or `AXIsProcessTrustedWithOptions` (checked the vendored registry
// sources) — so both probes below declare minimal `extern "C"` fallbacks,
// the same pattern 01-02 used for the idle probe. Zero new crates, zero
// new supply-chain surface (threat T-03-SC).
//
// API choice record: `CGPreflightScreenCaptureAccess` (macOS 10.15+) is the
// CHECK variant — true means granted, false means denied-or-undetermined,
// and it never prompts. The REQUEST variant
// (`CGRequestScreenCaptureAccess`, which prompts when undetermined) is
// reserved for the task-2 first-run preflight, gated by `wants_prompt`.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> u8;
    /// REQUEST variant: prompts when undetermined. Preflight-only callers
    /// never touch this — it fires at most once per grant (D-13).
    fn CGRequestScreenCaptureAccess() -> u8;
}

// COMPILE-TIME verification (01-03 task 2): ApplicationServices has no
// usable Rust binding in the tree, so the Accessibility probe declares its
// own 10-line fallback. `AXIsProcessTrusted` is the CHECK variant (never
// prompts); `...WithOptions` with the prompt key is the REQUEST variant.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: *const c_void;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

/// Raw Screen Recording grant probe: true = granted. False folds
/// denied/undetermined together — the caller splits them with
/// `classify_grant` using the persisted `prompted_*` flags.
fn probe_screen_recording() -> bool {
    // SAFETY: pure Quartz getter; no arguments, no out-pointers.
    unsafe { CGPreflightScreenCaptureAccess() != 0 }
}

/// REQUEST variant: shows the native Screen Recording dialog when
/// undetermined, then reports the outcome. At-most-once per D-13.
fn request_screen_recording() -> bool {
    // SAFETY: pure Quartz request; no arguments, no out-pointers.
    unsafe { CGRequestScreenCaptureAccess() != 0 }
}

/// Accessibility CHECK: `AXIsProcessTrusted` answers without prompting.
fn probe_accessibility() -> bool {
    // SAFETY: pure getter; no arguments.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Accessibility REQUEST: shows the native dialog once when undetermined
/// via the `{prompt: true}` options dict, then reports the outcome.
fn request_accessibility() -> bool {
    // SAFETY: synchronous call with global key/value pointers that outlive
    // it, so NULL retain/release callbacks are sound; the dict is released
    // before return on the success path.
    unsafe {
        let key = kAXTrustedCheckOptionPrompt;
        let value = kCFBooleanTrue;
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            &key,
            &value,
            1,
            std::ptr::null(),
            std::ptr::null(),
        );
        if dict.is_null() {
            return probe_accessibility();
        }
        let trusted = AXIsProcessTrustedWithOptions(dict) != 0;
        CFRelease(dict);
        trusted
    }
}

/// Input Monitoring probe: NO public TCC API exists, so attempting tap
/// creation is the standard probe — a failed `Tap::new` with no other error
/// means denied (RESEARCH.md). The attempt doubles as the REQUEST: the
/// system prompts on first listen when undetermined, which is why the
/// re-probe loop only attempts after `prompted_*` is set (Pitfall 6).
fn probe_input_monitoring() -> bool {
    keytap::Tap::new().is_ok()
}

/// Pure report assembly over INJECTED probe results, so the mapping is
/// unit-testable without touching TCC. The live path calls this with the
/// real `probe_all_check_only()` values.
fn build_report(
    input_granted: bool,
    accessibility_granted: bool,
    screen_granted: bool,
    prompted: &PermissionsConfig,
) -> PermissionsReport {
    let mut notes = Vec::new();
    notes.push("Grant Input Monitoring, Accessibility, and Screen Recording in System Settings.".into());
    notes.push("After granting Screen Recording, restart rustwatchd.".into());
    PermissionsReport {
        input_monitoring: classify_grant(input_granted, prompted.prompted_input_monitoring),
        accessibility: classify_grant(accessibility_granted, prompted.prompted_accessibility),
        screen_recording: classify_grant(screen_granted, prompted.prompted_screen_recording),
        notes,
    }
}

/// Where `run_keyboard_loop` gets its events.
///
/// Production uses [`KeytapSource`]; tests use a fixed vector, so the whole
/// translation pipeline is exercisable without an Accessibility grant or a
/// physical keystroke.
pub trait KeyEventSource {
    fn next_events(&mut self) -> Vec<(bool, keytap::Key)>;
}

/// Real keyboard tap. `bool` is `true` for key-down, `false` for key-up.
pub struct KeytapSource {
    tap: keytap::Tap,
}

impl KeyEventSource for KeytapSource {
    fn next_events(&mut self) -> Vec<(bool, keytap::Key)> {
        self.tap
            .iter()
            .map(|ev| match ev.kind {
                keytap::EventKind::KeyDown(key) | keytap::EventKind::KeyRepeat(key) => (true, key),
                keytap::EventKind::KeyUp(key) => (false, key),
            })
            .collect()
    }
}

/// Translate a batch of raw key events into capture events.
///
/// Pure with respect to I/O: `resolve_app` and `read_clipboard` are injected
/// so tests can drive the full pipeline deterministically. The hotkey chord
/// (if configured) is detected here — after the exclusion check, so excluded
/// apps never fire it (T-02-02) — and `shoot` is injected so tests never
/// touch the disk.
fn translate_key_events<A, C, S>(
    raw: &[(bool, keytap::Key)],
    state: &mut KeyState,
    tx: &EventSender<CaptureEvent>,
    exclude_apps: &[String],
    resolve_app: &A,
    read_clipboard: &C,
    hotkey: Option<&HotkeyChord>,
    shoot: &S,
) where
    A: Fn() -> Option<AppContext>,
    C: Fn() -> Option<String>,
    S: Fn() -> Option<(PathBuf, ScreenshotScope)>,
{
    for (is_down, key) in raw {
        if !is_down {
            state.release(*key);
            continue;
        }
        let fresh = state.press(*key);

        let app = resolve_app().or_else(|| state.last_app.clone());

        // Exclusion must be checked before any text is emitted: this loop
        // produces the keystrokes themselves, so skipping it here is what
        // let 1Password through despite the default config.
        if let Some(ctx) = &app {
            if is_excluded(&ctx.app_name, exclude_apps) {
                continue;
            }
        }
        state.last_app = app.clone();

        // Hotkey chord: shoot-first, synchronously, on the trigger key-down
        // — the screenshot never waits for typing. The trigger keypress is
        // consumed (no phantom space in the timeline). `fresh` suppresses
        // key-repeat refire while Space is held.
        if let Some(chord) = hotkey {
            if chord.matches(state, *key, fresh) {
                if let Some((path, scope)) = shoot() {
                    let _ = tx.send(CaptureEvent::new(
                        CaptureEventKind::Screenshot { path, scope },
                        app.clone(),
                    ));
                }
                continue;
            }
        }

        let mods = state.modifiers();
        let key_name = format!("{key:?}");

        // Cmd+V is a paste, not the letter "v". Emitting both would double-count
        // the clipboard content already carried by the Paste event.
        if state.meta_held && *key == keytap::Key::V {
            if let Some(content) = read_clipboard() {
                let _ = tx.send(CaptureEvent::new(
                    CaptureEventKind::Paste { content },
                    app.clone(),
                ));
            }
            let _ = tx.send(CaptureEvent::new(
                CaptureEventKind::Key {
                    key: key_name,
                    modifiers: state.modifier_names(),
                },
                app,
            ));
            continue;
        }

        if let Some(text) = to_logical_key(*key).and_then(|lk| key_to_text(lk, mods)) {
            let _ = tx.send(CaptureEvent::new(
                CaptureEventKind::TextDelta { text },
                app.clone(),
            ));
        }

        let _ = tx.send(CaptureEvent::new(
            CaptureEventKind::Key {
                key: key_name,
                modifiers: state.modifier_names(),
            },
            app,
        ));
    }
}

/// Modifier tracking across key up/down.
#[derive(Debug, Default, Clone)]
pub struct KeyState {
    pub meta_held: bool,
    pub shift_held: bool,
    pub caps_lock: bool,
    pub ctrl_held: bool,
    pub alt_held: bool,
    /// Currently held keys — distinguishes a fresh press from key-repeat
    /// (keytap reports repeats as key-down) so the hotkey fires once per
    /// deliberate press, not once per repeat.
    held: HashSet<keytap::Key>,
    last_app: Option<AppContext>,
}

impl KeyState {
    /// Record a key-down; returns true for a fresh press, false for a
    /// repeat of an already-held key.
    fn press(&mut self, key: keytap::Key) -> bool {
        match key {
            keytap::Key::MetaLeft | keytap::Key::MetaRight => self.meta_held = true,
            keytap::Key::ShiftLeft | keytap::Key::ShiftRight => self.shift_held = true,
            keytap::Key::ControlLeft | keytap::Key::ControlRight => self.ctrl_held = true,
            keytap::Key::AltLeft | keytap::Key::AltRight => self.alt_held = true,
            keytap::Key::CapsLock => self.caps_lock = !self.caps_lock,
            _ => {}
        }
        self.held.insert(key)
    }

    fn release(&mut self, key: keytap::Key) {
        match key {
            keytap::Key::MetaLeft | keytap::Key::MetaRight => self.meta_held = false,
            keytap::Key::ShiftLeft | keytap::Key::ShiftRight => self.shift_held = false,
            keytap::Key::ControlLeft | keytap::Key::ControlRight => self.ctrl_held = false,
            keytap::Key::AltLeft | keytap::Key::AltRight => self.alt_held = false,
            _ => {}
        }
        self.held.remove(&key);
    }

    /// Current modifier state, for both text translation and the `Key` event.
    fn modifiers(&self) -> Modifiers {
        Modifiers {
            shift: self.shift_held,
            caps_lock: self.caps_lock,
        }
    }

    /// Human-readable modifier names recorded on `CaptureEventKind::Key`.
    fn modifier_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if self.meta_held {
            names.push("Meta".to_string());
        }
        if self.shift_held {
            names.push("Shift".to_string());
        }
        if self.ctrl_held {
            names.push("Ctrl".to_string());
        }
        if self.alt_held {
            names.push("Alt".to_string());
        }
        if self.caps_lock {
            names.push("CapsLock".to_string());
        }
        names
    }
}

/// Chord-on-tap hotkey (default Ctrl+Shift+Space): zero new deps or
/// permissions, sub-ms detection on the existing keytap stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyChord {
    ctrl: bool,
    shift: bool,
    meta: bool,
    alt: bool,
    key: keytap::Key,
}

impl HotkeyChord {
    /// Fire only on the fresh press of the chord's main key with EXACTLY the
    /// configured modifiers held — partial presses and modifier-only
    /// sequences never fire, and extra modifiers don't count.
    fn matches(&self, state: &KeyState, key: keytap::Key, fresh: bool) -> bool {
        fresh
            && key == self.key
            && state.ctrl_held == self.ctrl
            && state.shift_held == self.shift
            && state.meta_held == self.meta
            && state.alt_held == self.alt
    }
}

/// Tiny case-insensitive chord parser: `ctrl/shift/meta/alt` (plus common
/// aliases) and exactly one main key (space, enter, tab, esc, a–z, 0–9).
/// Returns None for anything else — including a bare key with no modifiers,
/// which is a keypress, not a chord.
fn parse_hotkey(raw: &str) -> Option<HotkeyChord> {
    let (mut ctrl, mut shift, mut meta, mut alt) = (false, false, false, false);
    let mut main: Option<keytap::Key> = None;
    for token in raw.split('+') {
        match token.trim().to_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "meta" | "cmd" | "command" | "super" => meta = true,
            "alt" | "opt" | "option" => alt = true,
            "space" => main = Some(keytap::Key::Space),
            "enter" | "return" => main = Some(keytap::Key::Enter),
            "tab" => main = Some(keytap::Key::Tab),
            "esc" | "escape" => main = Some(keytap::Key::Escape),
            s if s.len() == 1 => main = key_from_char(s.chars().next()?),
            _ => return None,
        }
    }
    let key = main?;
    if !(ctrl || shift || meta || alt) {
        return None;
    }
    Some(HotkeyChord { ctrl, shift, meta, alt, key })
}

fn key_from_char(c: char) -> Option<keytap::Key> {
    use keytap::Key;
    Some(match c {
        'a' => Key::A,
        'b' => Key::B,
        'c' => Key::C,
        'd' => Key::D,
        'e' => Key::E,
        'f' => Key::F,
        'g' => Key::G,
        'h' => Key::H,
        'i' => Key::I,
        'j' => Key::J,
        'k' => Key::K,
        'l' => Key::L,
        'm' => Key::M,
        'n' => Key::N,
        'o' => Key::O,
        'p' => Key::P,
        'q' => Key::Q,
        'r' => Key::R,
        's' => Key::S,
        't' => Key::T,
        'u' => Key::U,
        'v' => Key::V,
        'w' => Key::W,
        'x' => Key::X,
        'y' => Key::Y,
        'z' => Key::Z,
        '0' => Key::Digit0,
        '1' => Key::Digit1,
        '2' => Key::Digit2,
        '3' => Key::Digit3,
        '4' => Key::Digit4,
        '5' => Key::Digit5,
        '6' => Key::Digit6,
        '7' => Key::Digit7,
        '8' => Key::Digit8,
        '9' => Key::Digit9,
        _ => return None,
    })
}

/// Capture-owned half of the daemon's visible note: ONLY the keyboard
/// thread writes this handle (the re-probe task owns a separate notice
/// field), so clearing it on tap success can never clobber daemon text.
fn set_keyboard_note(note: &Arc<Mutex<String>>, text: &str) {
    *note.lock().unwrap_or_else(|e| e.into_inner()) = text.to_string();
}

fn clear_keyboard_note(note: &Arc<Mutex<String>>) {
    note.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

fn run_keyboard_loop(
    tx: &EventSender<CaptureEvent>,
    paused: &AtomicBool,
    exclude_apps: &[String],
    hotkey: Option<&HotkeyChord>,
    screenshot_root: &std::path::Path,
    screenshots_live: &AtomicBool,
    keyboard_note: &Arc<Mutex<String>>,
) -> anyhow::Result<()> {
    let tap = match keytap::Tap::new() {
        Ok(tap) => {
            clear_keyboard_note(keyboard_note);
            KeytapSource { tap }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("keytap init failed: {e}"));
        }
    };
    let mut source = tap;
    let mut state = KeyState::default();
    // Shoot-first capture for the hotkey chord: synchronous Window shot
    // (Scope honesty from 2b labels the Screen fallback correctly).
    let shoot_root = screenshot_root.to_path_buf();

    loop {
        if paused.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }
        let raw = source.next_events();
        if raw.is_empty() {
            break;
        }
        // D-14: without Screen Recording the chord keys fall through to
        // normal handling — eating the keypress with no screenshot would
        // be worse than treating it as typing.
        let hotkey_ref = hotkey.filter(|_| screenshots_live.load(Ordering::Relaxed));
        translate_key_events(
            &raw,
            &mut state,
            tx,
            exclude_apps,
            &current_app_context,
            &|| read_clipboard_text().ok(),
            hotkey_ref,
            &|| capture_to_disk(ScreenshotScope::Window, shoot_root.clone()).ok(),
        );
    }
    Ok(())
}

fn run_focus_loop(
    tx: EventSender<CaptureEvent>,
    poll_focus_ms: u64,
    screenshot_on_focus_change: bool,
    min_interval_secs: u64,
    idle: Arc<AtomicBool>,
    screenshot_root: PathBuf,
    exclude_apps: Vec<String>,
    paused: Arc<AtomicBool>,
    screenshots_live: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut last: Option<AppContext> = None;
    // D-09 throttle state: timestamps of recent focus-loop shots.
    let mut shots: Vec<Instant> = Vec::new();
    let min_interval = Duration::from_secs(min_interval_secs);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(poll_focus_ms));
        if paused.load(Ordering::Relaxed) {
            continue;
        }
        let Some(current) = current_app_context() else {
            continue;
        };

        if is_excluded(&current.app_name, &exclude_apps) {
            continue;
        }

        if last.as_ref() != Some(&current) {
            let change = CaptureEvent::new(
                CaptureEventKind::FocusChange {
                    from: last.clone(),
                    to: current.clone(),
                },
                Some(current.clone()),
            );
            let _ = tx.send(change);

            // D-09 throttle (2 s cooldown + 3-per-10 s burst, T-02-03) and
            // D-20 idle suppression (hotkey stays live): the FocusChange
            // event above still flows, and `last` still advances below.
            // D-14: a revoked Screen Recording grant skips the shot while
            // titles continue.
            if screenshot_on_focus_change
                && !idle.load(Ordering::Relaxed)
                && screenshots_live.load(Ordering::Relaxed)
            {
                let now = Instant::now();
                shots.retain(|t| now.saturating_duration_since(*t) <= FOCUS_BURST_WINDOW);
                if should_shoot(now, &shots, min_interval) {
                    match capture_to_disk(ScreenshotScope::Window, screenshot_root.clone()) {
                        Ok((path, scope)) => {
                            shots.push(now);
                            let shot = CaptureEvent::new(
                                CaptureEventKind::Screenshot { path, scope },
                                Some(current.clone()),
                            );
                            let _ = tx.send(shot);
                        }
                        Err(err) => debug!(?err, "focus-loop screenshot skipped"),
                    }
                }
            }

            if let Some(snapshot) = read_focused_text_snapshot() {
                let snap = CaptureEvent::new(
                    CaptureEventKind::TextFieldSnapshot { value: snapshot },
                    Some(current.clone()),
                );
                let _ = tx.send(snap);
            }

            last = Some(current);
        }
    }
}

fn current_app_context() -> Option<AppContext> {
    let window = active_win_pos_rs::get_active_window().ok()?;
    Some(AppContext {
        app_name: window.app_name,
        window_title: window.title,
        process_id: window.process_id,
        bundle_id: None,
    })
}

fn read_clipboard_text() -> anyhow::Result<String> {
    let mut clipboard = arboard::Clipboard::new()?;
    Ok(clipboard.get_text()?)
}

fn read_focused_text_snapshot() -> Option<String> {
    // Best-effort placeholder: full AX integration can be expanded later.
    None
}

/// Translate a `keytap` key into the platform-independent `LogicalKey`.
///
/// Returns `None` for keys with no textual meaning (arrows, function row,
/// bare modifiers) so `key_to_text` never has to guess.
fn to_logical_key(key: keytap::Key) -> Option<LogicalKey> {
    use keytap::Key;
    let logical = match key {
        Key::A => LogicalKey::Letter('a'),
        Key::B => LogicalKey::Letter('b'),
        Key::C => LogicalKey::Letter('c'),
        Key::D => LogicalKey::Letter('d'),
        Key::E => LogicalKey::Letter('e'),
        Key::F => LogicalKey::Letter('f'),
        Key::G => LogicalKey::Letter('g'),
        Key::H => LogicalKey::Letter('h'),
        Key::I => LogicalKey::Letter('i'),
        Key::J => LogicalKey::Letter('j'),
        Key::K => LogicalKey::Letter('k'),
        Key::L => LogicalKey::Letter('l'),
        Key::M => LogicalKey::Letter('m'),
        Key::N => LogicalKey::Letter('n'),
        Key::O => LogicalKey::Letter('o'),
        Key::P => LogicalKey::Letter('p'),
        Key::Q => LogicalKey::Letter('q'),
        Key::R => LogicalKey::Letter('r'),
        Key::S => LogicalKey::Letter('s'),
        Key::T => LogicalKey::Letter('t'),
        Key::U => LogicalKey::Letter('u'),
        Key::V => LogicalKey::Letter('v'),
        Key::W => LogicalKey::Letter('w'),
        Key::X => LogicalKey::Letter('x'),
        Key::Y => LogicalKey::Letter('y'),
        Key::Z => LogicalKey::Letter('z'),
        Key::Digit0 => LogicalKey::Digit(0),
        Key::Digit1 => LogicalKey::Digit(1),
        Key::Digit2 => LogicalKey::Digit(2),
        Key::Digit3 => LogicalKey::Digit(3),
        Key::Digit4 => LogicalKey::Digit(4),
        Key::Digit5 => LogicalKey::Digit(5),
        Key::Digit6 => LogicalKey::Digit(6),
        Key::Digit7 => LogicalKey::Digit(7),
        Key::Digit8 => LogicalKey::Digit(8),
        Key::Digit9 => LogicalKey::Digit(9),
        Key::Numpad0 => LogicalKey::Numpad(0),
        Key::Numpad1 => LogicalKey::Numpad(1),
        Key::Numpad2 => LogicalKey::Numpad(2),
        Key::Numpad3 => LogicalKey::Numpad(3),
        Key::Numpad4 => LogicalKey::Numpad(4),
        Key::Numpad5 => LogicalKey::Numpad(5),
        Key::Numpad6 => LogicalKey::Numpad(6),
        Key::Numpad7 => LogicalKey::Numpad(7),
        Key::Numpad8 => LogicalKey::Numpad(8),
        Key::Numpad9 => LogicalKey::Numpad(9),
        Key::Backtick => LogicalKey::Backtick,
        Key::Minus => LogicalKey::Minus,
        Key::Equal => LogicalKey::Equal,
        Key::BracketLeft => LogicalKey::BracketLeft,
        Key::BracketRight => LogicalKey::BracketRight,
        Key::Backslash => LogicalKey::Backslash,
        Key::Semicolon => LogicalKey::Semicolon,
        Key::Quote => LogicalKey::Quote,
        Key::Comma => LogicalKey::Comma,
        Key::Period => LogicalKey::Period,
        Key::Slash => LogicalKey::Slash,
        Key::Space => LogicalKey::Space,
        Key::Enter => LogicalKey::Enter,
        Key::Tab => LogicalKey::Tab,
        Key::Backspace => LogicalKey::Backspace,
        Key::Escape => LogicalKey::Escape,
        _ => return None,
    };
    Some(logical)
}

/// D-09 window-change throttle: at most 3 shots per rolling 10 s window
/// plus the `min_interval_secs` cooldown. Pure over injected `Instant`s —
/// tests drive it with arithmetic, never a clock.
const FOCUS_BURST_MAX: usize = 3;
const FOCUS_BURST_WINDOW: Duration = Duration::from_secs(10);

fn should_shoot(now: Instant, history: &[Instant], min_interval: Duration) -> bool {
    if let Some(&last) = history.last() {
        if now.saturating_duration_since(last) < min_interval {
            return false;
        }
    }
    history
        .iter()
        .filter(|t| now.saturating_duration_since(**t) <= FOCUS_BURST_WINDOW)
        .count()
        < FOCUS_BURST_MAX
}

fn capture_to_disk(scope: ScreenshotScope, root: PathBuf) -> Result<(PathBuf, ScreenshotScope)> {
    use chrono::Utc;
    use xcap::{Monitor, Window};

    // Live finding (01-02 tracer): concurrent xcap captures stall each
    // other (12–22 s vs same-second sequential). ScreenCaptureKit calls
    // serialize here so interval, focus-change, and on-demand shots can
    // never overlap — a slow capture delays, never deadlocks.
    static CAPTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CAPTURE_LOCK.lock().map_err(|_| {
        rustwatch_core::Error::Other("screenshot capture lock poisoned".into())
    })?;

    // SYS-01: the date dir comes from DataPaths (sole path authority), not
    // an inline format duplicate.
    let date_dir = rustwatch_core::DataPaths::new(Some(root.clone()))?
        .screenshot_dir_for_date(Utc::now().date_naive());
    std::fs::create_dir_all(&date_dir).map_err(rustwatch_core::Error::from)?;
    let filename = format!("{}-{}.png", Utc::now().timestamp_millis(), scope_label(scope));
    let path = date_dir.join(filename);

    let actual = match scope {
        ScreenshotScope::Screen => {
            let monitors = Monitor::all().map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            let monitor = monitors.into_iter().next().ok_or_else(|| {
                rustwatch_core::Error::Other("no monitor found".into())
            })?;
            let image = monitor
                .capture_image()
                .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            image
                .save(&path)
                .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            ScreenshotScope::Screen
        }
        ScreenshotScope::Window => {
            let windows = Window::all().map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            match windows.into_iter().find(|w| w.is_focused().unwrap_or(false)) {
                Some(window) => {
                    let image = window
                        .capture_image()
                        .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
                    image
                        .save(&path)
                        .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
                    ScreenshotScope::Window
                }
                // T-02-01: the fallback captures the FULL screen, so the row
                // says Screen — never mislabel fullscreen as Window. And a
                // missing monitor fails loudly instead of writing a row for a
                // file that was never created.
                None => {
                    let monitor = Monitor::all()
                        .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?
                        .into_iter()
                        .next()
                        .ok_or_else(|| {
                            rustwatch_core::Error::Other("no focused window and no monitor".into())
                        })?;
                    let image = monitor
                        .capture_image()
                        .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
                    image
                        .save(&path)
                        .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
                    ScreenshotScope::Screen
                }
            }
        }
    };

    debug!(path = %path.display(), "screenshot saved");
    Ok((path, actual))
}

fn scope_label(scope: ScreenshotScope) -> &'static str {
    match scope {
        ScreenshotScope::Window => "window",
        ScreenshotScope::Screen => "screen",
    }
}

#[cfg(test)]
mod tests {
    use super::{should_shoot, translate_key_events, KeyState};
    use keytap::Key;
    use rustwatch_core::events::{AppContext, CaptureEvent, CaptureEventKind};
    use std::time::{Duration, Instant};

    fn app(name: &str) -> AppContext {
        AppContext {
            app_name: name.to_string(),
            window_title: format!("{name} window"),
            process_id: 1,
            bundle_id: Some(format!("com.example.{name}")),
        }
    }

    /// Drive the real translation pipeline with a scripted key sequence.
    /// No tap, no Accessibility grant, no physical keyboard.
    fn run(raw: &[(bool, Key)], exclude: &[&str], app_name: &str) -> Vec<CaptureEvent> {
        run_with_hotkey(raw, exclude, app_name, None)
    }

    /// Same pipeline with a hotkey chord armed; the injected shoot never
    /// touches the disk — it hands back a fake path.
    fn run_with_hotkey(
        raw: &[(bool, Key)],
        exclude: &[&str],
        app_name: &str,
        hotkey: Option<&super::HotkeyChord>,
    ) -> Vec<CaptureEvent> {
        let (tx, rx) = std::sync::mpsc::channel::<CaptureEvent>();
        let mut state = KeyState::default();
        let exclude: Vec<String> = exclude.iter().map(|s| s.to_string()).collect();
        let ctx = app(app_name);

        translate_key_events(
            raw,
            &mut state,
            &tx,
            &exclude,
            &|| Some(ctx.clone()),
            &|| None,
            hotkey,
            &|| {
                Some((
                    std::path::PathBuf::from("/tmp/fake-hotkey.png"),
                    rustwatch_core::ScreenshotScope::Window,
                ))
            },
        );

        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn text_of(events: &[CaptureEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match &e.kind {
                CaptureEventKind::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn down(k: Key) -> (bool, Key) {
        (true, k)
    }

    /// Regression: the old table ignored shift, so "Fix" was captured "fix".
    #[test]
    fn shift_produces_capital_letters() {
        // Shift held only for "F"; releasing it must lowercase the rest.
        let events = run(
            &[
                down(Key::ShiftLeft),
                down(Key::F),
                (false, Key::ShiftLeft),
                down(Key::I),
                down(Key::X),
            ],
            &[],
            "Safari",
        );
        assert_eq!(text_of(&events), "Fix");
    }

    /// Regression: punctuation returned None, so no TextDelta was emitted.
    #[test]
    fn punctuation_reaches_the_event_stream() {
        let events = run(
            &[down(Key::Period), down(Key::Comma), down(Key::Slash), down(Key::Quote)],
            &[],
            "Safari",
        );
        assert_eq!(text_of(&events), ".,/'");
    }

    #[test]
    fn shifted_slash_is_question_mark() {
        let events = run(
            &[down(Key::ShiftLeft), down(Key::Slash), (false, Key::ShiftLeft)],
            &[],
            "Safari",
        );
        assert_eq!(text_of(&events), "?");
    }

    #[test]
    fn unshifted_slash_is_slash() {
        let events = run(&[down(Key::Slash)], &[], "Safari");
        assert_eq!(text_of(&events), "/");
    }

    #[test]
    fn shifted_digit_is_symbol() {
        let events = run(
            &[down(Key::ShiftLeft), down(Key::Digit1), (false, Key::ShiftLeft)],
            &[],
            "Safari",
        );
        assert_eq!(text_of(&events), "!");
    }

    #[test]
    fn unshifted_digit_is_digit() {
        let events = run(&[down(Key::Digit1)], &[], "Safari");
        assert_eq!(text_of(&events), "1");
    }

    #[test]
    fn every_text_event_carries_app_attribution() {
        let events = run(&[down(Key::A), down(Key::B)], &[], "Cursor");
        let deltas: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, CaptureEventKind::TextDelta { .. }))
            .collect();
        assert_eq!(deltas.len(), 2);
        for ev in deltas {
            let ctx = ev.app.as_ref().expect("app context");
            assert_eq!(ctx.app_name, "Cursor");
            assert_eq!(ctx.bundle_id.as_deref(), Some("com.example.Cursor"));
        }
    }

    /// The headline regression: `exclude_apps` never reached this loop, so
    /// 1Password keystrokes were captured despite the shipped default.
    #[test]
    fn excluded_app_produces_no_events_at_all() {
        let events = run(
            &[down(Key::H), down(Key::I), down(Key::Period), down(Key::Slash)],
            &["1Password", "Keychain Access"],
            "1Password",
        );
        assert!(
            events.is_empty(),
            "excluded app leaked {} events",
            events.len()
        );
    }

    #[test]
    fn excluded_app_leaks_no_text_delta() {
        let events = run(&[down(Key::A), down(Key::B)], &["1Password"], "1Password");
        assert_eq!(text_of(&events), "");
    }

    #[test]
    fn versioned_excluded_name_still_matches() {
        let events = run(&[down(Key::A)], &["1Password"], "1Password 8");
        assert!(events.is_empty());
    }

    #[test]
    fn exclusion_is_case_insensitive_in_the_pipeline() {
        let events = run(&[down(Key::A)], &["1password"], "1PASSWORD");
        assert!(events.is_empty());
    }

    #[test]
    fn keychain_access_is_excluded_by_default_config() {
        let events = run(
            &[down(Key::A)],
            &["1Password", "Keychain Access"],
            "Keychain Access",
        );
        assert!(events.is_empty());
    }

    #[test]
    fn non_excluded_app_is_unaffected_by_the_list() {
        let events = run(&[down(Key::A), down(Key::B)], &["1Password"], "Safari");
        assert_eq!(text_of(&events), "ab");
    }

    #[test]
    fn empty_exclusion_list_captures_everything() {
        let events = run(&[down(Key::A)], &[], "1Password");
        assert_eq!(text_of(&events), "a");
    }

    #[test]
    fn key_events_are_emitted_alongside_text() {
        let events = run(&[down(Key::A)], &[], "Safari");
        let keys: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.kind, CaptureEventKind::Key { .. }))
            .collect();
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn shift_state_is_recorded_on_key_events() {
        let events = run(
            &[down(Key::ShiftLeft), down(Key::A), (false, Key::ShiftLeft)],
            &[],
            "Safari",
        );
        let key_ev = events
            .iter()
            .find_map(|e| match &e.kind {
                CaptureEventKind::Key { modifiers, .. } => Some(modifiers.clone()),
                _ => None,
            })
            .expect("key event");
        assert!(key_ev.contains(&"Shift".to_string()));
    }

    #[test]
    fn shift_release_restores_lowercase() {
        let events = run(
            &[
                down(Key::ShiftLeft),
                down(Key::A),
                (false, Key::ShiftLeft),
                down(Key::A),
            ],
            &[],
            "Safari",
        );
        assert_eq!(text_of(&events), "Aa");
    }

    #[test]
    fn meta_v_is_not_treated_as_a_letter() {
        let events = run(&[down(Key::MetaLeft), down(Key::V)], &[], "Safari");
        assert_eq!(text_of(&events), "");
    }

    #[test]
    fn meta_v_emits_paste_from_injected_clipboard() {
        let (tx, rx) = std::sync::mpsc::channel::<CaptureEvent>();
        let mut state = KeyState::default();
        let ctx = app("Safari");
        super::translate_key_events(
            &[
                (true, Key::MetaLeft),
                (true, Key::V),
                (false, Key::MetaLeft),
            ],
            &mut state,
            &tx,
            &[],
            &|| Some(ctx.clone()),
            &|| Some("pasted content".to_string()),
            None,
            &|| None,
        );

        let mut out = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            out.push(ev);
        }
        let pasted: String = out
            .iter()
            .filter_map(|e| match &e.kind {
                CaptureEventKind::Paste { content } => Some(content.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(pasted, "pasted content");
    }

    #[test]
    fn arrow_and_function_keys_produce_no_text() {
        let events = run(
            &[down(Key::ArrowUp), down(Key::ArrowDown), down(Key::F1), down(Key::F12)],
            &[],
            "Safari",
        );
        assert_eq!(text_of(&events), "");
    }

    #[test]
    fn escape_is_captured_as_a_control_character() {
        // Escape maps to ESC (0x1b) rather than dropping silently, so the
        // reconstructed text stays faithful to what was typed.
        let events = run(&[down(Key::Escape)], &[], "Safari");
        assert_eq!(text_of(&events), "\u{1b}");
    }

    #[test]
    fn space_and_enter_round_trip() {
        let events = run(&[down(Key::Space), down(Key::Enter)], &[], "Safari");
        assert_eq!(text_of(&events), " \n");
    }

    /// D-09 throttle over a fake clock: cooldown + burst cap.
    fn history_at(now: Instant, offsets_secs: &[u64]) -> Vec<Instant> {
        offsets_secs
            .iter()
            .map(|s| now - Duration::from_secs(*s))
            .collect()
    }

    #[test]
    fn first_shot_always_fires() {
        let now = Instant::now();
        assert!(should_shoot(now, &[], Duration::from_secs(2)));
    }

    #[test]
    fn cooldown_blocks_rapid_refire() {
        let now = Instant::now();
        // Last shot 1 s ago: 2 s cooldown says no.
        assert!(!should_shoot(now, &history_at(now, &[1]), Duration::from_secs(2)));
        // Last shot exactly at the cooldown boundary: yes.
        assert!(should_shoot(now, &history_at(now, &[2]), Duration::from_secs(2)));
        // Last shot 3 s ago: yes.
        assert!(should_shoot(now, &history_at(now, &[3]), Duration::from_secs(2)));
    }

    #[test]
    fn burst_cap_allows_three_per_ten_seconds() {
        let now = Instant::now();
        // Two shots in the window: room for a third (past cooldown).
        assert!(should_shoot(now, &history_at(now, &[9, 6]), Duration::from_secs(2)));
        // Three shots in the window: capped.
        assert!(!should_shoot(now, &history_at(now, &[9, 6, 3]), Duration::from_secs(2)));
        // A 4th inside the window is capped even past cooldown.
        assert!(!should_shoot(now, &history_at(now, &[9, 6, 4, 2]), Duration::from_secs(2)));
    }

    #[test]
    fn burst_window_slides() {
        let now = Instant::now();
        // 3 old shots aged out of the 10 s window: fire again.
        assert!(should_shoot(now, &history_at(now, &[30, 20, 11]), Duration::from_secs(2)));
        // But 3 shots inside the window still cap.
        assert!(!should_shoot(now, &history_at(now, &[9, 8, 1]), Duration::from_secs(0)));
    }

    #[test]
    fn zero_cooldown_leaves_only_the_burst_cap() {
        let now = Instant::now();
        assert!(should_shoot(now, &history_at(now, &[5]), Duration::ZERO));
        assert!(!should_shoot(
            now,
            &history_at(now, &[9, 5, 1]),
            Duration::ZERO
        ));
    }

    /// The Quartz TCC probe links and answers without prompting.
    #[test]
    fn screen_probe_links_and_answers() {
        // Value depends on this machine's grant — the assertion is only
        // that the FFI call returns instead of crashing or prompting.
        let _ = super::probe_screen_recording();
    }

    /// Injected probe results flow to the true three-state value: granted
    /// probes stay granted, unprompted denials read undetermined, prompted
    /// denials read denied — independently per grant.
    #[test]
    fn injected_screen_probe_maps_to_three_states() {
        use rustwatch_core::{PermissionsConfig, PermissionState};
        let fresh = PermissionsConfig::default();
        let granted = super::build_report(true, true, true, &fresh);
        assert_eq!(granted.screen_recording, PermissionState::Granted);
        assert_eq!(granted.input_monitoring, PermissionState::Granted);
        assert_eq!(granted.accessibility, PermissionState::Granted);

        let never_asked = super::build_report(false, false, false, &fresh);
        assert_eq!(never_asked.screen_recording, PermissionState::Undetermined);
        assert_eq!(never_asked.input_monitoring, PermissionState::Undetermined);

        let asked = PermissionsConfig {
            prompted_screen_recording: true,
            prompted_input_monitoring: true,
            ..Default::default()
        };
        let denied = super::build_report(false, false, false, &asked);
        assert_eq!(denied.screen_recording, PermissionState::Denied);
        assert_eq!(denied.input_monitoring, PermissionState::Denied);
        // Prompting one grant never flips the others.
        assert_eq!(denied.accessibility, PermissionState::Undetermined);
    }

    /// The Quartz idle probe links and returns a sane value.
    #[test]
    fn idle_probe_links_and_returns_finite() {
        let secs = super::PlatformCapture::system_idle_seconds()
            .expect("CGEventSourceSecondsSinceLastEventType must answer");
        assert!(secs.is_finite() && secs >= 0.0);
    }

    /// The default chord parses: ctrl+shift+space, case-insensitive.
    #[test]
    fn default_chord_parses() {
        let chord = super::parse_hotkey("ctrl+shift+space").expect("default chord");
        assert_eq!(
            chord,
            super::HotkeyChord {
                ctrl: true,
                shift: true,
                meta: false,
                alt: false,
                key: Key::Space,
            }
        );
        assert!(super::parse_hotkey("Ctrl+Shift+Space").is_some());
        assert!(super::parse_hotkey("control+shift+space").is_some());
    }

    #[test]
    fn chord_parser_rejects_non_chords() {
        // Bare keys are keypresses, not chords.
        assert!(super::parse_hotkey("space").is_none());
        assert!(super::parse_hotkey("a").is_none());
        // Unknown tokens fail loudly (hotkey disables with a warning).
        assert!(super::parse_hotkey("ctrl+shift+f13x").is_none());
        assert!(super::parse_hotkey("").is_none());
        // Letters and digits work as main keys.
        assert!(super::parse_hotkey("ctrl+m").is_some());
        assert!(super::parse_hotkey("meta+shift+1").is_some());
    }

    fn chord() -> super::HotkeyChord {
        super::parse_hotkey("ctrl+shift+space").unwrap()
    }

    fn screenshots_of(events: &[CaptureEvent]) -> Vec<&CaptureEvent> {
        events
            .iter()
            .filter(|e| matches!(e.kind, CaptureEventKind::Screenshot { .. }))
            .collect()
    }

    /// The full combination fires exactly one screenshot — and the trigger
    /// Space leaves no phantom text behind.
    #[test]
    fn full_chord_fires_one_screenshot() {
        let events = run_with_hotkey(
            &[
                down(Key::ControlLeft),
                down(Key::ShiftLeft),
                down(Key::Space),
            ],
            &[],
            "Safari",
            Some(&chord()),
        );
        let shots = screenshots_of(&events);
        assert_eq!(shots.len(), 1);
        assert_eq!(text_of(&events), "");
        // The shot carries the app context for segment linkage.
        assert_eq!(
            shots[0].app.as_ref().map(|a| a.app_name.as_str()),
            Some("Safari")
        );
    }

    /// Partial presses and modifier-only sequences never fire.
    #[test]
    fn partial_chord_does_not_fire() {
        let chord = chord();
        // Missing shift.
        let events = run_with_hotkey(
            &[down(Key::ControlLeft), down(Key::Space)],
            &[],
            "Safari",
            Some(&chord),
        );
        assert!(screenshots_of(&events).is_empty());
        // Modifiers only, no Space.
        let events = run_with_hotkey(
            &[down(Key::ControlLeft), down(Key::ShiftLeft)],
            &[],
            "Safari",
            Some(&chord),
        );
        assert!(screenshots_of(&events).is_empty());
        // Space alone.
        let events = run_with_hotkey(&[down(Key::Space)], &[], "Safari", Some(&chord));
        assert!(screenshots_of(&events).is_empty());
        assert_eq!(text_of(&events), " ");
        // Extra modifiers don't count.
        let events = run_with_hotkey(
            &[
                down(Key::ControlLeft),
                down(Key::ShiftLeft),
                down(Key::AltLeft),
                down(Key::Space),
            ],
            &[],
            "Safari",
            Some(&chord),
        );
        assert!(screenshots_of(&events).is_empty());
    }

    /// Holding Space doesn't repeat-fire; releasing and pressing again does.
    #[test]
    fn chord_repeat_is_suppressed_until_release() {
        let chord = chord();
        let (tx, rx) = std::sync::mpsc::channel::<CaptureEvent>();
        let mut state = KeyState::default();
        let ctx = app("Safari");
        let shoot = || {
            Some((
                std::path::PathBuf::from("/tmp/fake.png"),
                rustwatch_core::ScreenshotScope::Window,
            ))
        };
        let drive = |raw: &[(bool, Key)], state: &mut KeyState| {
            super::translate_key_events(
                raw,
                state,
                &tx,
                &[],
                &|| Some(ctx.clone()),
                &|| None,
                Some(&chord),
                &shoot,
            );
        };
        drive(
            &[down(Key::ControlLeft), down(Key::ShiftLeft), down(Key::Space)],
            &mut state,
        );
        // Key-repeat while held: no second shot.
        drive(&[down(Key::Space), down(Key::Space)], &mut state);
        // Release + fresh press: fires again.
        drive(&[(false, Key::Space), down(Key::Space)], &mut state);
        let shots: Vec<_> = {
            let mut out = Vec::new();
            while let Ok(ev) = rx.try_recv() {
                if matches!(ev.kind, CaptureEventKind::Screenshot { .. }) {
                    out.push(ev);
                }
            }
            out
        };
        assert_eq!(shots.len(), 2);
    }

    /// T-02-02: excluded apps emit nothing — not even on the chord keys.
    #[test]
    fn chord_in_excluded_app_emits_nothing() {
        let events = run_with_hotkey(
            &[
                down(Key::ControlLeft),
                down(Key::ShiftLeft),
                down(Key::Space),
            ],
            &["1Password"],
            "1Password",
            Some(&chord()),
        );
        assert!(events.is_empty());
    }

    /// Hotkey disabled (or unparsable): the chord keys behave like normal keys.
    #[test]
    fn no_hotkey_means_normal_keys() {
        let events = run_with_hotkey(
            &[
                down(Key::ControlLeft),
                down(Key::ShiftLeft),
                down(Key::Space),
            ],
            &[],
            "Safari",
            None,
        );
        assert!(screenshots_of(&events).is_empty());
    }
}
