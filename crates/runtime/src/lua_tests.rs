//! Lua-level tests: load and exercise `nur.utils` and `nur/stdlib.lua` through
//! a real `mlua::Lua` VM without GPUI. These cover the pure-Lua layer that
//! can't be reached by the Rust unit tests above.

use mlua::prelude::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a Lua VM with the `ui` table pre-seeded, then load the nur stdlib.
fn lua_with_stdlib() -> Lua {
    let lua = Lua::new();
    lua.globals()
        .set("ui", lua.create_table().unwrap())
        .unwrap();
    lua.load(assets::LUA_STDLIB)
        .set_name("nur/stdlib.lua")
        .exec()
        .unwrap();
    lua
}

/// Find and return the source for the named bundled module.
fn module_src(name: &str) -> &'static str {
    assets::LUA_MODULES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .unwrap_or_else(|| panic!("module '{name}' not found in LUA_MODULES"))
}

/// Load `nur.utils` into a fresh Lua VM and return the module table.
fn load_utils(lua: &Lua) -> LuaTable {
    lua.load(module_src("nur.utils"))
        .set_name("nur.utils")
        .eval::<LuaTable>()
        .unwrap()
}

// ---------------------------------------------------------------------------
// nur.utils — round
// ---------------------------------------------------------------------------

mod utils_round {
    use super::*;

    fn round(lua: &Lua, n: f64, digits: i32) -> f64 {
        let u = load_utils(lua);
        let f: LuaFunction = u.get("round").unwrap();
        f.call::<f64>((n, digits)).unwrap()
    }

    #[test]
    fn round_integer_unchanged() {
        let lua = Lua::new();
        assert!((round(&lua, 5.0, 0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn round_up() {
        let lua = Lua::new();
        assert!((round(&lua, 2.5, 0) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn round_down() {
        let lua = Lua::new();
        assert!((round(&lua, 2.4, 0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn round_one_decimal_place() {
        let lua = Lua::new();
        assert!((round(&lua, 3.14159, 1) - 3.1).abs() < 1e-6);
    }

    #[test]
    fn round_two_decimal_places() {
        let lua = Lua::new();
        assert!((round(&lua, 3.14159, 2) - 3.14).abs() < 1e-6);
    }

    #[test]
    fn round_negative_number() {
        let lua = Lua::new();
        assert!((round(&lua, -2.7, 0) - (-3.0)).abs() < 1e-9);
    }

    #[test]
    fn round_zero() {
        let lua = Lua::new();
        assert!((round(&lua, 0.0, 0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn round_preserves_three_decimal_places() {
        let lua = Lua::new();
        assert!((round(&lua, 1.2345, 3) - 1.235).abs() < 1e-6);
    }
}

// ---------------------------------------------------------------------------
// nur.utils — fmt_bytes
// ---------------------------------------------------------------------------
//
// Accuracy note: Lua's `/` operator always produces a float, even on two
// integers. Therefore `M.round(1024/1024, 1)` returns `1.0` (float), and
// `1.0 .. " KB"` renders as `"1.0 KB"` in Lua 5.4. Tests below reflect the
// actual formatter output rather than an idealised representation.

mod utils_fmt_bytes {
    use super::*;

    /// Call fmt_bytes with an integer byte count (avoids float-formatting noise
    /// in the B range, where the value is concatenated directly).
    fn fmt(lua: &Lua, bytes: i64) -> String {
        let u = load_utils(lua);
        let f: LuaFunction = u.get("fmt_bytes").unwrap();
        f.call::<String>(bytes).unwrap()
    }

    #[test]
    fn bytes_zero() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 0), "0 B");
    }

    #[test]
    fn bytes_under_1kb() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 512), "512 B");
    }

    #[test]
    fn bytes_max_before_kb() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 1023), "1023 B");
    }

    #[test]
    fn exactly_1kb() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 1024), "1 KB");
    }

    #[test]
    fn kilobytes_fractional() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 1536), "1.5 KB");
    }

    #[test]
    fn exactly_1mb() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 1024 * 1024), "1 MB");
    }

    #[test]
    fn megabytes_fractional() {
        let lua = Lua::new();
        assert_eq!(fmt(&lua, 1024 * 1024 * 3 / 2), "1.5 MB");
    }

    #[test]
    fn gigabytes_fractional() {
        let lua = Lua::new();
        let two_and_half_gb = 1024_i64 * 1024 * 1024 * 5 / 2;
        let r = fmt(&lua, two_and_half_gb);
        assert!(r.contains("2.5"), "expected '2.5 GB', got '{r}'");
        assert!(r.contains("GB"));
    }
}

