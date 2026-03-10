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

use gpui::App;
use mlua::prelude::*;

use crate::bridge::state::LuaState;
use crate::context;

pub fn register(lua: &Lua, cx: &mut App) -> LuaResult<()> {
    let globals = lua.globals();
    let shell: LuaTable = globals.get("shell")?;

    let services = lua.create_table()?;

    register_battery(lua, cx, &services)?;
    register_audio(lua, cx, &services)?;
    register_network(lua, cx, &services)?;
    register_compositor(lua, cx, &services)?;
    register_sysinfo(lua, cx, &services)?;

    shell.set("services", services)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Battery
// ---------------------------------------------------------------------------

fn battery_to_lua(lua: &Lua, state: &services::BatteryState) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;
    tbl.set("percent", state.percent)?;
    tbl.set("charging", state.charging)?;
    Ok(tbl)
}

fn register_battery(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::BatteryService::start(cx);

    let initial = battery_to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = battery_to_lua(lua, &new_state) {
                context::with_cx(cx, || {
                    state_clone.set(LuaValue::Table(tbl));
                });
            }
        });
    })
    .detach();

    services.set("battery", lua_state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

fn audio_to_lua(lua: &Lua, state: &services::AudioState) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;
    tbl.set("volume", state.volume)?;
    tbl.set("muted", state.muted)?;
    Ok(tbl)
}

fn register_audio(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::AudioService::start(cx);

    let initial = audio_to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = audio_to_lua(lua, &new_state) {
                context::with_cx(cx, || {
                    state_clone.set(LuaValue::Table(tbl));
                });
            }
        });
    })
    .detach();

    services.set("audio", lua_state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

fn network_to_lua(lua: &Lua, state: &services::NetworkState) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;
    tbl.set("connected", state.connected)?;
    tbl.set("ssid", state.ssid.as_deref())?;
    tbl.set("strength", state.strength)?;
    Ok(tbl)
}

fn register_network(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::NetworkService::start(cx);

    let initial = network_to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = network_to_lua(lua, &new_state) {
                context::with_cx(cx, || {
                    state_clone.set(LuaValue::Table(tbl));
                });
            }
        });
    })
    .detach();

    services.set("network", lua_state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Compositor
// ---------------------------------------------------------------------------

fn compositor_to_lua(lua: &Lua, state: &services::CompositorState) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;
    tbl.set("active_workspace", state.active_workspace)?;
    tbl.set("active_window", state.active_window.as_deref())?;

    let workspaces = lua.create_table()?;
    for (i, ws) in state.workspaces.iter().enumerate() {
        let w = lua.create_table()?;
        w.set("id", ws.id)?;
        w.set("name", ws.name.clone())?;
        w.set("active", ws.active)?;
        workspaces.set(i + 1, w)?;
    }
    tbl.set("workspaces", workspaces)?;
    Ok(tbl)
}

fn register_compositor(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::CompositorService::start(cx);

    let initial = compositor_to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = compositor_to_lua(lua, &new_state) {
                context::with_cx(cx, || {
                    state_clone.set(LuaValue::Table(tbl));
                });
            }
        });
    })
    .detach();

    services.set("compositor", lua_state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// SysInfo
// ---------------------------------------------------------------------------

fn sysinfo_to_lua(lua: &Lua, state: &services::SysInfoState) -> LuaResult<LuaTable> {
    let tbl = lua.create_table()?;
    tbl.set("cpu_percent", state.cpu_percent)?;
    tbl.set("memory_percent", state.memory_percent)?;
    tbl.set("memory_used_gb", state.memory_used_gb)?;
    tbl.set("memory_total_gb", state.memory_total_gb)?;
    tbl.set("temperature", state.temperature)?;
    tbl.set("gpu_percent", state.gpu_percent)?;
    Ok(tbl)
}

fn register_sysinfo(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::SysInfoService::start(cx);

    let initial = sysinfo_to_lua(lua, &entity.read(cx))?;
    let lua_state = LuaState::new(LuaValue::Table(initial));
    let state_clone = lua_state.clone();

    cx.observe(&entity, move |entity, cx| {
        let new_state = entity.read(cx).clone();
        crate::vm::with_lua(|lua| {
            if let Ok(tbl) = sysinfo_to_lua(lua, &new_state) {
                context::with_cx(cx, || {
                    state_clone.set(LuaValue::Table(tbl));
                });
            }
        });
    })
    .detach();

    services.set("sysinfo", lua_state)?;
    Ok(())
}
