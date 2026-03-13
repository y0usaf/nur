//! GPUI window creation and the `LuaView` render bridge.
//!
//! `open_shell_window` creates a layer-shell window and returns a
//! `LuaWindowHandle` userdata. The user then calls `handle:render(fn)` to
//! attach a Lua render function. GPUI calls `LuaView::render` on every
//! dirty frame, which invokes the stored Lua function and converts the
//! returned element table to GPUI elements.

use anyhow::Result;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Render,
    Size, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, div, layer_shell::*, point, prelude::*, px,
};
use mlua::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::bridge::element::lua_table_to_any_element;

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
    bg:        u32,
    fg:        u32,
    font_size: f32,
}

impl LuaView {
    fn new(bg: u32, fg: u32, font_size: f32, _cx: &mut Context<Self>) -> Self {
        Self { render_key: None, bg, fg, font_size }
    }

    pub fn set_render_fn(&mut self, key: LuaRegistryKey) {
        self.render_key = Some(key);
    }
}

impl Render for LuaView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, fg, font_size) = (self.bg, self.fg, self.font_size);

        let Some(key) = &self.render_key else {
            return div().size_full().bg(gpui::rgb(bg)).into_any_element();
        };

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
                    .child(format!("Lua render error: {e}"))
                    .into_any_element()
            })
        });

        div()
            .size_full()
            .flex()
            .items_center()
            .bg(gpui::rgb(bg))
            .text_color(gpui::rgb(fg))
            .text_size(px(font_size))
            .child(content)
            .into_any_element()
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
            "left"   => Self::Left,
            "right"  => Self::Right,
            _        => Self::Top,
        }
    }
}

/// Configuration for a layer-shell window, parsed from the Lua `shell.window({})` call.
pub struct WindowConfig {
    pub position:  BarPosition,
    /// Thickness in pixels — height for top/bottom bars, width for left/right.
    pub size:      f32,
    /// If true, an exclusive zone is set so other windows don't overlap the bar.
    pub exclusive: bool,
    pub layer:     Layer,
    pub bg:        u32,  // 0xRRGGBB
    pub fg:        u32,
    pub font_size: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            position:  BarPosition::Top,
            size:      32.0,
            exclusive: true,
            layer:     Layer::Top,
            bg:        0x1e1e2e,
            fg:        0xcdd6f4,
            font_size: 13.0,
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
        assert!(matches!(BarPosition::from_str("bottom"), BarPosition::Bottom));
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
        assert_eq!(WindowConfig::default().bg, 0x1e1e2e);
    }

    #[test]
    fn window_config_default_fg_color() {
        assert_eq!(WindowConfig::default().fg, 0xcdd6f4);
    }

    #[test]
    fn window_config_default_font_size() {
        let c = WindowConfig::default();
        assert!((c.font_size - 13.0).abs() < f32::EPSILON);
    }
}

/// Open a layer-shell window and return a handle the Lua config can use.
pub fn open_shell_window(config: WindowConfig, cx: &mut App) -> Result<LuaWindowHandle> {
    let display_size = cx
        .primary_display()
        .map(|d| d.bounds().size)
        .unwrap_or_else(|| Size::new(px(1920.0), px(1080.0)));

    let (window_size, anchor) = match config.position {
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

    let exclusive_zone = config.exclusive.then_some(px(config.size));

    let options = WindowOptions {
        titlebar: None,
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size:   window_size,
        })),
        app_id: Some("nur".to_string()),
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::LayerShell(LayerShellOptions {
            namespace: "nur".to_string(),
            layer: config.layer,
            anchor,
            exclusive_zone,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        }),
        ..Default::default()
    };

    // Capture the entity handle from inside the builder closure.
    // The closure runs synchronously so this is safe without a mutex.
    let captured: Rc<RefCell<Option<WeakEntity<LuaView>>>> = Rc::new(RefCell::new(None));
    let cap2 = captured.clone();

    let (cfg_bg, cfg_fg, cfg_fs) = (config.bg, config.fg, config.font_size);
    cx.open_window(options, move |_, cx| {
        let entity = cx.new(|cx| LuaView::new(cfg_bg, cfg_fg, cfg_fs, cx));
        *cap2.borrow_mut() = Some(entity.downgrade());
        entity
    })
    .map_err(|e| anyhow::anyhow!("Failed to open window: {e}"))?;

    let weak = captured
        .borrow_mut()
        .take()
        .expect("open_window builder did not set entity");

    // Register this view so that LuaState::set() can trigger re-renders.
    let notify_weak = weak.clone();
    crate::context::register_view_notifier(move |cx| {
        if let Some(entity) = notify_weak.upgrade() {
            entity.update(cx, |_, cx| cx.notify());
        }
    });

    Ok(LuaWindowHandle { entity: weak })
}
