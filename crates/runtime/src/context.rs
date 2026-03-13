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

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use gpui::App;

thread_local! {
    static APP_PTR: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };

    /// Callbacks that notify GPUI views to re-render.
    /// Populated when windows are created; called whenever any LuaState changes.
    static VIEW_NOTIFIERS: RefCell<Vec<Box<dyn Fn(&mut App)>>> = const { RefCell::new(vec![]) };
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
    APP_PTR.with(|cell| {
        let ptr = cell.get();
        assert!(
            !ptr.is_null(),
            "No active GPUI context. \
             This function must only be called during Lua config execution \
             or from inside a GPUI event/timer callback."
        );
        // SAFETY: pointer is set in `with_cx` which ensures the reference
        // stays valid for the entire duration of its scope.
        f(unsafe { &mut *(ptr as *mut App) })
    })
}

pub(crate) fn has_cx() -> bool {
    APP_PTR.with(|cell| !cell.get().is_null())
}

/// Register a callback that will be called with `&mut App` whenever any
/// `LuaState` value changes.  Used by GPUI views to schedule re-renders.
pub fn register_view_notifier(f: impl Fn(&mut App) + 'static) {
    VIEW_NOTIFIERS.with(|n| n.borrow_mut().push(Box::new(f)));
}

/// Call all registered view notifiers using the current active cx.
/// No-op if called outside a `with_cx` scope (no cx active).
pub fn notify_all_views() {
    if !has_cx() {
        return;
    }
    current_cx(|cx| {
        VIEW_NOTIFIERS.with(|n| {
            for f in n.borrow().iter() {
                f(cx);
            }
        });
    });
}
