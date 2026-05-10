//! `shell.*` Lua API — window creation, timers, state, utilities.

use gpui::{App, ClipboardEntry, ClipboardItem};
use mlua::prelude::*;
use std::cell::RefCell;
use std::time::Duration;

use gpui::layer_shell::{KeyboardInteractivity, Layer};

use crate::bridge::{
    state::LuaState,
    window::{
        BarPosition, LuaWindowHandle, WindowConfig, get_window, open_shell_window, register_window,
    },
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

    // shell.get_window(name) -> LuaWindowHandle | nil
    shell.set("get_window", lua.create_function(lua_get_window)?)?;

    // shell.quit() — gracefully stop nur
    shell.set("quit", lua.create_function(lua_quit)?)?;

    // shell.on_msg(fn) — register a handler called by `nur msg <text>`
    shell.set("on_msg", lua.create_function(lua_on_msg)?)?;

    // shell.clipboard_read() -> string | nil
    shell.set("clipboard_read", lua.create_function(lua_clipboard_read)?)?;

    // shell.clipboard_write(text)
    shell.set("clipboard_write", lua.create_function(lua_clipboard_write)?)?;

    // shell.displays() -> table of display info
    shell.set("displays", lua.create_function(lua_displays)?)?;

    // shell.reload() — re-execute the config file
    shell.set("reload", lua.create_function(lua_reload)?)?;

    // shell.tween(from, to, duration_ms, easing, callback) — animate a value
    shell.set("tween", lua.create_function(lua_tween)?)?;

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
    crate::vm::with_lua(|lua| match lua.registry_value::<LuaFunction>(key) {
        Ok(f) => context::with_cx(cx, || {
            if let Err(e) = f.call::<()>(arg) {
                tracing::error!("{label} callback error: {e}");
            }
        }),
        Err(e) => tracing::error!("{label} registry_value failed: {e}"),
    });
}

fn lua_window(_lua: &Lua, config: LuaTable) -> LuaResult<LuaWindowHandle> {
    let position = BarPosition::from_str(
        &config
            .get::<String>("position")
            .unwrap_or_else(|_| "top".into()),
    );
    let size: f32 = config
        .get("height")
        .or_else(|_| config.get("width"))
        .unwrap_or(32.0);
    let exclusive: bool = config.get("exclusive").unwrap_or(true);
    let layer_str: String = config.get("layer").unwrap_or_else(|_| "top".into());
    let layer = match layer_str.as_str() {
        "background" => Layer::Background,
        "bottom" => Layer::Bottom,
        "overlay" => Layer::Overlay,
        _ => Layer::Top,
    };

    let bg = parse_color(config.get::<String>("bg").unwrap_or_default(), 0x1e1e2eff);
    let fg = parse_color(config.get::<String>("fg").unwrap_or_default(), 0xcdd6f4ff);
    let font_size: f32 = config.get("font_size").unwrap_or(13.0);
    let font_family: Option<String> = config.get("font_family").ok();
    let popup_width: Option<f32> = config.get("popup_width").ok();
    let anchor: Option<String> = config.get("anchor").ok();
    let margin_top: f32 = config.get("margin_top").unwrap_or(0.0);
    let margin_right: f32 = config.get("margin_right").unwrap_or(0.0);
    let margin_bottom: f32 = config.get("margin_bottom").unwrap_or(0.0);
    let margin_left: f32 = config.get("margin_left").unwrap_or(0.0);

    let keyboard_str: String = config.get("keyboard").unwrap_or_else(|_| "none".into());
    let keyboard = parse_keyboard_interactivity(&keyboard_str);
    let name: Option<String> = config.get("name").ok();
    let monitor: Option<usize> = config.get::<usize>("monitor").ok();

    let win_config = WindowConfig {
        position,
        size,
        anchor,
        popup_width,
        margin: [margin_top, margin_right, margin_bottom, margin_left],
        exclusive,
        layer,
        bg,
        fg,
        font_size,
        font_family,
        keyboard,
        name: name.clone(),
        monitor,
    };

    let handle = context::current_cx(|cx| {
        open_shell_window(win_config, cx).map_err(|e| LuaError::RuntimeError(e.to_string()))
    })?;

    if let Some(name) = name {
        register_window(name, handle.clone());
    }

    Ok(handle)
}

