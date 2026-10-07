use std::path::PathBuf;

use rustwatch_core::{CaptureEvent, PermissionState, Result, ScreenshotScope};

#[derive(Debug, Clone, Default)]
pub struct PermissionsReport {
    pub input_monitoring: PermissionState,
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
    pub notes: Vec<String>,
}

pub struct PlatformCapture;

impl Clone for PlatformCapture {
    fn clone(&self) -> Self {
        Self
    }
}

impl PlatformCapture {
    pub fn new(_exclude_apps: Vec<String>) -> Result<Self> {
        Err(rustwatch_core::Error::UnsupportedPlatform(
            "capture is only implemented for macOS in v0.1".into(),
        ))
    }

    pub fn permissions() -> PermissionsReport {
        PermissionsReport {
            notes: vec!["macOS capture is not available on this platform.".into()],
            ..Default::default()
        }
    }

    pub fn permissions_with_prompted(
        _prompted: &rustwatch_core::PermissionsConfig,
    ) -> PermissionsReport {
        Self::permissions()
    }

    pub fn start(
        &self,
        _tx: std::sync::mpsc::Sender<CaptureEvent>,
        _poll_focus_ms: u64,
        _screenshot_on_focus_change: bool,
        _min_interval_secs: u64,
        _hotkey_enabled: bool,
        _hotkey_chord: String,
        _idle: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _screenshot_root: PathBuf,
        _keyboard_enabled: bool,
        _screenshots_live: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _keyboard_note: std::sync::Arc<std::sync::Mutex<String>>,
    ) -> Result<()> {
        Err(rustwatch_core::Error::UnsupportedPlatform(
            "capture is only implemented for macOS in v0.1".into(),
        ))
    }

    pub fn start_keyboard(
        &self,
        _launch: crate::KeyboardLaunch,
    ) -> Result<bool> {
        Err(rustwatch_core::Error::UnsupportedPlatform(
            "capture is only implemented for macOS in v0.1".into(),
        ))
    }

    pub fn probe_all_check_only() -> (bool, bool, bool) {
        (false, false, false)
    }

    pub fn request_grant(_grant: rustwatch_core::Grant) -> bool {
        false
    }

    pub fn capture_screenshot(
        &self,
        _scope: ScreenshotScope,
        _root: PathBuf,
    ) -> Result<(PathBuf, ScreenshotScope)> {
        Err(rustwatch_core::Error::UnsupportedPlatform(
            "capture is only implemented for macOS in v0.1".into(),
        ))
    }

    pub fn system_idle_seconds() -> Option<f64> {
        // No Quartz off macOS: never idle, never suppress.
        None
    }

    pub fn set_paused(&self, _paused: bool) {}

    pub fn threads_alive(&self) -> bool {
        // The stub never spawns threads; nothing to watch.
        true
    }
}
