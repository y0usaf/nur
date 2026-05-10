//! Sway / i3 IPC event stream for workspace and window tracking.
//!
//! Uses the `swayipc` crate to subscribe to workspace and window events
//! via the i3/Sway IPC socket.

use gpui::{App, Entity};
use std::sync::{Arc, Mutex};

use super::{CompositorState, Workspace};

pub fn start(entity: Entity<CompositorState>, cx: &mut App) {
    let slot: Arc<Mutex<Option<CompositorState>>> = Arc::new(Mutex::new(None));
    let slot_writer = slot.clone();

    // OS thread: connect to sway IPC and listen for events
    std::thread::spawn(move || {
        use swayipc::{Connection, EventType};

        // Initial state fetch
        let mut conn = match Connection::new() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("sway IPC connect failed: {e}");
                return;
            }
        };

        if let Ok(workspaces) = conn.get_workspaces() {
            let state = workspaces_to_state(&workspaces);
            *slot_writer.lock().unwrap() = Some(state);
        }

        // Event subscription
        let subs = [EventType::Workspace, EventType::Window];
        let event_conn = match Connection::new() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("sway IPC event connect failed: {e}");
                return;
            }
        };

        let events = match event_conn.subscribe(subs) {
            Ok(ev) => ev,
            Err(e) => {
                tracing::error!("sway IPC subscribe failed: {e}");
                return;
            }
        };

        for event in events {
            let Ok(_event) = event else { continue };

            // Re-fetch full state on any workspace/window event
            let mut fetch_conn = match Connection::new() {
                Ok(c) => c,
                Err(_) => continue,
            };

            if let Ok(workspaces) = fetch_conn.get_workspaces() {
                let mut state = workspaces_to_state(&workspaces);

                // Get focused window title
                if let Ok(tree) = fetch_conn.get_tree() {
                    state.active_window = find_focused_title(&tree);
                }

                *slot_writer.lock().unwrap() = Some(state);
            }
        }
    });

    // GPUI task: drain slot into entity
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;

            let update = slot.lock().unwrap().take();
            if let Some(state) = update {
                cx.update(|cx| {
                    entity.update(cx, |s, cx| {
                        *s = state;
                        cx.notify();
                    });
                });
            }
        }
    })
    .detach();
}

fn workspaces_to_state(workspaces: &[swayipc::Workspace]) -> CompositorState {
    let active = workspaces
        .iter()
        .find(|w| w.focused)
        .map(|w| w.num)
        .unwrap_or(0) as i32;

    let ws: Vec<Workspace> = workspaces
        .iter()
        .map(|w| Workspace {
            id: w.num as i32,
            name: w.name.clone(),
            active: w.focused,
        })
        .collect();

    CompositorState {
        active_workspace: active,
        workspaces: ws,
        active_window: None,
    }
}

fn find_focused_title(node: &swayipc::Node) -> Option<String> {
    if node.focused {
        return node.name.clone();
    }
    for child in &node.nodes {
        if let Some(title) = find_focused_title(child) {
            return Some(title);
        }
    }
    for child in &node.floating_nodes {
        if let Some(title) = find_focused_title(child) {
            return Some(title);
        }
    }
    None
}
