//! The Lumen Lua runtime crate.
//!
//! This crate owns everything related to Lua: the VM lifecycle, the full
//! Lua API surface (`shell.*`, `ui.*`, `shell.services.*`), and the bridge
//! types that translate between Lua values and GPUI elements.
//!
//! # Module layout
//!
//! ```text
//! runtime/
//!   vm.rs          — LuaRuntime struct; thread-local VM access via with_lua()
//!   context.rs     — Thread-local &mut App pointer; reactive view notifiers
//!   api/           — Lua global registration (shell.*, ui.*, shell.services.*)
//!   bridge/        — Types that cross the Lua↔GPUI boundary
//!     element.rs   — LuaElement: Lua table → GPUI AnyElement
//!     state.rs     — LuaState: reactive value with notifier chain
//!     window.rs    — LuaView (Render impl) + LuaWindowHandle userdata
//! ```
//!
//! # Threading model
//!
//! Everything runs on the main thread. GPUI is single-threaded; Lua is
//! `!Send` by default (mlua without the `send` feature). The Lua VM and all
//! GPUI entities live and die on the same thread. The only "async" is GPUI's
//! own foreground executor, which also runs on the main thread.
//!
//! # Key design decision: thread-local cx
//!
//! GPUI's `&mut App` context cannot be stored in a struct or passed into
//! closures that outlive the current stack frame. The solution is
//! `context::with_cx` / `context::current_cx` which temporarily store a raw
//! pointer in a thread-local. All Lua API functions that need GPUI use this.
//! See `context.rs` for the safety argument.

pub mod api;
pub mod bridge;
mod context;
mod vm;

pub use vm::LuaRuntime;

// Implement the GPUI Global marker so main.rs can call cx.set_global(runtime),
// keeping the LuaRuntime (and therefore the Lua VM) alive for the process lifetime.
impl gpui::Global for LuaRuntime {}
