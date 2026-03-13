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
        methods.add_method("map", |_lua, this, transform: LuaFunction| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // --- get / set ---

    #[test]
    fn initial_value_is_accessible() {
        let s = LuaState::new(LuaValue::Integer(42));
        assert_eq!(s.get(), LuaValue::Integer(42));
    }

    #[test]
    fn set_changes_the_value() {
        let s = LuaState::new(LuaValue::Integer(1));
        s.set(LuaValue::Integer(99));
        assert_eq!(s.get(), LuaValue::Integer(99));
    }

    #[test]
    fn set_nil_value() {
        let s = LuaState::new(LuaValue::Integer(5));
        s.set(LuaValue::Nil);
        assert_eq!(s.get(), LuaValue::Nil);
    }

    #[test]
    fn set_boolean_value() {
        let s = LuaState::new(LuaValue::Boolean(false));
        s.set(LuaValue::Boolean(true));
        assert_eq!(s.get(), LuaValue::Boolean(true));
    }

    #[test]
    fn set_float_value() {
        let s = LuaState::new(LuaValue::Number(0.0));
        s.set(LuaValue::Number(3.14));
        match s.get() {
            LuaValue::Number(n) => assert!((n - 3.14).abs() < 1e-9),
            other => panic!("expected Number, got {other:?}"),
        }
    }

    #[test]
    fn multiple_sets_accumulate_correctly() {
        let s = LuaState::new(LuaValue::Integer(0));
        for i in 1..=5 {
            s.set(LuaValue::Integer(i));
        }
        assert_eq!(s.get(), LuaValue::Integer(5));
    }

    // --- clone shares state ---

    #[test]
    fn clone_shares_inner_state() {
        let a = LuaState::new(LuaValue::Integer(1));
        let b = a.clone();
        a.set(LuaValue::Integer(2));
        assert_eq!(b.get(), LuaValue::Integer(2));
    }

    #[test]
    fn set_on_clone_visible_to_original() {
        let a = LuaState::new(LuaValue::Nil);
        let b = a.clone();
        b.set(LuaValue::Boolean(true));
        assert_eq!(a.get(), LuaValue::Boolean(true));
    }

    // --- notifiers ---

    #[test]
    fn notifier_called_on_set() {
        let s = LuaState::new(LuaValue::Nil);
        let count = Rc::new(Cell::new(0u32));
        let count2 = count.clone();
        s.add_notifier(move || count2.set(count2.get() + 1));

        s.set(LuaValue::Nil);
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn notifier_not_called_on_new() {
        let count = Rc::new(Cell::new(0u32));
        let count2 = count.clone();
        let s = LuaState::new(LuaValue::Nil);
        s.add_notifier(move || count2.set(count2.get() + 1));
        // No set() called — notifier must remain at 0
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn multiple_notifiers_all_fire() {
        let s = LuaState::new(LuaValue::Nil);
        let a = Rc::new(Cell::new(false));
        let b = Rc::new(Cell::new(false));
        let a2 = a.clone();
        let b2 = b.clone();
        s.add_notifier(move || a2.set(true));
        s.add_notifier(move || b2.set(true));

        s.set(LuaValue::Integer(1));
        assert!(a.get());
        assert!(b.get());
    }

    #[test]
    fn notifier_called_every_set() {
        let s = LuaState::new(LuaValue::Nil);
        let count = Rc::new(Cell::new(0u32));
        let count2 = count.clone();
        s.add_notifier(move || count2.set(count2.get() + 1));

        s.set(LuaValue::Integer(1));
        s.set(LuaValue::Integer(2));
        s.set(LuaValue::Integer(3));
        assert_eq!(count.get(), 3);
    }

    #[test]
    fn notifier_on_clone_also_fires() {
        let a = LuaState::new(LuaValue::Nil);
        let b = a.clone();
        let count = Rc::new(Cell::new(0u32));
        let count2 = count.clone();
        a.add_notifier(move || count2.set(count2.get() + 1));

        b.set(LuaValue::Integer(1)); // set on clone triggers shared notifier list
        assert_eq!(count.get(), 1);
    }
}
