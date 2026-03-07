//! Bridge types that cross the Lua↔GPUI language boundary.
//!
//! - `element` — converts Lua element tables to GPUI `AnyElement` trees
//! - `state`   — reactive `LuaState` userdata with a GPUI notifier chain
//! - `window`  — `LuaView` (GPUI `Render` impl) and `LuaWindowHandle` userdata

pub mod element;
pub mod state;
pub mod window;
