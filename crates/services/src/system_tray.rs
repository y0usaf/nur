//! System tray (StatusNotifierWatcher / SNI host) service.
//!
//! Registers as `org.kde.StatusNotifierWatcher` on the session bus. Tracks
//! registered `StatusNotifierItem` services, reads their properties, and
//! exposes the list as reactive state.
//!
//! Actions: `activate(id, x, y)`, `secondary_activate(id, x, y)`, `context_menu(id, x, y)`.
//!
//! Falls back to IconPixmap caching when IconName is missing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use gpui::{App, AppContext, Entity};
use image::{ImageBuffer, Rgba};
use zbus::message::Header;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayItem {
    /// Stable identifier exposed to Lua.
    pub id: String,
    pub title: String,
    pub icon_name: String,
    pub tooltip: String,
    /// `"Passive"`, `"Active"`, or `"NeedsAttention"`.
    pub status: String,
    pub category: String,
    /// Object path of the D-Bus menu, if any.
    pub menu: String,
    /// D-Bus destination used for actions and property reads.
    pub service_name: String,
    /// Object path implementing `org.kde.StatusNotifierItem`.
    pub object_path: String,
}

#[derive(Debug, Clone, Default)]
pub struct SystemTrayState {
    pub items: Vec<TrayItem>,
}

type Items = Arc<Mutex<HashMap<String, TrayItem>>>;
type SharedConn = Arc<Mutex<Option<zbus::blocking::Connection>>>;

static TRAY_ITEMS: OnceLock<Items> = OnceLock::new();
static WATCHER_CONN: OnceLock<SharedConn> = OnceLock::new();

pub struct SystemTrayService;

impl SystemTrayService {
    pub fn start(cx: &mut App) -> Entity<SystemTrayState> {
        let entity = cx.new(|_| SystemTrayState::default());
        let weak = entity.downgrade();

        let slot: Arc<Mutex<Option<SystemTrayState>>> = Arc::new(Mutex::new(None));
        let slot_for_watcher = slot.clone();
        let slot_reader = slot.clone();

        std::thread::spawn(move || {
            run_watcher(slot_for_watcher);
        });

        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
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

fn push_slot(items: &Items, slot: &Arc<Mutex<Option<SystemTrayState>>>) {
    let guard = items.lock().unwrap_or_else(|e| e.into_inner());
    let mut list: Vec<TrayItem> = guard.values().cloned().collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    let state = SystemTrayState { items: list };
    if let Ok(mut g) = slot.lock() {
        *g = Some(state);
    }
}

fn make_item_id(service_name: &str, object_path: &str) -> String {
    if object_path == "/StatusNotifierItem" {
        service_name.to_string()
    } else {
        format!("{service_name}{object_path}")
    }
}

fn read_property_value(
    conn: &zbus::blocking::Connection,
    service_name: &str,
    object_path: &str,
    interface: &str,
    property: &str,
) -> Option<zbus::zvariant::OwnedValue> {
    conn.call_method(
        Some(service_name),
        object_path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(interface, property),
    )
    .ok()?
    .body()
    .deserialize::<zbus::zvariant::OwnedValue>()
    .ok()
}

fn owned_value_to_string(value: zbus::zvariant::OwnedValue) -> Option<String> {
    if let Ok(zbus::zvariant::Value::Str(s)) = value.downcast_ref() {
        Some(s.to_string())
    } else if let Ok(zbus::zvariant::Value::ObjectPath(path)) = value.downcast_ref() {
        Some(path.to_string())
    } else {
        None
    }
}

fn tray_icon_cache_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir())
        .join("nur-tray-icon-cache")
}

fn owned_value_to_icon_pixmaps(
    value: zbus::zvariant::OwnedValue,
) -> Option<Vec<(i32, i32, Vec<u8>)>> {
    <Vec<(i32, i32, Vec<u8>)>>::try_from(value).ok()
}

