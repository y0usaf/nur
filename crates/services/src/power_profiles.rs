//! Power profiles service via `powerprofilesctl`.
//!
//! Polls the active power profile every 5 s using `powerprofilesctl get` and
//! the available profiles via `powerprofilesctl list`. A dedicated OS thread
//! runs the blocking calls; a GPUI task drains the shared slot.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone)]
pub struct PowerProfilesState {
    /// Currently active profile, e.g. `"balanced"`.
    pub active: String,
    /// All available profiles, e.g. `["performance", "balanced", "power-saver"]`.
    pub profiles: Vec<String>,
}

impl Default for PowerProfilesState {
    fn default() -> Self {
        Self {
            active: "balanced".to_string(),
            profiles: vec![
                "performance".to_string(),
                "balanced".to_string(),
                "power-saver".to_string(),
            ],
        }
    }
}

pub struct PowerProfilesService;

impl PowerProfilesService {
    pub fn start(cx: &mut App) -> Entity<PowerProfilesState> {
        let entity = cx.new(|_| PowerProfilesState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<PowerProfilesState>>> = Arc::new(Mutex::new(None));
        let slot_writer = slot.clone();
        let slot_reader = slot.clone();

        std::thread::spawn(move || {
            // Read initial state immediately, then poll every 5 s.
            loop {
                let state = read_power_profiles_state();
                if let Ok(mut guard) = slot_writer.lock() {
                    *guard = Some(state);
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });

        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;

                let state = slot_reader.lock().ok().and_then(|mut g| g.take());
                if let Some(state) = state {
                    cx.update(|cx| {
                        if let Some(e) = weak.upgrade() {
                            e.update(cx, |s, cx| {
                                *s = state;
                                cx.notify();
                            });
                        }
                    });
                }
            }
        })
        .detach();

        entity
    }
}

/// Parse `powerprofilesctl get` output (e.g. `"balanced\n"`) into a profile name.
pub(crate) fn parse_active_profile(s: &str) -> String {
    s.trim().to_string()
}

/// Parse `powerprofilesctl list` output into a list of profile names.
///
/// Each profile appears as a block starting with `"* <name>:"` (active) or
/// `"  <name>:"` (inactive). We collect the name token from each such line.
///
/// Example output:
/// ```text
/// * balanced:
///     Driver:     placeholder
///     Degraded:   no
///
///   power-saver:
///     Driver:     placeholder
///
///   performance:
///     Driver:     placeholder
/// ```
pub(crate) fn parse_profiles_list(s: &str) -> Vec<String> {
    let mut profiles = Vec::new();
    for line in s.lines() {
        let trimmed = line.trim_start_matches('*').trim();
        // Profile header lines end with ':'
        if let Some(name) = trimmed.strip_suffix(':') {
            let name = name.trim();
            if !name.is_empty() && !name.contains(' ') {
                profiles.push(name.to_string());
            }
        }
    }
    profiles
}

fn read_power_profiles_state() -> PowerProfilesState {
    let active = read_active_profile();
    let profiles = read_profiles_list();
    PowerProfilesState { active, profiles }
}

fn read_active_profile() -> String {
    let output = std::process::Command::new("powerprofilesctl")
        .arg("get")
        .output();
    match output {
        Ok(o) if o.status.success() => parse_active_profile(&String::from_utf8_lossy(&o.stdout)),
        _ => "balanced".to_string(),
    }
}

fn read_profiles_list() -> Vec<String> {
    let output = std::process::Command::new("powerprofilesctl")
        .arg("list")
        .output();
    match output {
        Ok(o) if o.status.success() => parse_profiles_list(&String::from_utf8_lossy(&o.stdout)),
        _ => vec![
            "performance".to_string(),
            "balanced".to_string(),
            "power-saver".to_string(),
        ],
    }
}

/// Set the active power profile via `powerprofilesctl`. Fire-and-forget.
pub fn set_profile(profile: String) {
    std::thread::spawn(move || {
        match std::process::Command::new("powerprofilesctl")
            .args(["set", &profile])
            .status()
        {
            Ok(s) if !s.success() => tracing::warn!("powerprofilesctl set {profile} exited {s}"),
            Err(e) => tracing::warn!("powerprofilesctl set {profile} failed: {e}"),
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_active_trims_whitespace() {
        assert_eq!(parse_active_profile("balanced\n"), "balanced");
        assert_eq!(parse_active_profile("  performance  "), "performance");
        assert_eq!(parse_active_profile("power-saver"), "power-saver");
    }

    #[test]
    fn parse_profiles_list_standard_output() {
        let output = "* balanced:\n    Driver:     placeholder\n    Degraded:   no\n\n  power-saver:\n    Driver:     placeholder\n\n  performance:\n    Driver:     placeholder\n";
        let profiles = parse_profiles_list(output);
        assert!(profiles.contains(&"balanced".to_string()));
        assert!(profiles.contains(&"power-saver".to_string()));
        assert!(profiles.contains(&"performance".to_string()));
        assert_eq!(profiles.len(), 3);
    }

    #[test]
    fn parse_profiles_list_empty_returns_empty() {
        assert!(parse_profiles_list("").is_empty());
    }

    #[test]
    fn parse_profiles_list_skips_driver_lines() {
        let output = "* balanced:\n    Driver:     something\n";
        let profiles = parse_profiles_list(output);
        // "Driver:     something" — the name part before ':' contains spaces, skip it
        assert_eq!(profiles, vec!["balanced"]);
    }

    #[test]
    fn power_profiles_state_default() {
        let s = PowerProfilesState::default();
        assert_eq!(s.active, "balanced");
        assert_eq!(s.profiles.len(), 3);
    }

    #[test]
    fn power_profiles_state_clone() {
        let a = PowerProfilesState {
            active: "performance".to_string(),
            profiles: vec!["performance".to_string()],
        };
        let b = a.clone();
        assert_eq!(b.active, "performance");
    }

    #[test]
    fn set_profile_does_not_panic_when_missing() {
        set_profile("balanced".to_string());
    }
}
