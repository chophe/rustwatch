use std::path::Path;
use std::time::Duration;

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
    /// 01-03 wire-type change (with 01-01 counters): the daemon populates
    /// this at startup from real TCC probes; CLI/TUI/doctor only render it.
    /// `#[serde(default)]` keeps older daemon replies parseable.
    #[serde(default)]
    pub permissions: crate::PermissionsState,
    /// Visible capture-path note (parked keyboard reason, restart-required):
    /// a dead path is a daemon-state fact, never a lone log warning.
    #[serde(default)]
    pub capture_note: String,
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

/// T-01-01: 16 MiB frame cap, enforced in both directions. A hostile local
/// process must not turn a length prefix into a 4 GiB allocation; oversize
/// frames are rejected with an error reply, never read.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// How long either IPC end waits for the other before surfacing a clear
/// daemon-busy error instead of hanging forever.
pub const IPC_IO_TIMEOUT: Duration = Duration::from_secs(30);

fn timeout_err() -> crate::Error {
    crate::Error::Other("ipc timed out waiting for the other end (daemon busy?)".into())
}

async fn write_frame(stream: &mut UnixStream, payload: &[u8]) -> Result<()> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(crate::Error::Other(format!(
            "ipc frame ({} bytes) exceeds the 16 MiB cap",
            payload.len()
        )));
    }
    let len = (payload.len() as u32).to_be_bytes();
    tokio::time::timeout(IPC_IO_TIMEOUT, stream.write_all(&len))
        .await
        .map_err(|_| timeout_err())??;
    tokio::time::timeout(IPC_IO_TIMEOUT, stream.write_all(payload))
        .await
        .map_err(|_| timeout_err())??;
    Ok(())
}

async fn read_frame(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(IPC_IO_TIMEOUT, stream.read_exact(&mut len_buf))
        .await
        .map_err(|_| timeout_err())??;
    let req_len = u32::from_be_bytes(len_buf) as usize;
    if req_len > MAX_FRAME_BYTES {
        return Err(crate::Error::Other(format!(
            "ipc frame ({req_len} bytes) exceeds the 16 MiB cap"
        )));
    }
    let mut req_buf = vec![0u8; req_len];
    tokio::time::timeout(IPC_IO_TIMEOUT, stream.read_exact(&mut req_buf))
        .await
        .map_err(|_| timeout_err())??;
    Ok(req_buf)
}

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
        let mut stream = tokio::time::timeout(IPC_IO_TIMEOUT, UnixStream::connect(&self.socket_path))
            .await
            .map_err(|_| timeout_err())?
            .map_err(|_| crate::Error::DaemonNotRunning)?;

        let payload = serde_json::to_vec(&command)?;
        write_frame(&mut stream, &payload).await?;
        let resp_buf = read_frame(&mut stream).await?;
        Ok(serde_json::from_slice(&resp_buf)?)
    }
}

pub async fn handle_connection(
    mut stream: UnixStream,
    handler: impl Fn(DaemonCommand) -> DaemonReply,
) -> Result<()> {
    let req_buf = match read_frame(&mut stream).await {
        Ok(buf) => buf,
        Err(err) => {
            // Tell the caller why before dropping the connection.
            let reply = DaemonReply::Error {
                message: err.to_string(),
            };
            if let Ok(payload) = serde_json::to_vec(&reply) {
                let _ = write_frame(&mut stream, &payload).await;
            }
            return Err(err);
        }
    };
    let command: DaemonCommand = serde_json::from_slice(&req_buf)?;
    let reply = handler(command);
    let payload = serde_json::to_vec(&reply)?;
    write_frame(&mut stream, &payload).await?;
    Ok(())
}
