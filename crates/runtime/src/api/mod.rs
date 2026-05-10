//! Lua global registration — the public API surface of nur.
//!
//! Each sub-module owns one top-level Lua global:
//!
//! | Module | Global | Purpose |
//! |--------|--------|---------|
//! | `shell` | `shell` | Window creation, timers, state, utilities |
//! | `ui` | `ui` | Element constructors (pure-Lua ones live in stdlib.lua) |
//! | `services` | `shell.services` | System data and actions: applications, battery, audio, network, compositor, sysinfo, power_profiles, mpris, bluetooth, notifications, systemtray |
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

// ---------------------------------------------------------------------------
// IPC dispatch helpers (called by the nur binary's IPC server callback)
// ---------------------------------------------------------------------------

/// Evaluate a Lua snippet and return the string result (or an error string).
pub fn eval_lua(cx: &mut App, code: String) -> Result<String, String> {
    crate::vm::with_lua(|lua| {
        crate::context::with_cx(cx, || {
            lua.load(&code)
                .set_name("nur-eval")
                .eval::<mlua::Value>()
                .map(|v| match v {
                    mlua::Value::Nil => String::new(),
                    mlua::Value::String(s) => s.to_str().map(|b| b.to_string()).unwrap_or_default(),
                    other => format!("{other:?}"),
                })
                .map_err(|e| format!("eval error: {e}"))
        })
    })
}

/// Forward a freeform message to the `shell.on_msg` handler.
pub fn send_msg(cx: &mut App, message: String) -> Result<String, String> {
    shell::invoke_msg_handler(cx, message)
}

/// Set the config path for `shell.reload()`.
pub fn set_config_path(path: std::path::PathBuf) {
    shell::set_config_path(path);
}
