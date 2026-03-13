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

use gpui::{AnyElement, div, px, prelude::*};
use mlua::prelude::*;

/// Parse a Lua element table and convert it directly to a GPUI `AnyElement`.
pub fn lua_table_to_any_element(table: LuaTable) -> LuaResult<AnyElement> {
    let type_name: String = table.get("type")?;

    match type_name.as_str() {
        "hbox" | "hstack" => {
            let gap: f32 = table.get("gap").unwrap_or(0.0);
            let [pt, pr, pb, pl] = parse_padding(&table);
            let fill: bool = table.get("fill").unwrap_or(false);
            let el = div()
                .flex().flex_row().items_center().h_full()
                .gap(px(gap)).pt(px(pt)).pr(px(pr)).pb(px(pb)).pl(px(pl))
                .children(parse_children(&table)?);
            Ok(if fill { el.flex_1().into_any_element() } else { el.into_any_element() })
        }

        "vbox" | "vstack" => {
            let gap: f32 = table.get("gap").unwrap_or(0.0);
            let [pt, pr, pb, pl] = parse_padding(&table);
            let fill: bool = table.get("fill").unwrap_or(false);
            let el = div()
                .flex().flex_col()
                .gap(px(gap)).pt(px(pt)).pr(px(pr)).pb(px(pb)).pl(px(pl))
                .children(parse_children(&table)?);
            Ok(if fill { el.flex_1().into_any_element() } else { el.into_any_element() })
        }

        "text" | "label" => {
            let content: String = table
                .get::<String>("content")
                .or_else(|_| table.get::<String>("text"))
                .unwrap_or_default();
            let el = div().child(content);
            Ok(match table.get::<f32>("size").ok() {
                Some(s) => el.text_size(px(s)).into_any_element(),
                None    => el.into_any_element(),
            })
        }

        "spacer" => Ok(div().flex_1().into_any_element()),

        "icon" => {
            let name: String = table.get("name")?;
            let size: f32 = table.get("size").unwrap_or(16.0);
            // TODO: render SVG icon from the bundled icon set.
            Ok(div().w(px(size)).h(px(size)).child(name).into_any_element())
        }

        other => Err(LuaError::RuntimeError(format!(
            "Unknown element type: '{other}'. Valid types: hbox, vbox, text, spacer, icon."
        ))),
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

fn parse_children(table: &LuaTable) -> LuaResult<Vec<AnyElement>> {
    let val: LuaValue = table.get("children").unwrap_or(LuaValue::Nil);
    match val {
        LuaValue::Table(t) => {
            let len = t.raw_len();
            let mut out = Vec::with_capacity(len);
            for i in 1..=len {
                out.push(lua_table_to_any_element(t.get(i)?)?);
            }
            Ok(out)
        }
        LuaValue::Nil => Ok(Vec::new()),
        _ => Err(LuaError::RuntimeError("`children` must be a sequential table".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    /// Build a minimal element table with just a `type` field.
    fn el(lua: &Lua, type_name: &str) -> LuaTable {
        let t = lua.create_table().unwrap();
        t.set("type", type_name).unwrap();
        t
    }

    /// Build a child list (sequential Lua table) containing the given element tables.
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
        // `content` and `text` both absent — should fall back to ""
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
        let result = lua_table_to_any_element(el(&lua, "button"));
        assert!(result.is_err());
    }

    #[test]
    fn unknown_type_error_message_contains_type_name() {
        let lua = Lua::new();
        let err = lua_table_to_any_element(el(&lua, "slider")).err().expect("expected Err");
        assert!(err.to_string().contains("slider"));
    }

    #[test]
    fn unknown_type_error_message_lists_valid_types() {
        let lua = Lua::new();
        let err = lua_table_to_any_element(el(&lua, "xyz")).err().expect("expected Err");
        let msg = err.to_string();
        assert!(msg.contains("hbox"));
        assert!(msg.contains("vbox"));
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
        // no children key at all — treated as nil → empty Vec
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
        inner_vbox.set("children", children(&lua, vec![txt])).unwrap();

        let outer_hbox = el(&lua, "hbox");
        outer_hbox.set("children", children(&lua, vec![inner_vbox])).unwrap();

        assert!(lua_table_to_any_element(outer_hbox).is_ok());
    }

    #[test]
    fn bar_layout_shape_hbox_spacer_hbox_spacer_hbox() {
        let lua = Lua::new();

        let make_hbox = |lua: &Lua| -> LuaTable {
            el(lua, "hbox")
        };
        let spacer = || el(&lua, "spacer");

        let root = el(&lua, "hbox");
        let kids = children(&lua, vec![
            make_hbox(&lua),
            spacer(),
            make_hbox(&lua),
            spacer(),
            make_hbox(&lua),
        ]);
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
        assert_eq!(p[0], 10.0); // top overridden
        assert_eq!(p[1], 4.0);  // right from uniform
        assert_eq!(p[2], 4.0);  // bottom from uniform
        assert_eq!(p[3], 4.0);  // left from uniform
    }
}
