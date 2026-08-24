use regex::Regex;
use rustwatch_core::Config;

pub struct Redactor {
    patterns: Vec<Regex>,
    exclude_apps: Vec<String>,
    max_chars: usize,
}

impl Redactor {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let patterns = config
            .privacy
            .redact_patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            patterns,
            exclude_apps: config.capture.exclude_apps.clone(),
            max_chars: config.memory.chunk_max_chars,
        })
    }

    pub fn scrub(&self, input: &str) -> String {
        let mut out = input.to_string();
        for pattern in &self.patterns {
            out = pattern.replace_all(&out, "[REDACTED]").to_string();
        }
        if out.len() > self.max_chars {
            out.truncate(self.max_chars);
        }
        out
    }

    pub fn is_excluded_app(&self, app_name: &str) -> bool {
        self.exclude_apps
            .iter()
            .any(|app| app_name.contains(app))
    }
}
