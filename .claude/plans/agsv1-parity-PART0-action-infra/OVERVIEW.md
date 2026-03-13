# Plan: Action Infrastructure for Services

**IDEA**: `.claude/plans/agsv1-parity-PART0-action-infra-IDEA.md`
**Status**: pending
**Phases**: 3

---

## Problem

The current `register_service` helper in `crates/runtime/src/api/services.rs` only supports one-way reactive state (Rust to Lua via `LuaState`). Upcoming services (Mpris, Bluetooth, Notifications, PowerProfiles) require Lua-to-Rust action methods (e.g., `shell.services.mpris:play()`). No pattern exists for this.

## Goal

Extend service registration so services can expose both reactive state AND callable action methods to Lua, while keeping the existing read-only API unchanged.

## Design Decision: ServiceHandle UserData

The chosen approach is a `ServiceHandle` LuaUserData struct that wraps both the `LuaState` (for reactive `:get()` / `:set()` / `:subscribe()`) and a type-erased action dispatch mechanism (for action methods). This replaces the bare `LuaState` as the value stored in `shell.services.<key>`.

Note on generics: mlua only allows one `UserData` implementation per concrete type, so `ServiceHandle` must NOT be generic over `S`. Instead, the struct stores the `LuaState` directly and action methods are registered dynamically (via a `HashMap<String, LuaFunction>` or by setting fields on the userdata's `__index` table at registration time). The GPUI `Entity<S>` is captured inside the action closures, not stored on the struct.

Key points:
- `ServiceHandle` implements `LuaUserData` with a `__index` metamethod
- `:get()`, `:map()`, `:subscribe()` delegate to the inner `LuaState` (backward compatible)
- Action methods (e.g., `:set_volume()`, `:toggle_mute()`) are registered via closures that capture `Entity<S>` and call `context::current_cx(|cx| entity.update(cx, ...))`.
- `register_service` remains unchanged for read-only services
- A new `register_service_with_actions` helper accepts an additional closure that registers action methods

## Dependencies

None. This is the foundation part.

## Success Criteria

- A new or extended registration helper exists that wires up both state and actions
- At least one proof-of-concept action works (e.g., audio mute toggle)
- Existing read-only services continue to work unchanged
- Pattern is documented in code comments for future service authors

## Constraints

- Lua API functions must return `LuaResult<T>`, not `anyhow::Result<T>`
- All GPUI operations require `context::current_cx`
- `entity.update(cx, f)` returns `()`, not `Result`
- `Entity<T>` is `Clone` and can be captured in Lua closures
- Must not break existing `shell.services.battery:get()` patterns

---

## Phase Table

| Phase | Name | Description | Dependencies | Files | Status |
|-------|------|-------------|--------------|-------|--------|
| 1 | [ServiceHandle UserData](PHASE-1-SERVICE-HANDLE.md) | Create a non-generic `ServiceHandle` struct in `bridge/` that wraps `LuaState` and a dynamic action method table. Implement LuaUserData with `__index` dispatching to `:get()`/`:map()`/`:subscribe()` (delegated to inner LuaState) and dynamically registered action methods. Add `register_service_with_actions` helper. Must handle mlua's single-registration-per-type constraint by using a type-erased design (no generics on the struct; `Entity<S>` captured only in action closures). | None | `crates/runtime/src/bridge/service_handle.rs`, `crates/runtime/src/bridge/mod.rs`, `crates/runtime/src/api/services.rs` | pending |
| 2 | [Audio Actions POC](PHASE-2-AUDIO-ACTIONS.md) | Add `set_volume` and `toggle_mute` methods to `AudioService`, register audio as an action-enabled service using the new helper | Phase 1 | `crates/services/src/audio.rs`, `crates/runtime/src/api/services.rs` | pending |
| 3 | [Tests and Documentation](PHASE-3-TESTS-DOCS.md) | Unit tests for `ServiceHandle` UserData methods, integration test for audio actions, code comments documenting the pattern for future service authors | Phase 2 | `crates/runtime/src/bridge/service_handle.rs`, `crates/runtime/src/api/services.rs` | pending |