fn lua_state(_lua: &Lua, initial: LuaValue) -> LuaResult<LuaState> {
    Ok(LuaState::new(initial))
}

fn lua_interval(lua: &Lua, (ms, callback): (u64, LuaFunction)) -> LuaResult<()> {
    let key = lua.create_registry_value(callback)?;

    context::current_cx(|cx| {
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(ms))
                    .await;
                cx.update(|cx| call_lua_key(cx, &key, (), "interval"));
            }
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
            let mut last_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;

                let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

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

/// Parse a color string to a `0xRRGGBBAA` u32 for use with `gpui::rgba()`.
///
/// Accepts:
/// - `"#rrggbb"` / `"rrggbb"` → fully opaque (alpha = 0xff)
/// - `"#rrggbbaa"` / `"rrggbbaa"` → explicit alpha
/// - `"transparent"` → `0x00000000`
///
/// Falls back to `default` on parse failure.
fn parse_color(s: String, default: u32) -> u32 {
    let s = s.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return 0x00000000;
    }
    let s = s.trim_start_matches('#');
    match s.len() {
        6 => u32::from_str_radix(s, 16)
            .map(|c| (c << 8) | 0xff)
            .unwrap_or(default),
        8 => u32::from_str_radix(s, 16).unwrap_or(default),
        _ => default,
    }
}

fn parse_keyboard_interactivity(s: &str) -> KeyboardInteractivity {
    match s {
        "exclusive" => KeyboardInteractivity::Exclusive,
        "on_demand" => KeyboardInteractivity::OnDemand,
        _ => KeyboardInteractivity::None,
    }
}

fn lua_get_window(_lua: &Lua, name: String) -> LuaResult<Option<LuaWindowHandle>> {
    Ok(get_window(&name))
}

fn lua_quit(_lua: &Lua, (): ()) -> LuaResult<()> {
    context::current_cx(|cx| cx.quit());
    Ok(())
}

// ---------------------------------------------------------------------------
// shell.on_msg — IPC message handler
// ---------------------------------------------------------------------------

thread_local! {
    /// Registry key for the function registered via `shell.on_msg(fn)`.
    static MSG_HANDLER: RefCell<Option<LuaRegistryKey>> = const { RefCell::new(None) };
}

fn lua_on_msg(lua: &Lua, callback: LuaFunction) -> LuaResult<()> {
    let key = lua.create_registry_value(callback)?;
    MSG_HANDLER.with(|cell| *cell.borrow_mut() = Some(key));
    Ok(())
}

fn lua_clipboard_read(_lua: &Lua, (): ()) -> LuaResult<Option<String>> {
    Ok(context::current_cx(|cx| {
        cx.read_from_clipboard().and_then(|item| {
            item.entries().into_iter().find_map(|entry| match entry {
                ClipboardEntry::String(s) => Some(s.text().to_string()),
                _ => None,
            })
        })
    }))
}

fn lua_clipboard_write(_lua: &Lua, text: String) -> LuaResult<()> {
    context::current_cx(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    });
    Ok(())
}

fn lua_displays(lua: &Lua, (): ()) -> LuaResult<LuaTable> {
    context::current_cx(|cx| {
        let displays = cx.displays();
        let result = lua.create_table()?;
        for (i, display) in displays.iter().enumerate() {
            let d = lua.create_table()?;
            let bounds = display.bounds();
            d.set("id", format!("{:?}", display.id()))?;
            d.set("x", f32::from(bounds.origin.x))?;
            d.set("y", f32::from(bounds.origin.y))?;
            d.set("width", f32::from(bounds.size.width))?;
            d.set("height", f32::from(bounds.size.height))?;
            d.set("is_primary", i == 0)?; // first display is typically primary
            result.set(i + 1, d)?;
        }
        Ok(result)
    })
}

// ---------------------------------------------------------------------------
// shell.reload — re-execute config
// ---------------------------------------------------------------------------

