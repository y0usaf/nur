//! Network state via NetworkManager D-Bus.

use gpui::{App, AppContext, Entity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    pub connected: bool,
    pub ssid: Option<String>,   // None when on ethernet or disconnected
    pub strength: u8,           // 0-100
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
        // TODO: subscribe to NetworkManager via zbus.
        entity
    }
}
