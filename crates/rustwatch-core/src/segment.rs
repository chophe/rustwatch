use chrono::Utc;
use uuid::Uuid;

use crate::{
    events::{AppContext, CaptureEvent, CaptureEventKind},
    SessionSegment,
};

const MAX_BUFFER_CHARS: usize = 16_384;

pub struct SegmentGrouper {
    current: Option<ActiveSegment>,
    /// D-18/D-19: set by `mark_idle`, cleared by `mark_active`. Segments
    /// opened while set carry `idle: true` — capture continues through idle,
    /// reports exclude it later.
    idle: bool,
}

struct ActiveSegment {
    id: String,
    app: AppContext,
    text_buffer: String,
    started_at: chrono::DateTime<Utc>,
    event_count: u64,
    idle: bool,
}

impl SegmentGrouper {
    pub fn new() -> Self {
        Self {
            current: None,
            idle: false,
        }
    }

    /// The currently open segment's id, if any — the writer fills
    /// `segment_id` on screenshot rows instead of hardcoding None.
    pub fn open_segment_id(&self) -> Option<String> {
        self.current.as_ref().map(|active| active.id.clone())
    }

    /// D-18: close the active segment marked `idle: true`. Pure — no clock,
    /// no I/O; the capture layer injects the signal with its own timestamp.
    /// A no-op (returning None) when already idle or with no open segment,
    /// but the flag is still set so later segments open idle.
    pub fn mark_idle(&mut self, timestamp: chrono::DateTime<Utc>) -> Option<SessionSegment> {
        self.idle = true;
        // Forced: the segment closed here covered active work, but D-18
        // marks the close itself idle — reports exclude it either way.
        self.current.take().map(|active| {
            let mut segment = active.into_segment(timestamp);
            segment.idle = true;
            segment
        })
    }

    /// D-19: end idle after sustained activity. Closes the idle segment so
    /// post-resume work starts fresh — otherwise it would append to an
    /// `idle: true` row until the next focus change. No-op when not idle.
    pub fn mark_active(&mut self, timestamp: chrono::DateTime<Utc>) -> Option<SessionSegment> {
        if !self.idle {
            return None;
        }
        self.idle = false;
        self.current.take().map(|active| active.into_segment(timestamp))
    }

