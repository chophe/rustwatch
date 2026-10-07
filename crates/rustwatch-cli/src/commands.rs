use std::{fs, process::{Command, Stdio}};

use anyhow::Context;
use chrono::{DateTime, NaiveDate, Utc};
use comfy_table::{Cell, Table};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use rustwatch_analyze::{analyze_pending, render_chart, ChartFormat, Redactor};
use rustwatch_capture::CaptureHandle;
use rustwatch_core::{
    DaemonClient, DaemonCommand, DaemonReply, DaemonState, DataPaths, Grant, Note,
    PermissionState, PermissionsState, Store, Config,
};
use rustwatch_core::ScreenshotScope;
use rustwatch_memory::MemoryEngine;

/// Render the launchd plist template: daemon path plus the T-03-01 log
/// directory. Pure so the KeepAlive dict and log paths are unit-tested
/// without touching ~/Library.
pub fn render_plist(daemon_path: &str, logs_dir: &str) -> String {
    include_str!("../../../deploy/macos/com.rustwatch.plist")
        .replace("{{RUSTWATCHD_PATH}}", daemon_path)
        .replace("{{LOGS_DIR}}", logs_dir)
}

pub fn install(_paths: &DataPaths) -> anyhow::Result<()> {
    let home = std::env::var("HOME")?;
    let plist_dir = format!("{home}/Library/LaunchAgents");
    fs::create_dir_all(&plist_dir)?;
    let plist_path = format!("{plist_dir}/com.rustwatch.plist");
    let exe = std::env::current_exe()?
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("rustwatchd"))
        .context("locate rustwatchd binary")?;

    // T-03-01: launchd logs live under ~/Library/Logs, never /tmp.
    let logs_dir = format!("{home}/Library/Logs/rustwatch");
    fs::create_dir_all(&logs_dir)?;
    let plist = render_plist(&exe.display().to_string(), &logs_dir);
    fs::write(&plist_path, plist)?;
    println!("{}", style("Installed launch agent").green());
    println!("  {plist_path}");
    println!("Run: launchctl load -w {plist_path}");
    Ok(())
}

pub async fn start(paths: &DataPaths) -> anyhow::Result<()> {
    // A answering socket is the liveness proof — cheaper and racier-free
    // than trusting the pid file from here.
    if paths.socket.exists() {
        let client = DaemonClient::new(&paths.socket);
        if let Ok(DaemonReply::Ok) =
            tokio::time::timeout(std::time::Duration::from_secs(2), client.send(DaemonCommand::Ping))
                .await
                .unwrap_or(Err(rustwatch_core::Error::DaemonNotRunning))
        {
            println!("{}", "Daemon already appears to be running".yellow());
            return Ok(());
        }
    }

    let exe = std::env::current_exe()?
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("rustwatchd"))
        .context("locate rustwatchd binary")?;

    // Daemon stderr goes to a log file so a failed start can report its tail.
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.root.join("rustwatchd.log"))
        .context("open rustwatchd.log")?;
    Command::new(exe)
        .stdout(Stdio::null())
        .stderr(log_file)
        .spawn()
        .context("spawn rustwatchd")?;

    // Poll socket Ping with a deadline instead of a fixed sleep (Pitfall 4).
    let client = DaemonClient::new(&paths.socket);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Ok(DaemonReply::Ok) =
            tokio::time::timeout(std::time::Duration::from_secs(1), client.send(DaemonCommand::Ping))
                .await
                .unwrap_or(Err(rustwatch_core::Error::DaemonNotRunning))
        {
            println!("{}", style("rustwatchd started").green());
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            println!("{}", style("rustwatchd failed to start — log tail:").red());
            print_log_tail(&paths.root.join("rustwatchd.log"), 20);
            anyhow::bail!("rustwatchd did not answer Ping within 5 s");
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

fn print_log_tail(path: &std::path::Path, lines: usize) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        println!("  (no log file at {})", path.display());
        return;
    };
    let all: Vec<&str> = raw.lines().collect();
    let start = all.len().saturating_sub(lines);
    for line in &all[start..] {
        println!("  {line}");
    }
}