// ---------------------------------------------------------------------------
// nur.utils — clamp
// ---------------------------------------------------------------------------

mod utils_clamp {
    use super::*;

    fn clamp(lua: &Lua, n: f64, lo: f64, hi: f64) -> f64 {
        let u = load_utils(lua);
        let f: LuaFunction = u.get("clamp").unwrap();
        f.call::<f64>((n, lo, hi)).unwrap()
    }

    #[test]
    fn clamp_value_in_range() {
        let lua = Lua::new();
        assert!((clamp(&lua, 5.0, 0.0, 10.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn clamp_value_below_lo() {
        let lua = Lua::new();
        assert!((clamp(&lua, -5.0, 0.0, 10.0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn clamp_value_above_hi() {
        let lua = Lua::new();
        assert!((clamp(&lua, 15.0, 0.0, 10.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn clamp_exactly_lo() {
        let lua = Lua::new();
        assert!((clamp(&lua, 0.0, 0.0, 10.0) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn clamp_exactly_hi() {
        let lua = Lua::new();
        assert!((clamp(&lua, 10.0, 0.0, 10.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn clamp_negative_range() {
        let lua = Lua::new();
        assert!((clamp(&lua, -15.0, -10.0, -1.0) - (-10.0)).abs() < 1e-9);
    }

    #[test]
    fn clamp_lo_equals_hi() {
        let lua = Lua::new();
        assert!((clamp(&lua, 7.0, 5.0, 5.0) - 5.0).abs() < 1e-9);
    }
}

// ---------------------------------------------------------------------------
// nur.utils — trim
// ---------------------------------------------------------------------------

mod utils_trim {
    use super::*;

    fn trim(lua: &Lua, s: &str) -> String {
        let u = load_utils(lua);
        let f: LuaFunction = u.get("trim").unwrap();
        f.call::<String>(s).unwrap()
    }

    #[test]
    fn trim_leading_spaces() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "   hello"), "hello");
    }

    #[test]
    fn trim_trailing_spaces() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "hello   "), "hello");
    }

    #[test]
    fn trim_both_ends() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "  hello world  "), "hello world");
    }

    #[test]
    fn trim_empty_string() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, ""), "");
    }

    #[test]
    fn trim_only_spaces() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "   "), "");
    }

    #[test]
    fn trim_no_whitespace() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "hello"), "hello");
    }

    #[test]
    fn trim_tabs_and_newlines() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "\t\nhello\n\t"), "hello");
    }

    #[test]
    fn trim_preserves_internal_whitespace() {
        let lua = Lua::new();
        assert_eq!(trim(&lua, "  hello   world  "), "hello   world");
    }
}

// ---------------------------------------------------------------------------
// nur/stdlib.lua — ui constructors
// ---------------------------------------------------------------------------

mod stdlib_constructors {
    use super::*;

    fn field<T: FromLua>(tbl: &LuaTable, key: &str) -> T {
        tbl.get::<T>(key).unwrap_or_else(|e| panic!("field '{key}': {e}"))
    }

    // --- API surface check ---

    #[test]
    fn ui_globals_all_present_after_stdlib_load() {
        let lua = lua_with_stdlib();
        let ui: LuaTable = lua.globals().get("ui").unwrap();
        for name in ["hbox", "vbox", "text", "icon", "spacer", "bar_layout",
                     "hstack", "vstack", "label"] {
            assert!(
                ui.get::<LuaFunction>(name).is_ok(),
                "ui.{name} should be a function after stdlib load"
            );
        }
    }

    // --- aliases produce the same type field ---

