//! Bundled assets: Lua stdlib, SVG icons, fonts.
//!
//! Everything is embedded at compile time via `include_str!` / `include_bytes!`
//! so the binary is fully self-contained with no runtime data-dir dependency.


// ---------------------------------------------------------------------------
// Embedded Lua standard library
// ---------------------------------------------------------------------------

/// Core `ui.*` constructors and helpers (pure Lua).
pub const LUA_STDLIB: &str = include_str!("../../../lua/nur/stdlib.lua");

/// Pre-built widget modules, exposed via Lua `package.preload`.
pub const LUA_MODULES: &[(&str, &str)] = &[
    ("nur.widgets.clock",      include_str!("../../../lua/nur/widgets/clock.lua")),
    ("nur.widgets.battery",    include_str!("../../../lua/nur/widgets/battery.lua")),
    ("nur.widgets.workspaces", include_str!("../../../lua/nur/widgets/workspaces.lua")),
    ("nur.utils",              include_str!("../../../lua/nur/utils.lua")),
];

// ---------------------------------------------------------------------------
// GPUI asset registration
// ---------------------------------------------------------------------------

pub struct NurAssets;

impl gpui::AssetSource for NurAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        // TODO: embed SVG icons and register them here so `ui.icon("name")`
        // can resolve to a real SVG via gpui's image rendering pipeline.
        let _ = path;
        Ok(None)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let _ = path;
        Ok(Vec::new())
    }
}

/// The bundled asset source — pass to `Application::new().with_assets(assets::source())`.
pub fn source() -> NurAssets {
    NurAssets
}
