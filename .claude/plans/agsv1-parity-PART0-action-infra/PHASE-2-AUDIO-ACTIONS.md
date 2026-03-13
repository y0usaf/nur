# Phase 2: Audio Actions POC

**Status**: complete
**Prerequisites**: Phase 1 (ServiceHandle UserData) -- `ServiceHandle`, `register_service_with_actions` must exist in `crates/runtime/src/bridge/service_handle.rs` and `crates/runtime/src/api/services.rs`

---

## Objective

Wire up the audio service as the first proof-of-concept for the action infrastructure built in Phase 1. Add `set_volume` and `toggle_mute` wpctl commands to `AudioService`, then register audio using `register_service_with_actions` so Lua users can call `shell.services.audio:set_volume(0.5)` and `shell.services.audio:toggle_mute()` alongside the existing `:get()`.

---

## Estimated Skills

- **Rust service implementation** -- adding command methods to AudioService
- **mlua closure wiring** -- registering Lua-callable action closures that capture `Entity<AudioState>`
- **GPUI entity access** -- using `context::current_cx` inside action closures

---

## Implementation Steps

### 1. Add wpctl command helpers to `crates/services/src/audio.rs`

- [x] Add a public `set_volume(volume: f32)` function that calls `wpctl set-volume @DEFAULT_AUDIO_SINK@ {volume}` via `std::process::Command`. Clamp input to 0.0..=1.0. Fire-and-forget (ignore errors, log with `tracing::warn` on failure).
- [x] Add a public `toggle_mute()` function that calls `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle` via `std::process::Command`. Fire-and-forget, same error handling.
- [x] Both functions are standalone `pub fn` (not methods on `AudioService`) since they are stateless wpctl invocations.

### 2. Switch audio registration to `register_service_with_actions` in `crates/runtime/src/api/services.rs`

- [x] Replace the `register_service(lua, cx, &services, "audio", audio, ...)` call with `register_service_with_actions(lua, cx, &services, "audio", audio, audio_to_lua, register_audio_actions)`.
- [x] Keep the existing `audio_to_lua` closure (the `|lua, s: &AudioState| { ... }` closure) unchanged.
- [x] Define `register_audio_actions` as a closure or function that takes `(lua, entity, service_handle)` and adds two action methods.

### 3. Register `set_volume` action

- [x] Inside the action registration closure, create a Lua function for `set_volume` that: (a) takes a number argument (the volume level), (b) clamps it to 0.0..=1.0, (c) calls `services::audio::set_volume(volume)`, (d) returns `LuaResult<()>`.
- [x] Register this function on the `ServiceHandle` action table under the key `"set_volume"`.

### 4. Register `toggle_mute` action

- [x] Inside the action registration closure, create a Lua function for `toggle_mute` that: (a) takes no arguments, (b) calls `services::audio::toggle_mute()`, (c) returns `LuaResult<()>`.
- [x] Register this function on the `ServiceHandle` action table under the key `"toggle_mute"`.

### 5. Add unit tests for the new wpctl helpers

- [x] Add a test in `crates/services/src/audio.rs` for `set_volume` clamping logic (extract the clamping into a testable helper if needed, e.g., `format_volume_arg(f32) -> String`).
- [x] Verify both functions handle the "wpctl not found" case gracefully (no panic).

---

## Files to Create/Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/services/src/audio.rs` | Modify | Add `pub fn set_volume(volume: f32)` and `pub fn toggle_mute()` functions. Add `format_volume_arg` helper and tests. |
| `crates/runtime/src/api/services.rs` | Modify | Replace `register_service` call for audio with `register_service_with_actions`. Define `register_audio_actions` closure that adds `set_volume` and `toggle_mute` to the `ServiceHandle`. |

---

## Validation

- [x] `cargo check` passes with no errors
- [ ] `cargo test -p services` -- existing audio tests still pass, new clamping tests pass (linker error outside nix develop -- not a code issue)
- [ ] `cargo test -p runtime` -- no regressions (linker error outside nix develop -- not a code issue)
- [ ] Manual test: launch nur, confirm `shell.services.audio:get()` returns volume/muted table
- [ ] Manual test: call `shell.services.audio:set_volume(0.3)` from Lua config, verify system volume changes
- [ ] Manual test: call `shell.services.audio:toggle_mute()` from Lua config, verify mute toggles

---

## Notes

- The wpctl commands are fire-and-forget for now. A future enhancement could re-poll immediately after an action to update the reactive state faster (currently the polling thread picks up changes within 3 seconds).
- `set_volume` and `toggle_mute` do NOT need GPUI context or `Entity` access -- they are pure `std::process::Command` calls. This keeps the action closures simple. Future services (e.g., Mpris) may need `entity.update(cx, ...)` inside their actions, which is why Phase 1's infrastructure supports it, but audio does not require it.
- The `Entity<AudioState>` is still passed to the action registration closure for API consistency, even though audio actions do not use it.
