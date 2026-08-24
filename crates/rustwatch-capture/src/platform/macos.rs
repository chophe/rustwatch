use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use rustwatch_core::{
    events::{AppContext, CaptureEvent, CaptureEventKind, ScreenshotScope},
    Result,
};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

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
}

impl Clone for PlatformCapture {
    fn clone(&self) -> Self {
        Self {
            exclude_apps: self.exclude_apps.clone(),
            paused: Arc::clone(&self.paused),
        }
    }
}

impl PlatformCapture {
    pub fn new(exclude_apps: Vec<String>) -> Result<Self> {
        Ok(Self {
            exclude_apps,
            paused: Arc::new(AtomicBool::new(false)),
        })
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
        tx: UnboundedSender<CaptureEvent>,
        poll_focus_ms: u64,
        _accessibility_poll_ms: u64,
        screenshot_on_focus_change: bool,
        screenshot_root: PathBuf,
    ) -> Result<()> {
        let exclude = self.exclude_apps.clone();
        let paused_kb = Arc::clone(&self.paused);
        let paused_focus = Arc::clone(&self.paused);

        let tx_focus = tx.clone();
        std::thread::spawn(move || {
            if let Err(err) = run_keyboard_loop(tx, paused_kb) {
                warn!(?err, "keyboard capture stopped");
            }
        });

        std::thread::spawn(move || {
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
        });

        Ok(())
    }

    pub fn capture_screenshot(&self, scope: ScreenshotScope, root: PathBuf) -> Result<PathBuf> {
        capture_to_disk(scope, root)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}

fn run_keyboard_loop(
    tx: UnboundedSender<CaptureEvent>,
    paused: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    use keytap::{EventKind, Key};

    let tap = keytap::Tap::new().map_err(|e| anyhow::anyhow!("keytap init failed: {e}"))?;
    let mut last_app = current_app_context();
    let mut meta_held = false;

    for event in tap.iter() {
        if paused.load(Ordering::Relaxed) {
            continue;
        }

        let app = current_app_context().or(last_app.clone());
        last_app = app.clone();

        match event.kind {
            EventKind::KeyDown(key) | EventKind::KeyRepeat(key) => {
                if matches!(key, Key::MetaLeft | Key::MetaRight) {
                    meta_held = true;
                }

                let key_name = format!("{key:?}");
                let modifiers = active_modifiers(meta_held);

                if meta_held && matches!(key, Key::V) {
                    if let Ok(content) = read_clipboard_text() {
                        let paste = CaptureEvent::new(
                            CaptureEventKind::Paste { content },
                            app.clone(),
                        );
                        let _ = tx.send(paste);
                    }
                }

                if let Some(text) = key_to_text(key) {
                    let delta = CaptureEvent::new(
                        CaptureEventKind::TextDelta { text },
                        app.clone(),
                    );
                    let _ = tx.send(delta);
                }

                let key_event = CaptureEvent::new(
                    CaptureEventKind::Key {
                        key: key_name,
                        modifiers,
                    },
                    app,
                );
                let _ = tx.send(key_event);
            }
            EventKind::KeyUp(key) => {
                if matches!(key, Key::MetaLeft | Key::MetaRight) {
                    meta_held = false;
                }
            }
        }
    }
    Ok(())
}

fn active_modifiers(meta_held: bool) -> Vec<String> {
    if meta_held {
        vec!["Meta".into()]
    } else {
        Vec::new()
    }
}

fn run_focus_loop(
    tx: UnboundedSender<CaptureEvent>,
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

        if exclude_apps.iter().any(|app| current.app_name.contains(app)) {
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

fn key_to_text(key: keytap::Key) -> Option<String> {
    use keytap::Key;
    match key {
        Key::A => Some("a".into()),
        Key::B => Some("b".into()),
        Key::C => Some("c".into()),
        Key::D => Some("d".into()),
        Key::E => Some("e".into()),
        Key::F => Some("f".into()),
        Key::G => Some("g".into()),
        Key::H => Some("h".into()),
        Key::I => Some("i".into()),
        Key::J => Some("j".into()),
        Key::K => Some("k".into()),
        Key::L => Some("l".into()),
        Key::M => Some("m".into()),
        Key::N => Some("n".into()),
        Key::O => Some("o".into()),
        Key::P => Some("p".into()),
        Key::Q => Some("q".into()),
        Key::R => Some("r".into()),
        Key::S => Some("s".into()),
        Key::T => Some("t".into()),
        Key::U => Some("u".into()),
        Key::V => Some("v".into()),
        Key::W => Some("w".into()),
        Key::X => Some("x".into()),
        Key::Y => Some("y".into()),
        Key::Z => Some("z".into()),
        Key::Digit0 => Some("0".into()),
        Key::Digit1 => Some("1".into()),
        Key::Digit2 => Some("2".into()),
        Key::Digit3 => Some("3".into()),
        Key::Digit4 => Some("4".into()),
        Key::Digit5 => Some("5".into()),
        Key::Digit6 => Some("6".into()),
        Key::Digit7 => Some("7".into()),
        Key::Digit8 => Some("8".into()),
        Key::Digit9 => Some("9".into()),
        Key::Space => Some(" ".into()),
        Key::Enter => Some("\n".into()),
        Key::Tab => Some("\t".into()),
        _ => None,
    }
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
