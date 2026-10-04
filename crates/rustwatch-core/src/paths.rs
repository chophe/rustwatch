use std::path::{Path, PathBuf};

use anyhow::Context;
use directories::ProjectDirs;

use crate::Result;

pub struct DataPaths {
    pub root: PathBuf,
    pub sqlite: PathBuf,
    pub lance: PathBuf,
    pub surreal: PathBuf,
    pub screenshots: PathBuf,
    pub config: PathBuf,
    pub socket: PathBuf,
    pub pid_file: PathBuf,
}

impl DataPaths {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let root = match root {
            Some(path) => expand_tilde(path),
            None => default_root()?,
        };

        Ok(Self {
            sqlite: root.join("rustwatch.db"),
            lance: root.join("lance"),
            surreal: root.join("surreal"),
            screenshots: root.join("screenshots"),
            config: root.join("config.toml"),
            socket: root.join("daemon.sock"),
            pid_file: root.join("daemon.pid"),
            root,
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root).map_err(crate::Error::from)?;
        // T-01-03: the data root holds full keystroke/screen history — owner-only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))
                .map_err(crate::Error::from)?;
        }
        std::fs::create_dir_all(&self.lance).map_err(crate::Error::from)?;
        std::fs::create_dir_all(&self.surreal).map_err(crate::Error::from)?;
        std::fs::create_dir_all(&self.screenshots).map_err(crate::Error::from)?;
        Ok(())
    }

    pub fn screenshot_dir_for_date(&self, date: chrono::NaiveDate) -> PathBuf {
        self.screenshots.join(date.format("%Y-%m-%d").to_string())
    }
}

fn default_root() -> Result<PathBuf> {
    if let Some(dirs) = ProjectDirs::from("com", "chophe", "rustwatch") {
        return Ok(dirs.data_dir().to_path_buf());
    }
    Ok(expand_tilde(PathBuf::from("~/.rustwatch")))
}

pub fn expand_tilde(path: PathBuf) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path;
    };
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return path;
    };
    if raw == "~" {
        return home;
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    path
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::Error::from)?;
    }
    Ok(())
}

pub fn load_or_create_config(path: &Path) -> anyhow::Result<crate::Config> {
    if path.exists() {
        let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        for warning in unknown_config_keys(&raw) {
            eprintln!("warning: {warning} (in {})", path.display());
        }
        return Ok(toml::from_str(&raw)?);
    }
    let config = crate::Config::default();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(&config)?)?;
    Ok(config)
}

/// SYS-01: every `config.toml` section/key not in the schema. Unknown keys
/// warn at startup instead of failing or being silently ignored, so removed
/// fields ([data], [ui], …) and future keys never brick startup.
pub fn unknown_config_keys(raw: &str) -> Vec<String> {
    const TOP_LEVEL: &[&str] = &[
        "capture",
        "analyze",
        "memory",
        "privacy",
        "permissions",
    ];
    fn section_keys(section: &str) -> &'static [&'static str] {
        match section {
            "capture" => &[
                "poll_focus_ms",
                "exclude_apps",
                "screenshot_on_focus_change",
                "screenshot_interval_secs",
                "min_interval_secs",
                "idle_start_secs",
                "idle_end_sustained_secs",
                "hotkey_enabled",
                "hotkey_chord",
            ],
            "analyze" => &["provider", "model"],
            "memory" => &["embedding_model", "chunk_max_chars", "graph_expand_hops"],
            "privacy" => &["redact_patterns"],
            "permissions" => &[
                "prompted_input_monitoring",
                "prompted_accessibility",
                "prompted_screen_recording",
            ],
            _ => &[],
        }
    }

    let Ok(value) = toml::from_str::<toml::Value>(raw) else {
        return Vec::new();
    };
    let Some(table) = value.as_table() else {
        return Vec::new();
    };
    let mut warnings = Vec::new();
    for (section, body) in table {
        if !TOP_LEVEL.contains(&section.as_str()) {
            warnings.push(format!("unknown config section [{section}]"));
            continue;
        }
        if let Some(sub) = body.as_table() {
            for key in sub.keys() {
                if !section_keys(section).contains(&key.as_str()) {
                    warnings.push(format!("unknown config key {section}.{key}"));
                }
            }
        }
    }
    warnings
}
