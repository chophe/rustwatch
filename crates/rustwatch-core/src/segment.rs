use chrono::Utc;
use uuid::Uuid;

use crate::{
    events::{AppContext, CaptureEvent, CaptureEventKind},
    SessionSegment,
};

const MAX_BUFFER_CHARS: usize = 16_384;

pub struct SegmentGrouper {
    current: Option<ActiveSegment>,
}

struct ActiveSegment {
    id: String,
    app: AppContext,
    text_buffer: String,
    started_at: chrono::DateTime<Utc>,
    event_count: u64,
}

impl SegmentGrouper {
    pub fn new() -> Self {
        Self { current: None }
    }

    pub fn on_event(&mut self, event: &CaptureEvent) -> Option<SessionSegment> {
        match &event.kind {
            CaptureEventKind::FocusChange { to, .. } => self.on_focus(to.clone(), event.timestamp),
            CaptureEventKind::TextDelta { text } => {
                if let Some(active) = &mut self.current {
                    append_text(&mut active.text_buffer, text);
                    active.event_count += 1;
                }
                None
            }
            CaptureEventKind::Paste { content } => {
                if let Some(active) = &mut self.current {
                    append_text(&mut active.text_buffer, content);
                    active.event_count += 1;
                }
                None
            }
            CaptureEventKind::TextFieldSnapshot { value } => {
                if let Some(active) = &mut self.current {
                    if !value.is_empty() && active.text_buffer.len() < value.len() {
                        active.text_buffer = truncate(value);
                    }
                    active.event_count += 1;
                }
                None
            }
            CaptureEventKind::Key { .. } | CaptureEventKind::Copy { .. } | CaptureEventKind::Screenshot { .. } => {
                if let Some(active) = &mut self.current {
                    active.event_count += 1;
                }
                None
            }
        }
    }

    pub fn flush(&mut self) -> Option<SessionSegment> {
        self.current.take().map(|active| active.into_segment(Utc::now()))
    }

    fn on_focus(
        &mut self,
        app: AppContext,
        timestamp: chrono::DateTime<Utc>,
    ) -> Option<SessionSegment> {
        let previous = self.current.take().map(|active| active.into_segment(timestamp));
        self.current = Some(ActiveSegment {
            id: Uuid::new_v4().to_string(),
            app,
            text_buffer: String::new(),
            started_at: timestamp,
            event_count: 0,
        });
        previous
    }
}

impl Default for SegmentGrouper {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveSegment {
    fn into_segment(self, ended_at: chrono::DateTime<Utc>) -> SessionSegment {
        SessionSegment {
            id: self.id,
            app_name: self.app.app_name,
            window_title: self.app.window_title,
            process_id: self.app.process_id,
            bundle_id: self.app.bundle_id,
            text_buffer: self.text_buffer,
            started_at: self.started_at,
            ended_at,
            event_count: self.event_count,
        }
    }
}

fn append_text(buffer: &mut String, text: &str) {
    if buffer.len() + text.len() > MAX_BUFFER_CHARS {
        let keep = MAX_BUFFER_CHARS.saturating_sub(text.len());
        if keep < buffer.len() {
            buffer.replace_range(..buffer.len() - keep, "");
        }
    }
    buffer.push_str(text);
}

fn truncate(value: &str) -> String {
    value.chars().take(MAX_BUFFER_CHARS).collect()
}
