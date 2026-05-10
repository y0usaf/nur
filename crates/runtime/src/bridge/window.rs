//! GPUI window creation and the `LuaView` render bridge.
//!
//! `open_shell_window` creates a layer-shell window and returns a
//! `LuaWindowHandle` userdata. The user then calls `handle:render(fn)` to
//! attach a Lua render function. GPUI calls `LuaView::render` on every
//! dirty frame, which invokes the stored Lua function and converts the
//! returned element table to GPUI elements.

use anyhow::Result;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, Bounds, Context, DisplayId, Render, Size,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
    WindowOptions, div, layer_shell::*, point, prelude::*, px, rgb, rgba,
};
use mlua::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::bridge::element::lua_table_to_any_element;

// ---------------------------------------------------------------------------
// Named window registry
// ---------------------------------------------------------------------------

thread_local! {
    static WINDOW_REGISTRY: RefCell<HashMap<String, LuaWindowHandle>> = RefCell::new(HashMap::new());
}
thread_local! {
    static LUA_VIEW_REGISTRY: RefCell<Vec<(WeakEntity<LuaView>, WindowHandle<LuaView>)>> = const { RefCell::new(Vec::new()) };
}

/// Register a window handle under a name so it can be retrieved later.
pub fn register_window(name: String, handle: LuaWindowHandle) {
    WINDOW_REGISTRY.with(|r| r.borrow_mut().insert(name, handle));
}

/// Look up a previously registered window handle by name.
pub fn get_window(name: &str) -> Option<LuaWindowHandle> {
    WINDOW_REGISTRY.with(|r| r.borrow().get(name).cloned())
}

/// Mark all live Lua-backed views dirty after reactive Lua state changes.
pub(crate) fn notify_all_lua_views(cx: &mut App) {
    let views = LUA_VIEW_REGISTRY.with(|views| views.borrow().clone());

    for (weak, window_handle) in &views {
        if weak.upgrade().is_some() {
            let _ = window_handle.update(cx, |_view, window, cx| {
                cx.notify();
                window.refresh();
            });
        }
    }

    LUA_VIEW_REGISTRY.with(|views| {
        views
            .borrow_mut()
            .retain(|(weak, _)| weak.upgrade().is_some());
    });
}

// ---------------------------------------------------------------------------
// LuaView — the GPUI view whose content is defined by a Lua function
// ---------------------------------------------------------------------------

/// The GPUI view whose content is driven by a Lua render function.
///
/// GPUI calls `render` on every dirty frame. The render function is stored
/// as a `LuaRegistryKey` (which is `'static`) rather than a `LuaFunction`
/// (which is lifetime-bound and cannot be stored in a struct).
pub struct LuaView {
    /// Registry key for the Lua render function; `None` until `handle:render(fn)` is called.
    render_key: Option<LuaRegistryKey>,
    bg: u32,
    fg: u32,
    font_size: f32,
    font_family: Option<String>,
    /// Whether the window content is visible. When `false`, `render()` returns
    /// an empty zero-size transparent div, effectively hiding the window.
    visible: bool,
}

impl LuaView {
    fn new(
        bg: u32,
        fg: u32,
        font_size: f32,
        font_family: Option<String>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            render_key: None,
            bg,
            fg,
            font_size,
            font_family,
            visible: true,
        }
    }

    pub fn set_render_fn(&mut self, key: LuaRegistryKey) {
        self.render_key = Some(key);
    }
}

impl Render for LuaView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg, font_size) = (self.bg, self.fg, self.font_size);
        let font_family = self.font_family.clone();

        // When hidden, render an empty transparent zero-size div.
        if !self.visible {
            return div().w(px(0.0)).h(px(0.0)).into_any_element();
        }

        let Some(key) = &self.render_key else {
            return div().size_full().bg(rgba(bg)).into_any_element();
        };

        // Reset button ID sequence so IDs are stable across frames (same
        // render order = same ID = GPUI can track click state correctly).
        crate::bridge::element::next_frame();

        // Wrap everything in a full-size flex container so the user's root
        // element fills the window and spacers work correctly.
        let content = crate::vm::with_lua(|lua| -> AnyElement {
            (|| -> LuaResult<AnyElement> {
                let f: LuaFunction = lua.registry_value(key)?;
                lua_table_to_any_element(f.call(())?)
            })()
            .unwrap_or_else(|e| {
                tracing::error!("Lua render error: {e}");
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .p(px(8.0))
                    .bg(rgb(0xf38ba8))
                    .text_color(rgb(0x1e1e2e))
                    .rounded(px(4.0))
                    .child("⚠ Render Error")
                    .child(format!("{e}"))
                    .into_any_element()
            })
        });

        let mut root = div()
            .size_full()
            .flex()
            .flex_row()
            .items_center()
            .text_color(rgb(fg >> 8))
            .text_size(px(font_size))
            .font_family(font_family.as_deref().unwrap_or("monospace").to_string())
            .child(
                div()
                    .w_full()
                    .h_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(content),
            );

        // For transparent layer-shell windows, avoid painting even a fully
        // transparent quad over the platform surface. Some Wayland/GPUI
        // combinations show that as the default grey window background.
        if bg & 0xff != 0 {
            root = root.bg(rgba(bg));
        }

        root.into_any_element()
    }
}