thread_local! {
    /// The config path, set by the daemon at startup.
    static CONFIG_PATH: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

/// Set the config path so `shell.reload()` knows what to re-execute.
pub fn set_config_path(path: std::path::PathBuf) {
    CONFIG_PATH.with(|cell| *cell.borrow_mut() = Some(path));
}

fn lua_reload(_lua: &Lua, (): ()) -> LuaResult<()> {
    let path = CONFIG_PATH.with(|cell| cell.borrow().clone());
    let Some(path) = path else {
        return Err(LuaError::RuntimeError("config path not set".into()));
    };
    let code = std::fs::read_to_string(&path)
        .map_err(|e| LuaError::RuntimeError(format!("Cannot read config: {e}")))?;
    context::current_cx(|cx| {
        crate::vm::with_lua(|lua| {
            context::with_cx(cx, || {
                if let Err(e) = lua
                    .load(&code)
                    .set_name(path.to_str().unwrap_or("init.lua"))
                    .exec()
                {
                    tracing::error!("Reload error: {e}");
                }
            });
        });
    });
    Ok(())
}

fn lua_tween(
    lua: &Lua,
    (from, to, duration_ms, easing, callback): (f64, f64, u64, Option<String>, LuaFunction),
) -> LuaResult<()> {
    let key = lua.create_registry_value(callback)?;
    let easing = easing.unwrap_or_else(|| "linear".into());
    let frame_ms: u64 = 16; // ~60fps
    let total_frames = (duration_ms / frame_ms).max(1);

    context::current_cx(|cx| {
        cx.spawn(async move |cx| {
            for frame in 0..=total_frames {
                let t = frame as f64 / total_frames as f64;
                let t = apply_easing(&easing, t);
                let value = from + (to - from) * t;

                cx.update(|cx| {
                    call_lua_key(cx, &key, value, "tween");
                });

                if frame < total_frames {
                    cx.background_executor()
                        .timer(Duration::from_millis(frame_ms))
                        .await;
                }
            }
        })
        .detach();
    });

    Ok(())
}

fn apply_easing(name: &str, t: f64) -> f64 {
    match name {
        "ease_in" => t * t,
        "ease_out" => 1.0 - (1.0 - t) * (1.0 - t),
        "ease_in_out" => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                let x = -2.0 * t + 2.0;
                1.0 - x * x / 2.0
            }
        }
        _ => t, // linear
    }
}

/// Invoke the `shell.on_msg` handler from inside a GPUI context.
/// Returns the string result or an error string.
pub fn invoke_msg_handler(cx: &mut App, message: String) -> Result<String, String> {
    crate::vm::with_lua(|lua| {
        MSG_HANDLER.with(|cell| {
            let borrow = cell.borrow();
            let Some(key) = borrow.as_ref() else {
                return Ok(String::new());
            };
            let call_result: LuaResult<LuaValue> = lua
                .registry_value::<LuaFunction>(key)
                .and_then(|f| context::with_cx(cx, || f.call::<LuaValue>(message.clone())));
            match call_result {
                Ok(LuaValue::String(s)) => {
                    Ok(s.to_str().map(|b| b.to_string()).unwrap_or_default())
                }
                Ok(LuaValue::Nil) | Ok(LuaValue::Boolean(false)) => Ok(String::new()),
                Ok(v) => Ok(format!("{v:?}")),
                Err(e) => Err(format!("on_msg handler error: {e}")),
            }
        })
    })
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

    // --- keyboard interactivity parsing ---

    #[test]
    fn parse_keyboard_none() {
        assert!(matches!(
            parse_keyboard_interactivity("none"),
            KeyboardInteractivity::None
        ));
    }

    #[test]
    fn parse_keyboard_exclusive() {
        assert!(matches!(
            parse_keyboard_interactivity("exclusive"),
            KeyboardInteractivity::Exclusive
        ));
    }

    #[test]
    fn parse_keyboard_on_demand() {
        assert!(matches!(
            parse_keyboard_interactivity("on_demand"),
            KeyboardInteractivity::OnDemand
        ));
    }

    #[test]
    fn parse_keyboard_unknown_defaults_to_none() {
        assert!(matches!(
            parse_keyboard_interactivity("something_else"),
            KeyboardInteractivity::None
        ));
    }
}
