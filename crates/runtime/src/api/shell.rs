//! `shell.*` Lua API — window creation, timers, state, utilities.

use gpui::App;
use mlua::prelude::*;
use std::time::Duration;

use gpui::layer_shell::Layer;

use crate::bridge::{
    state::LuaState,
    window::{BarPosition, LuaWindowHandle, WindowConfig, open_shell_window},
};
use crate::context;

pub fn register(lua: &Lua, _cx: &mut App) -> LuaResult<()> {
    let shell = lua.create_table()?;

    // shell.window(config) -> LuaWindowHandle
    shell.set("window", lua.create_function(lua_window)?)?;

    // shell.state(initial_value) -> LuaState
    shell.set("state", lua.create_function(lua_state)?)?;

    // shell.interval(ms, fn)  — repeating timer
    shell.set("interval", lua.create_function(lua_interval)?)?;

    // shell.once(ms, fn) — one-shot timer
    shell.set("once", lua.create_function(lua_once)?)?;

    // shell.exec(cmd) -> string  — run a shell command and capture stdout
    shell.set("exec", lua.create_function(lua_exec)?)?;

    // shell.watch_file(path, fn) — call fn(content) when the file changes
    shell.set("watch_file", lua.create_function(lua_watch_file)?)?;

    // shell.exec_async(cmd, fn) — run a shell command without blocking the UI
    shell.set("exec_async", lua.create_function(lua_exec_async)?)?;

    // shell.quit() — gracefully stop nur
    shell.set("quit", lua.create_function(lua_quit)?)?;

    lua.globals().set("shell", shell)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Implementations
// ---------------------------------------------------------------------------

fn lua_window(_lua: &Lua, config: LuaTable) -> LuaResult<LuaWindowHandle> {
    let position = BarPosition::from_str(
        &config.get::<String>("position").unwrap_or_else(|_| "top".into()),
    );
    let size: f32 = config.get("height").or_else(|_| config.get("width")).unwrap_or(32.0);
    let exclusive: bool = config.get("exclusive").unwrap_or(true);
    let layer_str: String = config.get("layer").unwrap_or_else(|_| "top".into());
    let layer = match layer_str.as_str() {
        "background" => Layer::Background,
        "bottom"     => Layer::Bottom,
        "overlay"    => Layer::Overlay,
        _            => Layer::Top,
    };

    let bg = parse_color(config.get::<String>("bg").unwrap_or_default(), 0x1e1e2e);
    let fg = parse_color(config.get::<String>("fg").unwrap_or_default(), 0xcdd6f4);
    let font_size: f32 = config.get("font_size").unwrap_or(13.0);

    let win_config = WindowConfig { position, size, exclusive, layer, bg, fg, font_size };

    context::current_cx(|cx| {
        open_shell_window(win_config, cx).map_err(|e| LuaError::RuntimeError(e.to_string()))
    })
}

fn lua_state(_lua: &Lua, initial: LuaValue) -> LuaResult<LuaState> {
    Ok(LuaState::new(initial))
}

fn lua_interval(lua: &Lua, (ms, callback): (u64, LuaFunction)) -> LuaResult<()> {
    let key = lua.create_registry_value(callback)?;

    context::current_cx(|cx| {
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(ms))
                .await;

            cx.update(|cx| {
                crate::vm::with_lua(|lua| {
                    if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                        context::with_cx(cx, || {
                            if let Err(e) = f.call::<()>(()) {
                                tracing::error!("interval callback error: {e}");
                            }
                        });
                    }
                });
            });
        })
        .detach();
    });

    Ok(())
}

fn lua_once(lua: &Lua, (ms, callback): (u64, LuaFunction)) -> LuaResult<()> {
    let key = lua.create_registry_value(callback)?;

    context::current_cx(|cx| {
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(ms))
                .await;

            cx.update(|cx| {
                crate::vm::with_lua(|lua| {
                    if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                        context::with_cx(cx, || {
                            if let Err(e) = f.call::<()>(()) {
                                tracing::error!("once callback error: {e}");
                            }
                        });
                    }
                });
            });
        })
        .detach();
    });

    Ok(())
}

fn lua_exec(_lua: &Lua, cmd: String) -> LuaResult<String> {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| LuaError::RuntimeError(format!("exec failed: {e}")))?;

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn lua_watch_file(lua: &Lua, (path, cb): (String, LuaFunction)) -> LuaResult<()> {
    let key = lua.create_registry_value(cb)?;

    context::current_cx(|cx| {
        cx.spawn(async move |cx| {
            let mut last_mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok();

            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;

                let mtime = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok();

                if mtime != last_mtime && mtime.is_some() {
                    last_mtime = mtime;
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    cx.update(|cx| {
                        crate::vm::with_lua(|lua| {
                            if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                                context::with_cx(cx, || {
                                    if let Err(e) = f.call::<()>(content.clone()) {
                                        tracing::error!("watch_file callback error: {e}");
                                    }
                                });
                            }
                        });
                    });
                }
            }
        })
        .detach();
    });

    Ok(())
}

fn lua_exec_async(lua: &Lua, (cmd, callback): (String, LuaFunction)) -> LuaResult<()> {
    let key = lua.create_registry_value(callback)?;

    context::current_cx(|cx| {
        cx.spawn(async move |cx| {
            let output = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default()
                })
                .await;

            cx.update(|cx| {
                crate::vm::with_lua(|lua| {
                    if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                        context::with_cx(cx, || {
                            if let Err(e) = f.call::<()>(output) {
                                tracing::error!("exec_async callback error: {e}");
                            }
                        });
                    }
                });
            });
        })
        .detach();
    });

    Ok(())
}

/// Parse `"#rrggbb"` or `"rrggbb"` to a u32. Falls back to `default`.
fn parse_color(s: String, default: u32) -> u32 {
    let s = s.trim_start_matches('#');
    u32::from_str_radix(s, 16).unwrap_or(default)
}

fn lua_quit(_lua: &Lua, (): ()) -> LuaResult<()> {
    context::current_cx(|cx| cx.quit());
    Ok(())
}