// ---------------------------------------------------------------------------
// LuaWindowHandle — Lua userdata returned by shell.window()
// ---------------------------------------------------------------------------

/// Lua userdata returned by `shell.window()`. Weak reference so the handle
/// does not keep the window alive if GPUI closes it.
#[derive(Clone)]
pub struct LuaWindowHandle {
    entity: WeakEntity<LuaView>,
    window: AnyWindowHandle,
}

impl LuaUserData for LuaWindowHandle {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // bar:render(function() return ui.hbox(...) end)
        methods.add_method("render", |lua, this, callback: LuaFunction| {
            let key = lua.create_registry_value(callback)?;
            let entity = this.entity.clone();

            crate::context::current_cx(|cx| {
                if let Some(e) = entity.upgrade() {
                    e.update(cx, |view, cx| {
                        view.set_render_fn(key);
                        cx.notify();
                    });
                }
            });

            Ok(())
        });

        // panel:close() — remove the window from the compositor
        methods.add_method("close", |_lua, this, ()| {
            let window = this.window;
            crate::context::current_cx(|cx| {
                let _ = cx.update_window(window, |_, window, _| window.remove_window());
            });
            Ok(())
        });

        // handle:hide() — hide the window without destroying it
        methods.add_method("hide", |_lua, this, ()| {
            let entity = this.entity.clone();
            crate::context::current_cx(|cx| {
                if let Some(e) = entity.upgrade() {
                    e.update(cx, |view, cx| {
                        view.visible = false;
                        cx.notify();
                    });
                }
            });
            Ok(())
        });

        // handle:show() — make the window visible again
        methods.add_method("show", |_lua, this, ()| {
            let entity = this.entity.clone();
            crate::context::current_cx(|cx| {
                if let Some(e) = entity.upgrade() {
                    e.update(cx, |view, cx| {
                        view.visible = true;
                        cx.notify();
                    });
                }
            });
            Ok(())
        });

        // handle:toggle() — flip visibility
        methods.add_method("toggle", |_lua, this, ()| {
            let entity = this.entity.clone();
            crate::context::current_cx(|cx| {
                if let Some(e) = entity.upgrade() {
                    e.update(cx, |view, cx| {
                        view.visible = !view.visible;
                        cx.notify();
                    });
                }
            });
            Ok(())
        });

        // handle:is_visible() -> bool
        methods.add_method("is_visible", |_lua, this, ()| {
            let entity = this.entity.clone();
            let visible = crate::context::current_cx(|cx| {
                entity
                    .upgrade()
                    .map(|e| e.read(cx).visible)
                    .unwrap_or(false)
            });
            Ok(visible)
        });
    }
}

// ---------------------------------------------------------------------------
// Window configuration and creation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum BarPosition {
    Top,
    Bottom,
    Left,
    Right,
}

impl BarPosition {
    pub fn from_str(s: &str) -> Self {
        match s {
            "bottom" => Self::Bottom,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::Top,
        }
    }
}