    pub fn on_event(&mut self, event: &CaptureEvent) -> Option<SessionSegment> {
        match &event.kind {
            CaptureEventKind::FocusChange { to, .. } => self.on_focus(to.clone(), event.timestamp),
            CaptureEventKind::TextDelta { text } => {
                self.ensure_segment(event);
                if let Some(active) = &mut self.current {
                    append_text(&mut active.text_buffer, text);
                    active.event_count += 1;
                }
                None
            }
            CaptureEventKind::Paste { content } => {
                self.ensure_segment(event);
                if let Some(active) = &mut self.current {
                    append_text(&mut active.text_buffer, content);
                    active.event_count += 1;
                }
                None
            }
            CaptureEventKind::TextFieldSnapshot { value } => {
                self.ensure_segment(event);
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
            CaptureEventKind::IdleStart { .. } => self.mark_idle(event.timestamp),
            CaptureEventKind::ActivityResumed => self.mark_active(event.timestamp),
        }
    }

    /// Open a segment if none is active.
    ///
    /// The focus loop polls, so keystrokes can arrive before the first
    /// `FocusChange` — or after a focus change that was suppressed because the
    /// app is excluded. Without this, that text is silently dropped.
    fn ensure_segment(&mut self, event: &CaptureEvent) {
        if self.current.is_some() {
            return;
        }
        let app = event.app.clone().unwrap_or_else(|| AppContext {
            app_name: "unknown".into(),
            window_title: String::new(),
            process_id: 0,
            bundle_id: None,
        });
        let idle = self.idle;
        self.current = Some(ActiveSegment {
            id: Uuid::new_v4().to_string(),
            app,
            text_buffer: String::new(),
            started_at: event.timestamp,
            event_count: 0,
            idle,
        });
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
        let idle = self.idle;
        self.current = Some(ActiveSegment {
            id: Uuid::new_v4().to_string(),
            app,
            text_buffer: String::new(),
            started_at: timestamp,
            event_count: 0,
            idle,
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
            idle: self.idle,
        }
    }
}

/// Append `text`, trimming from the front to stay under `MAX_BUFFER_CHARS`.
///
/// Trims on a char boundary: slicing at an arbitrary byte offset panics on
/// multi-byte UTF-8, which would kill the daemon's only writer task.
fn append_text(buffer: &mut String, text: &str) {
    // A single chunk larger than the cap: keep only its tail, which is the
    // most recent input. Round down to a char boundary so we never split a
    // multi-byte character.
    let text = if text.len() > MAX_BUFFER_CHARS {
        let mut start = text.len() - MAX_BUFFER_CHARS;
        while start < text.len() && !text.is_char_boundary(start) {
            start += 1;
        }
        &text[start..]
    } else {
        text
    };

    if buffer.len() + text.len() > MAX_BUFFER_CHARS {
        let keep = MAX_BUFFER_CHARS.saturating_sub(text.len());
        if keep < buffer.len() {
            let drop = buffer.len() - keep;
            // Round *up* to the next char boundary: trimming back instead would
            // remove fewer bytes than required and leave the buffer over cap.
            let mut start = drop.min(buffer.len());
            while start < buffer.len() && !buffer.is_char_boundary(start) {
                start += 1;
            }
            buffer.replace_range(..start, "");
        }
    }
    buffer.push_str(text);
}

fn truncate(value: &str) -> String {
    value.chars().take(MAX_BUFFER_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::{SegmentGrouper, MAX_BUFFER_CHARS};
    use crate::events::{AppContext, CaptureEvent, CaptureEventKind};

    fn app(name: &str) -> AppContext {
        AppContext {
            app_name: name.to_string(),
            window_title: format!("{name} window"),
            process_id: 42,
            bundle_id: Some(format!("com.example.{name}")),
        }
    }

    fn text_delta(t: &str) -> CaptureEvent {
        CaptureEvent::new(
            CaptureEventKind::TextDelta {
                text: t.to_string(),
            },
            Some(app("Safari")),
        )
    }

    fn focus(to: &str) -> CaptureEvent {
        CaptureEvent::new(
            CaptureEventKind::FocusChange {
                from: None,
                to: app(to),
            },
            Some(app(to)),
        )
    }

    /// Regression: text arriving before any FocusChange used to be dropped,
    /// because only FocusChange opened a segment and the focus loop polls.
    #[test]
    fn text_before_any_focus_change_is_not_lost() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("hello"));
        g.on_event(&text_delta(" world"));
        let seg = g.flush().expect("segment should exist");

        assert_eq!(seg.text_buffer, "hello world");
        assert_eq!(seg.event_count, 2);
        assert_eq!(seg.app_name, "Safari");
    }

    #[test]
    fn paste_before_focus_change_is_not_lost() {
        let mut g = SegmentGrouper::new();
        g.on_event(&CaptureEvent::new(
            CaptureEventKind::Paste {
                content: "pasted".into(),
            },
            Some(app("Safari")),
        ));
        let seg = g.flush().expect("segment should exist");
        assert_eq!(seg.text_buffer, "pasted");
    }

    #[test]
    fn snapshot_before_focus_change_is_not_lost() {
        let mut g = SegmentGrouper::new();
        g.on_event(&CaptureEvent::new(
            CaptureEventKind::TextFieldSnapshot {
                value: "snapshot value".into(),
            },
            Some(app("Safari")),
        ));
        let seg = g.flush().expect("segment should exist");
        assert_eq!(seg.text_buffer, "snapshot value");
    }

    #[test]
    fn text_with_no_app_context_still_lands() {
        let mut g = SegmentGrouper::new();
        g.on_event(&CaptureEvent::new(
            CaptureEventKind::TextDelta {
                text: "orphan".into(),
            },
            None,
        ));
        let seg = g.flush().expect("segment should exist");
        assert_eq!(seg.text_buffer, "orphan");
        assert_eq!(seg.app_name, "unknown");
    }

    /// An excluded app produces no FocusChange, so its keystrokes arrive with
    /// no open segment. They must still be attributed, not silently dropped.
    #[test]
    fn keystrokes_after_suppressed_focus_change_are_attributed() {
        let mut g = SegmentGrouper::new();
        // 1Password is excluded, so run_focus_loop never emitted a FocusChange.
        g.on_event(&text_delta("hunter2"));
        let seg = g.flush().expect("segment should exist");
        assert_eq!(seg.text_buffer, "hunter2");
        assert_eq!(seg.app_name, "Safari");
    }

    #[test]
    fn focus_change_still_splits_segments() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("in safari"));
        let closed = g.on_event(&focus("Terminal")).expect("previous closed");

        assert_eq!(closed.text_buffer, "in safari");
        assert_eq!(closed.app_name, "Safari");

