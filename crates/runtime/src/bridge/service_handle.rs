//! `ServiceHandle` -- a Lua userdata that combines reactive state with action methods.
//!
//! This is the bridge type for services that expose both readable state (via
//! `LuaState`) *and* callable actions (e.g. `audio:set_volume(0.5)`).
//!
//! # Why non-generic?
//!
//! mlua requires a single `impl LuaUserData for T` per concrete type. Using a
//! generic `ServiceHandle<S>` would require one registration per service type,
//! which mlua does not support. Instead, we use a type-erased design: the
//! reactive state is stored as a `LuaState` (which holds `LuaValue`) and action
//! methods are stored as `HashMap<String, LuaFunction>`.
//!
//! # How it works
//!
//! Known state methods (`get`, `set`, `map`, `subscribe`) are registered via
//! `add_method` -- mirroring `LuaState`'s own impl -- so they are resolved
//! directly without metamethod overhead. Dynamic action methods are dispatched
//! through `__index` which looks up the `actions` map.

use mlua::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::state::LuaState;

/// A service handle that wraps reactive state and dynamic action methods.
///
/// Exposed to Lua as userdata. State methods (`get`, `set`, `map`, `subscribe`)
/// are registered as direct methods. Dynamic action methods (e.g. `set_volume`)
/// are dispatched through the `__index` metamethod.
#[derive(Clone)]
pub struct ServiceHandle {
    /// The reactive state (delegates `:get()`, `:map()`, `:subscribe()`, `:set()`).
    pub state: LuaState,
    /// Dynamically registered action methods (e.g. `"set_volume"`, `"toggle_mute"`).
    ///
    /// These store `LuaFunction` values directly rather than `LuaRegistryKey`s.
    /// This is acceptable because:
    /// - The functions are created during service registration (synchronous, not async)
    /// - They live as long as the `ServiceHandle` in the `shell.services` table
    /// - They are only returned to Lua via `__index`, never held across async boundaries
    actions: Rc<RefCell<HashMap<String, LuaFunction>>>,
}

impl ServiceHandle {
    /// Create a new handle wrapping the given reactive state with no actions.
    pub fn new(state: LuaState) -> Self {
        Self {
            state,
            actions: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Register an action method that will be accessible from Lua via `handle:name(...)`.
    pub fn register_action(&self, name: String, func: LuaFunction) {
        self.actions.borrow_mut().insert(name, func);
    }
}

impl LuaUserData for ServiceHandle {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // --- State delegation (mirrors LuaState's add_methods) ---

        methods.add_method("get", |_lua, this, ()| Ok(this.state.get()));

        methods.add_method("set", |_lua, this, value: LuaValue| {
            this.state.set(value);
            Ok(())
        });

        methods.add_method("map", |_lua, this, transform: LuaFunction| {
            let val = this.state.get();
            transform.call::<LuaValue>(val)
        });

        methods.add_method("subscribe", |lua, this, callback: LuaFunction| {
            let key = lua.create_registry_value(callback)?;
            this.state.add_notifier(move || {
                crate::vm::with_lua(|lua| {
                    if let Ok(f) = lua.registry_value::<LuaFunction>(&key) {
                        let _ = f.call::<()>(());
                    }
                });
            });
            Ok(())
        });

        // --- Dynamic action dispatch via __index ---

        methods.add_meta_method(mlua::MetaMethod::Index, |_lua, this, key: String| {
            let actions = this.actions.borrow();
            match actions.get(&key) {
                Some(func) => Ok(LuaValue::Function(func.clone())),
                None => Ok(LuaValue::Nil),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Helper: create a `ServiceHandle` with an integer value and push it as a Lua global.
    fn setup_handle(lua: &Lua, value: i64) -> ServiceHandle {
        let state = LuaState::new(LuaValue::Integer(value));
        let handle = ServiceHandle::new(state);
        lua.globals().set("h", handle.clone()).unwrap();
        handle
    }

    #[test]
    fn get_delegates_to_inner_state() {
        let lua = Lua::new();
        setup_handle(&lua, 42);
        let result: i64 = lua.load("return h:get()").eval().unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn set_on_inner_state_visible_through_handle() {
        let lua = Lua::new();
        let handle = setup_handle(&lua, 1);
        handle.state.set(LuaValue::Integer(99));
        let result: i64 = lua.load("return h:get()").eval().unwrap();
        assert_eq!(result, 99);
    }

    #[test]
    fn map_delegates_to_inner_state() {
        let lua = Lua::new();
        setup_handle(&lua, 21);
        let result: i64 = lua
            .load("return h:map(function(v) return v * 2 end)")
            .eval()
            .unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn action_method_is_callable() {
        let lua = Lua::new();
        let handle = setup_handle(&lua, 0);
        let called = Rc::new(Cell::new(false));
        let called2 = called.clone();
        let func = lua
            .create_function(move |_lua, ()| {
                called2.set(true);
                Ok(())
            })
            .unwrap();
        handle.register_action("do_thing".into(), func);
        lua.load("h:do_thing()").exec().unwrap();
        assert!(called.get());
    }

    #[test]
    fn unknown_method_returns_nil() {
        let lua = Lua::new();
        setup_handle(&lua, 0);
        let result: LuaValue = lua.load("return h.nonexistent").eval().unwrap();
        assert_eq!(result, LuaValue::Nil);
    }

    #[test]
    fn multiple_actions_coexist() {
        let lua = Lua::new();
        let handle = setup_handle(&lua, 0);

        let a_called = Rc::new(Cell::new(false));
        let b_called = Rc::new(Cell::new(false));

        let a2 = a_called.clone();
        let func_a = lua
            .create_function(move |_lua, ()| {
                a2.set(true);
                Ok(())
            })
            .unwrap();

        let b2 = b_called.clone();
        let func_b = lua
            .create_function(move |_lua, ()| {
                b2.set(true);
                Ok(())
            })
            .unwrap();

        handle.register_action("action_a".into(), func_a);
        handle.register_action("action_b".into(), func_b);

        lua.load("h:action_a(); h:action_b()").exec().unwrap();
        assert!(a_called.get());
        assert!(b_called.get());
    }
}