/// Configuration for a layer-shell window, parsed from the Lua `shell.window({})` call.
pub struct WindowConfig {
    pub position: BarPosition,
    /// Thickness in pixels — height for top/bottom bars, width for left/right.
    pub size: f32,
    /// If set, overrides the anchor derived from `position`. Lua values:
    /// "top", "bottom", "left", "right",
    /// "top-left", "top-right", "bottom-left", "bottom-right",
    /// "top-center" (LEFT|RIGHT|TOP with equal h-margins), "bottom-center".
    pub anchor: Option<String>,
    /// Fixed width for popup windows. Required when `anchor` is set.
    pub popup_width: Option<f32>,
    /// Margins in pixels: (top, right, bottom, left). Applied only to anchored edges.
    pub margin: [f32; 4],
    /// If true, an exclusive zone is set so other windows don't overlap the bar.
    pub exclusive: bool,
    pub layer: Layer,
    pub bg: u32, // 0xRRGGBBFF
    pub fg: u32,
    pub font_size: f32,
    pub font_family: Option<String>,
    /// Keyboard interactivity mode for the layer-shell surface.
    pub keyboard: KeyboardInteractivity,
    /// Optional name for the window registry, allowing retrieval via `shell.get_window()`.
    pub name: Option<String>,
    /// Target display index (0-based). When set, the window opens on that monitor.
    pub monitor: Option<usize>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            position: BarPosition::Top,
            size: 32.0,
            anchor: None,
            popup_width: None,
            margin: [0.0; 4],
            exclusive: true,
            layer: Layer::Top,
            bg: 0x1e1e2eff,
            fg: 0xcdd6f4ff,
            font_size: 13.0,
            font_family: None,
            keyboard: KeyboardInteractivity::None,
            name: None,
            monitor: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- BarPosition::from_str ---

    #[test]
    fn bar_position_top() {
        assert!(matches!(BarPosition::from_str("top"), BarPosition::Top));
    }

    #[test]
    fn bar_position_bottom() {
        assert!(matches!(
            BarPosition::from_str("bottom"),
            BarPosition::Bottom
        ));
    }

    #[test]
    fn bar_position_left() {
        assert!(matches!(BarPosition::from_str("left"), BarPosition::Left));
    }

    #[test]
    fn bar_position_right() {
        assert!(matches!(BarPosition::from_str("right"), BarPosition::Right));
    }

    #[test]
    fn bar_position_unknown_defaults_to_top() {
        assert!(matches!(BarPosition::from_str("center"), BarPosition::Top));
    }

    #[test]
    fn bar_position_empty_defaults_to_top() {
        assert!(matches!(BarPosition::from_str(""), BarPosition::Top));
    }

    #[test]
    fn bar_position_case_sensitive_uppercase_defaults_to_top() {
        assert!(matches!(BarPosition::from_str("TOP"), BarPosition::Top));
        assert!(matches!(BarPosition::from_str("Bottom"), BarPosition::Top));
    }

    // --- WindowConfig::default ---

    #[test]
    fn window_config_default_position_is_top() {
        let c = WindowConfig::default();
        assert!(matches!(c.position, BarPosition::Top));
    }

    #[test]
    fn window_config_default_size() {
        let c = WindowConfig::default();
        assert!((c.size - 32.0).abs() < f32::EPSILON);
    }

    #[test]
    fn window_config_default_exclusive() {
        assert!(WindowConfig::default().exclusive);
    }

    #[test]
    fn window_config_default_bg_color() {
        assert_eq!(WindowConfig::default().bg, 0x1e1e2eff);
    }

    #[test]
    fn window_config_default_fg_color() {
        assert_eq!(WindowConfig::default().fg, 0xcdd6f4ff);
    }

    #[test]
    fn window_config_default_font_size() {
        let c = WindowConfig::default();
        assert!((c.font_size - 13.0).abs() < f32::EPSILON);
    }

    #[test]
    fn window_config_default_keyboard_is_none() {
        let c = WindowConfig::default();
        assert!(matches!(c.keyboard, KeyboardInteractivity::None));
    }

    #[test]
    fn window_config_default_name_is_none() {
        let c = WindowConfig::default();
        assert!(c.name.is_none());
    }

    // --- Window registry ---

    #[test]
    fn window_registry_get_missing_returns_none() {
        assert!(get_window("nonexistent").is_none());
    }
}

