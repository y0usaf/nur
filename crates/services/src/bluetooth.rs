//! Bluetooth service via `bluetoothctl`.
//!
//! Polls adapter state and paired/connected devices every 5 s using
//! `bluetoothctl` show` and `bluetoothctl devices`. A dedicated OS thread runs
//! the blocking calls; a GPUI task drains the shared slot.
//!
//! Action functions (`connect`, `disconnect`, `toggle_power`, `start_scan`,
//! `stop_scan`) spawn fire-and-forget threads.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone, Default)]
pub struct BluetoothDevice {
    pub name: String,
    pub address: String,
    pub connected: bool,
    pub paired: bool,
    pub icon: String,
}

#[derive(Debug, Clone)]
pub struct BluetoothState {
    /// Whether the Bluetooth adapter is powered on.
    pub enabled: bool,
    /// Whether discovery/scanning is active.
    pub discovering: bool,
    pub devices: Vec<BluetoothDevice>,
}

impl Default for BluetoothState {
    fn default() -> Self {
        Self {
            enabled: false,
            discovering: false,
            devices: vec![],
        }
    }
}

pub struct BluetoothService;

impl BluetoothService {
    pub fn start(cx: &mut App) -> Entity<BluetoothState> {
        let entity = cx.new(|_| BluetoothState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<BluetoothState>>> = Arc::new(Mutex::new(None));
        let slot_writer = slot.clone();
        let slot_reader = slot.clone();

        std::thread::spawn(move || {
            loop {
                let state = read_bluetooth_state();
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

fn btctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("bluetoothctl")
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

/// Parse `bluetoothctl show` output to extract `Powered` and `Discovering`.
pub(crate) fn parse_adapter_show(s: &str) -> (bool, bool) {
    let powered = s.lines().any(|l| l.trim() == "Powered: yes");
    let discovering = s.lines().any(|l| l.trim() == "Discovering: yes");
    (powered, discovering)
}

/// Parse `bluetoothctl devices` output (one `"Device AA:BB:CC:DD:EE:FF Name"` per line).
/// Returns a list of `(address, name)` pairs.
pub(crate) fn parse_devices_list(s: &str) -> Vec<(String, String)> {
    s.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() == 3 && parts[0] == "Device" {
                Some((parts[1].to_string(), parts[2].to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Parse `bluetoothctl info <addr>` output to extract Connected, Paired, Icon.
pub(crate) fn parse_device_info(s: &str) -> (bool, bool, String) {
    let connected = s.lines().any(|l| l.trim() == "Connected: yes");
    let paired = s.lines().any(|l| l.trim() == "Paired: yes");
    let icon = s
        .lines()
        .find(|l| l.trim().starts_with("Icon:"))
        .and_then(|l| l.trim().strip_prefix("Icon:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    (connected, paired, icon)
}

fn read_bluetooth_state() -> BluetoothState {
    let show_output = btctl(&["show"]).unwrap_or_default();
    let (enabled, discovering) = parse_adapter_show(&show_output);

    let devices_output = btctl(&["devices"]).unwrap_or_default();
    let pairs = parse_devices_list(&devices_output);

    let mut devices = Vec::new();
    for (address, name) in pairs {
        let info_out = btctl(&["info", &address]).unwrap_or_default();
        let (connected, paired, icon) = parse_device_info(&info_out);
        devices.push(BluetoothDevice {
            name,
            address,
            connected,
            paired,
            icon,
        });
    }

    BluetoothState {
        enabled,
        discovering,
        devices,
    }
}

fn btctl_command(args: Vec<String>) {
    std::thread::spawn(move || {
        let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
        match std::process::Command::new("bluetoothctl")
            .args(&args_ref)
            .status()
        {
            Ok(s) if !s.success() => tracing::warn!("bluetoothctl {:?} exited {s}", args_ref),
            Err(e) => tracing::warn!("bluetoothctl {:?} failed: {e}", args_ref),
            _ => {}
        }
    });
}

/// Toggle the Bluetooth adapter power. Fire-and-forget.
pub fn toggle_power(currently_enabled: bool) {
    let val = if currently_enabled { "off" } else { "on" };
    btctl_command(vec!["power".to_string(), val.to_string()]);
}

/// Connect to a Bluetooth device by address. Fire-and-forget.
pub fn connect(address: String) {
    btctl_command(vec!["connect".to_string(), address]);
}

/// Disconnect a Bluetooth device by address. Fire-and-forget.
pub fn disconnect(address: String) {
    btctl_command(vec!["disconnect".to_string(), address]);
}

/// Start Bluetooth discovery. Fire-and-forget.
pub fn start_scan() {
    btctl_command(vec!["scan".to_string(), "on".to_string()]);
}

/// Stop Bluetooth discovery. Fire-and-forget.
pub fn stop_scan() {
    btctl_command(vec!["scan".to_string(), "off".to_string()]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_adapter_show_powered_and_discovering() {
        let s = "Controller AA:BB:CC:DD:EE:FF (public)\n\tName: myhost\n\tPowered: yes\n\tDiscoverable: no\n\tDiscovering: yes\n";
        let (powered, discovering) = parse_adapter_show(s);
        assert!(powered);
        assert!(discovering);
    }

    #[test]
    fn parse_adapter_show_off() {
        let s = "\tPowered: no\n\tDiscovering: no\n";
        let (powered, discovering) = parse_adapter_show(s);
        assert!(!powered);
        assert!(!discovering);
    }

    #[test]
    fn parse_adapter_show_empty() {
        let (powered, discovering) = parse_adapter_show("");
        assert!(!powered);
        assert!(!discovering);
    }

    #[test]
    fn parse_devices_list_single() {
        let s = "Device AA:BB:CC:DD:EE:FF AirPods Pro\n";
        let pairs = parse_devices_list(s);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, "AA:BB:CC:DD:EE:FF");
        assert_eq!(pairs[0].1, "AirPods Pro");
    }

    #[test]
    fn parse_devices_list_multiple() {
        let s = "Device 11:22:33:44:55:66 Speaker\nDevice AA:BB:CC:DD:EE:FF Mouse\n";
        let pairs = parse_devices_list(s);
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn parse_devices_list_empty() {
        assert!(parse_devices_list("").is_empty());
    }

    #[test]
    fn parse_devices_list_skips_non_device_lines() {
        let s = "Controller blah blah\nDevice AA:BB:CC:DD:EE:FF Headset\n";
        let pairs = parse_devices_list(s);
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn parse_device_info_connected_paired_icon() {
        let s = "\tName: AirPods\n\tAddress: AA:BB:CC:DD:EE:FF\n\tConnected: yes\n\tPaired: yes\n\tIcon: audio-headphones\n";
        let (connected, paired, icon) = parse_device_info(s);
        assert!(connected);
        assert!(paired);
        assert_eq!(icon, "audio-headphones");
    }

    #[test]
    fn parse_device_info_not_connected() {
        let s = "\tConnected: no\n\tPaired: yes\n\tIcon: input-mouse\n";
        let (connected, paired, icon) = parse_device_info(s);
        assert!(!connected);
        assert!(paired);
        assert_eq!(icon, "input-mouse");
    }

    #[test]
    fn parse_device_info_no_icon() {
        let s = "\tConnected: yes\n\tPaired: yes\n";
        let (connected, _paired, icon) = parse_device_info(s);
        assert!(connected);
        assert!(icon.is_empty());
    }

    #[test]
    fn bluetooth_state_default() {
        let s = BluetoothState::default();
        assert!(!s.enabled);
        assert!(!s.discovering);
        assert!(s.devices.is_empty());
    }

    #[test]
    fn bluetooth_state_clone() {
        let a = BluetoothState {
            enabled: true,
            discovering: false,
            devices: vec![BluetoothDevice {
                name: "X".into(),
                address: "AA:BB:CC:DD:EE:FF".into(),
                connected: true,
                paired: true,
                icon: "".into(),
            }],
        };
        let b = a.clone();
        assert!(b.enabled);
        assert_eq!(b.devices.len(), 1);
    }

    #[test]
    fn toggle_power_does_not_panic_when_missing() {
        toggle_power(true);
        toggle_power(false);
    }
}
