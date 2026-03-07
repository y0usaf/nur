//! `ui.*` Lua API — element constructors.
//!
//! The simple layout primitives (`ui.hbox`, `ui.text`, `ui.spacer`, etc.)
//! are pure Lua functions defined in `lua/nur/stdlib.lua`. They simply
//! return tagged tables that the Rust bridge converts to GPUI elements.
//!
//! This file registers the Rust-backed `ui` table and adds any native
//! component constructors that require Rust to build (e.g. a rich Chart,
//! a virtualised Table, or a CodeEditor from gpui-component).
//!
//! To add a new native component:
//!   1. Implement `LuaUserData` for the component's config/handle type.
//!   2. Add a constructor function here: `ui.set("my_widget", ...)`.
//!   3. The Lua stdlib in `lua/nur/widgets/` can wrap it with helpers.

use mlua::prelude::*;

pub fn register(lua: &Lua) -> LuaResult<()> {
    // Create the `ui` table. Pure-Lua constructors are added by the stdlib
    // (see lua/nur/stdlib.lua). Native components go below.
    let ui = lua.create_table()?;

    // Future native components:
    // ui.set("chart", lua.create_function(lua_chart)?)?;
    // ui.set("table", lua.create_function(lua_table)?)?;

    lua.globals().set("ui", ui)?;
    Ok(())
}
