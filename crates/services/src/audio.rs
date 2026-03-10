//! Audio state via PipeWire / PulseAudio.
//!
//! Polls the default sink volume and mute state using `wpctl` every 3 s.
//! A dedicated OS thread runs the poll (to avoid blocking the UI) and writes
//! results to a shared slot. A GPUI async task picks up changes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, AppContext, Entity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            let s = String::from_utf8_lossy(&o.stdout);
            let muted = s.contains("[MUTED]");
            // Second token is the float volume value.
            let volume = s
                .split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            AudioState { volume, muted }
        }
        _ => AudioState::default(),
    }
}
