use serde::{Deserialize, Serialize};

/// D-13: three states per macOS grant. `Denied` vs `Undetermined` is a real
/// distinction, not pedantry — only undetermined grants may be prompted.
/// Re-prompting a denied grant spams system dialogs and risks TCC throttling
/// (Pitfall 6), so denied grants lead to System Settings, never re-prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionState {
    Granted,
    Denied,
    #[default]
    Undetermined,
}

impl PermissionState {
    pub fn label(self) -> &'static str {
        match self {
            PermissionState::Granted => "granted",
            PermissionState::Denied => "denied",
            PermissionState::Undetermined => "undetermined",
        }
    }
}

/// Wire type for per-grant state: daemon populates it at startup (and the
/// 01-03 re-probe loop refreshes it), CLI/TUI/doctor only render it. Added
/// with `#[serde(default)]` everywhere so older daemon replies still parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionsState {
    pub input_monitoring: PermissionState,
    pub accessibility: PermissionState,
    pub screen_recording: PermissionState,
}

impl PermissionsState {
    pub fn all_granted(&self) -> bool {
        self.input_monitoring == PermissionState::Granted
            && self.accessibility == PermissionState::Granted
            && self.screen_recording == PermissionState::Granted
    }

    /// D-16 banner fragment: quiet when everything is granted, explicit about
    /// exactly which grant is missing and in which state otherwise. Partial
    /// capture must never look like full capture.
    pub fn banner(&self) -> String {
        if self.all_granted() {
            return "permissions: all granted".to_string();
        }
        let mut parts = Vec::new();
        for (name, state) in [
            ("input monitoring", self.input_monitoring),
            ("accessibility", self.accessibility),
            ("screen recording", self.screen_recording),
        ] {
            if state != PermissionState::Granted {
                parts.push(format!("{name}: {}", state.label()));
            }
        }
        format!("permissions: degraded ({})", parts.join(", "))
    }
}

/// Pure mapping from a raw granted/not-granted probe plus our own
/// `prompted_*` bookkeeping. The OS check APIs answer only granted or not —
/// they cannot tell "denied" from "never asked" — so the distinction comes
/// from whether we already prompted once: not-granted + never-prompted is
/// still undetermined; not-granted + already-prompted means denied.
pub fn classify_grant(probed_granted: bool, prompted: bool) -> PermissionState {
    if probed_granted {
        PermissionState::Granted
    } else if prompted {
        PermissionState::Denied
    } else {
        PermissionState::Undetermined
    }
}

/// D-13/Pitfall 6 gate: request ONLY undetermined grants that were never
/// prompted. Everything else — granted, denied, already-prompted — never
/// triggers a native dialog. The 30 s re-probe loop only CHECKS.
pub fn wants_prompt(state: PermissionState, prompted: bool) -> bool {
    state == PermissionState::Undetermined && !prompted
}

#[cfg(test)]
mod permissions_tests {
    use super::*;

    /// granted stays granted regardless of bookkeeping.
    #[test]
    fn granted_probe_always_granted() {
        assert_eq!(classify_grant(true, false), PermissionState::Granted);
        assert_eq!(classify_grant(true, true), PermissionState::Granted);
    }

    /// The core D-13 distinction: the same `false` probe reads differently
    /// depending on whether we already asked once.
    #[test]
    fn unprompted_false_is_undetermined_prompted_false_is_denied() {
        assert_eq!(classify_grant(false, false), PermissionState::Undetermined);
        assert_eq!(classify_grant(false, true), PermissionState::Denied);
    }

    /// Only undetermined-never-prompted opens a native dialog.
    #[test]
    fn prompt_gate_fires_exactly_once_per_grant() {
        assert!(wants_prompt(PermissionState::Undetermined, false));
        assert!(!wants_prompt(PermissionState::Undetermined, true));
        assert!(!wants_prompt(PermissionState::Denied, false));
        assert!(!wants_prompt(PermissionState::Denied, true));
        assert!(!wants_prompt(PermissionState::Granted, false));
        assert!(!wants_prompt(PermissionState::Granted, true));
    }

    #[test]
    fn banner_quiet_when_all_granted() {
        let state = PermissionsState {
            input_monitoring: PermissionState::Granted,
            accessibility: PermissionState::Granted,
            screen_recording: PermissionState::Granted,
        };
        assert_eq!(state.banner(), "permissions: all granted");
    }

    /// Each missing grant in isolation names exactly its own path.
    #[test]
    fn banner_names_each_degraded_path() {
        let denied_screen = PermissionsState {
            screen_recording: PermissionState::Denied,
            ..PermissionsState {
                input_monitoring: PermissionState::Granted,
                accessibility: PermissionState::Granted,
                ..Default::default()
            }
        };
        assert_eq!(
            denied_screen.banner(),
            "permissions: degraded (screen recording: denied)"
        );

        let undetermined_input = PermissionsState {
            input_monitoring: PermissionState::Undetermined,
            accessibility: PermissionState::Granted,
            screen_recording: PermissionState::Granted,
        };
        assert_eq!(
            undetermined_input.banner(),
            "permissions: degraded (input monitoring: undetermined)"
        );

        let all_missing = PermissionsState::default();
        assert_eq!(
            all_missing.banner(),
            "permissions: degraded (input monitoring: undetermined, accessibility: undetermined, screen recording: undetermined)"
        );
    }

    /// Old daemon replies without the field still parse (wire compat).
    #[test]
    fn missing_permissions_field_defaults_to_undetermined() {
        let raw = r#"{"running":true,"paused":false,"pid":null,"events_captured":0,"segments_written":0,"started_at":null}"#;
        let state: crate::DaemonState = serde_json::from_str(raw).expect("old reply parses");
        assert_eq!(state.permissions, PermissionsState::default());
        assert!(!state.permissions.all_granted());
    }
}
