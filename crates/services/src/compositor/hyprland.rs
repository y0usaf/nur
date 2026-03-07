//! Hyprland IPC backend.

use gpui::{App, Entity};
use super::CompositorState;

pub fn start(entity: Entity<CompositorState>, cx: &mut App) {
    let weak = entity.downgrade();

    cx.spawn(async move |cx| {
        // TODO: subscribe to Hyprland socket events via the `hyprland` crate.
        //
        // use hyprland::event_listener::EventListenerMutable as EventListener;
        // let mut listener = EventListener::new();
        // listener.add_workspace_change_handler(|id, _| { ... });
        // listener.start_listener_async().await?;

        tracing::debug!("Hyprland compositor service started (stub)");
        let _ = weak;
        let _ = cx;
    })
    .detach();
}
