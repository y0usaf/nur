//! Lua table → GPUI `AnyElement` conversion.
//!
//! Lua render functions return a nested table describing the element tree:
//!
//! ```lua
//! return ui.hbox({ gap = 8, children = {
//!   ui.text("Hello"),
//!   ui.spacer(),
//!   ui.icon("battery"),
//! }})
//! ```
//!
//! `lua_table_to_any_element` walks the tree recursively and produces GPUI
//! elements in one pass. To add a new element type, add a match arm here and
//! a corresponding pure-Lua constructor in `lua/nur/stdlib.lua`.

use gpui::{
    AnyElement, AssetSource, Div, FontWeight, InteractiveElement, IntoElement, MouseButton,
    SharedString, StatefulInteractiveElement, Styled, div, img, prelude::*, px, relative, rgb, svg,
};
use mlua::prelude::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Per-frame counter, reset each render pass by `next_frame()`.
/// Used to generate stable-within-frame, unique-across-buttons IDs.
static BUTTON_ID: AtomicUsize = AtomicUsize::new(0);

/// Call at the start of each render pass to reset button ID sequencing.
/// This ensures button IDs are stable frame-to-frame (same render order = same ID).
pub fn next_frame() {
    BUTTON_ID.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Color parsing
// ---------------------------------------------------------------------------

/// Parse a color value from a Lua table field.
///
/// Accepts either a u32 (`0xRRGGBB`) or a string (`"#rrggbb"` / `"#rgb"`).
/// Returns `None` if the field is absent or nil.
fn parse_color(table: &LuaTable, key: &str) -> LuaResult<Option<u32>> {
    let val: LuaValue = table.get(key)?;
    match val {
        LuaValue::Integer(n) => Ok(Some(n as u32)),
        LuaValue::Number(n) => Ok(Some(n as u32)),
        LuaValue::String(s) => {
            let s: String = s
                .to_str()
                .map_err(|e| LuaError::RuntimeError(format!("invalid color string: {e}")))?
                .to_owned();
            let hex = s.strip_prefix('#').unwrap_or(&s);
            let parsed = match hex.len() {
                3 => {
                    let mut chars = hex.chars();
                    let r = chars.next().unwrap();
                    let g = chars.next().unwrap();
                    let b = chars.next().unwrap();
                    u32::from_str_radix(&format!("{r}{r}{g}{g}{b}{b}"), 16)
                }
                6 => u32::from_str_radix(hex, 16),
                _ => {
                    return Err(LuaError::RuntimeError(format!(
                        "invalid color format for '{key}': expected #RGB or #RRGGBB, got \"{s}\""
                    )));
                }
            };
            parsed
                .map(|c| Some(c))
                .map_err(|e| LuaError::RuntimeError(format!("invalid hex color for '{key}': {e}")))
        }
        LuaValue::Nil => Ok(None),
        _ => Err(LuaError::RuntimeError(format!(
            "'{key}' must be a number (0xRRGGBB) or string (\"#rrggbb\")"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Common style helpers
// ---------------------------------------------------------------------------

/// Apply common CSS-like style properties to a container div.
///
/// - `bg` — background color
/// - `border` / `border_color`
/// - `border_radius` / per-corner variants
/// - `opacity` (0.0–1.0)
/// - `width`, `height`, `min_width`, `max_width`, `min_height`, `max_height`
/// - `overflow` ("hidden")
/// - `cursor` ("pointer", "default", "text")
fn apply_common_style(mut el: Div, table: &LuaTable) -> LuaResult<Div> {
    // Background color
    if let Some(bg) = parse_color(table, "bg")? {
        el = el.bg(rgb(bg));
    }

    // Border
    let border_w: f32 = table.get("border").unwrap_or(0.0);
    if border_w > 0.0 {
        el = el.border(px(border_w));
        if let Some(c) = parse_color(table, "border_color")? {
            el = el.border_color(rgb(c));
        }
    }

    // Border radius — uniform
    if let Ok(r) = table.get::<f32>("border_radius") {
        el = el.rounded(px(r));
    }
    // Border radius — per-corner overrides
    if let Ok(r) = table.get::<f32>("border_radius_top_left") {
        el = el.rounded_tl(px(r));
    }
    if let Ok(r) = table.get::<f32>("border_radius_top_right") {
        el = el.rounded_tr(px(r));
    }
    if let Ok(r) = table.get::<f32>("border_radius_bottom_left") {
        el = el.rounded_bl(px(r));
    }
    if let Ok(r) = table.get::<f32>("border_radius_bottom_right") {
        el = el.rounded_br(px(r));
    }

    // Opacity
    if let Ok(o) = table.get::<f32>("opacity") {
        el = el.opacity(o);
    }

    // Sizing
    if let Ok(w) = table.get::<f32>("width") {
        el = el.w(px(w));
    }
    if let Ok(h) = table.get::<f32>("height") {
        el = el.h(px(h));
    }
    if let Ok(v) = table.get::<f32>("min_width") {
        el = el.min_w(px(v));
    }
    if let Ok(v) = table.get::<f32>("max_width") {
        el = el.max_w(px(v));
    }
    if let Ok(v) = table.get::<f32>("min_height") {
        el = el.min_h(px(v));
    }
    if let Ok(v) = table.get::<f32>("max_height") {
        el = el.max_h(px(v));
    }

    // Overflow
    if let Ok(overflow) = table.get::<String>("overflow") {
        if overflow == "hidden" {
            el = el.overflow_hidden();
        }
    }

    // Cursor
    if let Ok(cursor) = table.get::<String>("cursor") {
        match cursor.as_str() {
            "pointer" => el = el.cursor_pointer(),
            "default" => el = el.cursor_default(),
            "text" => el = el.cursor_text(),
            _ => {}
        }
    }

    Ok(el)
}

/// Parse a `FontWeight` from a Lua value (string name or numeric 100–900).
fn parse_font_weight(table: &LuaTable) -> Option<FontWeight> {
    let val: LuaValue = table.get("weight").ok()?;
    match val {
        LuaValue::Integer(n) => Some(FontWeight(n as f32)),
        LuaValue::Number(n) => Some(FontWeight(n as f32)),
        LuaValue::String(s) => {
            let s = s.to_str().ok()?;
            match s.as_ref() {
                "thin" => Some(FontWeight::THIN),
                "extra_light" | "extralight" => Some(FontWeight::EXTRA_LIGHT),
                "light" => Some(FontWeight::LIGHT),
                "normal" => Some(FontWeight::NORMAL),
                "medium" => Some(FontWeight::MEDIUM),
                "semibold" => Some(FontWeight::SEMIBOLD),
                "bold" => Some(FontWeight::BOLD),
                "extra_bold" | "extrabold" => Some(FontWeight::EXTRA_BOLD),
                "black" => Some(FontWeight::BLACK),
                _ => None,
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Element parsing
// ---------------------------------------------------------------------------

/// Parse a Lua element table and convert it directly to a GPUI `AnyElement`.
pub fn lua_table_to_any_element(table: LuaTable) -> LuaResult<AnyElement> {
    let type_name: String = table.get("type")?;

    match type_name.as_str() {
        "hbox" | "hstack" => {
            let gap: f32 = table.get("gap").unwrap_or(0.0);
            let [pt, pr, pb, pl] = parse_padding(&table);
            let fill: bool = table.get("fill").unwrap_or(false);
            let justify: String = table.get("justify").unwrap_or_default();
            let el = div()
                .flex()
                .flex_row()
                .items_center()
                .h_full()
                .gap(px(gap))
                .pt(px(pt))
                .pr(px(pr))
                .pb(px(pb))
                .pl(px(pl))
                .children(parse_children(&table)?);
            let el = apply_common_style(el, &table)?;
            let el = match justify.as_str() {
                "end" => el.justify_end(),
                "center" => el.justify_center(),
                _ => el,
            };
            Ok(if fill {
                el.flex_1().into_any_element()
            } else {
                el.into_any_element()
            })
        }

        "vbox" | "vstack" => {
            let gap: f32 = table.get("gap").unwrap_or(0.0);
            let [pt, pr, pb, pl] = parse_padding(&table);
            let fill: bool = table.get("fill").unwrap_or(false);
            let el = div()
                .flex()
                .flex_col()
                .gap(px(gap))
                .pt(px(pt))
                .pr(px(pr))
                .pb(px(pb))
                .pl(px(pl))
                .children(parse_children(&table)?);
            let el = apply_common_style(el, &table)?;
            Ok(if fill {
                el.flex_1().into_any_element()
            } else {
                el.into_any_element()
            })
        }

        "text" | "label" => {
            let content: String = table
                .get::<String>("content")
                .or_else(|_| table.get::<String>("text"))
                .unwrap_or_default();
            let mut el = div().child(content);

            if let Ok(s) = table.get::<f32>("size") {
                el = el.text_size(px(s));
            }
            if let Some(c) = parse_color(&table, "color")? {
                el = el.text_color(rgb(c));
            }
            if let Some(w) = parse_font_weight(&table) {
                el = el.font_weight(w);
            }
            if table.get::<bool>("italic").unwrap_or(false) {
                el = el.italic();
            }
            if let Ok(family) = table.get::<String>("font_family") {
                el = el.font_family(SharedString::from(family));
            }
            if let Ok(lh) = table.get::<f32>("line_height") {
                el = el.line_height(px(lh));
            }

            Ok(el.into_any_element())
        }

        "spacer" => Ok(div().flex_1().into_any_element()),

        "icon" => {
            let name: String = table.get("name")?;
            let size: f32 = table.get("size").unwrap_or(16.0);

            // Try SVG from assets first (path = "icons/{name}.svg"),
            // then try external file path, then fall back to text.
            let asset_path = format!("icons/{name}.svg");
            let external_path: Option<String> = table.get("path").ok();

            if external_path.is_some()
                || assets::NurAssets.load(&asset_path).ok().flatten().is_some()
            {
                let mut el = if let Some(path) = external_path {
                    svg().external_path(path)
                } else {
                    svg().path(SharedString::from(asset_path))
                };
                el = el.size(px(size));
                if let Some(c) = parse_color(&table, "color")? {
                    el = el.text_color(rgb(c));
                }
                Ok(el.into_any_element())
            } else {
                // Fallback: render icon name as text
                let mut el = div().w(px(size)).h(px(size)).child(name);
                if let Some(c) = parse_color(&table, "color")? {
                    el = el.text_color(rgb(c));
                }
                Ok(el.into_any_element())
            }
        }

        "image" => {
            let src: String = table.get("src")?;
            let width: f32 = table.get("width").unwrap_or(0.0);
            let height: f32 = table.get("height").unwrap_or(0.0);

            let mut el = img(src);
            if width > 0.0 {
                el = el.w(px(width));
            }
            if height > 0.0 {
                el = el.h(px(height));
            }
            Ok(el.into_any_element())
        }

        "button" => {
            let [pt, pr, pb, pl] = parse_padding(&table);
            let gap: f32 = table.get("gap").unwrap_or(4.0);
            let id = BUTTON_ID.fetch_add(1, Ordering::Relaxed);

            // Build base div, apply common style (handles bg, border_radius, etc.)
            let el = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(gap))
                .pt(px(pt))
                .pr(px(pr))
                .pb(px(pb))
                .pl(px(pl))
                .children(parse_children(&table)?);
            let el = apply_common_style(el, &table)?;

            // Make stateful for click handling
            let el = el.id(id).cursor_pointer();

            // Hover background
            let el = match parse_color(&table, "hover_bg")? {
                Some(hc) => el.hover(move |s| s.bg(rgb(hc))),
                None => el,
            };

            // on_click callback stored in the Lua registry
            let el = match table.get::<LuaFunction>("on_click").ok() {
                Some(f) => {
                    let key = crate::vm::with_lua(|lua| lua.create_registry_value(f))
                        .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                    el.on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                        crate::vm::with_lua(|lua| {
                            if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                                crate::context::with_cx(cx, || {
                                    if let Err(e) = f.call::<()>(()) {
                                        tracing::error!("button on_click error: {e}");
                                    }
                                    crate::context::notify_all_views();
                                });
                            }
                        });
                    })
                }
                None => el,
            };

            Ok(el.into_any_element())
        }

        "separator" => {
            let orientation: String = table
                .get::<String>("orientation")
                .unwrap_or_else(|_| "horizontal".into());
            let color: u32 = table.get("color").unwrap_or(0x45475a);
            let thickness: f32 = table.get("thickness").unwrap_or(1.0);
            let el = match orientation.as_str() {
                "vertical" => div().h_full().w(px(thickness)).bg(rgb(color)),
                _ => div().w_full().h(px(thickness)).bg(rgb(color)),
            };
            Ok(el.into_any_element())
        }

        "progress_bar" => {
            let value: f32 = table.get::<f32>("value").unwrap_or(0.0).clamp(0.0, 1.0);
            let color: u32 = table.get("color").unwrap_or(0x89b4fa);
            let bg_color: u32 = table.get("bg").unwrap_or(0x313244);
            let height: f32 = table.get("height").unwrap_or(4.0);
            let border_radius: f32 = table.get("border_radius").unwrap_or(2.0);

            let inner = div()
                .flex_basis(relative(value))
                .h_full()
                .rounded(px(border_radius))
                .bg(rgb(color));
            let remainder = div().flex_1();

            let mut outer = div()
                .flex()
                .flex_row()
                .h(px(height))
                .rounded(px(border_radius))
                .bg(rgb(bg_color))
                .overflow_hidden()
                .child(inner)
                .child(remainder);

            if let Ok(w) = table.get::<f32>("width") {
                outer = outer.w(px(w));
            } else {
                outer = outer.w_full();
            }

            Ok(outer.into_any_element())
        }

        // TODO: Replace with SVG-based arc rendering when GPUI gains SVG support.
        "circular_progress" => {
            let value: f32 = table.get::<f32>("value").unwrap_or(0.0).clamp(0.0, 1.0);
            let size: f32 = table.get("size").unwrap_or(16.0);
            let label = format!("{:.0}%", value * 100.0);

            let mut el = div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(size))
                .h(px(size))
                .child(label);

            if let Some(color) = parse_color(&table, "color")? {
                el = el.text_color(rgb(color));
            }

            Ok(el.into_any_element())
        }

        "overlay" | "stack" => {
            let w: f32 = table.get("width").unwrap_or(0.0);
            let h: f32 = table.get("height").unwrap_or(0.0);
            let child_elements = parse_children(&table)?;

            let mut container = div().relative().w(px(w)).h(px(h));
            for child in child_elements {
                container = container.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h_full()
                        .child(child),
                );
            }
            Ok(container.into_any_element())
        }

        "scroll" => {
            static SCROLL_ID: AtomicU64 = AtomicU64::new(0);
            let id = SCROLL_ID.fetch_add(1, Ordering::Relaxed);
            let element_id = gpui::ElementId::Name(format!("scroll-{id}").into());

            let direction: String = table
                .get::<String>("direction")
                .unwrap_or_else(|_| "vertical".into());
            let children = parse_children(&table)?;

            let mut el = match direction.as_str() {
                "horizontal" => div()
                    .id(element_id)
                    .overflow_x_scroll()
                    .flex()
                    .flex_row()
                    .children(children),
                _ => div()
                    .id(element_id)
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(children),
            };

            if let Ok(max_h) = table.get::<f32>("max_height") {
                el = el.max_h(px(max_h));
            }

            Ok(el.into_any_element())
        }

        "slider" => {
            let value: f32 = table.get::<f32>("value").unwrap_or(0.0).clamp(0.0, 1.0);
            let color: u32 = table.get("color").unwrap_or(0x89b4fa);
            let bg_color: u32 = table.get("bg").unwrap_or(0x313244);
            let track_height: f32 = table.get("track_height").unwrap_or(6.0);
            let thumb_size: f32 = table.get("thumb_size").unwrap_or(16.0);
            let border_radius: f32 = table.get("border_radius").unwrap_or(3.0);
            let slider_width: f32 = table.get("width").unwrap_or(200.0);
            let id = BUTTON_ID.fetch_add(1, Ordering::Relaxed);

            // Build a clickable track that calculates value from click position
            let track_fill = div()
                .flex_basis(relative(value))
                .h_full()
                .rounded(px(border_radius))
                .bg(rgb(color));
            let track_remainder = div().flex_1();

            let track = div()
                .flex()
                .flex_row()
                .h(px(track_height))
                .w(px(slider_width))
                .rounded(px(border_radius))
                .bg(rgb(bg_color))
                .overflow_hidden()
                .child(track_fill)
                .child(track_remainder);

            // Thumb indicator positioned at the value point
            let thumb_offset = value * (slider_width - thumb_size);
            let thumb = div()
                .absolute()
                .top(px(-(thumb_size - track_height) / 2.0))
                .left(px(thumb_offset))
                .w(px(thumb_size))
                .h(px(thumb_size))
                .rounded(px(thumb_size / 2.0))
                .bg(rgb(color));

            let container = div()
                .id(id)
                .cursor_pointer()
                .relative()
                .flex()
                .items_center()
                .h(px(thumb_size))
                .w(px(slider_width))
                .child(track)
                .child(thumb);

            // on_change callback: called with the new value (0.0-1.0) on click
            let container = match table.get::<LuaFunction>("on_change").ok() {
                Some(f) => {
                    let key = crate::vm::with_lua(|lua| lua.create_registry_value(f))
                        .map_err(|e| LuaError::RuntimeError(e.to_string()))?;
                    let w = slider_width;
                    container.on_mouse_down(MouseButton::Left, move |ev, _window, cx| {
                        let click_x = f32::from(ev.position.x);
                        let new_value = (click_x / w).clamp(0.0, 1.0);
                        crate::vm::with_lua(|lua| {
                            if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                                crate::context::with_cx(cx, || {
                                    if let Err(e) = f.call::<()>(new_value) {
                                        tracing::error!("slider on_change error: {e}");
                                    }
                                    crate::context::notify_all_views();
                                });
                            }
                        });
                    })
                }
                None => container,
            };

            Ok(container.into_any_element())
        }

        "input" => {
            let placeholder: String = table.get("placeholder").unwrap_or_default();
            let value: String = table.get("value").unwrap_or_default();
            let width: f32 = table.get("width").unwrap_or(200.0);
            let height: f32 = table.get("height").unwrap_or(28.0);
            let font_size: f32 = table.get("font_size").unwrap_or(13.0);

            // Text input is complex in GPUI — render as a styled div with text.
            // Full IME-capable input requires GPUI's InputHandler which needs a View.
            // For now, render a text display with an on_click callback for focus.
            let display = if value.is_empty() { placeholder } else { value };
            let mut el = div()
                .flex()
                .items_center()
                .w(px(width))
                .h(px(height))
                .px(px(8.0))
                .text_size(px(font_size))
                .cursor_text()
                .child(display);

            el = apply_common_style(el, &table)?;
            Ok(el.into_any_element())
        }

        other => Err(LuaError::RuntimeError(format!(
            "Unknown element type: '{other}'. Valid types: hbox, vbox, text, spacer, icon, \
             image, button, separator, progress_bar, circular_progress, overlay, scroll, \
             slider, input."
        ))),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn parse_padding(table: &LuaTable) -> [f32; 4] {
    let p: f32 = table.get("padding").unwrap_or(0.0);
    [
        table.get("padding_top").unwrap_or(p),
        table.get("padding_right").unwrap_or(p),
        table.get("padding_bottom").unwrap_or(p),
        table.get("padding_left").unwrap_or(p),
    ]
}

fn parse_children(table: &LuaTable) -> LuaResult<Vec<AnyElement>> {
    let val: LuaValue = table.get("children").unwrap_or(LuaValue::Nil);
    match val {
        LuaValue::Table(t) => {
            let len = t.raw_len();
            let mut out = Vec::with_capacity(len);
            for i in 1..=len {
                let child: LuaValue = t.get(i)?;
                match child {
                    LuaValue::Nil => continue, // skip nil (from ui.when)
                    LuaValue::Table(ct) => out.push(lua_table_to_any_element(ct)?),
                    _ => {} // skip non-table entries silently
                }
            }
            Ok(out)
        }
        LuaValue::Nil => Ok(Vec::new()),
        _ => Err(LuaError::RuntimeError(
            "`children` must be a sequential table".into(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    fn el(lua: &Lua, type_name: &str) -> LuaTable {
        let t = lua.create_table().unwrap();
        t.set("type", type_name).unwrap();
        t
    }

    fn children(lua: &Lua, items: Vec<LuaTable>) -> LuaTable {
        let t = lua.create_table().unwrap();
        for (i, item) in items.into_iter().enumerate() {
            t.set(i + 1, item).unwrap();
        }
        t
    }

    // --- valid top-level types ---

    #[test]
    fn hbox_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "hbox")).is_ok());
    }

    #[test]
    fn hstack_alias_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "hstack")).is_ok());
    }

    #[test]
    fn vbox_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "vbox")).is_ok());
    }

    #[test]
    fn vstack_alias_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "vstack")).is_ok());
    }

    #[test]
    fn text_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "hello").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn label_alias_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "label");
        t.set("text", "world").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_size_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "sized").unwrap();
        t.set("size", 14.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_missing_content_uses_empty_string() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "text")).is_ok());
    }

    #[test]
    fn text_fallback_to_text_field() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("text", "via text key").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn spacer_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "spacer")).is_ok());
    }

    #[test]
    fn icon_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "icon");
        t.set("name", "battery").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn icon_with_size_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "icon");
        t.set("name", "wifi").unwrap();
        t.set("size", 20.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- error cases ---

    #[test]
    fn unknown_type_returns_err() {
        let lua = Lua::new();
        let result = lua_table_to_any_element(el(&lua, "nonexistent"));
        assert!(result.is_err());
    }

    #[test]
    fn unknown_type_error_message_contains_type_name() {
        let lua = Lua::new();
        let err = lua_table_to_any_element(el(&lua, "slider"))
            .err()
            .expect("expected Err");
        assert!(err.to_string().contains("slider"));
    }

    #[test]
    fn unknown_type_error_message_lists_valid_types() {
        let lua = Lua::new();
        let err = lua_table_to_any_element(el(&lua, "xyz"))
            .err()
            .expect("expected Err");
        let msg = err.to_string();
        assert!(msg.contains("hbox"));
        assert!(msg.contains("vbox"));
        assert!(msg.contains("button"));
        assert!(msg.contains("separator"));
        assert!(msg.contains("progress_bar"));
        assert!(msg.contains("overlay"));
        assert!(msg.contains("scroll"));
    }

    #[test]
    fn children_as_non_table_returns_err() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("children", "not a table").unwrap();
        assert!(lua_table_to_any_element(t).is_err());
    }

    // --- layout props ---

    #[test]
    fn hbox_with_gap() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("gap", 8.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_uniform_padding() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("padding", 4.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_individual_padding_sides() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("padding_top", 2.0_f32).unwrap();
        t.set("padding_right", 4.0_f32).unwrap();
        t.set("padding_bottom", 2.0_f32).unwrap();
        t.set("padding_left", 4.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_fill_true() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("fill", true).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn vbox_fill_false() {
        let lua = Lua::new();
        let t = el(&lua, "vbox");
        t.set("fill", false).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- children ---

    #[test]
    fn hbox_with_empty_children() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("children", lua.create_table().unwrap()).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_nil_children() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_text_child() {
        let lua = Lua::new();
        let child = el(&lua, "text");
        child.set("content", "hi").unwrap();
        let t = el(&lua, "hbox");
        t.set("children", children(&lua, vec![child])).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- nested trees ---

    #[test]
    fn nested_hbox_vbox_text() {
        let lua = Lua::new();
        let txt = el(&lua, "text");
        txt.set("content", "deep").unwrap();
        let inner_vbox = el(&lua, "vbox");
        inner_vbox
            .set("children", children(&lua, vec![txt]))
            .unwrap();
        let outer_hbox = el(&lua, "hbox");
        outer_hbox
            .set("children", children(&lua, vec![inner_vbox]))
            .unwrap();
        assert!(lua_table_to_any_element(outer_hbox).is_ok());
    }

    #[test]
    fn bar_layout_shape_hbox_spacer_hbox_spacer_hbox() {
        let lua = Lua::new();
        let make_hbox = |lua: &Lua| -> LuaTable { el(lua, "hbox") };
        let spacer = || el(&lua, "spacer");
        let root = el(&lua, "hbox");
        let kids = children(
            &lua,
            vec![
                make_hbox(&lua),
                spacer(),
                make_hbox(&lua),
                spacer(),
                make_hbox(&lua),
            ],
        );
        root.set("fill", true).unwrap();
        root.set("children", kids).unwrap();
        assert!(lua_table_to_any_element(root).is_ok());
    }

    // --- parse_padding internals ---

    #[test]
    fn parse_padding_all_zero_by_default() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        assert_eq!(parse_padding(&t), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn parse_padding_uniform() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("padding", 8.0_f32).unwrap();
        assert_eq!(parse_padding(&t), [8.0, 8.0, 8.0, 8.0]);
    }

    #[test]
    fn parse_padding_individual_overrides_uniform() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("padding", 4.0_f32).unwrap();
        t.set("padding_top", 10.0_f32).unwrap();
        let p = parse_padding(&t);
        assert_eq!(p[0], 10.0);
        assert_eq!(p[1], 4.0);
        assert_eq!(p[2], 4.0);
        assert_eq!(p[3], 4.0);
    }

    // --- button ---

    #[test]
    fn button_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "button")).is_ok());
    }

    #[test]
    fn button_with_children() {
        let lua = Lua::new();
        let child = el(&lua, "text");
        child.set("content", "Click me").unwrap();
        let t = el(&lua, "button");
        t.set("children", children(&lua, vec![child])).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- common style props ---

    #[test]
    fn hbox_with_bg_hex_int() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("bg", 0xFF0000_u32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_bg_hex_string() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("bg", "#ff0000").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_bg_short_hex() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("bg", "#f00").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_border_radius() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("border_radius", 8.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_per_corner_border_radius() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("border_radius_top_left", 4.0_f32).unwrap();
        t.set("border_radius_bottom_right", 6.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_opacity() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("opacity", 0.5_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_dimensions() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("width", 200.0_f32).unwrap();
        t.set("height", 50.0_f32).unwrap();
        t.set("min_width", 100.0_f32).unwrap();
        t.set("max_width", 400.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_overflow_hidden() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("overflow", "hidden").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn hbox_with_cursor_pointer() {
        let lua = Lua::new();
        let t = el(&lua, "hbox");
        t.set("cursor", "pointer").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- text styling ---

    #[test]
    fn text_with_color_int() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "colored").unwrap();
        t.set("color", 0xFF0000_u32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_color_string() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "colored").unwrap();
        t.set("color", "#00ff00").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_weight_bold() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "bold").unwrap();
        t.set("weight", "bold").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_weight_number() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "600 weight").unwrap();
        t.set("weight", 600.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_italic() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "slanted").unwrap();
        t.set("italic", true).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_font_family() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "mono").unwrap();
        t.set("font_family", "JetBrains Mono").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn text_with_line_height() {
        let lua = Lua::new();
        let t = el(&lua, "text");
        t.set("content", "spaced").unwrap();
        t.set("line_height", 24.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- separator ---

    #[test]
    fn separator_horizontal_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "separator")).is_ok());
    }

    #[test]
    fn separator_vertical_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "separator");
        t.set("orientation", "vertical").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- progress_bar ---

    #[test]
    fn progress_bar_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "progress_bar");
        t.set("value", 0.5_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn progress_bar_defaults_returns_ok() {
        let lua = Lua::new();
        assert!(lua_table_to_any_element(el(&lua, "progress_bar")).is_ok());
    }

    // --- circular_progress ---

    #[test]
    fn circular_progress_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "circular_progress");
        t.set("value", 0.42_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- overlay / stack ---

    #[test]
    fn overlay_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "overlay");
        t.set("width", 100.0_f32).unwrap();
        t.set("height", 50.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn stack_alias_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "stack");
        t.set("width", 100.0_f32).unwrap();
        t.set("height", 50.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- scroll ---

    #[test]
    fn scroll_vertical_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "scroll");
        t.set("max_height", 300.0_f32).unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    #[test]
    fn scroll_horizontal_returns_ok() {
        let lua = Lua::new();
        let t = el(&lua, "scroll");
        t.set("direction", "horizontal").unwrap();
        assert!(lua_table_to_any_element(t).is_ok());
    }

    // --- parse_color ---

    #[test]
    fn parse_color_nil_returns_none() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        assert_eq!(parse_color(&t, "color").unwrap(), None);
    }

    #[test]
    fn parse_color_int() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("color", 0xFF0000_u32).unwrap();
        assert_eq!(parse_color(&t, "color").unwrap(), Some(0xFF0000));
    }

    #[test]
    fn parse_color_hex_string() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("color", "#ff0000").unwrap();
        assert_eq!(parse_color(&t, "color").unwrap(), Some(0xFF0000));
    }

    #[test]
    fn parse_color_short_hex() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("color", "#f00").unwrap();
        assert_eq!(parse_color(&t, "color").unwrap(), Some(0xFF0000));
    }

    #[test]
    fn parse_color_invalid_returns_err() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("color", "#xyz").unwrap();
        assert!(parse_color(&t, "color").is_err());
    }

    // --- parse_font_weight ---

    #[test]
    fn parse_font_weight_none_when_absent() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        assert!(parse_font_weight(&t).is_none());
    }

    #[test]
    fn parse_font_weight_bold_string() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("weight", "bold").unwrap();
        assert_eq!(parse_font_weight(&t).unwrap(), FontWeight::BOLD);
    }

    #[test]
    fn parse_font_weight_numeric() {
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("weight", 300.0_f32).unwrap();
        assert_eq!(parse_font_weight(&t).unwrap(), FontWeight::LIGHT);
    }
}
