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

pub fn register(lua: &Lua, cx: &mut App) -> LuaResult<()> {
    let shell: LuaTable = lua.globals().get("shell")?;
    let services = lua.create_table()?;

    let battery    = services::BatteryService::start(cx);
    let audio      = services::AudioService::start(cx);
    let network    = services::NetworkService::start(cx);
    let compositor = services::CompositorService::start(cx);
    let sysinfo    = services::SysInfoService::start(cx);

    register_service(lua, cx, &services, "battery", battery, |lua, s: &services::BatteryState| {
        let tbl = lua.create_table()?;
        tbl.set("percent", s.percent)?;
        tbl.set("charging", s.charging)?;
        Ok(tbl)
    })?;

    register_service(lua, cx, &services, "audio", audio, |lua, s: &services::AudioState| {
        let tbl = lua.create_table()?;
        tbl.set("volume", s.volume)?;
        tbl.set("muted", s.muted)?;
        Ok(tbl)
    })?;

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
