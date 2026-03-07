//! `shell.services.*` — system data exposed to Lua.
//!
//! Each service is a GPUI `Entity<T>` that updates asynchronously via D-Bus,
//! IPC, or polling. The values are exposed to Lua as plain tables/values so
//! no Lua-side knowledge of GPUI entities is required.
//!
//! # Current state
//!
//! Services are started and their *initial* values are written into static Lua
//! tables. Live updates do NOT yet propagate — service entities update but the
//! Lua tables are not refreshed.
//!
//! # TODO: live reactive updates
//!
//! For each service, replace the static table with a `LuaState`:
//!
//! ```rust
//! // Instead of writing a plain table:
//! let state = LuaState::new(initial_lua_table);
//! // Observe the entity; on change, call state.set(new_table):
//! cx.observe(&entity, move |_, entity, cx| {
//!     let new_val = entity.read(cx).clone();
//!     // convert new_val to LuaValue and call state.set(...)
//! }).detach();
//! services.set("battery", state)?;
//! ```
//!
//! `LuaState::set` automatically calls `context::notify_all_views()` so all
//! windows re-render. Users then call `shell.services.battery:get()` instead
//! of `shell.services.battery`.
//!
//! # Adding a new service
//! 1. Implement in `crates/services/src/`.
//! 2. Add a `register_*` function here.
//! 3. Call it from `register`.

use gpui::App;
use mlua::prelude::*;

pub fn register(lua: &Lua, cx: &mut App) -> LuaResult<()> {
    let globals = lua.globals();
    let shell: LuaTable = globals.get("shell")?;

    let services = lua.create_table()?;

    register_battery(lua, cx, &services)?;
    register_audio(lua, cx, &services)?;
    register_network(lua, cx, &services)?;
    register_compositor(lua, cx, &services)?;

    shell.set("services", services)?;
    Ok(())
}

fn register_battery(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::BatteryService::start(cx);

    // Build initial Lua table from current state.
    let state = entity.read(cx).clone();
    let tbl = lua.create_table()?;
    tbl.set("percent", state.percent)?;
    tbl.set("charging", state.charging)?;

    // TODO: observe the entity and push updates into a LuaState so Lua
    // render functions re-run when the battery changes.

    services.set("battery", tbl)?;
    Ok(())
}

fn register_audio(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::AudioService::start(cx);
    let state = entity.read(cx).clone();

    let tbl = lua.create_table()?;
    tbl.set("volume", state.volume)?;
    tbl.set("muted", state.muted)?;

    services.set("audio", tbl)?;
    Ok(())
}

fn register_network(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::NetworkService::start(cx);
    let state = entity.read(cx).clone();

    let tbl = lua.create_table()?;
    tbl.set("connected", state.connected)?;
    tbl.set("ssid", state.ssid.as_deref())?;
    tbl.set("strength", state.strength)?;

    services.set("network", tbl)?;
    Ok(())
}

fn register_compositor(lua: &Lua, cx: &mut App, services: &LuaTable) -> LuaResult<()> {
    let entity = services::CompositorService::start(cx);
    let state = entity.read(cx).clone();

    let tbl = lua.create_table()?;
    tbl.set("active_workspace", state.active_workspace)?;

    let workspaces = lua.create_table()?;
    for (i, ws) in state.workspaces.into_iter().enumerate() {
        let w = lua.create_table()?;
        w.set("id", ws.id)?;
        w.set("name", ws.name)?;
        w.set("active", ws.active)?;
        workspaces.set(i + 1, w)?;
    }
    tbl.set("workspaces", workspaces)?;

    services.set("compositor", tbl)?;
    Ok(())
}