        g.on_event(&text_delta("in terminal"));
        let seg = g.flush().expect("segment should exist");
        assert_eq!(seg.text_buffer, "in terminal");
        assert_eq!(seg.app_name, "Terminal");
    }

    #[test]
    fn ensure_segment_does_not_reopen_an_active_segment() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("a"));
        g.on_event(&text_delta("b"));
        g.on_event(&text_delta("c"));
        let seg = g.flush().expect("segment");
        assert_eq!(seg.text_buffer, "abc");
        assert_eq!(seg.event_count, 3);
    }

    #[test]
    fn buffer_trims_from_front_and_stays_bounded() {
        let mut g = SegmentGrouper::new();
        let chunk = "x".repeat(1000);
        for _ in 0..40 {
            g.on_event(&text_delta(&chunk));
        }
        let seg = g.flush().expect("segment");
        assert!(
            seg.text_buffer.len() <= MAX_BUFFER_CHARS,
            "buffer grew to {} > {MAX_BUFFER_CHARS}",
            seg.text_buffer.len()
        );
    }

    /// Regression: append_text sliced at a byte offset, panicking on
    /// multi-byte UTF-8 and killing the daemon's only writer task.
    #[test]
    fn multibyte_trim_does_not_panic_and_stays_valid_utf8() {
        let mut g = SegmentGrouper::new();
        // Seed with multi-byte characters so any byte-offset trim lands mid-char.
        for _ in 0..30 {
            g.on_event(&text_delta(&"日".repeat(900)));
        }
        let seg = g.flush().expect("segment");
        assert!(seg.text_buffer.len() <= MAX_BUFFER_CHARS);
        // Valid UTF-8 by construction; assert it round-trips.
        assert!(std::str::from_utf8(seg.text_buffer.as_bytes()).is_ok());
    }

    #[test]
    fn mixed_ascii_and_multibyte_trim_is_safe() {
        let mut g = SegmentGrouper::new();
        for i in 0..40 {
            if i % 2 == 0 {
                g.on_event(&text_delta(&"a".repeat(800)));
            } else {
                g.on_event(&text_delta(&"é日".repeat(400)));
            }
        }
        let seg = g.flush().expect("segment");
        assert!(seg.text_buffer.len() <= MAX_BUFFER_CHARS);
        assert!(std::str::from_utf8(seg.text_buffer.as_bytes()).is_ok());
    }

    #[test]
    fn flush_on_empty_grouper_returns_none() {
        let mut g = SegmentGrouper::new();
        assert!(g.flush().is_none());
    }

    /// D-18: idle closes the active segment marked `idle: true`.
    #[test]
    fn mark_idle_closes_segment_as_idle() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("work"));
        let closed = g.mark_idle(chrono::Utc::now()).expect("segment closed");
        assert!(closed.idle);
        assert_eq!(closed.text_buffer, "work");
    }

    /// Capture continues through idle: segments opened while idle are born
    /// idle, so a stray nudge never pollutes work reports.
    #[test]
    fn segments_opened_during_idle_are_idle() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("work"));
        g.mark_idle(chrono::Utc::now());
        g.on_event(&text_delta("nudge"));
        let seg = g.flush().expect("segment");
        assert!(seg.idle);
        assert_eq!(seg.text_buffer, "nudge");
    }

    /// D-19: resume closes the idle segment so post-resume work starts
    /// fresh instead of appending to an `idle: true` row.
    #[test]
    fn mark_active_closes_idle_segment_and_clears() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("work"));
        g.mark_idle(chrono::Utc::now());
        g.on_event(&text_delta("back"));
        let closed = g.mark_active(chrono::Utc::now()).expect("idle closed");
        assert!(closed.idle);
        g.on_event(&text_delta("fresh"));
        let seg = g.flush().expect("segment");
        assert!(!seg.idle);
        assert_eq!(seg.text_buffer, "fresh");
    }

    #[test]
    fn mark_active_is_noop_when_not_idle() {
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("work"));
        assert!(g.mark_active(chrono::Utc::now()).is_none());
        let seg = g.flush().expect("segment still open");
        assert!(!seg.idle);
    }

    #[test]
    fn double_mark_idle_stays_idle_without_panic() {
        let mut g = SegmentGrouper::new();
        assert!(g.mark_idle(chrono::Utc::now()).is_none());
        g.on_event(&text_delta("nudge"));
        let reopened = g.mark_idle(chrono::Utc::now()).expect("idle segment closed");
        assert!(reopened.idle);
    }

    #[test]
    fn open_segment_id_tracks_the_active_segment() {
        let mut g = SegmentGrouper::new();
        assert_eq!(g.open_segment_id(), None);
        g.on_event(&text_delta("work"));
        assert!(g.open_segment_id().is_some());
        g.mark_idle(chrono::Utc::now());
        assert_eq!(g.open_segment_id(), None);
    }

    /// The synthetic idle signals fold through `on_event` like any event.
    #[test]
    fn idle_signals_fold_through_on_event() {
        use crate::events::CaptureEventKind;
        let mut g = SegmentGrouper::new();
        g.on_event(&text_delta("work"));
        let idle_signal = CaptureEvent::new(
            CaptureEventKind::IdleStart { idle_secs: 301 },
            None,
        );
        let closed = g.on_event(&idle_signal).expect("closed at idle");
        assert!(closed.idle);
        let resume_signal = CaptureEvent::new(CaptureEventKind::ActivityResumed, None);
        assert!(g.on_event(&resume_signal).is_none());
        g.on_event(&text_delta("fresh"));
        assert!(!g.flush().expect("segment").idle);
    }
}
