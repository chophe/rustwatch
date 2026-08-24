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
        return Ok(toml::from_str(&raw)?);
    }
    let config = crate::Config::default();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, toml::to_string_pretty(&config)?)?;
    Ok(config)
}
