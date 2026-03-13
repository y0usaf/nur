//! `shell.services.*` — system data exposed to Lua as reactive `LuaState` values.
//!
//! Each service is a GPUI `Entity<T>` that updates asynchronously. The values
//! are exposed to Lua as `LuaState` userdata so that changes automatically
//! trigger re-renders. Users call `shell.services.battery:get()` to read the
//! current value, and the render function re-runs whenever the service updates.
//!
//! # Adding a new service
//! 1. Implement in `crates/services/src/`.
//! 2. Add a `register_*` function here following the existing pattern.
//! 3. Call it from `register`.

use gpui::{App, Entity};
use mlua::prelude::*;

use crate::bridge::service_handle::ServiceHandle;
use crate::bridge::state::LuaState;
use crate::context;

/// Start a service, expose its state as a reactive `LuaState`, and register it
/// under `key` in the `services` Lua table.
///
/// `to_lua` converts the Rust state to a Lua table. It is called once for the
/// initial value and again on every GPUI `observe` callback.
fn register_service<S, F>(
    lua: &Lua,
    cx: &mut App,
    services: &LuaTable,
    key: &'static str,
    entity: Entity<S>,
    to_lua: F,
) -> LuaResult<()>
where
    S: Clone + 'static,
    F: Fn(&Lua, &S) -> LuaResult<LuaTable> + 'static,
{
    let initial = to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = to_lua(lua, &new_state) {
                context::with_cx(cx, || state_clone.set(LuaValue::Table(tbl)));
            }
        });
    })
    .detach();

    services.set(key, lua_state)?;
    Ok(())
}

/// Like [`register_service`], but wraps the state in a [`ServiceHandle`] so the
/// caller can attach action methods (e.g. `set_volume`, `toggle_mute`).
///
/// Use this instead of `register_service` when a service needs to expose
/// callable actions **in addition to** reactive state. Read-only services
/// (battery, network, sysinfo) should use [`register_service`] instead.
///
/// # Parameters
///
/// - `lua` -- the Lua VM, used to create tables and functions.
/// - `cx` -- GPUI app context, needed to read initial state and set up observers.
/// - `services` -- the `shell.services` Lua table to register under.
/// - `key` -- the service name (e.g. `"audio"`), becomes `shell.services.<key>`.
/// - `entity` -- the GPUI entity holding the service's Rust state.
/// - `to_lua` -- converts the Rust state `S` into a Lua table; called for the
///   initial value and on every subsequent state change.
/// - `register_actions` -- a closure that receives the `ServiceHandle` and should
///   call `handle.register_action(name, func)` for each action the service exposes.
///
/// # Example (adding a new action-enabled service)
/// ```ignore
/// register_service_with_actions(
///     lua, cx, &services, "audio", entity,
///     |lua, s: &AudioState| {
///         let tbl = lua.create_table()?;
///         tbl.set("volume", s.volume)?;
///         tbl.set("muted", s.muted)?;
///         Ok(tbl)
///     },
///     |lua, handle, _entity| {
///         handle.register_action("set_volume".into(), lua.create_function(move |_lua, vol: f32| {
///             services::audio::set_volume(vol);
///             Ok(())
///         })?);
///         Ok(())
///     },
/// )?;
/// ```
fn register_service_with_actions<S, F, A>(
    lua: &Lua,
    cx: &mut App,
    services: &LuaTable,
    key: &'static str,
    entity: Entity<S>,
    to_lua: F,
    register_actions: A,
) -> LuaResult<()>
where
    S: Clone + 'static,
    F: Fn(&Lua, &S) -> LuaResult<LuaTable> + 'static,
    A: FnOnce(&Lua, &ServiceHandle, Entity<S>) -> LuaResult<()>,
{
    let initial = to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let handle = ServiceHandle::new(lua_state);

    let state_clone = handle.state.clone();
    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = to_lua(lua, &new_state) {
                context::with_cx(cx, || state_clone.set(LuaValue::Table(tbl)));
            }
        });
    })
    .detach();

    register_actions(lua, &handle, entity)?;
    services.set(key, handle)?;
    Ok(())
}

/// Convert a single `AppEntry` to a Lua table.
fn app_entry_to_lua(lua: &Lua, app: &services::AppEntry) -> LuaResult<LuaTable> {
    let a = lua.create_table()?;
    a.set("name", app.name.clone())?;
    a.set("exec", app.exec.clone())?;
    a.set("icon", app.icon.clone())?;
    a.set("comment", app.comment.clone())?;
    let kw = lua.create_table()?;
    for (j, k) in app.keywords.iter().enumerate() {
        kw.set(j + 1, k.clone())?;
    }
    a.set("keywords", kw)?;
    let cats = lua.create_table()?;
    for (j, c) in app.categories.iter().enumerate() {
        cats.set(j + 1, c.clone())?;
    }
    a.set("categories", cats)?;
    Ok(a)
}

/// Convert `ApplicationsState` to a Lua table with an `apps` array field.
fn apps_to_lua(lua: &Lua, state: &services::ApplicationsState) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;
    let apps = lua.create_table()?;
    for (i, app) in state.apps.iter().enumerate() {
        apps.set(i + 1, app_entry_to_lua(lua, app)?)?;
    }
    tbl.set("apps", apps)?;
    Ok(tbl)
}

