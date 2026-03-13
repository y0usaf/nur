# IDEA: Action Infrastructure for Services

## Parent IDEA
`agsv1-parity-IDEA.md`

## Dependencies
None. This is the foundation part.

## Problem
Current `register_service` in `crates/runtime/src/api/services.rs` only supports read-only reactive state (Rust->Lua via `LuaState`). New services (Mpris, Bluetooth, Notifications, PowerProfiles) need Lua->Rust action methods (e.g., `shell.services.mpris:play()`). There is no pattern for this yet.

## Goal
Extend the service registration infrastructure so services can expose both reactive state AND callable action methods to Lua.

## Scope
- Introduce a `register_service_with_actions` helper (or extend `register_service`) that accepts action definitions alongside the `to_lua` converter
- Actions are Lua functions that send commands to the GPUI entity via `entity.update(cx, ...)` or a channel
- The service Lua table should support both `:get()` (existing LuaState) and method calls like `:play()`, `:set_profile("balanced")`
- Decide on the pattern: LuaUserData with metamethods vs. plain table with function fields vs. hybrid

## Design Considerations
- Actions need access to the GPUI entity handle and cx -- must use `context::current_cx`
- Actions may be fire-and-forget (play/pause) or need to return results (scan results)
- Keep backward compatibility: existing read-only services must not change API
- The entity handle must be cloneable into Lua closures (GPUI `Entity<T>` is Clone)

## Success Criteria
- A new or extended registration helper exists that wires up both state and actions
- At least one proof-of-concept action works (can be tested with an existing service, e.g., audio mute toggle)
- Existing read-only services continue to work unchanged
- Pattern is documented in code comments for future service authors