/// Open a layer-shell window and return a handle the Lua config can use.
pub fn open_shell_window(config: WindowConfig, cx: &mut App) -> Result<LuaWindowHandle> {
    let target_display = config.monitor.and_then(|idx| {
        let displays = cx.displays();
        displays.get(idx).cloned()
    });

    let display_size = target_display
        .as_ref()
        .map(|d| d.bounds().size)
        .or_else(|| cx.primary_display().map(|d| d.bounds().size))
        .unwrap_or_else(|| Size::new(px(1920.0), px(1080.0)));

    let target_display_id: Option<DisplayId> = target_display.as_ref().map(|d| d.id());

    // Determine anchor bits, window size, and margins.
    //
    // If `config.anchor` is set (popup mode), parse it directly.
    // Otherwise derive from `config.position` (full-width bar mode).
    let (window_size, anchor, margin_opt) = if let Some(ref anchor_str) = config.anchor {
        let w = config.popup_width.map(px).unwrap_or(px(320.0));
        let h = px(config.size);
        let [mt, mr, mb, ml] = config.margin.map(px);

        let (a, margin) = match anchor_str.as_str() {
            "top-right" => (Anchor::TOP | Anchor::RIGHT, Some((mt, mr, mb, ml))),
            "top-left" => (Anchor::TOP | Anchor::LEFT, Some((mt, mr, mb, ml))),
            "bottom-right" => (Anchor::BOTTOM | Anchor::RIGHT, Some((mt, mr, mb, ml))),
            "bottom-left" => (Anchor::BOTTOM | Anchor::LEFT, Some((mt, mr, mb, ml))),
            "top-center" => (Anchor::TOP, Some((mt, mr, mb, ml))),
            "bottom-center" => (Anchor::BOTTOM, Some((mt, mr, mb, ml))),
            "top" => (Anchor::TOP, Some((mt, mr, mb, ml))),
            "bottom" => (Anchor::BOTTOM, Some((mt, mr, mb, ml))),
            "left" => (Anchor::LEFT, Some((mt, mr, mb, ml))),
            "right" => (Anchor::RIGHT, Some((mt, mr, mb, ml))),
            _ => (Anchor::TOP | Anchor::RIGHT, Some((mt, mr, mb, ml))),
        };
        (Size::new(w, h), a, margin)
    } else {
        // Bar mode: derive from position, full-width/height, no margin.
        let (sz, a) = match config.position {
            BarPosition::Top => (
                Size::new(display_size.width, px(config.size)),
                Anchor::LEFT | Anchor::RIGHT | Anchor::TOP,
            ),
            BarPosition::Bottom => (
                Size::new(display_size.width, px(config.size)),
                Anchor::LEFT | Anchor::RIGHT | Anchor::BOTTOM,
            ),
            BarPosition::Left => (
                Size::new(px(config.size), display_size.height),
                Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT,
            ),
            BarPosition::Right => (
                Size::new(px(config.size), display_size.height),
                Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT,
            ),
        };
        (sz, a, None)
    };

    let exclusive_zone = config.exclusive.then_some(px(config.size));
    let margin = margin_opt;

    let options = WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: window_size,
        })),
        app_id: Some("nur".to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        display_id: target_display_id,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "nur".to_string(),
            layer: config.layer,
            anchor,
            exclusive_zone,
            margin,
            keyboard_interactivity: config.keyboard,
            ..Default::default()
        }),
        ..Default::default()
    };

    // Capture the entity handle from inside the builder closure.
    // The closure runs synchronously so this is safe without a mutex.
    let captured: Rc<RefCell<Option<WeakEntity<LuaView>>>> = Rc::new(RefCell::new(None));
    let cap2 = captured.clone();

    let (cfg_bg, cfg_fg, cfg_fs, cfg_ff) = (
        config.bg,
        config.fg,
        config.font_size,
        config.font_family.clone(),
    );
    let window_handle = cx
        .open_window(options, move |_, cx| {
            let entity = cx.new(|cx| LuaView::new(cfg_bg, cfg_fg, cfg_fs, cfg_ff.clone(), cx));
            *cap2.borrow_mut() = Some(entity.downgrade());
            entity
        })
        .map_err(|e| anyhow::anyhow!("Failed to open window: {e}"))?;

    // Work around a Vulkan swapchain "out of date" issue that occurs on layer-shell
    // windows: after the compositor sends its first configure event, the swapchain
    // is recreated but may immediately become out-of-date again if the compositor
    // sends a second configure. Triggering one extra resize after a short delay
    // forces another `reconfigure_surface` call that recovers the swapchain.
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(200))
            .await;
        let _ = cx.update_window(window_handle.into(), |_, window, _| {
            window.resize(window_size);
        });
    })
    .detach();

    let weak = captured
        .borrow_mut()
        .take()
        .expect("open_window builder did not set entity");

    LUA_VIEW_REGISTRY.with(|views| views.borrow_mut().push((weak.clone(), window_handle)));

    Ok(LuaWindowHandle {
        entity: weak,
        window: window_handle.into(),
    })
}
