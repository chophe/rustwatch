use chrono::{DateTime, Utc};
use refinery::embed_migrations;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::{
    ActivityRecord, CaptureEvent, Note, Result, ScreenshotRecord, SessionSegment,
};

embed_migrations!("migrations");

/// D-04: capacity of the in-memory retry queue. Bounded (never unbounded),
/// overflow drops oldest and counts it — never silent.
pub const RETRY_QUEUE_CAP: usize = 10_000;

/// D-04: retry only SQLITE_BUSY / IO failures. Everything else (constraint,
/// schema drift, serialization) is poison — log + count immediately, never
/// loop. Classified by error kind, never by string matching.
pub fn is_retryable_db_error(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(code, _) => matches!(
            code.code,
            rusqlite::ffi::ErrorCode::DatabaseBusy | rusqlite::ffi::ErrorCode::SystemIoFailure
        ),
        _ => false,
    }
}

/// D-04 classification over the store's error type. `Io` (e.g. disk hiccup
/// under WAL) is retryable; all other non-DB failures are fatal.
pub fn is_retryable_store_error(err: &crate::Error) -> bool {
    match err {
        crate::Error::Db(inner) => is_retryable_db_error(inner),
        crate::Error::Io(_) => true,
        _ => false,
    }
}

/// D-03/D-04: bounded in-memory retry queue owned by the writer task.
/// Overflow drops the oldest event and counts it in `dropped`.
pub struct RetryQueue {
    inner: std::collections::VecDeque<CaptureEvent>,
    cap: usize,
    dropped: u64,
}

impl RetryQueue {
    pub fn new() -> Self {
        Self::with_cap(RETRY_QUEUE_CAP)
    }

    pub fn with_cap(cap: usize) -> Self {
        Self {
            inner: std::collections::VecDeque::new(),
            cap,
            dropped: 0,
        }
    }

    pub fn push(&mut self, event: CaptureEvent) {
        if self.inner.len() >= self.cap {
            self.inner.pop_front();
            self.dropped += 1;
        }
        self.inner.push_back(event);
    }

