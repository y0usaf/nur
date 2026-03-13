//! Network state via sysfs and nmcli.
//!
//! Polls every 5 s. Connection status comes from `/sys/class/net/*/operstate`;
//! WiFi SSID and signal strength come from `nmcli device wifi` when available.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone)]
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

/// Parse a sysfs `operstate` file content. Returns `true` when the value is `"up"`.
pub(crate) fn parse_operstate_str(s: &str) -> bool {
    s.trim() == "up"
}

/// Parse `nmcli -t -f ACTIVE,SSID,SIGNAL device wifi` stdout into `(ssid, strength)`.
///
/// Scans lines in order and returns the first active entry. Format per line:
///   `"yes:HomeNet:87"` — named network
///   `"yes::0"`          — hidden SSID
///   `"no:OtherNet:50"`  — inactive, skip
///
/// Returns `(None, 0)` when no active WiFi entry is found.
pub(crate) fn parse_nmcli_output(output: &str) -> (Option<String>, u8) {
    for line in output.lines() {
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
            .map(|s| parse_operstate_str(&s))
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

    parse_nmcli_output(&String::from_utf8_lossy(&o.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_operstate_str ---

    #[test]
    fn operstate_up_is_connected() {
        assert!(parse_operstate_str("up"));
    }

    #[test]
    fn operstate_up_with_newline() {
        assert!(parse_operstate_str("up\n"));
    }

    #[test]
    fn operstate_down_is_disconnected() {
        assert!(!parse_operstate_str("down\n"));
    }

    #[test]
    fn operstate_unknown_is_disconnected() {
        assert!(!parse_operstate_str("unknown\n"));
    }

    #[test]
    fn operstate_empty_is_disconnected() {
        assert!(!parse_operstate_str(""));
    }

    #[test]
    fn operstate_uppercase_up_is_disconnected() {
        // sysfs is lowercase; "UP" is not a valid value
        assert!(!parse_operstate_str("UP"));
    }

    // --- parse_nmcli_output ---

    #[test]
    fn nmcli_single_active_network() {
        let (ssid, strength) = parse_nmcli_output("yes:HomeNetwork:75\n");
        assert_eq!(ssid.as_deref(), Some("HomeNetwork"));
        assert_eq!(strength, 75);
    }

    #[test]
    fn nmcli_hidden_ssid_returns_none() {
        let (ssid, strength) = parse_nmcli_output("yes::0\n");
        assert_eq!(ssid, None);
        assert_eq!(strength, 0);
    }

    #[test]
    fn nmcli_selects_active_over_inactive() {
        let output = "no:OtherNet:50\nyes:MyNet:80\n";
        let (ssid, strength) = parse_nmcli_output(output);
        assert_eq!(ssid.as_deref(), Some("MyNet"));
        assert_eq!(strength, 80);
    }

    #[test]
    fn nmcli_empty_output_returns_none() {
        let (ssid, strength) = parse_nmcli_output("");
        assert_eq!(ssid, None);
        assert_eq!(strength, 0);
    }

    #[test]
    fn nmcli_no_active_lines_returns_none() {
        let (ssid, strength) = parse_nmcli_output("no:Net1:50\nno:Net2:30\n");
        assert_eq!(ssid, None);
        assert_eq!(strength, 0);
    }

    #[test]
    fn nmcli_invalid_signal_falls_back_to_zero() {
        let (ssid, strength) = parse_nmcli_output("yes:MyNet:invalid\n");
        assert_eq!(ssid.as_deref(), Some("MyNet"));
        assert_eq!(strength, 0);
    }

    #[test]
    fn nmcli_signal_with_whitespace_trimmed() {
        let (_, strength) = parse_nmcli_output("yes:Net:  65  \n");
        assert_eq!(strength, 65);
    }

    #[test]
    fn nmcli_first_active_wins() {
        // Only the first "yes" line should be returned
        let output = "yes:First:70\nyes:Second:90\n";
        let (ssid, _) = parse_nmcli_output(output);
        assert_eq!(ssid.as_deref(), Some("First"));
    }

    // --- NetworkState ---

    #[test]
    fn network_state_default() {
        let s = NetworkState::default();
        assert!(s.connected);
        assert_eq!(s.ssid, None);
        assert_eq!(s.strength, 100);
    }

    #[test]
    fn network_state_clone() {
        let a = NetworkState { connected: false, ssid: Some("Test".into()), strength: 50 };
        let b = a.clone();
        assert!(!b.connected);
        assert_eq!(b.ssid.as_deref(), Some("Test"));
        assert_eq!(b.strength, 50);
    }
}
