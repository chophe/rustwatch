use chrono::{DateTime, Utc};
use refinery::embed_migrations;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::{
    ActivityRecord, CaptureEvent, Result, ScreenshotRecord, SessionSegment,
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
            "INSERT OR REPLACE INTO segments (id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
            "SELECT id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count
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
                started_at: parse_ts(row.get(6)?),
                ended_at: parse_ts(row.get(7)?),
                event_count: row.get(8)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_unanalyzed_segments(&self, limit: usize) -> Result<Vec<SessionSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.app_name, s.window_title, s.process_id, s.bundle_id, s.text_buffer, s.started_at, s.ended_at, s.event_count
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
                started_at: parse_ts(row.get(6)?),
                ended_at: parse_ts(row.get(7)?),
                event_count: row.get(8)?,
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
                started_at: parse_ts(row.get(4)?),
                ended_at: parse_ts(row.get(5)?),
                apps: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default(),
                topics: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                segment_ids: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_segment(&self, id: &str) -> Result<Option<SessionSegment>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, app_name, window_title, process_id, bundle_id, text_buffer, started_at, ended_at, event_count
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
                started_at: parse_ts(row.get(6)?),
                ended_at: parse_ts(row.get(7)?),
                event_count: row.get(8)?,
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

fn parse_ts(raw: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
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
