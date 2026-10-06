mod platform;

pub use platform::PlatformCapture;
pub use platform::PermissionsReport;

use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc, Mutex};

use rustwatch_core::{CaptureEvent, Result, ScreenshotScope};

/// Cached keyboard-thread launch args, owned by the daemon for a D-15
/// HotAttach long after startup. Clone-cheap (Arcs + small values).
#[derive(Clone)]
pub struct KeyboardLaunch {
    pub tx: std::sync::mpsc::Sender<CaptureEvent>,
    pub exclude_apps: Vec<String>,
    pub hotkey_enabled: bool,
    pub hotkey_chord: String,
    pub screenshot_root: PathBuf,
    pub paused: Arc<AtomicBool>,
    pub screenshots_live: Arc<AtomicBool>,
    pub keyboard_note: Arc<Mutex<String>>,
}

pub struct CaptureHandle {
    inner: PlatformCapture,
}

impl Clone for CaptureHandle {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl CaptureHandle {
    pub fn new(exclude_apps: Vec<String>) -> Result<Self> {
        Ok(Self {
            inner: PlatformCapture::new(exclude_apps)?,
        })
    }

    pub fn start(
        &self,
        tx: std::sync::mpsc::Sender<CaptureEvent>,
        poll_focus_ms: u64,
        screenshot_on_focus_change: bool,
        min_interval_secs: u64,
        hotkey_enabled: bool,
        hotkey_chord: String,
        idle: std::sync::Arc<std::sync::atomic::AtomicBool>,
        screenshot_root: PathBuf,
        keyboard_enabled: bool,
        screenshots_live: Arc<AtomicBool>,
        keyboard_note: Arc<Mutex<String>>,
    ) -> Result<()> {
        self.inner.start(
            tx,
            poll_focus_ms,
            screenshot_on_focus_change,
            min_interval_secs,
            hotkey_enabled,
            hotkey_chord,
            idle,
            screenshot_root,
            keyboard_enabled,
            screenshots_live,
            keyboard_note,
        )
    }

    /// D-15 HotAttach: spawn the keyboard thread after a late Input
    /// Monitoring grant. At-most-once per process.
    pub fn start_keyboard(&self, launch: KeyboardLaunch) -> Result<bool> {
        self.inner.start_keyboard(launch)
    }

    /// D-15 check-only trio for the re-probe loop. Never prompts.
    pub fn probe_all_check_only() -> (bool, bool, bool) {
        PlatformCapture::probe_all_check_only()
    }

    /// D-13 first-run request, at most once per undetermined grant.
    pub fn request_grant(grant: rustwatch_core::Grant) -> bool {
        PlatformCapture::request_grant(grant)
    }

    pub fn capture_screenshot(
        &self,
        scope: ScreenshotScope,
        root: PathBuf,
    ) -> Result<(PathBuf, ScreenshotScope)> {
        self.inner.capture_screenshot(scope, root)
    }

    pub fn set_paused(&self, paused: bool) {
        self.inner.set_paused(paused);
    }

    /// D-06: the daemon watches this and exits nonzero when a capture
    /// thread dies, so launchd KeepAlive restarts it.
    pub fn threads_alive(&self) -> bool {
        self.inner.threads_alive()
    }

    pub fn permissions() -> PermissionsReport {
        PlatformCapture::permissions()
    }

    pub fn permissions_with_prompted(
        prompted: &rustwatch_core::PermissionsConfig,
    ) -> PermissionsReport {
        PlatformCapture::permissions_with_prompted(prompted)
    }
}
