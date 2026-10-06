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

/// The three macOS grants, as data instead of three copy-pasted match arms.
/// Doctor/status derive names, Settings panes, and attach semantics from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    InputMonitoring,
    Accessibility,
    ScreenRecording,
}

impl Grant {
    pub fn all() -> [Grant; 3] {
        [
            Grant::InputMonitoring,
            Grant::Accessibility,
            Grant::ScreenRecording,
        ]
    }

    pub fn name(self) -> &'static str {
        match self {
            Grant::InputMonitoring => "input monitoring",
            Grant::Accessibility => "accessibility",
            Grant::ScreenRecording => "screen recording",
        }
    }

    /// D-13: previously-denied grants lead here, never to another dialog.
    /// Pane fragments are the `Privacy_*` anchors System Settings honors.
    pub fn settings_url(self) -> &'static str {
        match self {
            Grant::InputMonitoring => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
            }
            Grant::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            Grant::ScreenRecording => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
        }
    }

    /// D-15: where macOS demands a process restart before a fresh grant
    /// takes effect. Screen Recording is restart-required (the capture
    /// pipeline reads the grant at stream/process scope); the tap-based
    /// paths re-attach in-process.
    pub fn restart_required_on_attach(self) -> bool {
        matches!(self, Grant::ScreenRecording)
    }

    pub fn state(self, perms: &PermissionsState) -> PermissionState {
        match self {
            Grant::InputMonitoring => perms.input_monitoring,
            Grant::Accessibility => perms.accessibility,
            Grant::ScreenRecording => perms.screen_recording,
        }
    }
}

/// D-14: which capture paths are live under a permission state. Keyboard
/// needs Input Monitoring; screenshots need Screen Recording. Accessibility
/// loss degrades window-title quality (named in the banner) but disables no
/// thread — the focus loop already skips event-less polls gracefully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathLiveness {
    pub keyboard: bool,
    pub screenshots: bool,
}

pub fn paths_live(perms: &PermissionsState) -> PathLiveness {
    PathLiveness {
        keyboard: perms.input_monitoring == PermissionState::Granted,
        screenshots: perms.screen_recording == PermissionState::Granted,
    }
}

/// D-13 preflight plan over already-classified state: the grant names to
/// request EXACTLY once each. Pure — the daemon performs the requests,
/// persists the flags, and never asks again.
pub fn grants_to_request(
    perms: &PermissionsState,
    prompted: &crate::PermissionsConfig,
) -> Vec<Grant> {
    Grant::all()
        .into_iter()
        .filter(|g| {
            let was_prompted = match g {
                Grant::InputMonitoring => prompted.prompted_input_monitoring,
                Grant::Accessibility => prompted.prompted_accessibility,
                Grant::ScreenRecording => prompted.prompted_screen_recording,
            };
            wants_prompt(g.state(perms), was_prompted)
        })
        .collect()
}

/// D-15 re-probe transition for one grant: what the daemon must do when a
/// 30 s check observes a state change. Pure over injected old/new states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantTransition {
    /// Freshly granted where the API permits in-process attach.
    HotAttach(Grant),
    /// Freshly granted where macOS demands a process restart first.
    RestartRequired(Grant),
    /// Grant vanished mid-run: stop the path now.
    Detached(Grant),
    /// No change, or a change with no path action (e.g. denied → denied).
    None,
}

pub fn grant_transition(grant: Grant, old: PermissionState, new: PermissionState) -> GrantTransition {
    if old == new {
        return GrantTransition::None;
    }
    if new == PermissionState::Granted {
        if grant.restart_required_on_attach() {
            return GrantTransition::RestartRequired(grant);
        }
        return GrantTransition::HotAttach(grant);
    }
    if old == PermissionState::Granted {
        return GrantTransition::Detached(grant);
    }
    GrantTransition::None
}