/// The short process name for a pid, via `ps`. `None` when it cannot be
/// determined — callers treat that as unverified and refuse to signal.
fn process_comm(pid: i32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .next()
        .map(str::to_string)
}

pub fn stop(paths: &DataPaths) -> anyhow::Result<()> {
    if !paths.pid_file.exists() {
        println!("{}", "Daemon is not running".yellow());
        return Ok(());
    }
    let pid_raw = fs::read_to_string(&paths.pid_file)?;
    let pid: i32 = pid_raw.trim().parse().context("parse pid")?;

    // T-01-02: never SIGTERM a pid we have not proven is ours. A stale pid
    // file plus OS pid recycling otherwise kills an unrelated process.
    // Probe 1: liveness.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    if !alive {
        println!("Stale pid file (no process {pid}); removing it");
        fs::remove_file(&paths.pid_file)?;
        if paths.socket.exists() {
            fs::remove_file(&paths.socket)?;
        }
        return Ok(());
    }
    // Probe 2: process-name verify.
    match process_comm(pid).as_deref() {
        Some("rustwatchd") => {}
        Some(other) => anyhow::bail!(
            "pid {pid} is '{other}', not rustwatchd — refusing to signal. \
             Remove {} manually if it is stale.",
            paths.pid_file.display()
        ),
        None => anyhow::bail!(
            "could not verify the process name for pid {pid} — refusing to signal. \
             Remove {} manually if it is stale.",
            paths.pid_file.display()
        ),
    }

    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    // Poll for exit with a timeout before removing files: removing the
    // socket while the daemon still drains would orphan its flush.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 {
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("daemon (pid {pid}) did not exit within 5 s; socket/pid files left in place");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = fs::remove_file(&paths.pid_file);
    let _ = fs::remove_file(&paths.socket);
    println!("{}", style("rustwatchd stopped").green());
    Ok(())
}

pub async fn status(paths: &DataPaths, config: &Config) -> anyhow::Result<()> {
    let store = Store::open(&paths.sqlite)?;
    let (events, segments, activities, screenshots) = store.stats()?;

    let mut table = Table::new();
    table.set_header(vec!["Metric", "Value"]);
    table.add_row(vec![Cell::new("Data dir"), Cell::new(paths.root.display().to_string())]);
    table.add_row(vec![Cell::new("Events"), Cell::new(events.to_string())]);
    table.add_row(vec![Cell::new("Segments"), Cell::new(segments.to_string())]);
    table.add_row(vec![Cell::new("Activities"), Cell::new(activities.to_string())]);
    table.add_row(vec![Cell::new("Screenshots"), Cell::new(screenshots.to_string())]);
    table.add_row(vec![
        Cell::new("LLM provider"),
        Cell::new(config.analyze.provider.clone()),
    ]);
    println!("{table}");

    if paths.socket.exists() {
        let client = DaemonClient::new(&paths.socket);
        if let Ok(DaemonReply::Status { state }) = client.send(DaemonCommand::Status).await {
            println!(
                "Daemon: running={} paused={} captured={} segments={}",
                state.running, state.paused, state.events_captured, state.segments_written
            );
            // D-07 health banner: quiet when healthy, loud with nonzero
            // detail counters when degraded.
            let banner = state.health_banner();
            if state.capture_health == "healthy" {
                println!("{}", banner.green());
            } else {
                println!("{}", banner.yellow());
                if state.write_errors > 0 {
                    println!("  write errors: {}", state.write_errors);
                }
                if state.dropped_events > 0 {
                    println!("  dropped events: {}", state.dropped_events);
                }
                if state.queued > 0 {
                    println!("  queued for retry: {}", state.queued);
                }
            }
            // D-16 permission line: partial capture never looks like full
            // capture. Detail (Settings fix per grant) lives in `doctor`.
            let perm_banner = state.permissions.banner();
            if state.permissions.all_granted() {
                println!("{}", perm_banner.green());
            } else {
                println!("{}", perm_banner.yellow());
            }
            // D-14: a degraded path names itself here (restart-required,
            // parked keyboard) — state, never a lone log warning.
            if !state.capture_note.is_empty() {
                println!("  note: {}", state.capture_note.yellow());
            }
        }
    } else {
        println!("{}", "Daemon: not running".yellow());
    }
    Ok(())
}

