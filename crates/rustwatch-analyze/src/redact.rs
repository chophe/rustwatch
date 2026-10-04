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
            // CAPT-05: floor to the char boundary — byte truncation panics
            // on CJK/emoji (same idiom as segment.rs).
            let mut end = self.max_chars;
            while end > 0 && !out.is_char_boundary(end) {
                end -= 1;
            }
            out.truncate(end);
        }
        out
    }

    pub fn is_excluded_app(&self, app_name: &str) -> bool {
        self.exclude_apps
            .iter()
            .any(|app| app_name.contains(app))
    }
}

#[cfg(test)]
mod redact_tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new(&Config::default()).expect("default config redacts")
    }

    /// CAPT-05: emoji/CJK floods never panic the truncation path and the
    /// result stays within budget on a char boundary.
    #[test]
    fn emoji_flood_truncates_on_char_boundary() {
        let redactor = redactor();
        let flood = "😀".repeat(3000) + &"日本語テスト".repeat(500);
        let out = redactor.scrub(&flood);
        assert!(out.len() <= redactor.max_chars);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn cjk_paste_survives_scrub_intact_when_short() {
        let redactor = redactor();
        let input = "パスワード secret sk-abc123 トークン 🔑";
        let out = redactor.scrub(input);
        assert!(out.contains("パスワード"));
        assert!(!out.contains("sk-abc123"));
    }
}
