//! Hyprland IPC backend.
//!
//! Runs a blocking `EventListener` in a dedicated OS thread. On every
//! relevant event the full compositor state is re-fetched from Hyprland's
//! socket (fast, millisecond-range) and written to a shared slot.
//! A GPUI async task polls the slot every 100 ms and notifies the entity.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Entity};
use hyprland::{
    data::{Client, Workspace as HWorkspace, Workspaces},
    event_listener::EventListener,
    prelude::*,
};

use super::{CompositorState, Workspace};

pub fn start(entity: Entity<CompositorState>, cx: &mut App) {
    let weak = entity.downgrade();
    let slot: Arc<Mutex<Option<CompositorState>>> = Arc::new(Mutex::new(None));
    let slot_writer = slot.clone();
    let slot_reader = slot.clone();

    // Fetch and publish initial state before the listener starts.
    if let Ok(mut guard) = slot_writer.lock() {
        *guard = Some(fetch_state());
    }

    // Dedicated thread: runs the blocking Hyprland event listener.
    // On every relevant event, re-fetches full state and writes to the slot.
    std::thread::spawn(move || {
        let mut listener = EventListener::new();

        macro_rules! on_event {
            ($method:ident) => {{
                let writer = slot_writer.clone();
                listener.$method(move |_| {
                    if let Ok(mut guard) = writer.lock() {
                        *guard = Some(fetch_state());
                    }
                });
            }};
        }

        on_event!(add_workspace_changed_handler);
        on_event!(add_workspace_added_handler);
        on_event!(add_workspace_deleted_handler);
        on_event!(add_active_window_changed_handler);
        on_event!(add_window_opened_handler);
        on_event!(add_window_closed_handler);
        on_event!(add_window_moved_handler);

        if let Err(e) = listener.start_listener() {
            tracing::error!("Hyprland event listener stopped: {e}");
        }
    });

    // GPUI task: drain the slot every 100 ms and notify the entity.
    cx.spawn(async move |cx| loop {
        cx.background_executor()
            .timer(Duration::from_millis(100))
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
}

/// Fetch the full compositor state by querying Hyprland's IPC socket.
fn fetch_state() -> CompositorState {
    let active_id = match HWorkspace::get_active() {
        Ok(w) => w.id,
        Err(e) => {
            tracing::warn!("Failed to get active Hyprland workspace: {e}");
            return CompositorState::default();
        }
    };

    let workspaces = match Workspaces::get() {
        Ok(ws) => ws
            .into_iter()
            .filter(|w| w.id > 0) // skip special workspaces (negative IDs)
            .map(|w| Workspace {
                id: w.id,
                name: w.name,
                active: w.id == active_id,
            })
            .collect(),
        Err(e) => {
            tracing::warn!("Failed to get Hyprland workspaces: {e}");
            vec![]
        }
    };

    let active_window = Client::get_active().ok().flatten().map(|w| w.title);

    CompositorState { active_workspace: active_id, workspaces, active_window }
}