/// All grant transitions between two snapshots, in Grant::all() order.
pub fn diff_permissions(old: &PermissionsState, new: &PermissionsState) -> Vec<GrantTransition> {
    Grant::all()
        .into_iter()
        .map(|g| grant_transition(g, g.state(old), g.state(new)))
        .filter(|t| *t != GrantTransition::None)
        .collect()
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

    fn granted_all() -> PermissionsState {
        PermissionsState {
            input_monitoring: PermissionState::Granted,
            accessibility: PermissionState::Granted,
            screen_recording: PermissionState::Granted,
        }
    }

    /// D-14 degradation matrix: each grant missing in isolation keeps the
    /// other paths live.
    #[test]
    fn degradation_matrix_is_per_path() {
        assert_eq!(
            paths_live(&granted_all()),
            PathLiveness { keyboard: true, screenshots: true }
        );
        // No Input Monitoring: keyboard dead, screenshots live.
        let no_input = PermissionsState {
            input_monitoring: PermissionState::Denied,
            ..granted_all()
        };
        assert_eq!(
            paths_live(&no_input),
            PathLiveness { keyboard: false, screenshots: true }
        );
        // No Screen Recording: screenshots dead, keyboard live.
        let no_screen = PermissionsState {
            screen_recording: PermissionState::Denied,
            ..granted_all()
        };
        assert_eq!(
            paths_live(&no_screen),
            PathLiveness { keyboard: true, screenshots: false }
        );
        // No Accessibility: both threads live (titles may degrade).
        let no_ax = PermissionsState {
            accessibility: PermissionState::Denied,
            ..granted_all()
        };
        assert_eq!(
            paths_live(&no_ax),
            PathLiveness { keyboard: true, screenshots: true }
        );
        // Undetermined disables exactly like denied (the path cannot run).
        let undetermined = PermissionsState::default();
        assert_eq!(
            paths_live(&undetermined),
            PathLiveness { keyboard: false, screenshots: false }
        );
    }

    /// D-13 prompt-once: first run requests every undetermined grant, the
    /// second run (flags persisted) requests nothing — including denials.
    #[test]
    fn preflight_requests_each_grant_exactly_once() {
        use crate::PermissionsConfig;
        let unknown = PermissionsState::default();
        let fresh = PermissionsConfig::default();
        assert_eq!(grants_to_request(&unknown, &fresh).len(), 3);

        // After the first request all flags persist: nothing left to ask.
        let prompted_all = PermissionsConfig {
            prompted_input_monitoring: true,
            prompted_accessibility: true,
            prompted_screen_recording: true,
        };
        assert!(grants_to_request(&unknown, &prompted_all).is_empty());
        // Denied-after-prompt never re-prompts either.
        let denied_all = PermissionsState {
            input_monitoring: PermissionState::Denied,
            accessibility: PermissionState::Denied,
            screen_recording: PermissionState::Denied,
        };
        assert!(grants_to_request(&denied_all, &prompted_all).is_empty());
        // Granted needs no prompt even if somehow never flagged.
        assert!(grants_to_request(&granted_all(), &fresh).is_empty());
    }

    /// D-15 re-probe transitions with injected probe results: hot-attach
    /// where the API permits, restart-required for Screen Recording, detach
    /// when a live grant vanishes, silence otherwise.
    #[test]
    fn reprobe_transitions_route_attach_detach() {
        use PermissionState::*;
        // Fresh grants: tap paths hot-attach, Screen Recording needs restart.
        assert_eq!(
            grant_transition(Grant::InputMonitoring, Undetermined, Granted),
            GrantTransition::HotAttach(Grant::InputMonitoring)
        );
        assert_eq!(
            grant_transition(Grant::Accessibility, Denied, Granted),
            GrantTransition::HotAttach(Grant::Accessibility)
        );
        assert_eq!(
            grant_transition(Grant::ScreenRecording, Undetermined, Granted),
            GrantTransition::RestartRequired(Grant::ScreenRecording)
        );
        // Lost grants detach their paths.
        assert_eq!(
            grant_transition(Grant::InputMonitoring, Granted, Denied),
            GrantTransition::Detached(Grant::InputMonitoring)
        );
        assert_eq!(
            grant_transition(Grant::ScreenRecording, Granted, Undetermined),
            GrantTransition::Detached(Grant::ScreenRecording)
        );
        // Non-events stay silent.
        assert_eq!(grant_transition(Grant::InputMonitoring, Granted, Granted), GrantTransition::None);
        assert_eq!(grant_transition(Grant::InputMonitoring, Denied, Denied), GrantTransition::None);
        assert_eq!(
            grant_transition(Grant::Accessibility, Undetermined, Denied),
            GrantTransition::None
        );
    }

    /// Full-snapshot diff: simultaneous changes surface in grant order.
    #[test]
    fn snapshot_diff_collects_simultaneous_changes() {
        let old = granted_all();
        let new = PermissionsState {
            input_monitoring: PermissionState::Denied,
            screen_recording: PermissionState::Denied,
            ..granted_all()
        };
        assert_eq!(
            diff_permissions(&old, &new),
            vec![
                GrantTransition::Detached(Grant::InputMonitoring),
                GrantTransition::Detached(Grant::ScreenRecording),
            ]
        );
        assert!(diff_permissions(&old, &old).is_empty());
    }

    /// Every grant resolves to a System Settings pane URL.
    #[test]
    fn every_grant_has_a_settings_pane() {
        for grant in Grant::all() {
            let url = grant.settings_url();
            assert!(
                url.starts_with("x-apple.systempreferences:"),
                "{} has no Settings URL",
                grant.name()
            );
        }
    }
}
