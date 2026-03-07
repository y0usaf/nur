//! Audio state via PipeWire / PulseAudio.

use gpui::{App, AppContext, Entity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioState {
    pub volume: f32, // 0.0–1.0
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
        // TODO: subscribe to PipeWire/libpulse events and update entity.
        entity
    }
}
