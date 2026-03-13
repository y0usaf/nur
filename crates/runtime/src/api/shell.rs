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

/// Retrieve a Lua function from the registry and call it with `arg`.
/// Must be called from inside `cx.update(|cx| ...)` so a GPUI context is active.
fn call_lua_key<A: mlua::IntoLuaMulti>(
    cx: &mut App,
    key: &LuaRegistryKey,
    arg: A,
    label: &'static str,
) {
    crate::vm::with_lua(|lua| {
        if let Ok(f) = lua.registry_value::<LuaFunction>(key) {
            context::with_cx(cx, || {
                if let Err(e) = f.call::<()>(arg) {
                    tracing::error!("{label} callback error: {e}");
                }
            });
        }
    });
}

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

            cx.update(|cx| call_lua_key(cx, &key, (), "interval"));
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

            cx.update(|cx| call_lua_key(cx, &key, (), "once"));
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
                    cx.update(|cx| call_lua_key(cx, &key, content, "watch_file"));
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

            cx.update(|cx| call_lua_key(cx, &key, output, "exec_async"));
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_color ---

    #[test]
    fn parse_color_with_hash_prefix() {
        assert_eq!(parse_color("#1e1e2e".into(), 0), 0x1e1e2e);
    }

    #[test]
    fn parse_color_without_hash_prefix() {
        assert_eq!(parse_color("1e1e2e".into(), 0), 0x1e1e2e);
    }

    #[test]
    fn parse_color_all_zeros() {
        assert_eq!(parse_color("#000000".into(), 0xff), 0x000000);
    }

    #[test]
    fn parse_color_all_f() {
        assert_eq!(parse_color("ffffff".into(), 0), 0xffffff);
    }

    #[test]
    fn parse_color_mixed_case() {
        // hex is case-insensitive
        assert_eq!(parse_color("AABBCC".into(), 0), 0xaabbcc);
    }

    #[test]
    fn parse_color_empty_string_returns_default() {
        assert_eq!(parse_color("".into(), 0xdeadbe), 0xdeadbe);
    }

    #[test]
    fn parse_color_invalid_string_returns_default() {
        assert_eq!(parse_color("not-a-color".into(), 0xff0000), 0xff0000);
    }

    #[test]
    fn parse_color_catppuccin_mocha_base() {
        // Actual default colours used in WindowConfig::default
        assert_eq!(parse_color("#1e1e2e".into(), 0), 0x1e1e2e);
        assert_eq!(parse_color("#cdd6f4".into(), 0), 0xcdd6f4);
    }

    #[test]
    fn parse_color_hash_only_returns_default() {
        assert_eq!(parse_color("#".into(), 0x123456), 0x123456);
    }

    #[test]
    fn parse_color_whitespace_returns_default() {
        // Leading/trailing spaces are not stripped — should fall back
        assert_eq!(parse_color("  ffffff  ".into(), 0xabcdef), 0xabcdef);
    }
}
