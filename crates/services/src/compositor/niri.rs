//! Niri IPC backend.

use gpui::{App, Entity};
use super::CompositorState;

pub fn start(entity: Entity<CompositorState>, cx: &mut App) {
    let weak = entity.downgrade();

    cx.spawn(async move |cx| {
        // TODO: connect to Niri socket and stream workspace events.
        tracing::debug!("Niri compositor service started (stub)");
        let _ = weak;
        let _ = cx;
    })
    .detach();
}
