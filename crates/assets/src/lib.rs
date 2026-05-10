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
    (
        "nur.widgets.clock",
        include_str!("../../../lua/nur/widgets/clock.lua"),
    ),
    (
        "nur.widgets.battery",
        include_str!("../../../lua/nur/widgets/battery.lua"),
    ),
    (
        "nur.widgets.workspaces",
        include_str!("../../../lua/nur/widgets/workspaces.lua"),
    ),
    (
        "nur.widgets.network",
        include_str!("../../../lua/nur/widgets/network.lua"),
    ),
    (
        "nur.widgets.mpris",
        include_str!("../../../lua/nur/widgets/mpris.lua"),
    ),
    (
        "nur.widgets.volume_panel",
        include_str!("../../../lua/nur/widgets/volume_panel.lua"),
    ),
    (
        "nur.widgets.media_panel",
        include_str!("../../../lua/nur/widgets/media_panel.lua"),
    ),
    (
        "nur.widgets.system_tray",
        include_str!("../../../lua/nur/widgets/system_tray.lua"),
    ),
    (
        "nur.widgets.ags_overlay",
        include_str!("../../../lua/nur/widgets/ags_overlay.lua"),
    ),
    ("nur.utils", include_str!("../../../lua/nur/utils.lua")),
    ("nur.theme", include_str!("../../../lua/nur/theme.lua")),
    ("nur.wallust", include_str!("../../../lua/nur/wallust.lua")),
];

// ---------------------------------------------------------------------------
// GPUI asset registration
// ---------------------------------------------------------------------------

pub struct NurAssets;

impl gpui::AssetSource for NurAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        // Try to resolve "icons/{name}.svg" by searching XDG icon theme dirs.
        if let Some(icon_name) = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"))
        {
            if let Some(data) = find_system_icon_svg(icon_name) {
                return Ok(Some(std::borrow::Cow::Owned(data)));
            }
        }
        Ok(None)
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let _ = path;
        Ok(Vec::new())
    }
}

/// Search for a named SVG icon in common icon theme directories.
fn find_system_icon_svg(name: &str) -> Option<Vec<u8>> {
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/share:/usr/local/share".into());

    let search_dirs: Vec<String> = data_dirs
        .split(':')
        .flat_map(|base| {
            // Search scalable icons in common themes
            ["hicolor", "Adwaita", "breeze"]
                .iter()
                .map(move |theme| format!("{base}/icons/{theme}/scalable"))
        })
        .collect();

    let categories = [
        "actions",
        "apps",
        "categories",
        "devices",
        "emblems",
        "mimetypes",
        "places",
        "status",
        "panel",
    ];

    for dir in &search_dirs {
        for cat in &categories {
            let path = format!("{dir}/{cat}/{name}.svg");
            if let Ok(data) = std::fs::read(&path) {
                return Some(data);
            }
        }
        // Also try directly in the scalable dir
        let path = format!("{dir}/{name}.svg");
        if let Ok(data) = std::fs::read(&path) {
            return Some(data);
        }
    }

    // Also try ~/.local/share/icons and /run/current-system/sw/share/icons (NixOS)
    let extra_dirs = [
        format!(
            "{}/.local/share/icons",
            std::env::var("HOME").unwrap_or_default()
        ),
        "/run/current-system/sw/share/icons".into(),
    ];

    for base in &extra_dirs {
        for theme in &["hicolor", "Adwaita", "breeze"] {
            for cat in &categories {
                let path = format!("{base}/{theme}/scalable/{cat}/{name}.svg");
                if let Ok(data) = std::fs::read(&path) {
                    return Some(data);
                }
            }
        }
    }

    None
}

/// The bundled asset source — pass to `Application::new().with_assets(assets::source())`.
pub fn source() -> NurAssets {
    NurAssets
}
