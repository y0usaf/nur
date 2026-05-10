//! Freedesktop notification daemon with a small, practical capability set.
//!
//! Claims `org.freedesktop.Notifications` on the session bus via zbus.
//! Received notifications are stored in a capped ring buffer (latest 100).
//! The service exposes reactive state to Lua via the standard slot+GPUI pattern.
//!
//! Actions: `dismiss(id)`, `invoke_action(id, key)`, `clear_all()`, `set_dnd(bool)`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{App, AppContext, Entity};
use zbus::interface;

const MAX_NOTIFICATIONS: usize = 100;
const NOTIFICATION_CLOSED_REASON_DISMISSED: u32 = 2;
const NOTIFICATION_CLOSED_REASON_CLOSED_BY_APP: u32 = 3;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: String,
    /// Action pairs: `[key, label, key, label, ...]`
    pub actions: Vec<String>,
    /// Unix timestamp (seconds).
    pub time: u64,
}

#[derive(Debug, Clone, Default)]
pub struct NotificationsState {
    pub notifications: VecDeque<Notification>,
    pub dnd: bool,
}

/// Shared mutable state owned by the D-Bus interface handler.
type SharedState = Arc<Mutex<NotificationsStateInner>>;

static NOTIFICATIONS_STATE: OnceLock<SharedState> = OnceLock::new();
static NOTIFICATIONS_CONN: OnceLock<zbus::blocking::Connection> = OnceLock::new();

struct NotificationsStateInner {
    notifications: VecDeque<Notification>,
    dnd: bool,
    next_id: u32,
    /// Pending updates written here; GPUI task drains them.
    slot: Arc<Mutex<Option<NotificationsState>>>,
}

impl NotificationsStateInner {
    fn push_update(&self) {
        let snapshot = NotificationsState {
            notifications: self.notifications.clone(),
            dnd: self.dnd,
        };
        if let Ok(mut g) = self.slot.lock() {
            *g = Some(snapshot);
        }
    }
}

struct NotificationsInterface {
    state: SharedState,
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationsInterface {
    fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".to_string(),
            "actions".to_string(),
            "icon-static".to_string(),
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &mut self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        let mut inner = self.state.lock().unwrap_or_else(|e| e.into_inner());

        let id = if replaces_id != 0 {
            replaces_id
        } else {
            let id = inner.next_id;
            // Advance counter; skip 0 (reserved as "no id" in the spec).
            inner.next_id = inner.next_id.wrapping_add(1);
            if inner.next_id == 0 {
                inner.next_id = 1;
            }
            id
        };

        // When replacing, remove the old entry regardless of DND.
        if replaces_id != 0 {
            inner.notifications.retain(|n| n.id != replaces_id);
        }

        // Don't add new notifications while DND is active.
        if inner.dnd {
            return id;
        }

        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        inner.notifications.push_back(Notification {
            id,
            app: app_name,
            summary,
            body,
            icon: app_icon,
            actions,
            time,
        });

        // Cap ring buffer.
        while inner.notifications.len() > MAX_NOTIFICATIONS {
            inner.notifications.pop_front();
        }

        inner.push_update();
        id
    }

    fn close_notification(&mut self, id: u32) {
        let mut inner = self.state.lock().unwrap_or_else(|e| e.into_inner());
        inner.notifications.retain(|n| n.id != id);
        inner.push_update();

        emit_notification_closed(id, NOTIFICATION_CLOSED_REASON_CLOSED_BY_APP);
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "nur".to_string(),
            "nur".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "1.2".to_string(),
        )
    }
}

pub struct NotificationsService;

