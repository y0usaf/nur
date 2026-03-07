use anyhow::{Context, Result};
use std::path::PathBuf;

/// Locate the user's `init.lua` config file.
///
/// Resolution order:
///   1. `$NUR_CONFIG` env var
///   2. `$XDG_CONFIG_HOME/nur/init.lua`
///   3. `~/.config/nur/init.lua`
pub fn find() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("NUR_CONFIG") {
        let p = PathBuf::from(path);
        return p
            .exists()
            .then_some(p)
            .context("NUR_CONFIG points to a non-existent file");
    }

    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME is not set");
            PathBuf::from(home).join(".config")
        });

    let path = base.join("nur").join("init.lua");
    path.exists().then_some(path).with_context(|| {
        format!(
            "No config found at {}.\n\
             Create it to get started, or set NUR_CONFIG.\n\
             See examples/ in the nur repository.",
            base.join("nur/init.lua").display()
        )
    })
}
