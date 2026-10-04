use std::{fs, process::{Command, Stdio}};

use anyhow::Context;
use chrono::{DateTime, NaiveDate, Utc};
use comfy_table::{Cell, Table};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use rustwatch_analyze::{analyze_pending, render_chart, ChartFormat};
use rustwatch_capture::CaptureHandle;
use rustwatch_core::{
    DaemonClient, DaemonCommand, DaemonReply, DataPaths, Store,
    Config,
};
use rustwatch_core::ScreenshotScope;
use rustwatch_memory::MemoryEngine;

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

    let plist = include_str!("../../../deploy/macos/com.rustwatch.plist")
        .replace("{{RUSTWATCHD_PATH}}", &exe.display().to_string());
    fs::write(&plist_path, plist)?;
    println!("{}", style("Installed launch agent").green());
    println!("  {plist_path}");
    println!("Run: launchctl load -w {plist_path}");
    Ok(())
}

pub async fn start(paths: &DataPaths) -> anyhow::Result<()> {
    if paths.pid_file.exists() {
        println!("{}", "Daemon already appears to be running".yellow());
        return Ok(());
    }

    let exe = std::env::current_exe()?
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("rustwatchd"))
        .context("locate rustwatchd binary")?;

    Command::new(exe)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn rustwatchd")?;

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("{}", style("rustwatchd started").green());
    Ok(())
}

pub fn stop(paths: &DataPaths) -> anyhow::Result<()> {
    if !paths.pid_file.exists() {
        println!("{}", "Daemon is not running".yellow());
        return Ok(());
    }
    let pid_raw = fs::read_to_string(&paths.pid_file)?;
    let pid: i32 = pid_raw.trim().parse().context("parse pid")?;
    unsafe {
        libc::kill(pid, libc::SIGTERM);
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
        }
    } else {
        println!("{}", "Daemon: not running".yellow());
    }
    Ok(())
}

pub fn permissions() -> anyhow::Result<()> {
    let report = CaptureHandle::permissions();
    println!("{}", style("macOS permissions checklist").bold());
    println!("  Input Monitoring:  {}", flag(report.input_monitoring));
    println!("  Accessibility:     {}", flag(report.accessibility));
    println!("  Screen Recording:  {}", flag(report.screen_recording));
    for note in report.notes {
        println!("  - {note}");
    }
    Ok(())
}

fn flag(ok: bool) -> String {
    if ok {
        "granted".green().to_string()
    } else {
        "missing".red().to_string()
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
    let path = handle.capture_screenshot(scope, paths.screenshots.clone())?;
    println!("Saved screenshot: {}", path.display());
    Ok(())
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
        let snippet = if hit.text.len() > 120 {
            format!("{}...", &hit.text[..120])
        } else {
            hit.text.clone()
        };
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