fn sanitize_cache_key_part(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn cache_icon_pixmap(
    service_name: &str,
    object_path: &str,
    pixmaps: Vec<(i32, i32, Vec<u8>)>,
) -> Option<String> {
    let (width, height, bytes) = pixmaps
        .into_iter()
        .filter(|(w, h, bytes)| *w > 0 && *h > 0 && bytes.len() == (*w as usize * *h as usize * 4))
        .min_by_key(|(w, h, _)| {
            let area = (*w as i64) * (*h as i64);
            (area - 24 * 24).abs()
        })?;

    let mut rgba = Vec::with_capacity(bytes.len());
    for chunk in bytes.chunks_exact(4) {
        // SNI IconPixmap is ARGB32 in network byte order => [A, R, G, B].
        rgba.extend_from_slice(&[chunk[1], chunk[2], chunk[3], chunk[0]]);
    }

    let image: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_vec(width as u32, height as u32, rgba)?;

    let cache_dir = tray_icon_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);
    let key = format!(
        "{}_{}-{}x{}.png",
        sanitize_cache_key_part(service_name),
        sanitize_cache_key_part(object_path),
        width,
        height
    );
    let path = cache_dir.join(key);
    image.save(&path).ok()?;
    Some(path.to_string_lossy().into_owned())
}

