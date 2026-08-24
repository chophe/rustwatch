use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppContext {
    pub app_name: String,
    pub window_title: String,
    pub process_id: u64,
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureEventKind {
    Key {
        key: String,
        modifiers: Vec<String>,
    },
    TextDelta {
        text: String,
    },
    Copy {
        content_hash: String,
    },
    Paste {
        content: String,
    },
    FocusChange {
        from: Option<AppContext>,
        to: AppContext,
    },
    TextFieldSnapshot {
        value: String,
    },
    Screenshot {
        path: PathBuf,
        scope: ScreenshotScope,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotScope {
    Window,
    Screen,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub app: Option<AppContext>,
    pub kind: CaptureEventKind,
}

impl CaptureEvent {
    pub fn new(kind: CaptureEventKind, app: Option<AppContext>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            app,
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSegment {
    pub id: String,
    pub app_name: String,
    pub window_title: String,
    pub process_id: u64,
    pub bundle_id: Option<String>,
    pub text_buffer: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub event_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityRecord {
    pub id: String,
    pub label: String,
    pub category: String,
    pub confidence: f32,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub apps: Vec<String>,
    pub topics: Vec<String>,
    pub segment_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotRecord {
    pub id: String,
    pub path: PathBuf,
    pub scope: ScreenshotScope,
    pub captured_at: DateTime<Utc>,
    pub segment_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLabel {
    pub label: String,
    pub category: String,
    pub confidence: f32,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub apps: Vec<String>,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentBatch {
    pub segments: Vec<SessionSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityBatchResponse {
    pub activities: Vec<ActivityLabel>,
}
