//! Reactive state values accessible from Lua.
//!
//! `LuaState` is the bridge between Lua code and GPUI's render system.
//! When a state value changes, registered notifiers fire — typically
//! `entity.update(cx, |_, cx| cx.notify())` — which schedules a re-render
//! of every view that renders content depending on that state.
//!
//! # Usage (Lua)
//! ```lua
//! local time = shell.state("00:00")
//! shell.interval(60000, function()
//!   time:set(os.date("%H:%M"))
//! end)
//! -- In a render function:
//! ui.text(time:get())
//! ```

use mlua::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

// Rc<dyn Fn()> lets us snapshot the notifier list cheaply (clone the Rcs)
// and call them with the RefCell borrow already released — no unsafe needed.
type Notifier = Rc<dyn Fn()>;

struct StateInner {
    value: LuaValue,
    notifiers: Vec<Notifier>,
}

/// A reactive Lua value. When set, registered notifiers fire and every GPUI
/// view is marked dirty, triggering a re-render on the next frame.
#[derive(Clone)]
pub struct LuaState {
    inner: Rc<RefCell<StateInner>>,
}

impl LuaState {
    pub fn new(value: LuaValue) -> Self {
        Self {
            inner: Rc::new(RefCell::new(StateInner {
                value,
                notifiers: Vec::new(),
            })),
        }
    }

    pub fn get(&self) -> LuaValue {
        self.inner.borrow().value.clone()
    }

    pub fn set(&self, value: LuaValue) {
        self.inner.borrow_mut().value = value;
        // Run per-state notifiers (cheap Rc snapshot so borrow is released).
        let notifiers: Vec<Notifier> = self.inner.borrow().notifiers.clone();
        for n in notifiers {
            n();
        }
        // Also trigger a global re-render of all GPUI views that display
        // Lua content — this is the simple "mark everything dirty" approach.
        crate::context::notify_all_views();
    }

    /// Register a callback that fires whenever this state changes.
    ///
    /// Used internally by `subscribe` (called from Lua) and by the view
    /// registration path in `window.rs` to trigger `cx.notify()`.
    pub fn add_notifier(&self, f: impl Fn() + 'static) {
        self.inner.borrow_mut().notifiers.push(Rc::new(f));
    }
}

impl LuaUserData for LuaState {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_lua, this, ()| Ok(this.get()));

        methods.add_method("set", |_lua, this, value: LuaValue| {
            this.set(value);
            Ok(())
        });

        // state:map(fn) — transform the value before returning.
        methods.add_method("map", |lua, this, transform: LuaFunction| {
            let val = this.get();
            transform.call::<LuaValue>(val)
        });

        // state:subscribe(fn) — called whenever the value changes.
        methods.add_method("subscribe", |lua, this, callback: LuaFunction| {
            // Store the function in the Lua registry (RegistryKey is 'static).
            let key = lua.create_registry_value(callback)?;
            this.add_notifier(move || {
                // Reach back into the Lua VM via the thread-local to call the
                // stored function — avoids holding a &Lua across the closure.
                crate::vm::with_lua(|lua| {
                    if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                        let _ = f.call::<()>(());
                    }
                });
            });
            Ok(())
        });
    }
}
