//! Battery state via UPower D-Bus.

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone)]
pub struct BatteryState {
    pub percent: u8,
    pub charging: bool,
}

impl Default for BatteryState {
    fn default() -> Self {
        Self { percent: 100, charging: false }
    }
}

pub struct BatteryService;

impl BatteryService {
    /// Start the service. Returns a GPUI entity that updates on battery events.
    pub fn start(cx: &mut App) -> Entity<BatteryState> {
        let entity = cx.new(|_| BatteryState::default());
        let weak = entity.downgrade();

        cx.spawn(async move |cx| {
            // TODO: connect to UPower via zbus and stream property changes.
            //
            // This is the highest-priority service TODO because battery state
            // changes frequently and users expect live updates.
            //
            // Preferred approach (event-driven):
            //   let conn = zbus::Connection::system().await?;
            //   let proxy = UPowerDeviceProxy::new(&conn, battery_path).await?;
            //   let mut stream = proxy.receive_percentage_changed().await;
            //   while let Some(change) = stream.next().await { ... }
            //
            // After updating the entity, the runtime bridge (api/services.rs)
            // needs to propagate the change to a LuaState so shell.services.battery
            // reflects the new value — see the services.rs TODO comment.

            // Poll sysfs every 30 s
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;

                let percent = read_sysfs_percent();
                let charging = read_sysfs_charging();
                cx.update(|cx| {
                    if let Some(e) = weak.upgrade() {
                        e.update(cx, |state, cx| {
                            state.percent = percent;
                            state.charging = charging;
                            cx.notify();
                        });
                    }
                });
            }
        })
        .detach();

        entity
    }
}

/// Parse a sysfs capacity string (e.g. "75\n") into a percentage.
/// Returns 100 on parse failure.
pub(crate) fn parse_capacity_str(s: &str) -> u8 {
    s.trim().parse().unwrap_or(100)
}

/// Parse a sysfs status string (e.g. "Charging\n") into a charging bool.
pub(crate) fn parse_status_str(s: &str) -> bool {
    s.trim() == "Charging"
}

fn read_sysfs_percent() -> u8 {
    std::fs::read_to_string("/sys/class/power_supply/BAT0/capacity")
        .map(|s| parse_capacity_str(&s))
        .unwrap_or(100)
}

fn read_sysfs_charging() -> bool {
    std::fs::read_to_string("/sys/class/power_supply/BAT0/status")
        .map(|s| parse_status_str(&s))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_capacity_str ---

    #[test]
    fn capacity_parses_plain_number() {
        assert_eq!(parse_capacity_str("75"), 75);
    }

    #[test]
    fn capacity_parses_with_trailing_newline() {
        assert_eq!(parse_capacity_str("42\n"), 42);
    }

    #[test]
    fn capacity_parses_zero() {
        assert_eq!(parse_capacity_str("0"), 0);
    }

    #[test]
    fn capacity_parses_full() {
        assert_eq!(parse_capacity_str("100"), 100);
    }

    #[test]
    fn capacity_fallback_on_empty() {
        assert_eq!(parse_capacity_str(""), 100);
    }

    #[test]
    fn capacity_fallback_on_garbage() {
        assert_eq!(parse_capacity_str("unknown"), 100);
    }

    #[test]
    fn capacity_fallback_on_float() {
        // sysfs always gives integers; floats are invalid and should fall back
        assert_eq!(parse_capacity_str("75.5"), 100);
    }

    #[test]
    fn capacity_strips_surrounding_whitespace() {
        assert_eq!(parse_capacity_str("  88  "), 88);
    }

    // --- parse_status_str ---

    #[test]
    fn status_charging_returns_true() {
        assert!(parse_status_str("Charging"));
    }

    #[test]
    fn status_charging_with_newline() {
        assert!(parse_status_str("Charging\n"));
    }

    #[test]
    fn status_discharging_returns_false() {
        assert!(!parse_status_str("Discharging\n"));
    }

    #[test]
    fn status_full_returns_false() {
        assert!(!parse_status_str("Full\n"));
    }

    #[test]
    fn status_not_charging_returns_false() {
        assert!(!parse_status_str("Not charging"));
    }

    #[test]
    fn status_empty_returns_false() {
        assert!(!parse_status_str(""));
    }

    #[test]
    fn status_case_sensitive_lowercase_false() {
        assert!(!parse_status_str("charging"));
    }

    // --- BatteryState ---

    #[test]
    fn battery_state_default_values() {
        let s = BatteryState::default();
        assert_eq!(s.percent, 100);
        assert!(!s.charging);
    }

    #[test]
    fn battery_state_clone_is_independent() {
        let a = BatteryState { percent: 50, charging: true };
        let mut b = a.clone();
        b.percent = 99;
        assert_eq!(a.percent, 50);
        assert_eq!(b.percent, 99);
    }
}
