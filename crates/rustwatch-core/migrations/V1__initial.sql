-- Initial schema for rustwatch capture pipeline

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY NOT NULL,
    timestamp TEXT NOT NULL,
    app_json TEXT,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);

CREATE TABLE IF NOT EXISTS segments (
    id TEXT PRIMARY KEY NOT NULL,
    app_name TEXT NOT NULL,
    window_title TEXT NOT NULL,
    process_id INTEGER NOT NULL,
    bundle_id TEXT,
    text_buffer TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NOT NULL,
    event_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_segments_started_at ON segments(started_at);

CREATE TABLE IF NOT EXISTS screenshots (
    id TEXT PRIMARY KEY NOT NULL,
    path TEXT NOT NULL,
    scope TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    segment_id TEXT
);

CREATE TABLE IF NOT EXISTS activities (
    id TEXT PRIMARY KEY NOT NULL,
    label TEXT NOT NULL,
    category TEXT NOT NULL,
    confidence REAL NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NOT NULL,
    apps_json TEXT NOT NULL,
    topics_json TEXT NOT NULL,
    segment_ids_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_activities_started_at ON activities(started_at);
