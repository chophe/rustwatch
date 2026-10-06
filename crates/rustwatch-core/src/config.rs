use serde::{Deserialize, Serialize};

/// File-first configuration (`~/.rustwatch/config.toml`).
///
/// SYS-01 honesty contract: every field here is read somewhere (see SUMMARY
/// for the wire-vs-delete audit). Unknown keys only warn — they never fail
/// startup — so old files with removed sections keep loading.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub capture: CaptureConfig,
    pub analyze: AnalyzeConfig,
    pub memory: MemoryConfig,
    pub privacy: PrivacyConfig,
    pub permissions: PermissionsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub poll_focus_ms: u64,
    pub exclude_apps: Vec<String>,
    pub screenshot_on_focus_change: bool,
    /// CAPT-02: interval between automatic screenshots; 0 disables.
    pub screenshot_interval_secs: u64,
    /// D-09: window-change screenshot cooldown.
    pub min_interval_secs: u64,
    /// D-17/D-19: idle starts after this many input-free seconds.
    pub idle_start_secs: u64,
    /// D-19: idle ends only after this many sustained-activity seconds.
    pub idle_end_sustained_secs: u64,
    /// D-11: chord-on-tap hotkey master switch.
    pub hotkey_enabled: bool,
    /// D-11: hotkey chord; parsed by 01-02 (default Ctrl+Shift+Space).
    pub hotkey_chord: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            poll_focus_ms: 500,
            exclude_apps: vec!["1Password".into(), "Keychain Access".into()],
            screenshot_on_focus_change: true,
            screenshot_interval_secs: 300,
            min_interval_secs: 2,
            idle_start_secs: 300,
            idle_end_sustained_secs: 30,
            hotkey_enabled: true,
            hotkey_chord: "ctrl+shift+space".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzeConfig {
    pub provider: String,
    pub model: String,
}

impl Default for AnalyzeConfig {
    fn default() -> Self {
        Self {
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub embedding_model: String,
    pub chunk_max_chars: usize,
    pub graph_expand_hops: u8,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            embedding_model: "BGE-small-en-v1.5".into(),
            chunk_max_chars: 2000,
            graph_expand_hops: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    pub redact_patterns: Vec<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            redact_patterns: vec![r"sk-[A-Za-z0-9]+".into()],
        }
    }
}

/// D-13 request-once bookkeeping, consumed by the 01-03 preflight: the 30 s
/// re-probe loop only *checks*, and only requests grants never prompted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PermissionsConfig {
    pub prompted_input_monitoring: bool,
    pub prompted_accessibility: bool,
    pub prompted_screen_recording: bool,
}

#[cfg(test)]
mod config_tests {
    use super::*;

    /// Old files carrying removed sections ([data], [ui]) plus a future
    /// unknown key must still load — with warnings, never an error.
    #[test]
    fn removed_and_unknown_keys_warn_but_load() {
        let raw = r#"
[capture]
poll_focus_ms = 500

[data]
dir = "~/.rustwatch"

[ui]
tui_enabled = true

[future_section]
some_key = 1
"#;
        let warnings = crate::paths::unknown_config_keys(raw);
        assert!(
            warnings.iter().any(|w| w.contains("[data]")),
            "expected [data] warning, got {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("[ui]")),
            "expected [ui] warning, got {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("future_section")),
            "expected unknown-section warning, got {warnings:?}"
        );
        let config: Config = toml::from_str(raw).expect("old config must load");
        assert_eq!(config.capture.poll_focus_ms, 500);
        // Missing sections fall back to wired defaults.
        assert_eq!(config.capture.screenshot_interval_secs, 300);
        assert_eq!(config.capture.hotkey_chord, "ctrl+shift+space");
        assert!(!config.permissions.prompted_input_monitoring);
    }

    /// A clean default round-trips with zero warnings.
    #[test]
    fn default_config_is_warning_free() {
        let raw = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(
            crate::paths::unknown_config_keys(&raw).is_empty(),
            "default config warns: {raw}"
        );
    }
}
