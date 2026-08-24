use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub data: DataConfig,
    pub capture: CaptureConfig,
    pub analyze: AnalyzeConfig,
    pub memory: MemoryConfig,
    pub ui: UiConfig,
    pub privacy: PrivacyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataConfig {
    pub dir: PathBuf,
    pub sqlite_path: PathBuf,
    pub lance_path: PathBuf,
    pub surreal_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    pub poll_focus_ms: u64,
    pub accessibility_poll_ms: u64,
    pub exclude_apps: Vec<String>,
    pub screenshot_on_focus_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeConfig {
    pub provider: String,
    pub model: String,
    pub vision_model: String,
    pub batch_interval_minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub vector_backend: String,
    pub graph_backend: String,
    pub surreal_engine: String,
    pub embedding_model: String,
    pub chunk_max_chars: usize,
    pub graph_expand_hops: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub tui_enabled: bool,
    pub progress_bars: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub redact_patterns: Vec<String>,
    pub send_screenshots_to_llm: bool,
}

impl Default for Config {
    fn default() -> Self {
        let root = PathBuf::from("~/.rustwatch");
        Self {
            data: DataConfig {
                dir: root.clone(),
                sqlite_path: root.join("rustwatch.db"),
                lance_path: root.join("lance"),
                surreal_path: root.join("surreal"),
            },
            capture: CaptureConfig {
                poll_focus_ms: 500,
                accessibility_poll_ms: 2000,
                exclude_apps: vec![
                    "1Password".into(),
                    "Keychain Access".into(),
                ],
                screenshot_on_focus_change: true,
            },
            analyze: AnalyzeConfig {
                provider: "openai".into(),
                model: "gpt-4o-mini".into(),
                vision_model: "gpt-4o".into(),
                batch_interval_minutes: 10,
            },
            memory: MemoryConfig {
                vector_backend: "lancedb".into(),
                graph_backend: "surrealdb".into(),
                surreal_engine: "surrealkv".into(),
                embedding_model: "BGE-small-en-v1.5".into(),
                chunk_max_chars: 2000,
                graph_expand_hops: 2,
            },
            ui: UiConfig {
                tui_enabled: true,
                progress_bars: true,
            },
            privacy: PrivacyConfig {
                redact_patterns: vec![r"sk-[A-Za-z0-9]+".into()],
                send_screenshots_to_llm: true,
            },
        }
    }
}

impl Config {
    pub fn paths(&self) -> crate::Result<crate::DataPaths> {
        crate::DataPaths::new(Some(self.data.dir.clone()))
    }
}
