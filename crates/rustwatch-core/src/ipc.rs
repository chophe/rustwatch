use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonState {
    pub running: bool,
    pub paused: bool,
    pub pid: Option<u32>,
    pub events_captured: u64,
    pub segments_written: u64,
    pub started_at: Option<String>,
    #[serde(default)]
    pub write_errors: u64,
    #[serde(default)]
    pub dropped_events: u64,
    #[serde(default)]
    pub queued: u64,
    #[serde(default)]
    pub capture_health: String,
}

impl DaemonState {
    /// D-07 health label derived from the loss counters: `healthy` when all
    /// are zero, `degraded` otherwise. Computed daemon-side so every reader
    /// (status, doctor, TUI) renders the same banner.
    pub fn health_label(write_errors: u64, dropped_events: u64, queued: u64) -> String {
        if write_errors == 0 && dropped_events == 0 && queued == 0 {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        }
    }

    /// D-07 banner line, e.g. `capture: healthy` or
    /// `capture: degraded (3 write errors, 12 queued)`. Detail counters only
    /// appear when nonzero.
    pub fn health_banner(&self) -> String {
        if self.capture_health == "healthy" {
            return "capture: healthy".to_string();
        }
        let mut parts = Vec::new();
        if self.write_errors > 0 {
            parts.push(format!("{} write errors", self.write_errors));
        }
        if self.dropped_events > 0 {
            parts.push(format!("{} dropped", self.dropped_events));
        }
        if self.queued > 0 {
            parts.push(format!("{} queued", self.queued));
        }
        if parts.is_empty() {
            "capture: degraded".to_string()
        } else {
            format!("capture: degraded ({})", parts.join(", "))
        }
    }
}

/// Server-side cap for `Tail { limit }` (D-07/Pitfall 7): an unbounded tail
/// can grow a single reply past the IPC frame cap and turn a legit read into
/// an error. The daemon clamps larger requests to this.
pub const TAIL_MAX_LIMIT: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum DaemonCommand {
    Ping,
    Status,
    Pause,
    Resume,
    Tail { limit: usize },
    Screenshot { window: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum DaemonReply {
    Ok,
    Status { state: DaemonState },
    Events { events: Vec<crate::CaptureEvent> },
    Error { message: String },
    Screenshot { path: String },
}

pub struct DaemonClient {
    socket_path: std::path::PathBuf,
}

impl DaemonClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub async fn send(&self, command: DaemonCommand) -> Result<DaemonReply> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(|_| crate::Error::DaemonNotRunning)?;

        let payload = serde_json::to_vec(&command)?;
        let len = (payload.len() as u32).to_be_bytes();
        stream.write_all(&len).await?;
        stream.write_all(&payload).await?;

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;
        let mut resp_buf = vec![0u8; resp_len];
        stream.read_exact(&mut resp_buf).await?;
        Ok(serde_json::from_slice(&resp_buf)?)
    }
}

pub async fn handle_connection(
    mut stream: UnixStream,
    handler: impl Fn(DaemonCommand) -> DaemonReply,
) -> Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let req_len = u32::from_be_bytes(len_buf) as usize;
    let mut req_buf = vec![0u8; req_len];
    stream.read_exact(&mut req_buf).await?;
    let command: DaemonCommand = serde_json::from_slice(&req_buf)?;
    let reply = handler(command);
    let payload = serde_json::to_vec(&reply)?;
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&payload).await?;
    Ok(())
}