impl NotificationsService {
    pub fn start(cx: &mut App) -> Entity<NotificationsState> {
        let entity = cx.new(|_| NotificationsState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<NotificationsState>>> = Arc::new(Mutex::new(None));
        let slot_for_inner = slot.clone();
        let slot_reader = slot.clone();

        // Spawn OS thread to run the zbus D-Bus server.
        std::thread::spawn(move || {
            // Build a blocking zbus connection on session bus and claim the well-known name.
            let inner = Arc::new(Mutex::new(NotificationsStateInner {
                notifications: VecDeque::new(),
                dnd: false,
                next_id: 1,
                slot: slot_for_inner,
            }));

            let iface = NotificationsInterface {
                state: inner.clone(),
            };

            let conn = match zbus::blocking::connection::Builder::session()
                .and_then(|b| b.name("org.freedesktop.Notifications"))
                .and_then(|b| b.serve_at("/org/freedesktop/Notifications", iface))
                .and_then(|b| b.build())
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("notifications: failed to claim D-Bus name: {e}");
                    return;
                }
            };

            tracing::info!("notifications: claimed org.freedesktop.Notifications");
            let _ = NOTIFICATIONS_STATE.set(inner);
            let _ = NOTIFICATIONS_CONN.set(conn.clone());

            // Keep the connection alive forever.
            loop {
                std::thread::sleep(Duration::from_secs(60));
                // Connection is kept alive by the thread holding it.
                let _ = &conn;
            }
        });

        // GPUI task drains the slot.
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
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
            }
        })
        .detach();

        entity
    }
}

/// Emit `NotificationClosed` (dismissed) for the given notification ID.
pub fn dismiss(id: u32) {
    if let Some(state) = NOTIFICATIONS_STATE.get() {
        let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
        inner.notifications.retain(|n| n.id != id);
        inner.push_update();
    }

    emit_notification_closed(id, NOTIFICATION_CLOSED_REASON_DISMISSED);
}

/// Emit ActionInvoked for a notification. Fire-and-forget.
pub fn invoke_action(id: u32, action_key: String) {
    emit_action_invoked(id, &action_key);
}

pub fn clear_all() {
    let ids = if let Some(state) = NOTIFICATIONS_STATE.get() {
        let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<u32> = inner.notifications.iter().map(|n| n.id).collect();
        inner.notifications.clear();
        inner.push_update();
        ids
    } else {
        Vec::new()
    };

    for id in ids {
        emit_notification_closed(id, NOTIFICATION_CLOSED_REASON_DISMISSED);
    }
}

pub fn set_dnd(val: bool) {
    if let Some(state) = NOTIFICATIONS_STATE.get() {
        let mut inner = state.lock().unwrap_or_else(|e| e.into_inner());
        inner.dnd = val;
        inner.push_update();
    }
}

fn emit_notification_closed(id: u32, reason: u32) {
    if let Some(conn) = NOTIFICATIONS_CONN.get() {
        let _ = conn.emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "NotificationClosed",
            &(id, reason),
        );
    }
}

fn emit_action_invoked(id: u32, action_key: &str) {
    if let Some(conn) = NOTIFICATIONS_CONN.get() {
        let _ = conn.emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "ActionInvoked",
            &(id, action_key),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_state_default() {
        let s = NotificationsState::default();
        assert_eq!(s.notifications.len(), 0);
        assert!(!s.dnd);
    }

    #[test]
    fn notifications_state_clone() {
        let mut s = NotificationsState::default();
        s.notifications.push_back(Notification {
            id: 1,
            app: "test".into(),
            summary: "hi".into(),
            body: "".into(),
            icon: "".into(),
            actions: vec![],
            time: 0,
        });
        let b = s.clone();
        assert_eq!(b.notifications.len(), 1);
    }

    #[test]
    fn notification_clone() {
        let n = Notification {
            id: 5,
            app: "discord".into(),
            summary: "msg".into(),
            body: "hi".into(),
            icon: "discord".into(),
            actions: vec!["default".into()],
            time: 12345,
        };
        let m = n.clone();
        assert_eq!(m.id, 5);
        assert_eq!(m.app, "discord");
    }

    #[test]
    fn set_dnd_without_service_does_not_panic() {
        set_dnd(true);
    }

    #[test]
    fn clear_all_without_service_does_not_panic() {
        clear_all();
    }
}
