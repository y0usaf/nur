//! Compositor IPC — workspaces, active window, etc.
//!
//! Auto-detects the running compositor (Hyprland / Niri) and delegates to
//! the appropriate backend. Adding support for a new compositor means adding
//! a module here and extending the `detect` function.

pub mod hyprland;
pub mod niri;

use gpui::{App, AppContext, Entity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Workspace {
    pub id: i32,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompositorState {
    pub active_workspace: i32,
    pub workspaces: Vec<Workspace>,
    pub active_window: Option<String>,
}

pub struct CompositorService;

impl CompositorService {
    pub fn start(cx: &mut App) -> Entity<CompositorState> {
        let entity = cx.new(|_| CompositorState::default());

        match detect_compositor() {
            Compositor::Hyprland => hyprland::start(entity.clone(), cx),
            Compositor::Niri     => niri::start(entity.clone(), cx),
            Compositor::Unknown  => {
                tracing::warn!("Unknown compositor — workspace tracking disabled");
            }
        }

        entity
    }
}

enum Compositor { Hyprland, Niri, Unknown }

fn detect_compositor() -> Compositor {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        Compositor::Hyprland
    } else if std::env::var("NIRI_SOCKET").is_ok() {
        Compositor::Niri
    } else {
        Compositor::Unknown
    }
}
