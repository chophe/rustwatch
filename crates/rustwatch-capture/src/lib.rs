mod platform;

pub use platform::PlatformCapture;
pub use platform::PermissionsReport;

use std::path::PathBuf;

use rustwatch_core::{CaptureEvent, Result, ScreenshotScope};

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
        screenshot_root: PathBuf,
    ) -> Result<()> {
        self.inner.start(
            tx,
            poll_focus_ms,
            screenshot_on_focus_change,
            screenshot_root,
        )
    }

    pub fn capture_screenshot(&self, scope: ScreenshotScope, root: PathBuf) -> Result<PathBuf> {
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
}
