use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::DefaultTerminal;
use rustwatch_core::{
    DaemonClient, DaemonCommand, DaemonReply, DaemonState, DataPaths, Store, Config,
};

/// Banner parity with `status` (D-07/D-16): healthy/degraded plus permission
/// state, counters only when nonzero, capture note when set. Pure over
/// injected state so the wording is unit-tested without a terminal.
/// `None` distinguishes daemon-unreachable (socket exists, no reply) from
/// daemon-not-running (no socket).
pub fn tui_status_line(state: Option<&DaemonState>, socket_exists: bool) -> Line<'static> {
    let Some(state) = state else {
        if socket_exists {
            return Line::from(vec![Span::styled(
                "Daemon unreachable (socket exists, no reply)",
                Style::default().fg(Color::Red),
            )]);
        }
        return Line::from(vec![Span::styled(
            "Daemon not running",
            Style::default().fg(Color::Yellow),
        )]);
    };
    let mut spans = vec![Span::raw(format!(
        "Capturing | events={} segments={} | ",
        state.events_captured, state.segments_written
    ))];
    let (health_color, health_text) = if state.capture_health == "healthy" {
        (Color::Green, state.health_banner())
    } else {
        // Detail counters only when nonzero — same rule as `status`.
        let mut parts = Vec::new();
        if state.write_errors > 0 {
            parts.push(format!("{} write errors", state.write_errors));
        }
        if state.dropped_events > 0 {
            parts.push(format!("{} dropped", state.dropped_events));
        }
        if state.queued > 0 {
            parts.push(format!("{} queued", state.queued));
        }
        let text = if parts.is_empty() {
            "capture: degraded".to_string()
        } else {
            format!("capture: degraded ({})", parts.join(", "))
        };
        (Color::Yellow, text)
    };
    spans.push(Span::styled(health_text, Style::default().fg(health_color)));
    spans.push(Span::raw(" | "));
    let perm_color = if state.permissions.all_granted() {
        Color::Green
    } else {
        Color::Yellow
    };
    spans.push(Span::styled(
        state.permissions.banner(),
        Style::default().fg(perm_color),
    ));
    if !state.capture_note.is_empty() {
        spans.push(Span::raw(" | "));
        spans.push(Span::styled(
            state.capture_note.clone(),
            Style::default().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

pub async fn run(paths: &DataPaths, _config: &Config) -> anyhow::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let terminal = ratatui::init();
    let result = run_loop(terminal, paths).await;
    ratatui::restore();
    crossterm::terminal::disable_raw_mode()?;
    result
}

async fn run_loop(mut terminal: DefaultTerminal, paths: &DataPaths) -> anyhow::Result<()> {
    let store = Store::open(&paths.sqlite)?;
    let mut paused = false;

    loop {
        let activities = store.list_activities_for_date(chrono::Utc::now().date_naive())?;
        let socket_exists = paths.socket.exists();
        let state = if socket_exists {
            let client = DaemonClient::new(&paths.socket);
            match client.send(DaemonCommand::Status).await {
                Ok(DaemonReply::Status { state }) => {
                    paused = state.paused;
                    Some(state)
                }
                _ => None,
            }
        } else {
            None
        };
        // `None` renders distinctly: socket-without-reply reads
        // "unreachable", no-socket reads "not running".
        let status = tui_status_line(state.as_ref(), socket_exists);

        let items: Vec<ListItem> = activities
            .iter()
            .map(|a| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", a.started_at.format("%H:%M")),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{} — {}", a.label, a.apps.join(", ")),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect();

        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(frame.area());

            frame.render_widget(
                Paragraph::new(status).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("rustwatch"),
                ),
                chunks[0],
            );

            frame.render_widget(
                List::new(items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Activity timeline"),
                ),
                chunks[1],
            );

            let help = if paused {
                "[r] refresh  [s] resume  [q] quit"
            } else {
                "[r] refresh  [p] pause  [q] quit"
            };
            frame.render_widget(
                Paragraph::new(help).block(Block::default().borders(Borders::ALL)),
                chunks[2],
            );
        })?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
                {
                    break;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => {}
                    KeyCode::Char('p') => {
                        if paths.socket.exists() {
                            let client = DaemonClient::new(&paths.socket);
                            let _ = client.send(DaemonCommand::Pause).await;
                        }
                    }
                    KeyCode::Char('s') => {
                        if paths.socket.exists() {
                            let client = DaemonClient::new(&paths.socket);
                            let _ = client.send(DaemonCommand::Resume).await;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tui_tests {
    use super::tui_status_line;
    use rustwatch_core::{DaemonState, PermissionsState, PermissionState};

    fn healthy_state() -> DaemonState {
        DaemonState {
            running: true,
            paused: false,
            pid: Some(1),
            events_captured: 10,
            segments_written: 2,
            started_at: None,
            write_errors: 0,
            dropped_events: 0,
            queued: 0,
            capture_health: "healthy".to_string(),
            permissions: PermissionsState {
                input_monitoring: PermissionState::Granted,
                accessibility: PermissionState::Granted,
                screen_recording: PermissionState::Granted,
            },
            capture_note: String::new(),
        }
    }

    fn line_text(line: &ratatui::text::Line) -> String {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    /// Parity with `status`: healthy banner plus all-granted permissions.
    #[test]
    fn healthy_banner_matches_status() {
        let text = line_text(&tui_status_line(Some(&healthy_state()), true));
        assert!(text.contains("capture: healthy"), "{text}");
        assert!(text.contains("permissions: all granted"), "{text}");
        assert!(!text.contains("write errors"), "{text}");
    }

    /// Degraded shows only nonzero counters; the permission gap names its
    /// path; the capture note rides along.
    #[test]
    fn degraded_banner_shows_only_nonzero_counters() {
        let mut state = healthy_state();
        state.capture_health = "degraded".to_string();
        state.write_errors = 3;
        state.capture_note = "screen recording revoked — screenshots stopped".to_string();
        state.permissions.screen_recording = PermissionState::Denied;
        let text = line_text(&tui_status_line(Some(&state), true));
        assert!(text.contains("capture: degraded (3 write errors)"), "{text}");
        assert!(!text.contains("dropped"), "{text}");
        assert!(!text.contains("queued"), "{text}");
        assert!(text.contains("screen recording: denied"), "{text}");
        assert!(text.contains("screenshots stopped"), "{text}");
    }

    /// Unreachable (socket, no reply) reads distinctly from not-running.
    #[test]
    fn unreachable_differs_from_not_running() {
        let wedged = line_text(&tui_status_line(None, true));
        let down = line_text(&tui_status_line(None, false));
        assert!(wedged.contains("unreachable"), "{wedged}");
        assert!(down.contains("not running"), "{down}");
        assert_ne!(wedged, down);
    }
}