    pub fn pop_front(&mut self) -> Option<CaptureEvent> {
        self.inner.pop_front()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// D-05: shutdown drain counts the undrained remainder as dropped.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Take (and reset) the overflow-drop count so the daemon can fold it
    /// into the shared `dropped_events` counter exactly once per drain.
    pub fn take_dropped(&mut self) -> u64 {
        std::mem::take(&mut self.dropped)
    }
}

impl Default for RetryQueue {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(crate::Error::from)?;
        }
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrations::runner().run(&mut conn).map_err(|e| {
            crate::Error::Other(format!("migration failed: {e}"))
        })?;
        Ok(Self { conn })
    }

    pub fn insert_event(&self, event: &CaptureEvent) -> Result<()> {
        let payload = serde_json::to_string(&event.kind)?;
        let app_json = event
            .app
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.conn.execute(
            "INSERT INTO events (id, timestamp, app_json, payload_json) VALUES (?1, ?2, ?3, ?4)",
            params![
                event.id,
                event.timestamp.to_rfc3339(),
                app_json,
                payload
            ],
        )?;
        Ok(())
    }

    pub fn insert_segment(&self, segment: &SessionSegment) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO segments (id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count, idle)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                segment.id,
                segment.app_name,
                segment.window_title,
                segment.process_id,
                segment.bundle_id,
                segment.text_buffer,
                segment.started_at.to_rfc3339(),
                segment.ended_at.to_rfc3339(),
                segment.event_count,
                segment.idle as i64,
            ],
        )?;
        Ok(())
    }

    pub fn insert_activity(&self, activity: &ActivityRecord) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO activities (id, label, category, confidence, started_at, ended_at, apps_json, topics_json, segment_ids_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                activity.id,
                activity.label,
                activity.category,
                activity.confidence,
                activity.started_at.to_rfc3339(),
                activity.ended_at.to_rfc3339(),
                serde_json::to_string(&activity.apps)?,
                serde_json::to_string(&activity.topics)?,
                serde_json::to_string(&activity.segment_ids)?,
            ],
        )?;
        Ok(())
    }

    pub fn insert_screenshot(&self, shot: &ScreenshotRecord) -> Result<()> {
        self.conn.execute(
            "INSERT INTO screenshots (id, path, scope, captured_at, segment_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                shot.id,
                shot.path.to_string_lossy().to_string(),
                format!("{:?}", shot.scope).to_lowercase(),
                shot.captured_at.to_rfc3339(),
                shot.segment_id,
            ],
        )?;
        Ok(())
    }

    /// D-10: persist an annotation note (text already redacted by the
    /// caller) linked to the screenshot it annotates.
    pub fn insert_note(&self, note: &Note) -> Result<()> {
        self.conn.execute(
            "INSERT INTO notes (id, ts, note, screenshot_id) VALUES (?1, ?2, ?3, ?4)",
            params![
                note.id,
                note.ts.to_rfc3339(),
                note.note,
                note.screenshot_id,
            ],
        )?;
        Ok(())
    }

    /// D-11: the annotate prompt targets the most recent screenshot that has
    /// no note yet — one prompt, one shot, no backlog of unannotated rows.
    pub fn latest_screenshot_without_note(&self) -> Result<Option<ScreenshotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, scope, captured_at, segment_id FROM screenshots s
             WHERE NOT EXISTS (SELECT 1 FROM notes n WHERE n.screenshot_id = s.id)
             ORDER BY captured_at DESC LIMIT 1",
        )?;
        stmt.query_row([], map_screenshot_row)
            .optional()
            .map_err(Into::into)
    }

    /// Digest use (Phase 5): all notes logged on a calendar date.
    pub fn list_notes_for_date(&self, date: chrono::NaiveDate) -> Result<Vec<Note>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, note, screenshot_id FROM notes
             WHERE date(ts) = date(?1) ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![date.format("%Y-%m-%d").to_string()], |row| {
            Ok(Note {
                id: row.get(0)?,
                ts: parse_ts(row.get(1)?)?,
                note: row.get(2)?,
                screenshot_id: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_events_since(
        &self,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<CaptureEvent>> {
        let mut events = Vec::new();
        match since {
            Some(since) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, timestamp, app_json, payload_json FROM events
                     WHERE timestamp >= ?1 ORDER BY timestamp ASC LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![since.to_rfc3339(), limit as i64], map_event_row)?;
                for row in rows {
                    events.push(row?);
                }
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, timestamp, app_json, payload_json FROM events
                     ORDER BY timestamp ASC LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![limit as i64], map_event_row)?;
                for row in rows {
                    events.push(row?);
                }
            }
        }
        Ok(events)
    }

    pub fn list_segments_between(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Vec<SessionSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count, idle
             FROM segments WHERE started_at >= ?1 AND ended_at <= ?2 ORDER BY started_at ASC",
        )?;
        let rows = stmt.query_map(params![from.to_rfc3339(), to.to_rfc3339()], |row| {
            Ok(SessionSegment {
                id: row.get(0)?,
                app_name: row.get(1)?,
                window_title: row.get(2)?,
                process_id: row.get(3)?,
                bundle_id: row.get(4)?,
                text_buffer: row.get(5)?,
                started_at: parse_ts(row.get(6)?)?,
                ended_at: parse_ts(row.get(7)?)?,
                event_count: row.get(8)?,
                idle: row.get::<_, i64>(9)? != 0,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_unanalyzed_segments(&self, limit: usize) -> Result<Vec<SessionSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.app_name, s.window_title, s.process_id, s.bundle_id, s.text_buffer, s.started_at, s.ended_at, s.event_count, s.idle
             FROM segments s
             LEFT JOIN activities a ON a.segment_ids_json LIKE '%' || s.id || '%'
             WHERE a.id IS NULL
             ORDER BY s.started_at ASC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(SessionSegment {
                id: row.get(0)?,
                app_name: row.get(1)?,
                window_title: row.get(2)?,
                process_id: row.get(3)?,
                bundle_id: row.get(4)?,
                text_buffer: row.get(5)?,
                started_at: parse_ts(row.get(6)?)?,
                ended_at: parse_ts(row.get(7)?)?,
                event_count: row.get(8)?,
                idle: row.get::<_, i64>(9)? != 0,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_activities_for_date(&self, date: chrono::NaiveDate) -> Result<Vec<ActivityRecord>> {
        let start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
        let end = date.and_hms_opt(23, 59, 59).unwrap().and_utc();
        let mut stmt = self.conn.prepare(
            "SELECT id, label, category, confidence, started_at, ended_at, apps_json, topics_json, segment_ids_json
             FROM activities WHERE started_at >= ?1 AND started_at <= ?2 ORDER BY started_at ASC",
        )?;
        let rows = stmt.query_map(params![start.to_rfc3339(), end.to_rfc3339()], |row| {
            Ok(ActivityRecord {
                id: row.get(0)?,
                label: row.get(1)?,
                category: row.get(2)?,
                confidence: row.get(3)?,
                started_at: parse_ts(row.get(4)?)?,
                ended_at: parse_ts(row.get(5)?)?,
                apps: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                topics: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                segment_ids: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_segment(&self, id: &str) -> Result<Option<SessionSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count, idle
             FROM segments WHERE id = ?1",
        )?;
        stmt.query_row(params![id], |row| {
            Ok(SessionSegment {
                id: row.get(0)?,
                app_name: row.get(1)?,
                window_title: row.get(2)?,
                process_id: row.get(3)?,
                bundle_id: row.get(4)?,
                text_buffer: row.get(5)?,
                started_at: parse_ts(row.get(6)?)?,
                ended_at: parse_ts(row.get(7)?)?,
                event_count: row.get(8)?,
                idle: row.get::<_, i64>(9)? != 0,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn stats(&self) -> Result<(u64, u64, u64, u64)> {
        let events: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        let segments: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))?;
        let activities: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM activities", [], |r| r.get(0))?;
        let screenshots: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM screenshots", [], |r| r.get(0))?;
        Ok((events, segments, activities, screenshots))
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn new_activity_id() -> String {
        Uuid::new_v4().to_string()
    }
}

/// Corrupt timestamps error like `map_event_row` does instead of
/// substituting `Utc::now()`, which would silently corrupt the time series.
fn parse_ts(raw: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// First reader of the screenshots table: scope was write-only until the
/// annotate flow needed it. Unknown values fall back to Screen — the honest
/// fullscreen reading — rather than failing the prompt.
fn map_screenshot_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScreenshotRecord> {
    let scope_raw: String = row.get(2)?;
    let scope = match scope_raw.as_str() {
        "window" => crate::ScreenshotScope::Window,
        _ => crate::ScreenshotScope::Screen,
    };
    Ok(ScreenshotRecord {
        id: row.get(0)?,
        path: std::path::PathBuf::from(row.get::<_, String>(1)?),
        scope,
        captured_at: parse_ts(row.get(3)?)?,
        segment_id: row.get(4)?,
    })
}

fn map_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureEvent> {
    let id: String = row.get(0)?;
    let timestamp: String = row.get(1)?;
    let app_json: Option<String> = row.get(2)?;
    let payload_json: String = row.get(3)?;
    let timestamp = DateTime::parse_from_rfc3339(&timestamp)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
        .with_timezone(&Utc);
    let app = app_json
        .map(|v| serde_json::from_str(&v))
        .transpose()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    let kind = serde_json::from_str(&payload_json)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(CaptureEvent {
        id,
        timestamp,
        app,
        kind,
    })
}

#[cfg(test)]
mod retry_queue_tests {
    use super::*;
    use crate::events::CaptureEventKind;

    fn synthetic_event() -> CaptureEvent {
        CaptureEvent::new(
            CaptureEventKind::TextDelta {
                text: "x".to_string(),
            },
            None,
        )
    }

    fn sqlite_failure(code: rusqlite::ffi::ErrorCode) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code,
                extended_code: 0,
            },
            None,
        )
    }

    /// D-04: only SQLITE_BUSY / IO retry; everything else is poison.
    #[test]
    fn retryable_vs_fatal_classification() {
        assert!(is_retryable_db_error(&sqlite_failure(
            rusqlite::ffi::ErrorCode::DatabaseBusy
        )));
        assert!(is_retryable_db_error(&sqlite_failure(
            rusqlite::ffi::ErrorCode::SystemIoFailure
        )));
        // Constraint violations (e.g. duplicate id) and lock errors must
        // never loop on the 500 ms timer.
        assert!(!is_retryable_db_error(&sqlite_failure(
            rusqlite::ffi::ErrorCode::ConstraintViolation
        )));
        assert!(!is_retryable_db_error(&sqlite_failure(
            rusqlite::ffi::ErrorCode::DatabaseLocked
        )));
        assert!(!is_retryable_db_error(&rusqlite::Error::InvalidPath(
            "gone".into()
        )));
        // Store-level: Io (disk hiccup under WAL) retries; config/other never.
        assert!(is_retryable_store_error(&crate::Error::Db(sqlite_failure(
            rusqlite::ffi::ErrorCode::DatabaseBusy
        ))));
        assert!(is_retryable_store_error(&crate::Error::Db(sqlite_failure(
            rusqlite::ffi::ErrorCode::ConstraintViolation
        ))) == false);
        assert!(is_retryable_store_error(&crate::Error::Config(
            "bad".into()
        )) == false);
    }

    /// D-04: bounded at cap, overflow drops oldest and counts it — never
    /// unbounded, never silent.
    #[test]
    fn drop_oldest_accounting_at_cap() {
        let mut queue = RetryQueue::with_cap(3);
        for _ in 0..5 {
            queue.push(synthetic_event());
        }
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.dropped(), 2);
        // take_dropped folds into the daemon counter exactly once.
        assert_eq!(queue.take_dropped(), 2);
        assert_eq!(queue.take_dropped(), 0);
    }

    /// Flushing an empty queue is a no-op: no pops, no drops, no panic.
    #[test]
    fn flush_on_empty_is_noop() {
        let mut queue = RetryQueue::new();
        assert!(queue.is_empty());
        assert!(queue.pop_front().is_none());
        assert_eq!(queue.take_dropped(), 0);
        queue.clear();
        assert!(queue.is_empty());
    }

    /// Corrupt timestamps error instead of becoming `now()`: the time
    /// series must never be silently rewritten.
    #[test]
    fn corrupt_timestamp_errors_instead_of_now() {        let dir = std::env::temp_dir().join(format!(
            "rustwatch-corrupt-ts-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir.join("test.db")).unwrap();
        store
            .connection()
            .execute(
                "INSERT INTO events (id, timestamp, app_json, payload_json) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["bad-row", "not-a-timestamp", None::<String>, "{}"],
            )
            .unwrap();
        assert!(store.list_events_since(None, 10).is_err());
    }
}

#[cfg(test)]
mod notes_idle_tests {
    use super::*;
    use crate::events::CaptureEventKind;

    fn test_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!(
            "rustwatch-notes-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir.join("test.db")).unwrap()
    }

    /// V2 applies on a fresh database: the notes table exists and segments
    /// carry the idle column defaulting to 0.
    #[test]
    fn v2_migration_applies_clean() {
        let store = test_store("v2");
        let notes: String = store
            .connection()
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='notes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(notes, "notes");
        let idle_default: i64 = store
            .connection()
            .query_row("SELECT dflt_value FROM pragma_table_info('segments') WHERE name='idle'", [], |r| {
                r.get::<_, String>(0)
            })
            .map(|v| v.parse().unwrap())
            .unwrap();
        assert_eq!(idle_default, 0);
    }

    /// Idle segments round-trip their flag; work segments stay false.
    #[test]
    fn idle_flag_round_trips() {
        let store = test_store("idle");
        let mut grouper = crate::SegmentGrouper::new();
        let now = chrono::Utc::now();
        grouper.on_event(&CaptureEvent::new(
            CaptureEventKind::TextDelta { text: "work".into() },
            None,
        ));
        let idle_seg = grouper.mark_idle(now).unwrap();
        store.insert_segment(&idle_seg).unwrap();
        let from = now - chrono::Duration::hours(1);
        let to = now + chrono::Duration::hours(1);
        let segs = store.list_segments_between(from, to).unwrap();
        assert_eq!(segs.len(), 1);
        assert!(segs[0].idle);
        assert_eq!(store.get_segment(&segs[0].id).unwrap().unwrap().idle, true);
    }

    /// D-10/D-11: annotate targets the newest unnoted screenshot; a noted
    /// screenshot is never offered again; notes list by date.
    #[test]
    fn annotate_targets_newest_screenshot_without_note() {
        let store = test_store("annotate");
        let now = chrono::Utc::now();
        for (id, secs) in [("old", 300), ("new", 60)] {
            store
                .insert_screenshot(&ScreenshotRecord {
                    id: id.to_string(),
                    path: std::path::PathBuf::from(format!("/tmp/{id}.png")),
                    scope: crate::ScreenshotScope::Screen,
                    captured_at: now - chrono::Duration::seconds(secs),
                    segment_id: None,
                })
                .unwrap();
        }
        let target = store.latest_screenshot_without_note().unwrap().unwrap();
        assert_eq!(target.id, "new");
        store
            .insert_note(&Note {
                id: "n1".into(),
                ts: now,
                note: "annotated".into(),
                screenshot_id: Some("new".into()),
            })
            .unwrap();
        let target = store.latest_screenshot_without_note().unwrap().unwrap();
        assert_eq!(target.id, "old");
        let notes = store.list_notes_for_date(now.date_naive()).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].screenshot_id.as_deref(), Some("new"));
    }

    /// Everything noted: the prompt reports nothing to annotate.
    #[test]
    fn no_unnoted_screenshots_returns_none() {
        let store = test_store("empty");
        assert!(store.latest_screenshot_without_note().unwrap().is_none());
    }

    /// A database written by 01-01 (bare V1 schema, no history row) gains
    /// the idle column with old rows reading back non-idle.
    #[test]
    fn v2_upgrades_an_existing_v1_database() {
        let dir = std::env::temp_dir().join(format!(
            "rustwatch-v1-upgrade-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(include_str!("../migrations/V1__initial.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO segments (id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count)
             VALUES ('old', 'Safari', 't', 1, NULL, 'work', '2026-01-01T00:00:00+00:00', '2026-01-01T01:00:00+00:00', 5)",
            [],
        )
        .unwrap();
        drop(conn);
        let store = Store::open(&path).unwrap();
        let old = store.get_segment("old").unwrap().unwrap();
        assert!(!old.idle);
        assert_eq!(old.event_count, 5);
    }
}
