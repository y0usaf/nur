//! Niri IPC backend.
//!
//! Connects to the Niri Unix socket and subscribes to the event stream.
//! The `EventStreamState` tracker from niri-ipc handles all bookkeeping.
//! State is written to a shared slot; a GPUI async task polls every 100 ms.

use std::io::{BufRead, BufReader, Write as IoWrite};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{App, Entity};
use niri_ipc::{Event, Reply, Request, state::{EventStreamState, EventStreamStatePart}};

use super::{CompositorState, Workspace};

pub fn start(entity: Entity<CompositorState>, cx: &mut App) {
    let weak = entity.downgrade();
    let slot: Arc<Mutex<Option<CompositorState>>> = Arc::new(Mutex::new(None));
    let slot_writer = slot.clone();
    let slot_reader = slot.clone();

    // Dedicated thread: connects to Niri socket and streams events.
    std::thread::spawn(move || {
        if let Err(e) = run(slot_writer) {
            tracing::error!("Niri event stream ended: {e}");
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

fn run(slot: Arc<Mutex<Option<CompositorState>>>) -> anyhow::Result<()> {
    let socket_path = std::env::var_os("NIRI_SOCKET")
        .ok_or_else(|| anyhow::anyhow!("NIRI_SOCKET not set"))?;

    let mut stream = UnixStream::connect(&socket_path)?;

    // Request the event stream.
    let mut request = serde_json::to_string(&Request::EventStream)?;
    request.push('\n');
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    // Read the handshake reply.
    reader.read_line(&mut line)?;
    let reply: Reply = serde_json::from_str(&line)?;
    reply.map_err(|e| anyhow::anyhow!("Niri refused event stream: {e}"))?;

    // Stop writing; we only read from here on.
    reader.get_ref().shutdown(Shutdown::Write).ok();

    // Read the initial burst of events (niri sends current state upfront).
    // A short read timeout lets us detect the end of the burst.
    reader
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(500)))
        .ok();

    let mut state = EventStreamState::default();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()), // EOF
            Ok(_) => {
                if let Ok(event) = serde_json::from_str::<Event>(&line) {
                    state.apply(event);
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                break; // initial burst done
            }
            Err(e) => return Err(e.into()),
        }
    }

    // Publish the initial state.
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(map_state(&state));
    }

    // Remove the timeout; block on each subsequent event.
    reader.get_ref().set_read_timeout(None).ok();

    // Main event loop.
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()), // EOF — niri exited
            Ok(_) => {
                let event = match serde_json::from_str::<Event>(&line) {
                    Ok(ev) => ev,
                    Err(e) => {
                        // Unknown events (IPC version skew) — skip gracefully.
                        tracing::debug!("Unknown Niri event (skipping): {e}");
                        continue;
                    }
                };
                state.apply(event);
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(map_state(&state));
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Map niri's `EventStreamState` to nur's simpler `CompositorState`.
fn map_state(state: &EventStreamState) -> CompositorState {
    let active_workspace_id = state
        .workspaces
        .workspaces
        .values()
        .find(|w| w.is_focused)
        .map(|w| w.id as i32)
        .unwrap_or(0);

    let mut workspaces: Vec<Workspace> = state
        .workspaces
        .workspaces
        .values()
        .map(|w| Workspace {
            id: w.id as i32,
            name: w.name.clone().unwrap_or_else(|| w.idx.to_string()),
            active: w.is_focused,
        })
        .collect();
    workspaces.sort_by_key(|w| w.id);

    let active_window = state
        .windows
        .windows
        .values()
        .find(|w| w.is_focused)
        .and_then(|w| w.title.clone());

    CompositorState { active_workspace: active_workspace_id, workspaces, active_window }
}
