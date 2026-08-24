use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::DefaultTerminal;
use rustwatch_core::{DaemonClient, DaemonCommand, DaemonReply, DataPaths, Store, Config};

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
        let status = if paths.socket.exists() {
            let client = DaemonClient::new(&paths.socket);
            match client.send(DaemonCommand::Status).await {
                Ok(DaemonReply::Status { state }) => {
                    paused = state.paused;
                    format!(
                        "Capturing | events={} segments={}",
                        state.events_captured, state.segments_written
                    )
                }
                _ => "Daemon unreachable".into(),
            }
        } else {
            "Daemon not running".into()
        };

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
