use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::Utc;
use rustwatch_capture::CaptureHandle;
use rustwatch_core::{
    is_retryable_store_error,
    paths::{ensure_parent, load_or_create_config},
    CaptureEvent, CaptureEventKind, Config, DataPaths, DaemonCommand, DaemonReply, DaemonState,
    RetryQueue, SegmentGrouper, Store,
};
use tokio::net::UnixListener;
use tokio::time::MissedTickBehavior;
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
    // SYS-01 / T-01-05: lock FIRST, then write pid, then bind the socket.
    // The advisory lock releases on any process death (crash included), so
    // no stale-file dance can race a second instance. Never delete-then-bind.
    ensure_parent(&paths.pid_file)?;
    let pid_file_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&paths.pid_file)?;
    if fs2::FileExt::try_lock_exclusive(&pid_file_lock).is_err() {
        eprintln!(
            "rustwatchd is already running (pid file locked: {})",
            paths.pid_file.display()
        );
        std::process::exit(2);
    }
    std::fs::write(&paths.pid_file, std::process::id().to_string())?;

    // Stale socket removal is safe only here: the lock above proves no other
    // daemon is alive to race the bind below.
    ensure_parent(&paths.socket)?;
    if paths.socket.exists() {
        std::fs::remove_file(&paths.socket).context("remove stale daemon socket")?;
    }

    let store = Store::open(&paths.sqlite)?;
    let paused = Arc::new(AtomicBool::new(false));
    let counters = WriterCounters::new();
    let events_captured = Arc::clone(&counters.events);
    let segments_written = Arc::clone(&counters.segments);
    let write_errors = Arc::clone(&counters.write_errors);
    let dropped_events = Arc::clone(&counters.dropped);
    let queued = Arc::clone(&counters.queued);
    let started_at = Utc::now().to_rfc3339();

    std::fs::write(&paths.pid_file, std::process::id().to_string())?;

    let (event_tx, event_rx) = std::sync::mpsc::channel::<CaptureEvent>();
    // Cloned before `start` consumes the original: the interval scheduler
    // emits onto the SAME channel as keyboard/focus events (uniform
    // retry/loss accounting — no second code path).
    let scheduler_tx = event_tx.clone();
    let capture = CaptureHandle::new(config.capture.exclude_apps.clone())?;
    let capture_for_ipc = capture.clone();
    let capture_for_watch = capture.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    capture.start(
        event_tx,
        config.capture.poll_focus_ms,
        config.capture.screenshot_on_focus_change,
        paths.screenshots.clone(),
    )?;

    // D-05: SIGTERM/SIGINT stops capture, then the writer drains with a
    // 2 s deadline and the process exits cleanly.
    {
        let sig_shutdown = Arc::clone(&shutdown);
        let sig_notify = Arc::clone(&shutdown_notify);
        let sig_capture = capture.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("install SIGTERM handler");
                tokio::select! {
                    _ = term.recv() => info!("SIGTERM received: flushing and exiting"),
                    _ = tokio::signal::ctrl_c() => info!("SIGINT received: flushing and exiting"),
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            sig_capture.set_paused(true);
            sig_shutdown.store(true, Ordering::SeqCst);
            // Wakes EVERY waiter (accept loop + scheduler), not just one:
            // `notify_one` could wake the scheduler and leave the accept
            // loop parked forever.
            sig_notify.notify_waiters();
        });
    }

    let writer_counters = counters.clone_handle();
    let writer_shutdown = Arc::clone(&shutdown);
    // D-03/D-04: the writer owns Store + SegmentGrouper + retry queue on its
    // own thread — never on the tokio runtime (Pitfall 1). Small blocking
    // units; the 500 ms recv timeout doubles as the 500 ms retry timer.
    // D-02: the writer awaits events / owns the store — it never drops an
    // event on IPC contention. Every persistence call is counted: failures
    // surface via `error!` AND the `write_errors` counter (D-01), never `let _ =`.
    let writer_handle = std::thread::Builder::new()
        .name("rustwatch-writer".into())
        .spawn(move || {
            let mut grouper = SegmentGrouper::new();
            let mut queue = RetryQueue::new();
            loop {
                match event_rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(event) => {
                        handle_event(
                            &store,
                            &mut grouper,
                            &mut queue,
                            &writer_counters,
                            event,
                        );
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if writer_shutdown.load(Ordering::SeqCst) {
                            break;
                        }
                        drain_retry_queue(
                            &store,
                            &mut grouper,
                            &mut queue,
                            &writer_counters,
                            Duration::from_millis(250),
                            false,
                        );
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            // Shutdown tail: drain the channel backlog, then the retry queue
            // with a 2 s deadline (remainder counts as dropped), then flush
            // the tail segment so no in-memory fold is lost.
            while let Ok(event) = event_rx.try_recv() {
                handle_event(&store, &mut grouper, &mut queue, &writer_counters, event);
            }
            drain_retry_queue(
                &store,
                &mut grouper,
                &mut queue,
                &writer_counters,
                Duration::from_secs(2),
                true,
            );
            if let Some(segment) = grouper.flush() {
                match store.insert_segment(&segment) {
                    Ok(()) => {
                        writer_counters.segments.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(err) => {
                        error!(?err, "writer: flush insert_segment failed");
                        writer_counters.write_errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        })
        .context("spawn writer thread")?;

    // CAPT-02: interval screenshots ride the same mpsc channel as capture
    // events. A 0 interval disables interval shots entirely.
    let scheduler_handle = interval_duration(config.capture.screenshot_interval_secs).map(|period| {
        tokio::spawn(run_screenshot_scheduler(
            capture.clone(),
            scheduler_tx,
            period,
            paths.screenshots.clone(),
            Arc::clone(&paused),
            Arc::clone(&shutdown),
            Arc::clone(&shutdown_notify),
        ))
    });

    let listener = UnixListener::bind(&paths.socket).context("bind daemon socket")?;
    // T-01-03: the socket streams full keystroke/screen history — owner-only
    // after every bind (covers pre-existing sockets too).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&paths.socket, std::fs::Permissions::from_mode(0o600))
            .context("chmod 0600 daemon socket")?;
    }
    info!(socket = %paths.socket.display(), "rustwatchd listening");

    let mut health_tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = shutdown_notify.notified() => {
                info!("shutdown requested: waiting for writer to flush");
                break;
            }
            _ = health_tick.tick() => {
                // D-06: any task death exits nonzero so launchd KeepAlive
                // restarts the daemon. In-process supervisors were rejected;
                // the exit code IS the supervision signal.
                if writer_handle.is_finished() && !shutdown.load(Ordering::SeqCst) {
                    error!("writer thread died unexpectedly; exiting for restart");
                    std::process::exit(1);
                }
                if !shutdown.load(Ordering::SeqCst) && !capture_for_watch.threads_alive() {
                    error!("capture thread died unexpectedly; exiting for restart");
                    std::process::exit(1);
                }
                // D-06 covers the scheduler too: a dead interval task must
                // restart the daemon, never silently stop screenshots.
                if let Some(handle) = &scheduler_handle {
                    if handle.is_finished() && !shutdown.load(Ordering::SeqCst) {
                        error!("scheduler task died unexpectedly; exiting for restart");
                        std::process::exit(1);
                    }
                }
            }
            res = listener.accept() => {
                let (stream, _) = res?;
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
    }

    match writer_handle.join() {
        Ok(()) => info!("writer flushed; exiting cleanly"),
        Err(_) => {
            error!("writer thread panicked during shutdown");
            std::process::exit(1);
        }
    }
    // The scheduler broke out of its select! on the shutdown notification;
    // await it so an in-flight interval shot finishes before the runtime
    // drops (a detached send failure is harmless — the writer is gone).
    if let Some(handle) = scheduler_handle {
        let _ = handle.await;
    }
    Ok(())
}

/// Shared loss counters behind the writer thread and the IPC status path.
struct WriterCounters {
    events: Arc<AtomicU64>,
    segments: Arc<AtomicU64>,
    write_errors: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    queued: Arc<AtomicU64>,
}

impl WriterCounters {
    fn new() -> Self {
        Self {
            events: Arc::new(AtomicU64::new(0)),
            segments: Arc::new(AtomicU64::new(0)),
            write_errors: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            queued: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Split handle so the writer thread owns one copy while `run_daemon`
    /// keeps the originals for the status path. Both point at the same
    /// atomics the tracer task created.
    fn clone_handle(&self) -> WriterCounters {
        WriterCounters {
            events: Arc::clone(&self.events),
            segments: Arc::clone(&self.segments),
            write_errors: Arc::clone(&self.write_errors),
            dropped: Arc::clone(&self.dropped),
            queued: Arc::clone(&self.queued),
        }
    }

    fn sync_queue(&self, queue: &mut RetryQueue) {
        self.queued.store(queue.len() as u64, Ordering::Relaxed);
        let overflow = queue.take_dropped();
        if overflow > 0 {
            self.dropped.fetch_add(overflow, Ordering::Relaxed);
        }
    }
}

/// D-12 slack: a tick later than `interval + slack` implies the machine
/// slept (normal scheduling jitter is milliseconds, never seconds).
const WAKE_SLACK: Duration = Duration::from_secs(5);

/// CAPT-02 knob mapping: 0 disables interval shots, anything else is the
/// tick period. Pure so the disable path is unit-testable without a timer.
fn interval_duration(secs: u64) -> Option<Duration> {
    if secs == 0 {
        None
    } else {
        Some(Duration::from_secs(secs))
    }
}

/// D-12 wake detection over the monotonic clock: `elapsed` much larger than
/// the interval (plus slack) means missed ticks were slept through, not
/// merely jittered. Pure — the scheduler injects `Instant`s, tests inject
/// arithmetic.
fn detect_wake(elapsed: Duration, interval: Duration) -> bool {
    elapsed > interval + WAKE_SLACK
}

/// CAPT-02/D-12: one `tokio::time::interval` ticking at `period`.
///
/// Every tick shoots EXACTLY once via `spawn_blocking` (xcap is blocking)
/// and emits a normal `Screenshot` event on the shared channel, so interval
/// shots get the writer's retry/loss accounting for free. `Skip` discards
/// sleep-missed ticks instead of bursting them (Pitfall 5); a detected wake
/// gap only logs — the current tick IS the single wake shot, keeping cadence
/// instead of queuing stale rows.
async fn run_screenshot_scheduler(
    capture: CaptureHandle,
    tx: std::sync::mpsc::Sender<CaptureEvent>,
    period: Duration,
    screenshot_root: std::path::PathBuf,
    paused: Arc<AtomicBool>,
    shutdown_flag: Arc<AtomicBool>,
    shutdown: Arc<tokio::sync::Notify>,
) {
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // The first tick fires immediately: consume it for alignment so the
    // first real shot lands one full period after startup.
    ticker.tick().await;
    let mut last_tick = Instant::now();
    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                info!("scheduler shutting down");
                break;
            }
            _ = ticker.tick() => {
                let now = Instant::now();
                let elapsed = now.saturating_duration_since(last_tick);
                last_tick = now;
                // The Notify is lossy: a SIGTERM landing mid-capture misses
                // the waiter, so the flag (set synchronously by the signal
                // handler) is the authoritative stop — never shoot post-TERM.
                if shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }
                if paused.load(Ordering::Relaxed) {
                    continue;
                }
                if detect_wake(elapsed, period) {
                    info!(elapsed = ?elapsed, "wake gap detected: single wake shot, backlog skipped");
                }
                let pending = {
                    let capture = capture.clone();
                    let root = screenshot_root.clone();
                    tokio::task::spawn_blocking(move || {
                        capture.capture_screenshot(
                            rustwatch_core::ScreenshotScope::Screen,
                            root,
                        )
                    })
                };
                // Abandon (don't await) an in-flight capture on shutdown:
                // the detached thread finishes its file write harmlessly and
                // its send fails silently against the drained writer, while
                // SIGTERM-to-exit stays in milliseconds, not capture-length.
                let shot = tokio::select! {
                    result = pending => Some(result),
                    _ = shutdown.notified() => None,
                };
                let Some(shot) = shot else {
                    info!("scheduler shutting down mid-capture");
                    break;
                };
                if shutdown_flag.load(Ordering::SeqCst) {
                    break;
                }
                match shot {
                    Ok(Ok(path)) => {
                        let event = CaptureEvent::new(
                            CaptureEventKind::Screenshot {
                                path,
                                scope: rustwatch_core::ScreenshotScope::Screen,
                            },
                            None,
                        );
                        if tx.send(event).is_err() {
                            error!("scheduler: writer gone; exiting");
                            break;
                        }
                    }
                    // D-14: no Screen Recording grant (or headless CI) skips
                    // the shot quietly — never a counter, never a crash.
                    Ok(Err(err)) => tracing::debug!(?err, "scheduler: interval shot skipped"),
                    Err(join_err) => {
                        error!(?join_err, "scheduler blocking task failed; exiting");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod scheduler_tests {
    use super::*;

    /// Just under the boundary is jitter, not sleep.
    #[test]
    fn normal_tick_is_not_wake() {
        let interval = Duration::from_secs(300);
        assert!(!detect_wake(Duration::from_secs(300), interval));
        assert!(!detect_wake(Duration::from_secs(304), interval));
        assert!(!detect_wake(interval + WAKE_SLACK, interval));
    }

    /// Just over the boundary (plus a 2-minute nap on a 10 s test cadence)
    /// is unambiguously sleep.
    #[test]
    fn gap_beyond_interval_plus_slack_is_wake() {
        let interval = Duration::from_secs(300);
        assert!(detect_wake(interval + WAKE_SLACK + Duration::from_millis(1), interval));
        assert!(detect_wake(Duration::from_secs(120), Duration::from_secs(10)));
        assert!(detect_wake(Duration::from_secs(3600), interval));
    }

    /// 0 disables interval shots; anything else ticks at face value.
    #[test]
    fn interval_zero_disables() {
        assert_eq!(interval_duration(0), None);
        assert_eq!(interval_duration(300), Some(Duration::from_secs(300)));
        assert_eq!(interval_duration(10), Some(Duration::from_secs(10)));
    }

    /// Pitfall 5: after a simulated sleep (10 periods with no polls), Skip
    /// yields exactly one ready tick — the wake slot — and the next tick
    /// pends a full period. No backlog burst, ever.
    #[tokio::test]
    async fn skip_behavior_produces_no_backlog() {
        tokio::time::pause();
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        ticker.tick().await; // t=0 alignment tick
        tokio::time::advance(Duration::from_secs(10)).await; // sleep
        ticker.tick().await; // the single wake-slot tick
        // The next tick must NOT be immediately ready: with Burst it would
        // be (backlog), with Skip it pends until the next multiple.
        use std::future::Future;
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(&waker);
        let mut next = Box::pin(ticker.tick());
        assert!(
            matches!(next.as_mut().poll(&mut cx), std::task::Poll::Pending),
            "Skip must discard slept-through ticks, not queue them"
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistOutcome {
    Persisted,
    /// Retryable (SQLITE_BUSY/IO) failure: the event waits in the queue.
    Queued,
    /// Poison (constraint/drift) failure: counted, never looped.
    Poisoned,
}

/// D-04: one event through the full write path. Retryable failures are
/// queued for the 500 ms timer; poison is logged + counted immediately.
fn handle_event(
    store: &Store,
    grouper: &mut SegmentGrouper,
    queue: &mut RetryQueue,
    counters: &WriterCounters,
    event: CaptureEvent,
) -> PersistOutcome {
    counters.events.fetch_add(1, Ordering::Relaxed);
    if let Err(err) = store.insert_event(&event) {
        if is_retryable_store_error(&err) {
            queue.push(event);
            counters.sync_queue(queue);
            return PersistOutcome::Queued;
        }
        error!(?err, "writer: insert_event failed");
        counters
            .write_errors
            .fetch_add(1, Ordering::Relaxed);
        // Fall through: the segment fold and screenshot rows are still worth
        // persisting; the event-row failure is counted, never looped.
        return persist_rest(store, grouper, counters, &event, PersistOutcome::Poisoned);
    }
    persist_rest(store, grouper, counters, &event, PersistOutcome::Persisted)
}

fn persist_rest(
    store: &Store,
    grouper: &mut SegmentGrouper,
    counters: &WriterCounters,
    event: &CaptureEvent,
    outcome: PersistOutcome,
) -> PersistOutcome {
    if let Some(segment) = grouper.on_event(event) {
        match store.insert_segment(&segment) {
            Ok(()) => {
                counters.segments.fetch_add(1, Ordering::Relaxed);
            }
            Err(err) => {
                error!(?err, "writer: insert_segment failed");
                counters
                    .write_errors
                    .fetch_add(1, Ordering::Relaxed);
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
            counters
                .write_errors
                .fetch_add(1, Ordering::Relaxed);
        }
    }
    outcome
}

/// D-04 drain pass: every queued event is attempted at most once.
/// Retryable failures stay queued, poison is counted, successes leave.
/// On shutdown (`drop_remainder`) the undrained remainder counts as
/// dropped per D-05. Returns `(redelivered, dropped)`.
fn drain_retry_queue(
    store: &Store,
    grouper: &mut SegmentGrouper,
    queue: &mut RetryQueue,
    counters: &WriterCounters,
    deadline: Duration,
    drop_remainder: bool,
) -> (usize, u64) {
    let start = Instant::now();
    let mut redelivered = 0;
    let mut attempts = queue.len();
    while attempts > 0 && start.elapsed() < deadline {
        let Some(event) = queue.pop_front() else {
            break;
        };
        attempts -= 1;
        match handle_event(store, grouper, queue, counters, event) {
            PersistOutcome::Queued => {}
            PersistOutcome::Persisted | PersistOutcome::Poisoned => redelivered += 1,
        }
        counters.sync_queue(queue);
    }
    let mut dropped = 0;
    if drop_remainder {
        dropped = queue.len() as u64;
        queue.clear();
        if dropped > 0 {
            counters.dropped.fetch_add(dropped, Ordering::Relaxed);
        }
        counters.sync_queue(queue);
    }
    (redelivered, dropped)
}

#[cfg(test)]
mod writer_tests {
    use super::*;
    use rustwatch_core::CaptureEventKind;

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_store() -> Store {
        let dir = std::env::temp_dir().join(format!(
            "rustwatch-writer-test-{}-{}",
            std::process::id(),
            TEST_DIR_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("test.db")).unwrap()
    }

    fn text_event(text: &str) -> CaptureEvent {
        CaptureEvent::new(
            CaptureEventKind::TextDelta {
                text: text.to_string(),
            },
            None,
        )
    }

    /// D-05: a zero deadline drains nothing and counts the whole synthetic
    /// burst as dropped — no sleeping hardware, fully deterministic.
    #[test]
    fn zero_deadline_shutdown_drain_drops_remainder() {
        let store = test_store();
        let mut grouper = SegmentGrouper::new();
        let mut queue = RetryQueue::new();
        let counters = WriterCounters::new();
        for _ in 0..25 {
            queue.push(text_event("burst"));
        }
        let (redelivered, dropped) =
            drain_retry_queue(&store, &mut grouper, &mut queue, &counters, Duration::ZERO, true);
        assert_eq!(redelivered, 0);
        assert_eq!(dropped, 25);
        assert!(queue.is_empty());
        assert_eq!(counters.dropped.load(Ordering::Relaxed), 25);
        // Nothing reached the database.
        assert_eq!(store.stats().unwrap().0, 0);
    }

    /// The periodic (non-shutdown) drain replays queued events through the
    /// real store and leaves nothing behind.
    #[test]
    fn periodic_drain_redelivers_through_real_store() {
        let store = test_store();
        let mut grouper = SegmentGrouper::new();
        let mut queue = RetryQueue::new();
        let counters = WriterCounters::new();
        for i in 0..5 {
            queue.push(text_event(&format!("event-{i}")));
        }
        counters.sync_queue(&mut queue);
        assert_eq!(counters.queued.load(Ordering::Relaxed), 5);
        let (redelivered, dropped) = drain_retry_queue(
            &store,
            &mut grouper,
            &mut queue,
            &counters,
            Duration::from_secs(5),
            false,
        );
        assert_eq!(redelivered, 5);
        assert_eq!(dropped, 0);
        assert!(queue.is_empty());
        assert_eq!(counters.queued.load(Ordering::Relaxed), 0);
        assert_eq!(store.stats().unwrap().0, 5);
    }

    /// Poison (duplicate id → constraint violation) is counted once and
    /// never requeued — the retry loop cannot spin on it.
    #[test]
    fn fatal_errors_never_requeued() {
        let store = test_store();
        let mut grouper = SegmentGrouper::new();
        let mut queue = RetryQueue::new();
        let counters = WriterCounters::new();
        let first = text_event("once");
        let duplicate = CaptureEvent {
            id: first.id.clone(),
            timestamp: first.timestamp,
            app: None,
            kind: first.kind.clone(),
        };
        assert_eq!(
            handle_event(&store, &mut grouper, &mut queue, &counters, first),
            PersistOutcome::Persisted
        );
        assert_eq!(
            handle_event(&store, &mut grouper, &mut queue, &counters, duplicate),
            PersistOutcome::Poisoned
        );
        assert!(queue.is_empty());
        assert_eq!(counters.write_errors.load(Ordering::Relaxed), 1);
    }

    /// Draining an empty queue is a no-op: no redeliveries, no drops.
    #[test]
    fn drain_on_empty_is_noop() {
        let store = test_store();
        let mut grouper = SegmentGrouper::new();
        let mut queue = RetryQueue::new();
        let counters = WriterCounters::new();
        let (redelivered, dropped) = drain_retry_queue(
            &store,
            &mut grouper,
            &mut queue,
            &counters,
            Duration::from_secs(1),
            true,
        );
        assert_eq!((redelivered, dropped), (0, 0));
    }
}
