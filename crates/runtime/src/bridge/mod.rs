//! Bridge types that cross the Lua↔GPUI language boundary.
//!
//! - `element`        — converts Lua element tables to GPUI `AnyElement` trees
//! - `state`          — reactive `LuaState` userdata with a GPUI notifier chain
//! - `service_handle` — `ServiceHandle` userdata combining `LuaState` with action methods
//! - `window`         — `LuaView` (GPUI `Render` impl) and `LuaWindowHandle` userdata

pub mod element;
pub mod service_handle;
pub mod state;
pub mod window;
