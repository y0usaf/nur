//! Lua global registration — the public API surface of Lumen.
//!
//! Each sub-module owns one top-level Lua global:
//!
//! | Module | Global | Purpose |
//! |--------|--------|---------|
//! | `shell` | `shell` | Window creation, timers, state, utilities |
//! | `ui` | `ui` | Element constructors (pure-Lua ones live in stdlib.lua) |
//! | `services` | `shell.services` | System data: battery, audio, network, compositor |
//!
//! # Error handling note
//!
//! All sub-module `register` functions return `LuaResult<()>` because
//! `mlua::Error` is `!Send + !Sync` and cannot be converted to `anyhow::Error`
//! with bare `?`. The conversion happens here at the boundary.

mod services;
mod shell;
mod ui;

use anyhow::Result;
use gpui::App;
use mlua::prelude::*;

/// Register all Lua globals. Called once before user config executes.
///
/// To add a new top-level API namespace:
/// 1. Create a new module in this directory.
/// 2. Call its `register` function here.
pub fn register_all(lua: &Lua, cx: &mut App) -> Result<()> {
    let wrap = |e: LuaError| anyhow::anyhow!("{e}");
    shell::register(lua, cx).map_err(wrap)?;
    ui::register(lua).map_err(wrap)?;
    services::register(lua, cx).map_err(wrap)?;
    Ok(())
}