/// D-08 `doctor`: the single trusted install check. FAILS (exit 1) only on
/// live breakage — launchd missing, daemon down/wedged, DB not writable.
/// Everything else (permission gaps, nonzero history counters, ownership
/// notes, missing API key) WARNS and exits 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorStatus {
    Pass,
    Warn,
    Fail,
}

pub struct DoctorCheck {
    pub name: &'static str,
    pub status: DoctorStatus,
    pub detail: String,
}

/// D-08 exit semantics over injected checks — unit-tested without a daemon.
pub fn doctor_exit_code(checks: &[DoctorCheck]) -> i32 {
    if checks.iter().any(|c| c.status == DoctorStatus::Fail) {
        1
    } else {
        0
    }
}

/// Best-effort `launchctl list` probe: true when the label is loaded.
/// A missing launchctl binary (non-macOS) reads as not-loaded, never panics.
fn launchd_loaded() -> bool {
    Command::new("launchctl")
        .args(["list", "com.rustwatch"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Best-effort disk-avail note via `df -k`. Informational only (retention
/// caps are Phase 4) — `None` when df is missing or unparsable.
fn disk_avail_note(dir: &std::path::Path) -> Option<String> {
    let out = Command::new("df").args(["-k"]).arg(dir).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let row = text.lines().nth(1)?;
    let avail: u64 = row.split_whitespace().nth(3)?.parse().ok()?;
    Some(format!("disk: ~{} MiB available", avail / 1024))
}

/// Per-grant fix lines (D-16): exact Settings path as a runnable `open`
/// command, plus the attach note (restart vs live). Pure over injected
/// state so the wording is unit-tested.
pub fn permission_fix_lines(perms: &PermissionsState) -> Vec<String> {
    let mut lines = Vec::new();
    for grant in Grant::all() {
        if grant.state(perms) == PermissionState::Granted {
            continue;
        }
        let attach = if grant.restart_required_on_attach() {
            "restart rustwatchd after granting (macOS requires it for this grant)"
        } else {
            "re-grant attaches live within ~30 s, no restart needed"
        };
        lines.push(format!(
            "{}: {} — fix: open '{}' ({attach})",
            grant.name(),
            grant.state(perms).label(),
            grant.settings_url(),
        ));
    }
    lines
}

pub async fn doctor(paths: &DataPaths) -> anyhow::Result<()> {
    let mut checks: Vec<DoctorCheck> = Vec::new();

    // 1. launchd plist installed + loaded.
    let home = std::env::var("HOME").unwrap_or_default();
    let plist_path = format!("{home}/Library/LaunchAgents/com.rustwatch.plist");
    let plist_exists = std::path::Path::new(&plist_path).exists();
    let loaded = launchd_loaded();
    checks.push(DoctorCheck {
        name: "launchd plist",
        status: if plist_exists && loaded {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Fail
        },
        detail: if plist_exists && loaded {
            format!("installed + loaded ({plist_path})")
        } else if !plist_exists {
            format!("missing {plist_path} — run `rustwatch install` then `launchctl load -w {plist_path}`")
        } else {
            format!("installed but not loaded — run `launchctl load -w {plist_path}`")
        },
    });

    // 2. Daemon socket Ping within timeout: distinguishes not-running
    // (no socket) from wedged (socket, no reply).
    let mut live_state: Option<DaemonState> = None;
    if !paths.socket.exists() {
        checks.push(DoctorCheck {
            name: "daemon socket",
            status: DoctorStatus::Fail,
            detail: "not running (no socket) — run `rustwatch start`".to_string(),
        });
    } else {
        let client = DaemonClient::new(&paths.socket);
        match tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.send(DaemonCommand::Status),
        )
        .await
        {
            Ok(Ok(DaemonReply::Status { state })) => {
                live_state = Some(state);
                checks.push(DoctorCheck {
                    name: "daemon socket",
                    status: DoctorStatus::Pass,
                    detail: "Ping answered within timeout".to_string(),
                });
            }
            _ => checks.push(DoctorCheck {
                name: "daemon socket",
                status: DoctorStatus::Fail,
                detail: "socket exists but no reply (wedged?) — `rustwatch stop`, then `start`"
                    .to_string(),
            }),
        }
    }

    // 3. DB writable: open plus a probe write inside a rolled-back
    // transaction (never leaves a row behind).
    match (|| -> anyhow::Result<()> {
        let store = Store::open(&paths.sqlite)?;
        store.connection().execute_batch(
            "BEGIN IMMEDIATE; CREATE TEMP TABLE doctor_probe(x INTEGER); \
             INSERT INTO doctor_probe VALUES (1); ROLLBACK;",
        )?;
        Ok(())
    })() {
        Ok(()) => checks.push(DoctorCheck {
            name: "database",
            status: DoctorStatus::Pass,
            detail: format!("writable ({})", paths.sqlite.display()),
        }),
        Err(err) => checks.push(DoctorCheck {
            name: "database",
            status: DoctorStatus::Fail,
            detail: format!("not writable: {err:#}"),
        }),
    }

    // 4. Socket/pid ownership note (T-03-04 adjacent: `stop`/`doctor` must
    // never trust a world-readable socket or a stale pid).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let socket_meta = std::fs::metadata(&paths.socket)
            .map(|m| m.permissions().mode() & 0o777)
            .ok();
        let pid_sane = std::fs::read_to_string(&paths.pid_file)
            .ok()
            .and_then(|raw| raw.trim().parse::<i32>().ok())
            .is_some();
        let (status, detail) = match socket_meta {
            None => (
                DoctorStatus::Warn,
                "socket absent (daemon not running) — re-check after `rustwatch start`".to_string(),
            ),
            Some(0o600) if pid_sane || !paths.pid_file.exists() => (
                DoctorStatus::Pass,
                "socket 0600, pid file sane".to_string(),
            ),
            Some(mode) => (
                DoctorStatus::Warn,
                format!(
                    "socket mode {mode:o} (want 600), pid file {} — restart the daemon to re-apply",
                    if paths.pid_file.exists() { "unparsed" } else { "absent" },
                ),
            ),
        };
        checks.push(DoctorCheck {
            name: "socket/pid ownership",
            status,
            detail,
        });
    }

    // 5. Per-permission state + fix. Live daemon state when reachable,
    // else a local check-only probe (report-only, never prompts).
    let perms = live_state
        .as_ref()
        .map(|s| s.permissions)
        .unwrap_or_else(|| {
            let report = CaptureHandle::permissions();
            PermissionsState {
                input_monitoring: report.input_monitoring,
                accessibility: report.accessibility,
                screen_recording: report.screen_recording,
            }
        });
    let fixes = permission_fix_lines(&perms);
    checks.push(DoctorCheck {
        name: "permissions",
        status: if fixes.is_empty() {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Warn
        },
        detail: if fixes.is_empty() {
            "all granted".to_string()
        } else {
            fixes.join("\n    ")
        },
    });

    // 6. Screenshots dir writable + disk-space note (retention is Phase 4).
    let shots_writable = (|| -> bool {
        if std::fs::create_dir_all(&paths.screenshots).is_err() {
            return false;
        }
        let probe = paths.screenshots.join(".doctor-probe");
        match std::fs::write(&probe, b"ok") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(_) => false,
        }
    })();
    let disk = disk_avail_note(&paths.screenshots).unwrap_or_else(|| {
        "disk-space check unavailable (retention caps land in Phase 4)".to_string()
    });
    checks.push(DoctorCheck {
        name: "screenshots dir",
        status: if shots_writable {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Warn
        },
        detail: format!(
            "{}{}; {disk}",
            paths.screenshots.display(),
            if shots_writable { " writable" } else { " NOT writable" },
        ),
    });

    // 7. History counters WARN, never fail (D-08).
    if let Some(state) = &live_state {
        let noisy = state.write_errors > 0 || state.dropped_events > 0;
        checks.push(DoctorCheck {
            name: "loss history",
            status: if noisy {
                DoctorStatus::Warn
            } else {
                DoctorStatus::Pass
            },
            detail: if noisy {
                format!(
                    "{} write errors, {} dropped (history — investigate, not fatal)",
                    state.write_errors, state.dropped_events,
                )
            } else {
                "no write errors, no drops".to_string()
            },
        });
        if !state.capture_note.is_empty() {
            checks.push(DoctorCheck {
                name: "capture note",
                status: DoctorStatus::Warn,
                detail: state.capture_note.clone(),
            });
        }
    }

    // 8. launchd gets no EnvironmentVariables from this plist: warn when
    // the key is absent so cloud-classification silence is not a mystery.
    let has_key = std::env::var("OPENAI_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if !has_key {
        checks.push(DoctorCheck {
            name: "OPENAI_API_KEY",
            status: DoctorStatus::Warn,
            detail: "absent (and launchd provides no EnvironmentVariables): cloud classification disabled; local capture unaffected".to_string(),
        });
    }

    // Report.
    println!("{}", style("rustwatch doctor").bold());
    for check in &checks {
        let (flag, colored) = match check.status {
            DoctorStatus::Pass => ("[pass]", check.detail.green().to_string()),
            DoctorStatus::Warn => ("[warn]", check.detail.yellow().to_string()),
            DoctorStatus::Fail => ("[fail]", check.detail.red().to_string()),
        };
        println!("  {flag} {}: {colored}", check.name);
    }
    let code = doctor_exit_code(&checks);
    if code == 0 {
        println!("{}", style("doctor: healthy").green());
    } else {
        println!("{}", style("doctor: live breakage found (exit 1)").red());
    }
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

pub fn permissions() -> anyhow::Result<()> {
    let report = CaptureHandle::permissions();
    println!("{}", style("macOS permissions checklist").bold());
    println!("  Input Monitoring:  {}", perm_flag(report.input_monitoring));
    println!("  Accessibility:     {}", perm_flag(report.accessibility));
    println!("  Screen Recording:  {}", perm_flag(report.screen_recording));
    for note in report.notes {
        println!("  - {note}");
    }
    Ok(())
}

/// D-16 three-state rendering: denied and never-prompted are different facts
/// with different fixes (`doctor` prints the per-grant fix).
fn perm_flag(state: rustwatch_core::PermissionState) -> String {
    use rustwatch_core::PermissionState;
    match state {
        PermissionState::Granted => "granted".green().to_string(),
        PermissionState::Denied => "denied".red().to_string(),
        PermissionState::Undetermined => "undetermined".yellow().to_string(),
    }
}

pub async fn tail(paths: &DataPaths, limit: usize) -> anyhow::Result<()> {
    let client = DaemonClient::new(&paths.socket);
    match client.send(DaemonCommand::Tail { limit }).await? {
        DaemonReply::Events { events } => {
            for event in events {
                println!(
                    "{} {:?} {:?}",
                    event.timestamp.to_rfc3339(),
                    event.app.as_ref().map(|a| &a.app_name),
                    event.kind
                );
            }
        }
        DaemonReply::Error { message } => anyhow::bail!(message),
        other => anyhow::bail!("unexpected reply: {:?}", other),
    }
    Ok(())
}

pub async fn screenshot(paths: &DataPaths, window: bool, screen: bool) -> anyhow::Result<()> {
    if paths.socket.exists() {
        let client = DaemonClient::new(&paths.socket);
        let reply = client
            .send(DaemonCommand::Screenshot { window: window || !screen })
            .await?;
        if let DaemonReply::Screenshot { path } = reply {
            println!("Saved screenshot: {path}");
            return Ok(());
        }
    }

    let scope = if screen {
        ScreenshotScope::Screen
    } else {
        ScreenshotScope::Window
    };
    let handle = CaptureHandle::new(Vec::new())?;
    let (path, _) = handle.capture_screenshot(scope, paths.screenshots.clone())?;
    println!("Saved screenshot: {}", path.display());
    Ok(())
}

/// D-10/D-11 annotate: the daemon never prompts (headless under launchd),
/// so the CLI prompts for the most recent screenshot lacking a note.
/// Enter saves (redacted), Esc discards.
pub fn annotate(paths: &DataPaths, config: &Config) -> anyhow::Result<()> {
    let store = Store::open(&paths.sqlite)?;
    let Some(shot) = store.latest_screenshot_without_note()? else {
        println!("No unannotated screenshots — nothing to annotate.");
        return Ok(());
    };
    println!("Screenshot: {} ({})", shot.path.display(), shot.captured_at.format("%H:%M"));
    println!("Type a note, Enter saves, Esc discards.");
    match prompt_note()? {
        Some(text) => {
            let redactor = Redactor::new(config)?;
            store.insert_note(&Note {
                id: uuid::Uuid::new_v4().to_string(),
                ts: chrono::Utc::now(),
                note: redactor.scrub(&text),
                screenshot_id: Some(shot.id),
            })?;
            println!("Note saved.");
        }
        None => println!("Discarded — no note saved."),
    }
    Ok(())
}

/// Minimal line editor for the annotate prompt: printable chars append,
/// Backspace deletes, Enter saves, Esc discards. Pure — unit-tested
/// without a terminal.
#[derive(Debug, Default)]
struct NoteEditor {
    buf: String,
}

enum NoteOutcome {
    Pending,
    Save(String),
    Discard,
}

impl NoteEditor {
    fn key(&mut self, code: crossterm::event::KeyCode) -> NoteOutcome {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Enter => NoteOutcome::Save(std::mem::take(&mut self.buf)),
            KeyCode::Esc => NoteOutcome::Discard,
            KeyCode::Backspace => {
                self.buf.pop();
                NoteOutcome::Pending
            }
            KeyCode::Char(c) => {
                self.buf.push(c);
                NoteOutcome::Pending
            }
            _ => NoteOutcome::Pending,
        }
    }
}

/// Raw-mode prompt driver: echoes input with basic Backspace support.
/// Restores the terminal on every exit path.
fn prompt_note() -> anyhow::Result<Option<String>> {
    use crossterm::event::{read, Event};
    use std::io::Write;

    crossterm::terminal::enable_raw_mode()?;
    let result = (|| -> anyhow::Result<Option<String>> {
        let mut editor = NoteEditor::default();
        let mut stdout = std::io::stdout();
        write!(stdout, "> ")?;
        stdout.flush()?;
        loop {
            let Event::Key(key) = read()? else {
                continue;
            };
            match editor.key(key.code) {
                NoteOutcome::Pending => {
                    // Re-render the line: CR, clear, prompt, buffer.
                    write!(stdout, "\r\x1b[2K> {}", editor.buf)?;
                    stdout.flush()?;
                }
                NoteOutcome::Save(note) => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok(Some(note));
                }
                NoteOutcome::Discard => {
                    write!(stdout, "\r\n")?;
                    stdout.flush()?;
                    return Ok(None);
                }
            }
        }
    })();
    crossterm::terminal::disable_raw_mode()?;
    result
}

