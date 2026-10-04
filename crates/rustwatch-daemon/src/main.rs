use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use anyhow::Context;
use chrono::Utc;
use rustwatch_capture::CaptureHandle;
use rustwatch_core::{
    paths::{ensure_parent, load_or_create_config},
    CaptureEvent, CaptureEventKind, Config, DataPaths, DaemonCommand, DaemonReply, DaemonState,
    SegmentGrouper, Store,
};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let paths = DataPaths::new(None)?;
    paths.ensure_dirs()?;
    let config = load_or_create_config(&paths.config)?;
    run_daemon(paths, config).await
}

async fn run_daemon(paths: DataPaths, config: Config) -> anyhow::Result<()> {
    ensure_parent(&paths.socket)?;
    if paths.socket.exists() {
        let _ = std::fs::remove_file(&paths.socket);
    }

    let store = Store::open(&paths.sqlite)?;
    let paused = Arc::new(AtomicBool::new(false));
    let events_captured = Arc::new(AtomicU64::new(0));
    let segments_written = Arc::new(AtomicU64::new(0));
    let write_errors = Arc::new(AtomicU64::new(0));
    let dropped_events = Arc::new(AtomicU64::new(0));
    let queued = Arc::new(AtomicU64::new(0));
    let started_at = Utc::now().to_rfc3339();

    std::fs::write(&paths.pid_file, std::process::id().to_string())?;

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<CaptureEvent>();
    let capture = CaptureHandle::new(config.capture.exclude_apps.clone())?;
    let capture_for_ipc = capture.clone();
    capture.start(
        event_tx,
        config.capture.poll_focus_ms,
        config.capture.accessibility_poll_ms,
        config.capture.screenshot_on_focus_change,
        paths.screenshots.clone(),
    )?;

    let writer_segments = Arc::clone(&segments_written);
    let writer_events = Arc::clone(&events_captured);
    let writer_errors = Arc::clone(&write_errors);
    // D-02: the writer awaits the lock / owns the store — it never drops an
    // event on IPC contention. Every persistence call is counted: failures
    // surface via `error!` AND the `write_errors` counter (D-01), never `let _ =`.
    tokio::spawn(async move {
        let mut grouper = SegmentGrouper::new();
        while let Some(event) = event_rx.recv().await {
            writer_events.fetch_add(1, Ordering::Relaxed);
            if let Err(err) = store.insert_event(&event) {
                error!(?err, "writer: insert_event failed");
                writer_errors.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(segment) = grouper.on_event(&event) {
                match store.insert_segment(&segment) {
                    Ok(()) => {
                        writer_segments.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(err) => {
                        error!(?err, "writer: insert_segment failed");
                        writer_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            if let CaptureEventKind::Screenshot { path, scope } = &event.kind {
                if let Err(err) = store.insert_screenshot(&rustwatch_core::ScreenshotRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    path: path.clone(),
                    scope: *scope,
                    captured_at: event.timestamp,
                    segment_id: None,
                }) {
                    error!(?err, "writer: insert_screenshot failed");
                    writer_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if let Some(segment) = grouper.flush() {
            match store.insert_segment(&segment) {
                Ok(()) => {
                    writer_segments.fetch_add(1, Ordering::Relaxed);
                }
                Err(err) => {
                    error!(?err, "writer: flush insert_segment failed");
                    writer_errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    let listener = UnixListener::bind(&paths.socket).context("bind daemon socket")?;
    info!(socket = %paths.socket.display(), "rustwatchd listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let paused_flag = Arc::clone(&paused);
        let events_ref = Arc::clone(&events_captured);
        let segments_ref = Arc::clone(&segments_written);
        let errors_ref = Arc::clone(&write_errors);
        let dropped_ref = Arc::clone(&dropped_events);
        let queued_ref = Arc::clone(&queued);
        let started = started_at.clone();
        let capture_ref = paths.screenshots.clone();
        let capture_handle = capture_for_ipc.clone();
        let sqlite_path = paths.sqlite.clone();

        let capture_shots = capture_ref.clone();

        tokio::spawn(async move {
            let handler = move |command: DaemonCommand| -> DaemonReply {
                match command {
                    DaemonCommand::Ping => DaemonReply::Ok,
                    DaemonCommand::Pause => {
                        paused_flag.store(true, Ordering::Relaxed);
                        capture_handle.set_paused(true);
                        DaemonReply::Ok
                    }
                    DaemonCommand::Resume => {
                        paused_flag.store(false, Ordering::Relaxed);
                        capture_handle.set_paused(false);
                        DaemonReply::Ok
                    }
                    DaemonCommand::Status => {
                        let write_errors = errors_ref.load(Ordering::Relaxed);
                        let dropped_events = dropped_ref.load(Ordering::Relaxed);
                        let queued = queued_ref.load(Ordering::Relaxed);
                        DaemonReply::Status {
                            state: DaemonState {
                                running: true,
                                paused: paused_flag.load(Ordering::Relaxed),
                                pid: Some(std::process::id()),
                                events_captured: events_ref.load(Ordering::Relaxed),
                                segments_written: segments_ref.load(Ordering::Relaxed),
                                started_at: Some(started.clone()),
                                write_errors,
                                dropped_events,
                                queued,
                                capture_health: DaemonState::health_label(
                                    write_errors,
                                    dropped_events,
                                    queued,
                                ),
                            },
                        }
                    }
                    DaemonCommand::Tail { limit } => {
                        // Tail reads through a short-lived separate connection so a
                        // long scan can never stall the writer (Pattern 1). The
                        // limit is clamped server-side (Pitfall 7) so a huge tail
                        // cannot blow past the IPC frame cap.
                        let limit = limit.min(rustwatch_core::ipc::TAIL_MAX_LIMIT);
                        match Store::open(&sqlite_path)
                            .and_then(|store| store.list_events_since(None, limit))
                        {
                            Ok(events) => DaemonReply::Events { events },
                            Err(err) => {
                                error!(?err, "tail read failed");
                                DaemonReply::Error {
                                    message: err.to_string(),
                                }
                            }
                        }
                    }
                    DaemonCommand::Screenshot { window } => {
                        let scope = if window {
                            rustwatch_core::ScreenshotScope::Window
                        } else {
                            rustwatch_core::ScreenshotScope::Screen
                        };
                        match CaptureHandle::new(Vec::new())
                            .and_then(|h| h.capture_screenshot(scope, capture_shots.clone()))
                        {
                            Ok(path) => DaemonReply::Screenshot {
                                path: path.display().to_string(),
                            },
                            Err(err) => DaemonReply::Error {
                                message: err.to_string(),
                            },
                        }
                    }
                }
            };

            if let Err(err) = rustwatch_core::ipc::handle_connection(stream, handler).await {
                error!(?err, "daemon connection failed");
            }
        });
    }
}
