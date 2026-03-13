//! Audio state via PipeWire / PulseAudio.
//!
//! Polls the default sink volume and mute state using `wpctl` every 3 s.
//! A dedicated OS thread runs the poll (to avoid blocking the UI) and writes
//! results to a shared slot. A GPUI async task picks up changes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};

#[derive(Debug, Clone)]
pub struct AudioState {
    /// Master output volume, 0.0–1.0.
    pub volume: f32,
    /// Whether the output is muted.
    pub muted: bool,
}

impl Default for AudioState {
    fn default() -> Self {
        Self { volume: 1.0, muted: false }
    }
}

pub struct AudioService;

impl AudioService {
    pub fn start(cx: &mut App) -> Entity<AudioState> {
        let entity = cx.new(|_| AudioState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<AudioState>>> = Arc::new(Mutex::new(None));
        let slot_writer = slot.clone();
        let slot_reader = slot.clone();

        // Polling thread — wpctl is blocking so we run it off the GPUI thread.
        std::thread::spawn(move || loop {
            let state = read_audio_state();
            if let Ok(mut guard) = slot_writer.lock() {
                *guard = Some(state);
            }
            std::thread::sleep(Duration::from_secs(3));
        });

        // GPUI task — picks up updates from the slot.
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(3))
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

/// Parse a `wpctl get-volume` output string into an `AudioState`.
///
/// Handles:
///   `"Volume: 0.62\n"`         → volume=0.62, muted=false
///   `"Volume: 0.62 [MUTED]\n"` → volume=0.62, muted=true
///
/// Returns `AudioState::default()` when the string cannot be parsed.
pub(crate) fn parse_wpctl_output(s: &str) -> AudioState {
    let muted = s.contains("[MUTED]");
    // "Volume: 0.62 …" — second whitespace-separated token is the float.
    let volume = s
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    AudioState { volume, muted }
}

/// Read default sink volume and mute state via `wpctl`.
///
/// Parses `wpctl get-volume @DEFAULT_AUDIO_SINK@` output:
///   "Volume: 0.62"          (unmuted)
///   "Volume: 0.62 [MUTED]"  (muted)
fn read_audio_state() -> AudioState {
    let output = std::process::Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            parse_wpctl_output(&String::from_utf8_lossy(&o.stdout))
        }
        _ => AudioState::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_wpctl_output ---

    #[test]
    fn wpctl_unmuted_volume() {
        let s = parse_wpctl_output("Volume: 0.62\n");
        assert!((s.volume - 0.62).abs() < 1e-5);
        assert!(!s.muted);
    }

    #[test]
    fn wpctl_muted_volume() {
        let s = parse_wpctl_output("Volume: 0.62 [MUTED]\n");
        assert!((s.volume - 0.62).abs() < 1e-5);
        assert!(s.muted);
    }

    #[test]
    fn wpctl_full_volume() {
        let s = parse_wpctl_output("Volume: 1.00\n");
        assert!((s.volume - 1.0).abs() < 1e-5);
        assert!(!s.muted);
    }

    #[test]
    fn wpctl_zero_volume() {
        let s = parse_wpctl_output("Volume: 0.00\n");
        assert!((s.volume - 0.0).abs() < 1e-5);
        assert!(!s.muted);
    }

    #[test]
    fn wpctl_over_max_is_clamped_to_1() {
        // wpctl can report >1.0 for boosted output
        let s = parse_wpctl_output("Volume: 1.50\n");
        assert!((s.volume - 1.0).abs() < 1e-5);
    }

    #[test]
    fn wpctl_empty_string_gives_default() {
        let s = parse_wpctl_output("");
        assert!((s.volume - 1.0).abs() < 1e-5);
        assert!(!s.muted);
    }

    #[test]
    fn wpctl_garbage_gives_default_volume() {
        let s = parse_wpctl_output("some garbage output");
        assert!((s.volume - 1.0).abs() < 1e-5);
    }

    #[test]
    fn wpctl_muted_only_no_volume() {
        // Volume token missing but [MUTED] present
        let s = parse_wpctl_output("[MUTED]\n");
        assert!(s.muted);
        assert!((s.volume - 1.0).abs() < 1e-5); // fallback
    }

    #[test]
    fn wpctl_muted_with_zero_volume() {
        let s = parse_wpctl_output("Volume: 0.00 [MUTED]\n");
        assert!(s.muted);
        assert!((s.volume - 0.0).abs() < 1e-5);
    }

    // --- AudioState ---

    #[test]
    fn audio_state_default() {
        let s = AudioState::default();
        assert!((s.volume - 1.0).abs() < 1e-5);
        assert!(!s.muted);
    }

    #[test]
    fn audio_state_clone() {
        let a = AudioState { volume: 0.5, muted: true };
        let b = a.clone();
        assert!((b.volume - 0.5).abs() < 1e-5);
        assert!(b.muted);
    }
}