pub fn export(
    paths: &DataPaths,
    from: Option<String>,
    to: Option<String>,
) -> anyhow::Result<()> {
    let store = Store::open(&paths.sqlite)?;
    let from_ts = parse_opt_ts(from)?.unwrap_or_else(|| Utc::now() - chrono::Duration::days(1));
    let to_ts = parse_opt_ts(to)?.unwrap_or_else(Utc::now);
    let segments = store.list_segments_between(from_ts, to_ts)?;
    let out_path = paths.root.join(format!(
        "export-{}.json",
        Utc::now().format("%Y%m%d-%H%M%S")
    ));
    fs::write(&out_path, serde_json::to_string_pretty(&segments)?)?;
    println!("Exported {} segments to {}", segments.len(), out_path.display());
    Ok(())
}

pub async fn analyze(paths: &DataPaths, config: &Config) -> anyhow::Result<()> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message("Analyzing activities via LLM...");
    let store = Store::open(&paths.sqlite)?;
    let records = analyze_pending(&store, config).await?;
    pb.finish_with_message(format!("Created {} activities", records.len()));

    if !records.is_empty() {
        let memory = MemoryEngine::open(paths, config).await?;
        memory.ingest_activities(&records).await?;
    }
    Ok(())
}

pub fn chart(paths: &DataPaths, date: Option<String>, format: String) -> anyhow::Result<()> {
    let store = Store::open(&paths.sqlite)?;
    let date = match date {
        Some(value) => NaiveDate::parse_from_str(&value, "%Y-%m-%d")?,
        None => Utc::now().date_naive(),
    };
    let chart_format = match format.as_str() {
        "json" => ChartFormat::Json,
        "html" => ChartFormat::Html,
        _ => ChartFormat::Terminal,
    };
    let rendered = render_chart(&store, date, chart_format)?;
    if matches!(chart_format, ChartFormat::Html) {
        let out = paths.root.join(format!("chart-{}.html", date));
        fs::write(&out, &rendered)?;
        println!("Wrote {}", out.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}

pub async fn memory_search(
    paths: &DataPaths,
    config: &Config,
    query: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let memory = MemoryEngine::open(paths, config).await?;
    let hits = memory.search(query, limit).await?;
    let mut table = Table::new();
    table.set_header(vec!["Score", "App", "Text"]);
    for hit in hits {
        let snippet = snippet_for(&hit.text);
        table.add_row(vec![
            Cell::new(format!("{:.3}", hit.score)),
            Cell::new(hit.app_name),
            Cell::new(snippet),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub async fn memory_ingest(
    paths: &DataPaths,
    config: &Config,
    rebuild: bool,
) -> anyhow::Result<()> {
    let store = Store::open(&paths.sqlite)?;
    let memory = MemoryEngine::open(paths, config).await?;
    let pb = ProgressBar::new_spinner();
    pb.set_message(if rebuild {
        "Rebuilding memory index..."
    } else {
        "Ingesting new memory chunks..."
    });
    let count = if rebuild {
        memory.rebuild_from_store(&store).await?
    } else {
        let from = Utc::now() - chrono::Duration::days(7);
        let segments = store.list_segments_between(from, Utc::now())?;
        memory.ingest_segments(&segments).await?
    };
    pb.finish_with_message(format!("Ingested {count} chunks"));
    Ok(())
}

pub async fn memory_graph(
    paths: &DataPaths,
    config: &Config,
    around: &str,
) -> anyhow::Result<()> {
    memory_search(paths, config, around, 10).await
}

fn parse_opt_ts(raw: Option<String>) -> anyhow::Result<Option<DateTime<Utc>>> {
    Ok(match raw {
        Some(value) => Some(DateTime::parse_from_rfc3339(&value)?.with_timezone(&Utc)),
        None => None,
    })
}

/// CAPT-05: char-boundary truncation for search snippets — byte-slicing
/// panics on CJK/emoji (same idiom as segment.rs).
fn snippet_for(text: &str) -> String {
    let snippet: String = text.chars().take(120).collect();
    if text.chars().count() > 120 {
        format!("{snippet}...")
    } else {
        snippet
    }
}

#[cfg(test)]
mod commands_tests {
    use super::{snippet_for, NoteEditor, NoteOutcome};
    use crossterm::event::KeyCode;
    /// CAPT-05: emoji/CJK floods never panic the snippet path.
    #[test]
    fn emoji_flood_snippet_is_char_safe() {
        let flood = "🎉".repeat(500);
        let snippet = snippet_for(&flood);
        assert_eq!(snippet.chars().count(), 123); // 120 + "..."
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn short_cjk_text_passes_through() {
        assert_eq!(snippet_for("日本語テスト"), "日本語テスト");
    }

    /// D-16: denied and never-prompted render as different facts.
    #[test]
    fn perm_flag_distinguishes_three_states() {
        use rustwatch_core::PermissionState;
        assert!(super::perm_flag(PermissionState::Granted).contains("granted"));
        assert!(super::perm_flag(PermissionState::Denied).contains("denied"));
        assert!(super::perm_flag(PermissionState::Undetermined).contains("undetermined"));
    }

    /// D-11: typing + Enter saves the typed text.
    #[test]
    fn annotate_enter_saves_typed_text() {
        let mut editor = NoteEditor::default();
        for c in "standup notes".chars() {
            assert!(matches!(editor.key(KeyCode::Char(c)), NoteOutcome::Pending));
        }
        match editor.key(KeyCode::Enter) {
            NoteOutcome::Save(note) => assert_eq!(note, "standup notes"),
            _ => panic!("Enter must save"),
        }
    }

    /// D-11: Esc discards, even with typed text.
    #[test]
    fn annotate_esc_discards() {
        let mut editor = NoteEditor::default();
        for c in "never mind".chars() {
            assert!(matches!(editor.key(KeyCode::Char(c)), NoteOutcome::Pending));
        }
        assert!(matches!(editor.key(KeyCode::Esc), NoteOutcome::Discard));
    }

    /// Backspace edits; other keys (arrows, F-keys) are ignored, never saved.
    #[test]
    fn annotate_backspace_edits_and_arrows_ignore() {
        let mut editor = NoteEditor::default();
        for c in "abc".chars() {
            editor.key(KeyCode::Char(c));
        }
        assert!(matches!(editor.key(KeyCode::Backspace), NoteOutcome::Pending));
        assert!(matches!(editor.key(KeyCode::Left), NoteOutcome::Pending));
        match editor.key(KeyCode::Enter) {
            NoteOutcome::Save(note) => assert_eq!(note, "ab"),
            _ => panic!("Enter must save"),
        }
    }

    /// D-08 exit-code matrix: live breakage fails, history-only warns.
    #[test]
    fn doctor_exit_code_matrix() {
        use super::{DoctorCheck, DoctorStatus};
        let pass = |name: &'static str| DoctorCheck {
            name,
            status: DoctorStatus::Pass,
            detail: String::new(),
        };
        // Healthy install exits 0.
        assert_eq!(super::doctor_exit_code(&vec![pass("a"), pass("b")]), 0);
        // History-only warnings (loss counters, missing key) still exit 0.
        assert_eq!(
            super::doctor_exit_code(&vec![
                pass("daemon socket"),
                DoctorCheck {
                    name: "loss history",
                    status: DoctorStatus::Warn,
                    detail: "3 write errors".into(),
                },
            ]),
            0
        );
        // Daemon down fails.
        assert_eq!(
            super::doctor_exit_code(&vec![
                pass("launchd plist"),
                DoctorCheck {
                    name: "daemon socket",
                    status: DoctorStatus::Fail,
                    detail: "not running".into(),
                },
            ]),
            1
        );
        // DB unwritable fails even when the daemon answers.
        assert_eq!(
            super::doctor_exit_code(&vec![
                pass("daemon socket"),
                DoctorCheck {
                    name: "database",
                    status: DoctorStatus::Fail,
                    detail: "not writable".into(),
                },
            ]),
            1
        );
    }

    /// Every ungranted grant gets a runnable Settings fix line; granted
    /// grants stay silent.
    #[test]
    fn permission_fix_lines_name_each_gap() {
        use rustwatch_core::{PermissionsState, PermissionState};
        let all = PermissionsState {
            input_monitoring: PermissionState::Granted,
            accessibility: PermissionState::Granted,
            screen_recording: PermissionState::Granted,
        };
        assert!(super::permission_fix_lines(&all).is_empty());

        let denied_screen = PermissionsState {
            screen_recording: PermissionState::Denied,
            ..all
        };
        let lines = super::permission_fix_lines(&denied_screen);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("screen recording"));
        assert!(lines[0].contains("open 'x-apple.systempreferences:"));
        assert!(lines[0].contains("restart rustwatchd"));
        // No restart demand for the live-attachable grants.
        let denied_input = PermissionsState {
            input_monitoring: PermissionState::Denied,
            ..all
        };
        let lines = super::permission_fix_lines(&denied_input);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("~30 s, no restart needed"));
    }

    /// The plist renders with substituted paths and the D-06 KeepAlive dict —
    /// no leftover placeholders, no /tmp logs.
    #[test]
    fn plist_template_renders_hardened() {
        let rendered = super::render_plist("/opt/rw/rustwatchd", "/Users/x/Logs/rustwatch");
        assert!(rendered.contains("/opt/rw/rustwatchd"));
        assert!(rendered.contains("/Users/x/Logs/rustwatch/rustwatchd.out.log"));
        assert!(rendered.contains("/Users/x/Logs/rustwatch/rustwatchd.err.log"));
        assert!(rendered.contains("<key>SuccessfulExit</key>"));
        assert!(rendered.contains("<key>ThrottleInterval</key>"));
        assert!(!rendered.contains("{{"), "unsubstituted placeholder remains");
        assert!(!rendered.contains("/tmp/"), "world-writable log path remains");
    }
}
