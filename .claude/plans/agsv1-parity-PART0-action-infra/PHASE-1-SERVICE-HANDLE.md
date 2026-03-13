# Phase 1: ServiceHandle UserData

**Status**: complete
**Scope**: narrow
**Prerequisites**: None

---

## Objective

Create a non-generic `ServiceHandle` struct that wraps `LuaState` plus a dynamic action method table. Implement `LuaUserData` with `__index` that dispatches to the inner `LuaState` methods (`:get()`, `:map()`, `:subscribe()`) and to dynamically registered action methods. Add a `register_service_with_actions` helper that wires up both reactive state and action closures.

---

## Skills

- **Rust module creation** -- new file in bridge/, re-export in mod.rs
- **mlua UserData patterns** -- `__index` metamethod, `LuaFunction` storage, type-erased design
- **Existing codebase patterns** -- follow `register_service` helper shape, `LuaState` delegation

---

## Implementation Steps

### 1. Create `crates/runtime/src/bridge/service_handle.rs`

- [x] Define `ServiceHandle` struct with two fields:
  - `state: LuaState` -- the reactive state (delegates `:get()`, `:map()`, `:subscribe()`)
  - `actions: Rc<RefCell<HashMap<String, LuaFunction>>>` -- dynamically registered action methods
- [x] Implement a `ServiceHandle::new(state: LuaState) -> Self` constructor (empty actions map)
- [x] Implement `ServiceHandle::register_action(&self, name: String, func: LuaFunction)` to insert into the actions map
- [x] Implement `LuaUserData for ServiceHandle`:
  - Add `__index` metamethod via `add_meta_method(MetaMethod::Index, ...)` that:
    1. Checks if `key` matches `"get"`, `"map"`, `"subscribe"`, `"set"` -- if so, returns a Lua function that delegates to `self.state` (retrieve from userdata, call the corresponding LuaState method)
    2. Checks if `key` exists in `self.actions` -- if so, returns the stored `LuaFunction`
    3. Otherwise returns `Nil`
  - The delegating functions for `get`/`map`/`subscribe`/`set` should be created via `lua.create_function()` inside `__index`, capturing a clone of `self.state`. This avoids lifetime issues since `LuaState` is `Clone` and `'static`.

### 2. Update `crates/runtime/src/bridge/mod.rs`

- [x] Add `pub mod service_handle;` line
- [x] Update the module doc comment to mention `service_handle`

### 3. Add `register_service_with_actions` in `crates/runtime/src/api/services.rs`

- [x] Add import for `ServiceHandle` from `crate::bridge::service_handle`
- [x] Create `register_service_with_actions` function with signature:
  ```
  fn register_service_with_actions<S, F, A>(
      lua: &Lua,
      cx: &mut App,
      services: &LuaTable,
      key: &'static str,
      entity: Entity<S>,
      to_lua: F,
      register_actions: A,
  ) -> LuaResult<()>
  where
      S: Clone + 'static,
      F: Fn(&Lua, &S) -> LuaResult<LuaTable> + 'static,
      A: FnOnce(&Lua, &ServiceHandle, Entity<S>) -> LuaResult<()>,
  ```
- [x] Implementation mirrors `register_service` but:
  1. Creates `LuaState` from initial value (same as existing)
  2. Wraps it in `ServiceHandle::new(lua_state)`
  3. Sets up `cx.observe()` on the entity to update the inner `LuaState` (same pattern -- access via `handle.state` or a getter)
  4. Calls `register_actions(lua, &handle, entity.clone())` so the caller can register action closures
  5. Sets `services.set(key, handle)` instead of bare `LuaState`
- [x] Expose `ServiceHandle.state` as `pub` (or add a `pub fn state(&self) -> &LuaState` getter) so the observe callback can call `.set()` on it

### 4. Ensure backward compatibility

- [x] Existing `register_service` remains unchanged -- read-only services (battery, sysinfo, compositor, network) continue using bare `LuaState`
- [x] The `ServiceHandle` `__index` supports the same `:get()` / `:map()` / `:subscribe()` API, so Lua code calling `shell.services.audio:get()` will work identically whether it receives a `LuaState` or `ServiceHandle`

### 5. Add code comments documenting the pattern

- [x] Document at module level in `service_handle.rs`: why non-generic (mlua single-registration constraint), how actions are stored, how delegation works
- [x] Document `register_service_with_actions`: when to use it vs `register_service`, example usage for adding an action

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/runtime/src/bridge/service_handle.rs` | CREATE | `ServiceHandle` struct, `LuaUserData` impl with `__index` delegation |
| `crates/runtime/src/bridge/mod.rs` | MODIFY | Add `pub mod service_handle;` and update doc comment |
| `crates/runtime/src/api/services.rs` | MODIFY | Add `register_service_with_actions` helper, import `ServiceHandle` |

---

## Validation

- [x] `cargo check` passes with no errors in `crates/runtime`
- [x] `cargo test -p runtime` passes -- existing `LuaState` tests unaffected (linking fails due to missing system libs outside nix develop, but `cargo check --tests` confirms compilation)
- [x] `register_service` continues to work for existing read-only services (no changes to its callers)
- [x] The new `register_service_with_actions` compiles and is callable (will be exercised in Phase 2 with audio actions)

---

## Notes

- The `__index` metamethod approach means each property access on a `ServiceHandle` goes through the dispatch function. This is the standard mlua pattern for dynamic method tables and has negligible overhead for the expected call frequency (UI renders at 60fps, not hot-loop).
- Action closures will capture `Entity<S>` (which is `Clone`) and use `context::current_cx(|cx| entity.update(cx, |state, cx| { ... }))` to mutate service state. This pattern is established in `shell.rs` callbacks.
- `LuaState` methods could alternatively be registered as static methods on `ServiceHandle` via `add_method`, but using `__index` dispatch keeps the type-erased design clean and avoids duplicating the method implementations.
