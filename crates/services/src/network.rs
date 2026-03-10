//! Network state via sysfs and nmcli.
//!
//! Polls every 5 s. Connection status comes from `/sys/class/net/*/operstate`;
//! WiFi SSID and signal strength come from `nmcli device wifi` when available.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    /// Whether any non-loopback interface is up and has a carrier.
    pub connected: bool,
    /// WiFi SSID, `None` when on ethernet or disconnected.
    pub ssid: Option<String>,
    /// WiFi signal strength 0–100, 0 when not on WiFi.
    pub strength: u8,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self { connected: true, ssid: None, strength: 100 }
    }
}

pub struct NetworkService;

impl NetworkService {
    pub fn start(cx: &mut App) -> Entity<NetworkState> {
        let entity = cx.new(|_| NetworkState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<NetworkState>>> = Arc::new(Mutex::new(None));
        let slot_writer = slot.clone();
        let slot_reader = slot.clone();

        // Polling thread — sysfs reads and nmcli are blocking.
        std::thread::spawn(move || loop {
            let state = read_network_state();
            if let Ok(mut guard) = slot_writer.lock() {
                *guard = Some(state);
            }
            std::thread::sleep(Duration::from_secs(5));
        });

        // GPUI task — picks up updates from the slot.
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(5))
                .await;

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
        })
        .detach();

        entity
    }
}

fn read_network_state() -> NetworkState {
    let connected = is_connected();
    let (ssid, strength) = if connected { read_wifi() } else { (None, 0) };
    NetworkState { connected, ssid, strength }
}

/// Check whether any non-loopback interface has an active carrier.
fn is_connected() -> bool {
    let Ok(dir) = std::fs::read_dir("/sys/class/net") else {
        return false;
    };
    dir.filter_map(|e| e.ok()).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "lo" {
            return false;
        }
        // operstate == "up" means the interface is connected.
        let operstate = format!("/sys/class/net/{name}/operstate");
        std::fs::read_to_string(&operstate)
            .map(|s| s.trim() == "up")
            .unwrap_or(false)
    })
}

/// Read the active WiFi SSID and signal strength via `nmcli`.
///
/// Parses `nmcli -t -f ACTIVE,SSID,SIGNAL device wifi` output.
/// Returns `(None, 0)` if nmcli is unavailable or no WiFi is active.
fn read_wifi() -> (Option<String>, u8) {
    let output = std::process::Command::new("nmcli")
        .args(["-t", "-f", "ACTIVE,SSID,SIGNAL", "device", "wifi"])
        .output();

    let Ok(o) = output else { return (None, 0) };
    if !o.status.success() {
        return (None, 0);
    }

    let stdout = String::from_utf8_lossy(&o.stdout);
    for line in stdout.lines() {
        // Format: "yes:MySSID:87" or "yes::0" (hidden SSID)
        let mut parts = line.splitn(3, ':');
        let active = parts.next().unwrap_or("");
        let ssid_raw = parts.next().unwrap_or("");
        let signal_raw = parts.next().unwrap_or("0");

        if active == "yes" {
            let ssid = if ssid_raw.is_empty() { None } else { Some(ssid_raw.to_string()) };
            let strength = signal_raw.trim().parse::<u8>().unwrap_or(0);
            return (ssid, strength);
        }
    }

    (None, 0)
}
