use std::path::PathBuf;

use rustwatch_core::{CaptureEvent, Result, ScreenshotScope};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, Default)]
pub struct PermissionsReport {
    pub input_monitoring: bool,
    pub accessibility: bool,
    pub screen_recording: bool,
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

    pub fn start(
        &self,
        _tx: UnboundedSender<CaptureEvent>,
        _poll_focus_ms: u64,
        _accessibility_poll_ms: u64,
        _screenshot_on_focus_change: bool,
        _screenshot_root: PathBuf,
    ) -> Result<()> {
        Err(rustwatch_core::Error::UnsupportedPlatform(
            "capture is only implemented for macOS in v0.1".into(),
        ))
    }

    pub fn capture_screenshot(
        &self,
        _scope: ScreenshotScope,
        _root: PathBuf,
    ) -> Result<PathBuf> {
        Err(rustwatch_core::Error::UnsupportedPlatform(
            "capture is only implemented for macOS in v0.1".into(),
        ))
    }

    pub fn set_paused(&self, _paused: bool) {}
}
