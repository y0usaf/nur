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
//! Rust walks the table recursively and produces GPUI elements.
//! Adding a new element type only requires extending the `LuaElement` enum
//! and the `from_lua_table` / `into_any_element` match arms — no changes
//! needed to the Lua side unless new props are required.

use gpui::{AnyElement, div, px, prelude::*};
use mlua::prelude::*;

/// A UI element parsed from a Lua tagged table, before GPU rendering.
///
/// Lua render functions produce a tree of these; `into_any_element` converts
/// the whole tree to GPUI elements in one pass. Lua is never called during
/// the GPU draw phase.
#[derive(Debug, Clone)]
pub enum LuaElement {
    HBox {
        gap: f32,
        padding: [f32; 4], // top right bottom left
        fill: bool,        // flex_1: grow to fill parent width
        children: Vec<LuaElement>,
    },
    VBox {
        gap: f32,
        padding: [f32; 4],
        fill: bool,
        children: Vec<LuaElement>,
    },
    Text {
        content: String,
        size: Option<f32>,
    },
    Icon {
        name: String,
        size: f32,
    },
    Spacer,
}

impl LuaElement {
    /// Parse a Lua table into a `LuaElement`.
    pub fn from_lua_table(table: LuaTable) -> LuaResult<Self> {
        let type_name: String = table.get("type")?;

        match type_name.as_str() {
            "hbox" | "hstack" => {
                let gap = table.get("gap").unwrap_or(0.0_f32);
                let padding = parse_padding(&table);
                let fill: bool = table.get("fill").unwrap_or(false);
                let children = parse_children(&table)?;
                Ok(LuaElement::HBox { gap, padding, fill, children })
            }

            "vbox" | "vstack" => {
                let gap = table.get("gap").unwrap_or(0.0_f32);
                let padding = parse_padding(&table);
                let fill: bool = table.get("fill").unwrap_or(false);
                let children = parse_children(&table)?;
                Ok(LuaElement::VBox { gap, padding, fill, children })
            }

            "text" | "label" => {
                let content: String = table
                    .get::<String>("content")
                    .or_else(|_| table.get::<String>("text"))
                    .unwrap_or_default();
                let size: Option<f32> = table.get("size").ok();
                Ok(LuaElement::Text { content, size })
            }

            "spacer" => Ok(LuaElement::Spacer),

            "icon" => {
                let name: String = table.get("name")?;
                let size: f32 = table.get("size").unwrap_or(16.0);
                Ok(LuaElement::Icon { name, size })
            }

            other => Err(LuaError::RuntimeError(format!(
                "Unknown element type: '{other}'. \
                 Valid types: hbox, vbox, text, spacer, icon."
            ))),
        }
    }

    /// Convert to a GPUI `AnyElement` ready for rendering.
    pub fn into_any_element(self) -> AnyElement {
        match self {
            LuaElement::HBox { gap, padding: [pt, pr, pb, pl], fill, children } => {
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
                    .children(children.into_iter().map(|c| c.into_any_element()));
                if fill { el.flex_1().into_any_element() } else { el.into_any_element() }
            }

            LuaElement::VBox { gap, padding: [pt, pr, pb, pl], fill, children } => {
                let el = div()
                    .flex()
                    .flex_col()
                    .gap(px(gap))
                    .pt(px(pt))
                    .pr(px(pr))
                    .pb(px(pb))
                    .pl(px(pl))
                    .children(children.into_iter().map(|c| c.into_any_element()));
                if fill { el.flex_1().into_any_element() } else { el.into_any_element() }
            }

            LuaElement::Text { content, size } => {
                let el = div().child(content);
                match size {
                    Some(s) => el.text_size(px(s)).into_any_element(),
                    None => el.into_any_element(),
                }
            }

            LuaElement::Spacer => div().flex_1().into_any_element(),

            LuaElement::Icon { name, size } => {
                // TODO: render SVG icon from the bundled icon set.
                //
                // Implementation sketch:
                //   1. Load SVG bytes from `assets::ICONS` map keyed by name.
                //   2. Use GPUI's image rendering: `img(ImageSource::Data(bytes))`
                //      sized to `px(size) x px(size)`.
                //   3. Fall back to a coloured square placeholder if name not found.
                //
                // For now: render the icon name as text (useful for debugging layout).
                div()
                    .w(px(size))
                    .h(px(size))
                    .child(name)
                    .into_any_element()
            }
        }
    }
}

fn parse_padding(table: &LuaTable) -> [f32; 4] {
    let p: f32 = table.get("padding").unwrap_or(0.0);
    [
        table.get("padding_top").unwrap_or(p),
        table.get("padding_right").unwrap_or(p),
        table.get("padding_bottom").unwrap_or(p),
        table.get("padding_left").unwrap_or(p),
    ]
}

fn parse_children(table: &LuaTable) -> LuaResult<Vec<LuaElement>> {
    let val: LuaValue = table.get("children").unwrap_or(LuaValue::Nil);
    match val {
        LuaValue::Table(t) => {
            let len = t.raw_len();
            let mut out = Vec::with_capacity(len);
            for i in 1..=len {
                let child: LuaTable = t.get(i)?;
                out.push(LuaElement::from_lua_table(child)?);
            }
            Ok(out)
        }
        LuaValue::Nil => Ok(Vec::new()),
        _ => Err(LuaError::RuntimeError(
            "`children` must be a sequential table".to_string(),
        )),
    }
}
