# Lumen — AI Development Guide

Read `ARCHITECTURE.md` first for the full technical picture.

---

## Project identity

Lumen is a **GPU-accelerated Lua-scriptable Wayland desktop shell** — think
AGS/EWW/QuickShell but built on GPUI (Zed's UI framework) instead of GTK/Qt.
Users write `~/.config/lumen/init.lua` to define bars, overlays, and widgets.
Rust handles rendering, window management, and system service integration.

---

## Key invariants — never break these

1. **Lua API functions must return `LuaResult<T>`, not `anyhow::Result<T>`.**
   `mlua::Error` is `!Send + !Sync` and cannot be converted to `anyhow::Error`
   via `?`. Convert at the `api/mod.rs` boundary with `.map_err(|e| anyhow::anyhow!("{e}"))`.

2. **Never hold a `&Lua` reference across an async boundary or store it in a struct.**
   Use `vm::with_lua(|lua| ...)` which reads from the thread-local `LUA`.
   Functions that need to call back into Lua later must store a `LuaRegistryKey`
   (which *is* `'static`) not a `LuaFunction` (which is lifetime-bound).

3. **All GPUI operations require an active cx. Use `context::current_cx`.**
   The `APP_PTR` thread-local is set by `context::with_cx(cx, || {...})`.
   It is valid during: Lua config execution, `cx.spawn` update callbacks,
   and timer callbacks. It is NOT valid during: async await points,
   background executor tasks, or after `Application::run` returns.

4. **`cx.new(f)` requires `use gpui::AppContext;` to be in scope.** The method
   is on the `AppContext` trait, not an inherent method of `App`.

5. **`entity.update(cx, f)` returns `()`, not `Result`.** Do not call `.ok()`
   on it. Similarly `cx.update(f)` on `AsyncApp` returns `()` in this GPUI version.

6. **The Lua VM must outlive all GPUI windows.** It is kept alive as a GPUI
   global via `cx.set_global(runtime)` in `main.rs`. Do not remove this.

---

## Adding a new Lua API function

1. Add the function implementation in the appropriate `crates/runtime/src/api/` module.
2. Register it in the `register` function of that module:
   `shell.set("my_fn", lua.create_function(lua_my_fn)?)?;`
3. The function signature must be `fn lua_my_fn(lua: &Lua, args: T) -> LuaResult<R>`.
4. If it needs GPUI context: call `context::current_cx(|cx| { ... })`.
5. If it needs the Lua VM inside an async task: use `vm::with_lua(|lua| ...)`.

---

## Adding a new element type

1. Add a variant to `LuaElement` in `crates/runtime/src/bridge/element.rs`.
2. Add a `from_lua_table` match arm parsing props from the Lua table.
3. Add an `into_any_element` match arm constructing the GPUI element.
4. Add a pure-Lua constructor to `lua/lumen/stdlib.lua` returning
   `{ type = "my_type", ... }`.

---

## Adding a new system service

1. Create `crates/services/src/my_service.rs`.
2. Define `MyState` (derive `Clone`) and `MyService`.
3. `MyService::start(cx: &mut App) -> Entity<MyState>` — create entity, spawn
   async task to subscribe to D-Bus/sysfs/IPC events and call
   `entity.update(cx, |s, cx| { *s = new_state; cx.notify(); })`.
4. Export from `crates/services/src/lib.rs`.
5. Register in `crates/runtime/src/api/services.rs` — start the service,
   read initial state, populate a Lua table on `shell.services.my_service`.

---

## Adding a new Lua widget module

1. Create `lua/lumen/widgets/my_widget.lua`.
2. Add `("lumen.widgets.my_widget", include_str!("../../../lua/lumen/widgets/my_widget.lua"))`
   to `LUA_MODULES` in `crates/assets/src/lib.rs`.
3. Users access it via `local W = require("lumen.widgets.my_widget")`.

---

## Build & run

```bash
# Enter dev shell (sets LD_LIBRARY_PATH for Wayland/Vulkan)
nix develop

# Check for errors
cargo check

# Build
cargo build --bin lumen

# Run (must be inside nix develop for library paths)
./target/debug/lumen

# Install example config
cp examples/simple-bar/init.lua ~/.config/lumen/init.lua
```

The config path resolution order: `$LUMEN_CONFIG` → `$XDG_CONFIG_HOME/lumen/init.lua`
→ `~/.config/lumen/init.lua`.

---

## GPUI API quick reference (this fork's version)

```rust
use gpui::AppContext; // required for cx.new()

// Create an entity (model or view)
let entity: Entity<T> = cx.new(|cx| T::new(cx));

// Read entity state
let val: &T = entity.read(cx);

// Update entity state (returns (), not Result)
entity.update(cx, |state, cx| {
    state.field = new_value;
    cx.notify(); // schedule re-render
});

// Open a window
cx.open_window(options, |_, cx| cx.new(MyView::new))?;

// Spawn an async task (no Send required)
cx.spawn(async move |cx| {
    cx.background_executor().timer(Duration::from_secs(1)).await;
    cx.update(|cx| { /* synchronous GPUI work here */ });
}).detach();

// Layer shell window
WindowKind::LayerShell(LayerShellOptions {
    layer: Layer::Top,
    anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
    exclusive_zone: Some(px(32.0)),
    keyboard_interactivity: KeyboardInteractivity::None,
    ..Default::default()
})
```

---

## Common pitfalls

| Symptom | Cause | Fix |
|---|---|---|
| `method 'new' not found on &mut App` | `AppContext` trait not in scope | `use gpui::AppContext;` |
| `?` send/sync error on mlua | Returning `anyhow::Result` from Lua fn | Change return type to `LuaResult<T>` |
| `.ok()` fails with "unit type" | `cx.update()` / `entity.update()` return `()` | Remove `.ok()` |
| State changes but window doesn't update | View not registered as notifier | `context::register_view_notifier` called in `open_shell_window` |
| `NoWaylandLib` panic | Running outside `nix develop` | Run inside dev shell; `LD_LIBRARY_PATH` must include Wayland/Vulkan libs |
| `Lua runtime not initialised` panic | `vm::with_lua` called before `LuaRuntime::new()` | Ensure `LuaRuntime` is created before any render callbacks fire |

---

## File map

```
crates/lumen/src/
  main.rs         — entry point; Application::new().with_assets().run()
  config.rs       — config file location resolution

crates/runtime/src/
  lib.rs          — pub re-exports; Global impl for LuaRuntime
  vm.rs           — LuaRuntime struct; thread-local LUA; with_lua()
  context.rs      — APP_PTR thread-local; with_cx / current_cx / notify_all_views
  api/
    mod.rs        — register_all(); error type boundary
    shell.rs      — shell.window / state / interval / once / exec / quit
    ui.rs         — ui table (Rust-native components)
    services.rs   — shell.services.*
  bridge/
    mod.rs        — re-exports
    element.rs    — LuaElement enum; from_lua_table; into_any_element
    state.rs      — LuaState; reactive value with notifier chain
    window.rs     — LuaView (Render impl); LuaWindowHandle userdata; open_shell_window

crates/services/src/
  lib.rs          — pub re-exports of all services
  battery.rs      — BatteryService / BatteryState (sysfs + upower TODO)
  audio.rs        — AudioService / AudioState (PipeWire TODO)
  network.rs      — NetworkService / NetworkState (NetworkManager TODO)
  compositor/
    mod.rs        — CompositorService; auto-detect Hyprland vs Niri
    hyprland.rs   — Hyprland IPC event stream (stub)
    niri.rs       — Niri IPC event stream (stub)

crates/assets/src/
  lib.rs          — LUA_STDLIB / LUA_MODULES (include_str!); LumenAssets

lua/lumen/
  stdlib.lua      — ui.hbox / vbox / text / spacer / icon / bar_layout
  utils.lua       — round / fmt_bytes / clamp / trim
  widgets/
    clock.lua     — Clock.new({ format, interval })
    battery.lua   — Battery.new()
    workspaces.lua — Workspaces.new()

nix/
  module.nix      — home-manager programs.lumen module
  lib.nix         — mkBar / mkClock / mkBattery / mkWorkspaces helpers
  package.nix     — (unused stub; derivation is inline in flake.nix)
```