fn read_sni_item(
    conn: &zbus::blocking::Connection,
    service_name: &str,
    object_path: &str,
) -> TrayItem {
    let iface = "org.kde.StatusNotifierItem";
    let get_string_prop = |prop: &str| {
        read_property_value(conn, service_name, object_path, iface, prop)
            .and_then(owned_value_to_string)
            .unwrap_or_default()
    };

    let title = get_string_prop("Title");
    let mut icon_name = get_string_prop("IconName");
    if icon_name.is_empty() {
        if let Some(pixmaps) = read_property_value(conn, service_name, object_path, iface, "IconPixmap")
            .and_then(owned_value_to_icon_pixmaps)
        {
            if let Some(path) = cache_icon_pixmap(service_name, object_path, pixmaps) {
                icon_name = path;
            }
        }
    }
    let status = get_string_prop("Status");
    let category = get_string_prop("Category");
    let menu = get_string_prop("Menu");

    let tooltip = read_property_value(conn, service_name, object_path, iface, "ToolTip")
        .and_then(|value| {
            if let Ok(zbus::zvariant::Value::Structure(fields)) = value.downcast_ref() {
                fields.fields().get(2).and_then(|field| {
                    if let zbus::zvariant::Value::Str(desc) = field {
                        Some(desc.to_string())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .unwrap_or_default();

    TrayItem {
        id: make_item_id(service_name, object_path),
        title,
        icon_name,
        tooltip,
        status,
        category,
        menu,
        service_name: service_name.to_string(),
        object_path: object_path.to_string(),
    }
}
fn discover_sni_items(
    conn: &zbus::blocking::Connection,
    names: &[String],
) -> Vec<(String, String)> {
    let mut out = Vec::new();

    for name in names {
        if name.starts_with("org.kde.StatusNotifierItem") {
            out.push((name.clone(), "/StatusNotifierItem".to_string()));
        }
    }

    // Some SNI providers export a stable well-known name and a non-default
    // object path instead of org.kde.StatusNotifierItem*/StatusNotifierItem.
    // blueman does this in the current desktop session.
    if names.iter().any(|name| name == "org.blueman.Tray")
        && read_property_value(
            conn,
            "org.blueman.Tray",
            "/org/blueman/sni",
            "org.kde.StatusNotifierItem",
            "Title",
        )
        .is_some()
    {
        out.push((
            "org.blueman.Tray".to_string(),
            "/org/blueman/sni".to_string(),
        ));
    }

    out
}

fn watcher_connection() -> Option<zbus::blocking::Connection> {
    WATCHER_CONN
        .get()
        .and_then(|conn| conn.lock().ok().and_then(|g| g.clone()))
}

fn emit_watcher_signal(member: &str, item_id: &str) {
    if let Some(conn) = watcher_connection() {
        let _ = conn.emit_signal(
            None::<&str>,
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
            member,
            &(item_id,),
        );
    }
}

fn register_item(
    items: &Items,
    slot: &Arc<Mutex<Option<SystemTrayState>>>,
    service_name: String,
    object_path: String,
) {
    let id = make_item_id(&service_name, &object_path);
    let placeholder = TrayItem {
        id: id.clone(),
        service_name: service_name.clone(),
        object_path: object_path.clone(),
        ..Default::default()
    };

    {
        let mut guard = items.lock().unwrap_or_else(|e| e.into_inner());
        guard.entry(id.clone()).or_insert(placeholder);
    }
    push_slot(items, slot);

    if let Some(conn) = watcher_connection() {
        let items = items.clone();
        let slot = slot.clone();
        std::thread::spawn(move || {
            let item = read_sni_item(&conn, &service_name, &object_path);
            {
                let mut guard = items.lock().unwrap_or_else(|e| e.into_inner());
                guard.insert(id.clone(), item);
            }
            push_slot(&items, &slot);
            emit_watcher_signal("StatusNotifierItemRegistered", &id);
        });
    }
}

struct StatusNotifierWatcher {
    items: Items,
    slot: Arc<Mutex<Option<SystemTrayState>>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcher {
    fn register_status_notifier_item(&mut self, service: &str, #[zbus(header)] header: Header<'_>) {
        let (service_name, object_path) = if service.starts_with('/') {
            (
                header.sender().map(ToString::to_string).unwrap_or_default(),
                service.to_string(),
            )
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };

        if service_name.is_empty() {
            tracing::warn!("system_tray: missing sender for registration of {service}");
            return;
        }

        register_item(&self.items, &self.slot, service_name, object_path);
    }

    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}

fn run_watcher(slot: Arc<Mutex<Option<SystemTrayState>>>) {
    let items: Items = Arc::new(Mutex::new(HashMap::new()));
    let conn_holder: SharedConn = Arc::new(Mutex::new(None));
    let _ = TRAY_ITEMS.set(items.clone());
    let _ = WATCHER_CONN.set(conn_holder.clone());

    let watcher = StatusNotifierWatcher {
        items: items.clone(),
        slot: slot.clone(),
    };

    let conn = match zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name("org.kde.StatusNotifierWatcher"))
        .and_then(|b| b.serve_at("/StatusNotifierWatcher", watcher))
        .and_then(|b| b.build())
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("system_tray: failed to claim StatusNotifierWatcher: {e}");
            return;
        }
    };

    if let Ok(mut guard) = conn_holder.lock() {
        *guard = Some(conn.clone());
    }

    tracing::info!("system_tray: registered as org.kde.StatusNotifierWatcher");

    let conn_for_poll = conn.clone();
    let items_for_poll = items.clone();
    let slot_for_poll = slot.clone();

    std::thread::spawn(move || {
        loop {
            if let Ok(reply) = conn_for_poll.call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "ListNames",
                &(),
            ) {
                if let Ok(names) = reply.body().deserialize::<Vec<String>>() {
                    let discovered_items = discover_sni_items(&conn_for_poll, &names);
                    let mut changed = false;
                    let mut removed = Vec::new();

                    {
                        let mut guard = items_for_poll.lock().unwrap_or_else(|e| e.into_inner());
                        let stale_ids: Vec<String> = guard
                            .iter()
                            .filter(|(_, item)| !names.contains(&item.service_name))
                            .map(|(id, _)| id.clone())
                            .collect();

                        for id in stale_ids {
                            if guard.remove(&id).is_some() {
                                removed.push(id);
                                changed = true;
                            }
                        }

                        for item in guard.values_mut() {
                            if names.contains(&item.service_name) {
                                let refreshed = read_sni_item(
                                    &conn_for_poll,
                                    &item.service_name,
                                    &item.object_path,
                                );
                                if *item != refreshed {
                                    *item = refreshed;
                                    changed = true;
                                }
                            }
                        }

                        for (service_name, object_path) in &discovered_items {
                            let id = make_item_id(service_name, object_path);
                            if !guard.contains_key(&id) {
                                guard.insert(
                                    id,
                                    read_sni_item(&conn_for_poll, service_name, object_path),
                                );
                                changed = true;
                            }
                        }
                    }

                    if changed {
                        push_slot(&items_for_poll, &slot_for_poll);
                    }
                    for id in removed {
                        emit_watcher_signal("StatusNotifierItemUnregistered", &id);
                    }
                }
            }

            std::thread::sleep(Duration::from_secs(3));
        }
    });

    loop {
        std::thread::sleep(Duration::from_secs(60));
        let _ = &conn;
    }
}

fn resolve_item(id: &str) -> Option<(String, String)> {
    TRAY_ITEMS
        .get()
        .and_then(|items| items.lock().ok().and_then(|guard| guard.get(id).cloned()))
        .map(|item| (item.service_name, item.object_path))
}

fn sni_action(id: &str, method: &str, x: i32, y: i32) {
    let Some((service_name, object_path)) = resolve_item(id) else {
        tracing::warn!("system_tray: unknown tray item {id}");
        return;
    };

    let method = method.to_string();
    std::thread::spawn(move || {
        if let Ok(conn) = zbus::blocking::Connection::session() {
            let _ = conn.call_method(
                Some(service_name.as_str()),
                object_path.as_str(),
                Some("org.kde.StatusNotifierItem"),
                method.as_str(),
                &(x, y),
            );
        }
    });
}

pub fn activate(id: &str, x: i32, y: i32) {
    sni_action(id, "Activate", x, y);
}

pub fn secondary_activate(id: &str, x: i32, y: i32) {
    sni_action(id, "SecondaryActivate", x, y);
}

pub fn context_menu(id: &str, x: i32, y: i32) {
    sni_action(id, "ContextMenu", x, y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_tray_state_default() {
        let s = SystemTrayState::default();
        assert!(s.items.is_empty());
    }

    #[test]
    fn tray_item_default() {
        let t = TrayItem::default();
        assert!(t.id.is_empty());
        assert!(t.status.is_empty());
    }

    #[test]
    fn system_tray_state_clone() {
        let s = SystemTrayState {
            items: vec![TrayItem {
                id: "discord".into(),
                title: "Discord".into(),
                ..Default::default()
            }],
        };
        let b = s.clone();
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].title, "Discord");
    }

    #[test]
    fn push_slot_sorts_by_id() {
        let items: Items = Arc::new(Mutex::new(HashMap::new()));
        {
            let mut g = items.lock().unwrap();
            g.insert(
                "zzz".into(),
                TrayItem {
                    id: "zzz".into(),
                    ..Default::default()
                },
            );
            g.insert(
                "aaa".into(),
                TrayItem {
                    id: "aaa".into(),
                    ..Default::default()
                },
            );
        }
        let slot: Arc<Mutex<Option<SystemTrayState>>> = Arc::new(Mutex::new(None));
        push_slot(&items, &slot);
        let state = slot.lock().unwrap().take().unwrap();
        assert_eq!(state.items[0].id, "aaa");
        assert_eq!(state.items[1].id, "zzz");
    }

    #[test]
    fn make_item_id_uses_path_for_non_default_items() {
        assert_eq!(
            make_item_id("org.kde.StatusNotifierItem-1", "/StatusNotifierItem"),
            "org.kde.StatusNotifierItem-1"
        );
        assert_eq!(
            make_item_id("org.kde.StatusNotifierItem-1", "/Tray"),
            "org.kde.StatusNotifierItem-1/Tray"
        );
    }
}
