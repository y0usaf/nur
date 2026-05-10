//! Thread-local GPUI context pointer.
//!
//! GPUI requires a `&mut App` for most operations, but that reference cannot
//! be stored across async boundaries or passed into Lua closures directly.
//!
//! The solution: temporarily store a raw pointer in a thread-local during
//! any scope where `cx` is valid (Lua config execution, timer callbacks,
//! event handlers). All Lua API functions that need `cx` call `current_cx`.
//!
//! This is safe because:
//!   - GPUI is single-threaded (main thread only).
//!   - The pointer is cleared at the end of every `with_cx` scope.
//!   - Lua config execution is synchronous within `with_cx`.

use gpui::App;
use std::cell::Cell;
use std::ffi::c_void;

thread_local! {
    static APP_PTR: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
}

/// Run `f` with `cx` registered as the active GPUI context.
pub fn with_cx<R>(cx: &mut App, f: impl FnOnce() -> R) -> R {
    APP_PTR.with(|cell| {
        let prev = cell.get();
        cell.set(cx as *mut App as *mut c_void);
        let result = f();
        cell.set(prev); // restore (supports nested calls)
        result
    })
}

/// Access the active GPUI context.
///
/// # Panics
/// Panics when called outside of a `with_cx` scope — i.e. not during Lua
/// config execution or a GPUI callback.
pub fn current_cx<R>(f: impl FnOnce(&mut App) -> R) -> R {
    try_current_cx(f).expect(
        "No active GPUI context. \
         This function must only be called during Lua config execution \
         or from inside a GPUI event/timer callback.",
    )
}

/// Access the active GPUI context, returning `None` when no context is active.
pub fn try_current_cx<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP_PTR.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            return None;
        }

        // SAFETY: pointer is set in `with_cx` which ensures the reference
        // stays valid for the entire duration of its scope.
        Some(f(unsafe { &mut *(ptr as *mut App) }))
    })
}

/// Called by `LuaState::set` when state changes.
///
/// Marks every Lua-backed view dirty. This is intentionally coarse-grained:
/// Lua render functions can read arbitrary state, and dependency tracking is
/// not implemented yet.
pub fn notify_all_views() {
    let _ = try_current_cx(|cx| {
        crate::bridge::window::notify_all_lua_views(cx);
        cx.refresh_windows();
    });
}