/// Register the applications service with `:get()`, `:subscribe()`, `:search()`,
/// and `:launch()` methods.
///
/// Unlike other services that use `register_service` (returning a `LuaState`
/// userdata), applications uses a plain Lua table wrapper so we can attach
/// the extra `search` and `launch` closures that capture the GPUI entity.
fn register_applications(
    lua: &Lua,
    cx: &mut App,
    services: &LuaTable,
    entity: Entity<services::ApplicationsState>,
) -> LuaResult<()> {
    // Create reactive LuaState for :get()/:subscribe() (same logic as register_service)
    let initial = apps_to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = apps_to_lua(lua, &new_state) {
                context::with_cx(cx, || state_clone.set(LuaValue::Table(tbl)));
            }
        });
    })
    .detach();

    // Build wrapper table
    let wrapper = lua.create_table()?;

    // :get() delegates to LuaState
    let state_for_get = lua_state.clone();
    wrapper.set(
        "get",
        lua.create_function(move |_lua, _this: LuaValue| Ok(state_for_get.get()))?,
    )?;

    // :subscribe(fn) delegates to LuaState
    let state_for_sub = lua_state.clone();
    wrapper.set(
        "subscribe",
        lua.create_function(
            move |lua, (_this, callback): (LuaValue, LuaFunction)| {
                let key = lua.create_registry_value(callback)?;
                state_for_sub.add_notifier(move || {
                    crate::vm::with_lua(|lua| {
                        if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                            let _ = f.call::<()>(());
                        }
                    });
                });
                Ok(())
            },
        )?,
    )?;

    // :search(query) reads entity state and filters
    let weak = entity.downgrade();
    wrapper.set(
        "search",
        lua.create_function(
            move |lua, (_this, query): (LuaValue, String)| {
                context::current_cx(|cx| {
                    let Some(entity) = weak.upgrade() else {
                        return Ok(lua.create_table()?);
                    };
                    let state = entity.read(cx);
                    let results = services::search_apps(&state.apps, &query);
                    let tbl = lua.create_table()?;
                    for (i, app) in results.iter().enumerate() {
                        tbl.set(i + 1, app_entry_to_lua(lua, app)?)?;
                    }
                    Ok(tbl)
                })
            },
        )?,
    )?;

    // :launch(exec_string) spawns a process
    wrapper.set(
        "launch",
        lua.create_function(move |_lua, (_this, exec): (LuaValue, String)| {
            let cleaned = services::strip_field_codes(&exec);
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&cleaned)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| mlua::Error::runtime(format!("launch failed: {e}")))?;
            Ok(())
        })?,
    )?;

    // Use metatable so method-call syntax (colon) works
    let mt = lua.create_table()?;
    mt.set("__index", wrapper.clone())?;
    wrapper.set_metatable(Some(mt));

    services.set("applications", wrapper)?;
    Ok(())
}

pub fn register(lua: &Lua, cx: &mut App) -> LuaResult<()> {
    let shell: LuaTable = lua.globals().get("shell")?;
    let services = lua.create_table()?;

    let applications = services::ApplicationsService::start(cx);
    let battery      = services::BatteryService::start(cx);
    let audio        = services::AudioService::start(cx);
    let network      = services::NetworkService::start(cx);
    let compositor   = services::CompositorService::start(cx);
    let sysinfo      = services::SysInfoService::start(cx);

    register_applications(lua, cx, &services, applications)?;

    register_service(lua, cx, &services, "battery", battery, |lua, s: &services::BatteryState| {
        let tbl = lua.create_table()?;
        tbl.set("percent", s.percent)?;
        tbl.set("charging", s.charging)?;
        Ok(tbl)
    })?;

    register_service_with_actions(
        lua, cx, &services, "audio", audio, |lua, s: &services::AudioState| {
            let tbl = lua.create_table()?;
            tbl.set("volume", s.volume)?;
            tbl.set("muted", s.muted)?;
            Ok(tbl)
        },
        |lua, handle, _entity| {
            handle.register_action(
                "set_volume".into(),
                lua.create_function(|_lua, vol: f32| {
                    services::audio::set_volume(vol);
                    Ok(())
                })?,
            );
            handle.register_action(
                "toggle_mute".into(),
                lua.create_function(|_lua, ()| {
                    services::audio::toggle_mute();
                    Ok(())
                })?,
            );
            Ok(())
        },
    )?;

    register_service(lua, cx, &services, "network", network, |lua, s: &services::NetworkState| {
        let tbl = lua.create_table()?;
        tbl.set("connected", s.connected)?;
        tbl.set("ssid", s.ssid.as_deref())?;
        tbl.set("strength", s.strength)?;
        Ok(tbl)
    })?;

    register_service(lua, cx, &services, "compositor", compositor, |lua, s: &services::CompositorState| {
        let tbl = lua.create_table()?;
        tbl.set("active_workspace", s.active_workspace)?;
        tbl.set("active_window", s.active_window.as_deref())?;
        let workspaces = lua.create_table()?;
        for (i, ws) in s.workspaces.iter().enumerate() {
            let w = lua.create_table()?;
            w.set("id", ws.id)?;
            w.set("name", ws.name.clone())?;
            w.set("active", ws.active)?;
            workspaces.set(i + 1, w)?;
        }
        tbl.set("workspaces", workspaces)?;
        Ok(tbl)
    })?;

    register_service(lua, cx, &services, "sysinfo", sysinfo, |lua, s: &services::SysInfoState| {
        let tbl = lua.create_table()?;
        tbl.set("cpu_percent", s.cpu_percent)?;
        tbl.set("memory_percent", s.memory_percent)?;
        tbl.set("memory_used_gb", s.memory_used_gb)?;
        tbl.set("memory_total_gb", s.memory_total_gb)?;
        tbl.set("temperature", s.temperature)?;
        tbl.set("gpu_percent", s.gpu_percent)?;
        Ok(tbl)
    })?;

    shell.set("services", services)?;
    Ok(())
}