    #[test]
    fn hstack_alias_returns_hbox_type() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.hstack({})").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "hbox");
    }

    #[test]
    fn vstack_alias_returns_vbox_type() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.vstack({})").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "vbox");
    }

    #[test]
    fn label_alias_returns_text_type() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load(r#"return ui.label("hi")"#).eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "text");
    }

    // --- ui.hbox ---

    #[test]
    fn hbox_sets_type_field() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.hbox({})").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "hbox");
    }

    #[test]
    fn hbox_preserves_gap() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.hbox({ gap = 8 })").eval().unwrap();
        assert_eq!(field::<i64>(&result, "gap"), 8);
    }

    #[test]
    fn hbox_nil_props_still_sets_type() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.hbox()").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "hbox");
    }

    #[test]
    fn hbox_preserves_fill_flag() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.hbox({ fill = true })").eval().unwrap();
        assert!(field::<bool>(&result, "fill"));
    }

    #[test]
    fn hbox_preserves_padding() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.hbox({ padding = 4 })").eval().unwrap();
        assert_eq!(field::<i64>(&result, "padding"), 4);
    }

    // --- ui.vbox ---

    #[test]
    fn vbox_sets_type_field() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.vbox({})").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "vbox");
    }

    #[test]
    fn vbox_nil_props_still_sets_type() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.vbox()").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "vbox");
    }

    #[test]
    fn vbox_preserves_children_key() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load("return ui.vbox({ children = {} })")
            .eval()
            .unwrap();
        let children: LuaTable = result.get("children").unwrap();
        assert_eq!(children.raw_len(), 0);
    }

    // --- ui.spacer ---

    #[test]
    fn spacer_type_field_is_spacer() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load("return ui.spacer()").eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "spacer");
    }

    // --- ui.text ---

    #[test]
    fn text_string_arg_sets_content() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load(r#"return ui.text("hello")"#).eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "text");
        assert_eq!(field::<String>(&result, "content"), "hello");
    }

    #[test]
    fn text_empty_string() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load(r#"return ui.text("")"#).eval().unwrap();
        assert_eq!(field::<String>(&result, "content"), "");
    }

    #[test]
    fn text_props_table_preserves_content_and_size() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load(r#"return ui.text({ content = "world", size = 14 })"#)
            .eval()
            .unwrap();
        assert_eq!(field::<String>(&result, "type"), "text");
        assert_eq!(field::<String>(&result, "content"), "world");
        assert_eq!(field::<i64>(&result, "size"), 14);
    }

    // --- ui.icon ---

    #[test]
    fn icon_string_arg_sets_name_and_type() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua.load(r#"return ui.icon("battery")"#).eval().unwrap();
        assert_eq!(field::<String>(&result, "type"), "icon");
        assert_eq!(field::<String>(&result, "name"), "battery");
    }

    #[test]
    fn icon_props_table_preserves_size() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load(r#"return ui.icon({ name = "wifi", size = 20 })"#)
            .eval()
            .unwrap();
        assert_eq!(field::<String>(&result, "type"), "icon");
        assert_eq!(field::<String>(&result, "name"), "wifi");
        assert_eq!(field::<i64>(&result, "size"), 20);
    }

    // --- ui.bar_layout ---

    #[test]
    fn bar_layout_root_type_is_hbox() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load("return ui.bar_layout({}, {}, {})")
            .eval()
            .unwrap();
        assert_eq!(field::<String>(&result, "type"), "hbox");
    }

    #[test]
    fn bar_layout_fill_is_true() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load("return ui.bar_layout({}, {}, {})")
            .eval()
            .unwrap();
        assert!(field::<bool>(&result, "fill"));
    }

    #[test]
    fn bar_layout_has_five_children() {
        // Structure: left-hbox | spacer | center-hbox | spacer | right-hbox
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load("return ui.bar_layout({}, {}, {})")
            .eval()
            .unwrap();
        let children: LuaTable = result.get("children").unwrap();
        assert_eq!(children.raw_len(), 5);
    }

    #[test]
    fn bar_layout_child_types_are_correct() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load("return ui.bar_layout({}, {}, {})")
            .eval()
            .unwrap();
        let children: LuaTable = result.get("children").unwrap();
        let types: Vec<String> = (1..=5_usize)
            .map(|i| {
                let child: LuaTable = children.get(i).unwrap();
                child.get::<String>("type").unwrap()
            })
            .collect();
        assert_eq!(types, ["hbox", "spacer", "hbox", "spacer", "hbox"]);
    }

    #[test]
    fn bar_layout_nil_sections_do_not_crash() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load("return ui.bar_layout(nil, nil, nil)")
            .eval()
            .unwrap();
        assert_eq!(field::<String>(&result, "type"), "hbox");
    }

    #[test]
    fn bar_layout_with_items_in_left_section() {
        let lua = lua_with_stdlib();
        let result: LuaTable = lua
            .load(r#"
                local left = { ui.text("A"), ui.text("B") }
                return ui.bar_layout(left, {}, {})
            "#)
            .eval()
            .unwrap();
        // Dig into children[1] (left hbox) and count its children
        let children: LuaTable = result.get("children").unwrap();
        let left_box: LuaTable = children.get(1_i64).unwrap();
        let left_children: LuaTable = left_box.get("children").unwrap();
        assert_eq!(left_children.raw_len(), 2);
    }
}
