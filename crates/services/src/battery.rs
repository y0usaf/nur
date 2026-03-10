//! Battery state via UPower D-Bus.

use gpui::{App, AppContext, Entity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryState {
    pub percent: u8,
    pub charging: bool,
    pub time_remaining_secs: Option<u64>,
}

impl Default for BatteryState {
    fn default() -> Self {
        Self { percent: 100, charging: false, time_remaining_secs: None }
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

fn read_sysfs_percent() -> u8 {
    std::fs::read_to_string("/sys/class/power_supply/BAT0/capacity")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(100)
}

fn read_sysfs_charging() -> bool {
    std::fs::read_to_string("/sys/class/power_supply/BAT0/status")
        .ok()
        .map(|s| s.trim() == "Charging")
        .unwrap_or(false)
}
