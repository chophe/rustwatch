use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use rustwatch_core::{
    events::{AppContext, CaptureEvent, CaptureEventKind, ScreenshotScope},
    is_excluded, key_to_text, LogicalKey, Modifiers, Result,
};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

pub type EventSender<T> = std::sync::mpsc::Sender<T>;

#[derive(Debug, Clone, Default)]
pub struct PermissionsReport {
    pub input_monitoring: bool,
    pub accessibility: bool,
    pub screen_recording: bool,
    pub notes: Vec<String>,
}

pub struct PlatformCapture {
    exclude_apps: Vec<String>,
    paused: Arc<AtomicBool>,
    /// D-06: join handles for the capture threads. The daemon watches them
    /// and exits nonzero if any dies, so launchd KeepAlive restarts it.
    threads: Arc<Mutex<Vec<std::thread::JoinHandle<()>>>>,
}

impl Clone for PlatformCapture {
    fn clone(&self) -> Self {
        Self {
            exclude_apps: self.exclude_apps.clone(),
            paused: Arc::clone(&self.paused),
            threads: Arc::clone(&self.threads),
        }
    }
}

impl PlatformCapture {
    pub fn new(exclude_apps: Vec<String>) -> Result<Self> {
        Ok(Self {
            exclude_apps,
            paused: Arc::new(AtomicBool::new(false)),
            threads: Arc::new(Mutex::new(Vec::new())),
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
        let mut notes = Vec::new();
        notes.push("Grant Input Monitoring, Accessibility, and Screen Recording in System Settings.".into());
        notes.push("After granting Screen Recording, restart rustwatchd.".into());
        PermissionsReport {
            input_monitoring: false,
            accessibility: false,
            screen_recording: false,
            notes,
        }
    }

    pub fn start(
        &self,
        tx: EventSender<CaptureEvent>,
        poll_focus_ms: u64,
        _accessibility_poll_ms: u64,
        screenshot_on_focus_change: bool,
        screenshot_root: PathBuf,
    ) -> Result<()> {
        let exclude = self.exclude_apps.clone();
        let paused_kb = Arc::clone(&self.paused);
        let paused_focus = Arc::clone(&self.paused);

        let exclude_kb = exclude.clone();
        let tx_focus = tx.clone();
        let threads = Arc::clone(&self.threads);
        let keyboard = std::thread::Builder::new()
            .name("capture-keyboard".into())
            .spawn(move || {
                if let Err(err) = run_keyboard_loop(tx, paused_kb, exclude_kb) {
                    warn!(?err, "keyboard capture stopped");
                }
            })
            .map_err(|e| rustwatch_core::Error::Other(format!("spawn keyboard thread: {e}")))?;

        let focus = std::thread::Builder::new()
            .name("capture-focus".into())
            .spawn(move || {
                if let Err(err) = run_focus_loop(
                    tx_focus,
                    poll_focus_ms,
                    screenshot_on_focus_change,
                    screenshot_root,
                    exclude,
                    paused_focus,
                ) {
                    warn!(?err, "focus capture stopped");
                }
            })
            .map_err(|e| rustwatch_core::Error::Other(format!("spawn focus thread: {e}")))?;

        threads
            .lock()
            .map_err(|_| rustwatch_core::Error::Other("capture thread registry poisoned".into()))?
            .extend([keyboard, focus]);

        Ok(())
    }

    pub fn capture_screenshot(&self, scope: ScreenshotScope, root: PathBuf) -> Result<PathBuf> {
        capture_to_disk(scope, root)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
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
/// so tests can drive the full pipeline deterministically.
fn translate_key_events<A, C>(
    raw: &[(bool, keytap::Key)],
    state: &mut KeyState,
    tx: &EventSender<CaptureEvent>,
    exclude_apps: &[String],
    resolve_app: &A,
    read_clipboard: &C,
) where
    A: Fn() -> Option<AppContext>,
    C: Fn() -> Option<String>,
{
    for (is_down, key) in raw {
        if !is_down {
            state.release(*key);
            continue;
        }
        state.press(*key);

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
    last_app: Option<AppContext>,
}

impl KeyState {
    fn press(&mut self, key: keytap::Key) {
        match key {
            keytap::Key::MetaLeft | keytap::Key::MetaRight => self.meta_held = true,
            keytap::Key::ShiftLeft | keytap::Key::ShiftRight => self.shift_held = true,
            keytap::Key::CapsLock => self.caps_lock = !self.caps_lock,
            _ => {}
        }
    }

    fn release(&mut self, key: keytap::Key) {
        match key {
            keytap::Key::MetaLeft | keytap::Key::MetaRight => self.meta_held = false,
            keytap::Key::ShiftLeft | keytap::Key::ShiftRight => self.shift_held = false,
            _ => {}
        }
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
        if self.caps_lock {
            names.push("CapsLock".to_string());
        }
        names
    }
}

fn run_keyboard_loop(
    tx: EventSender<CaptureEvent>,
    paused: Arc<AtomicBool>,
    exclude_apps: Vec<String>,
) -> anyhow::Result<()> {
    let tap = keytap::Tap::new().map_err(|e| anyhow::anyhow!("keytap init failed: {e}"))?;
    let mut source = KeytapSource { tap };
    let mut state = KeyState::default();

    loop {
        if paused.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }
        let raw = source.next_events();
        if raw.is_empty() {
            break;
        }
        translate_key_events(
            &raw,
            &mut state,
            &tx,
            &exclude_apps,
            &current_app_context,
            &|| read_clipboard_text().ok(),
        );
    }
    Ok(())
}

fn run_focus_loop(
    tx: EventSender<CaptureEvent>,
    poll_focus_ms: u64,
    screenshot_on_focus_change: bool,
    screenshot_root: PathBuf,
    exclude_apps: Vec<String>,
    paused: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut last: Option<AppContext> = None;
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

            if screenshot_on_focus_change {
                if let Ok(path) = capture_to_disk(ScreenshotScope::Window, screenshot_root.clone())
                {
                    let shot = CaptureEvent::new(
                        CaptureEventKind::Screenshot {
                            path,
                            scope: ScreenshotScope::Window,
                        },
                        Some(current.clone()),
                    );
                    let _ = tx.send(shot);
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

fn capture_to_disk(scope: ScreenshotScope, root: PathBuf) -> Result<PathBuf> {
    use chrono::Utc;
    use xcap::{Monitor, Window};

    let date_dir = root.join(Utc::now().format("%Y-%m-%d").to_string());
    std::fs::create_dir_all(&date_dir).map_err(rustwatch_core::Error::from)?;
    let filename = format!("{}-{}.png", Utc::now().timestamp_millis(), scope_label(scope));
    let path = date_dir.join(filename);

    match scope {
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
        }
        ScreenshotScope::Window => {
            let windows = Window::all().map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            let window = windows.into_iter().find(|w| w.is_focused().unwrap_or(false));
            if let Some(window) = window {
                let image = window
                    .capture_image()
                    .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
                image
                    .save(&path)
                    .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            } else if let Some(monitor) = Monitor::all()
                .ok()
                .and_then(|m| m.into_iter().next())
            {
                let image = monitor
                    .capture_image()
                    .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
                image
                    .save(&path)
                    .map_err(|e| rustwatch_core::Error::Other(e.to_string()))?;
            }
        }
    }

    debug!(path = %path.display(), "screenshot saved");
    Ok(path)
}

fn scope_label(scope: ScreenshotScope) -> &'static str {
    match scope {
        ScreenshotScope::Window => "window",
        ScreenshotScope::Screen => "screen",
    }
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{translate_key_events, KeyState};
    use keytap::Key;
    use rustwatch_core::events::{AppContext, CaptureEvent, CaptureEventKind};

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
}
