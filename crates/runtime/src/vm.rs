//! The Lua virtual machine and its lifecycle.

use anyhow::Result;
use mlua::prelude::*; // includes LuaResult, LuaValue, etc.
use std::path::Path;
use std::rc::Rc;
use std::cell::RefCell;

use gpui::App;

use crate::{api, context};

thread_local! {
    /// The global Lua VM. Stored in a thread-local so render callbacks and
    /// timer closures can reach it without carrying a reference.
    static LUA: RefCell<Option<Rc<Lua>>> = const { RefCell::new(None) };
}

/// Run `f` with a reference to the active Lua VM.
///
/// # Panics
/// Panics if the runtime has not been initialised yet.
pub fn with_lua<R>(f: impl FnOnce(&Lua) -> R) -> R {
    LUA.with(|cell| {
        let borrow = cell.borrow();
        let lua = borrow.as_ref().expect("Lua runtime not initialised");
        f(lua)
    })
}

/// The Nur Lua runtime. Stored as a GPUI global to keep it alive.
///
/// Owns the `mlua::Lua` VM and is the single entry point for executing
/// the user config. Also registered in a thread-local (`LUA`) so that render
/// callbacks and timer closures can reach it via `with_lua` without carrying
/// a reference.
pub struct LuaRuntime {
    lua: Rc<Lua>,
}

impl LuaRuntime {
    pub fn new() -> Self {
        let lua = Rc::new(Lua::new());
        // Register in thread-local immediately so API functions can reach it.
        LUA.with(|cell| *cell.borrow_mut() = Some(lua.clone()));
        Self { lua }
    }

    /// Register all globals, load the stdlib, then execute the user config.
    pub fn run(&self, path: &Path, cx: &mut App) -> Result<()> {
        api::register_all(&self.lua, cx)?;
        self.load_stdlib().map_err(|e| anyhow::anyhow!("{e}"))?;

        context::with_cx(cx, || {
            let code = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Cannot read '{}': {e}", path.display()))?;

            self.lua
                .load(&code)
                .set_name(path.to_str().unwrap_or("init.lua"))
                .exec()
                .map_err(|e| anyhow::anyhow!("Lua error in config:\n{e}"))
        })
    }

    /// Load the bundled Lua standard library into the VM.
    ///
    /// Core `ui.*` constructors live in Lua (not Rust) so they can be
    /// extended without recompiling. Native components added later will be
    /// registered into the same `ui` table from Rust.
    fn load_stdlib(&self) -> LuaResult<()> {
        // Load ui.* constructors (pure Lua, no cx required)
        self.lua
            .load(assets::LUA_STDLIB)
            .set_name("nur/stdlib.lua")
            .exec()?;

        // Register bundled widget modules in package.preload so users can
        // `require("nur.widgets.clock")` etc.
        let preload: LuaTable = self
            .lua
            .globals()
            .get::<LuaTable>("package")?
            .get("preload")?;

        for &(name, source) in assets::LUA_MODULES {
            preload.set(
                name,
                self.lua.create_function(move |lua, ()| {
                    lua.load(source).set_name(name).eval::<LuaValue>()
                })?,
            )?;
        }

        Ok(())
    }
}

