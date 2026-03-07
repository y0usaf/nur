# Lumen Architecture

Lumen is a GPU-accelerated, Lua-scriptable Wayland desktop shell built on GPUI
(Zed's UI framework). Users write `~/.config/lumen/init.lua`; Rust executes it,
creates Wayland layer-shell windows, and renders whatever the Lua script
describes using GPUI's GPU-accelerated element pipeline.

---

## Stack

```
~/.config/lumen/init.lua   — user configuration (Lua 5.4)
         ↓
crates/runtime             — Lua VM lifecycle, all Lua↔Rust bridging
         ↓
crates/services            — system data (battery, network, audio, compositor IPC)
         ↓
GPUI (git dep, zed fork)   — reactive rendering, event loop, window management
         ↓
Blade (Vulkan)             — GPU abstraction, actual draw calls
         ↓
wlr-layer-shell (in GPUI)  — Wayland protocol for docking windows to screen edges
```

---

## Crate layout

| Crate | Role |
|---|---|
| `crates/lumen` | Binary entry point. Finds config, boots GPUI, hands off to runtime. |
| `crates/runtime` | Everything Lua-related: VM lifecycle, the Lua API surface, the Lua↔GPUI bridge. |
| `crates/services` | System integrations. No Lua dependency. Exposes GPUI `Entity<T>` objects. |
| `crates/assets` | All embedded resources. `include_str!` for Lua stdlib; `include_bytes!` for icons/fonts (TODO). |

---

## The runtime crate in detail

### `runtime/src/vm.rs` — LuaRuntime

`LuaRuntime` is the single owner of the `mlua::Lua` VM. It is stored as a GPUI
global (`cx.set_global(runtime)`) so it stays alive for the lifetime of the
process and GPUI does not drop it when `main` returns from `Application::run`.

The Lua VM is *also* stored in a thread-local `Rc<Lua>` so that render
callbacks and timer closures can reach it via `vm::with_lua(|lua| ...)` without
needing to carry a reference.

Startup sequence inside `LuaRuntime::run`:
1. `api::register_all` — register `shell.*`, `ui`, and `shell.services.*` globals.
2. `load_stdlib` — load `lua/lumen/stdlib.lua` (pure Lua `ui.*` constructors) and
   register widget modules into `package.preload`.
3. `context::with_cx(cx, ...)` — set the thread-local cx pointer, execute the
   user config, clear the pointer.

---

### `runtime/src/context.rs` — the cx bridge

**Problem:** GPUI requires `&mut App` for almost everything, but Rust's borrow
checker prevents storing it or passing it into closures.

**Solution:** A thread-local raw pointer (`APP_PTR`) that is set during any
scope where `cx` is valid and cleared immediately after.

```
with_cx(cx, || {          ← sets APP_PTR = cx as *mut _
    lua.load(code).exec() ← Lua API calls use current_cx() here
})                        ← clears APP_PTR
```

`with_cx` supports nesting (saves/restores the previous pointer), which is
necessary for timer callbacks that themselves make Lua API calls.

**View notifiers** (`VIEW_NOTIFIERS`): A second thread-local that holds
`Vec<Box<dyn Fn(&mut App)>>`. Every `LuaView` (GPUI view) registers one entry
when its window is created. `LuaState::set()` calls `notify_all_views()` which
iterates this list and calls `cx.notify()` on every view, scheduling a re-render.
This is the "mark everything dirty" reactive model — simple but correct for a
desktop shell.

---

### `runtime/src/bridge/element.rs` — Lua table → GPUI element

Lua render functions return nested tables:
```lua
{ type = "hbox", gap = 8, children = {
    { type = "text", content = "hello" },
    { type = "spacer" },
}}
```

`LuaElement::from_lua_table` parses this recursively into a `LuaElement` enum.
`LuaElement::into_any_element` converts it to a GPUI `AnyElement`.

To add a new element type:
1. Add a variant to `LuaElement`.
2. Add a `match` arm in `from_lua_table` (parsing props from the Lua table).
3. Add a `match` arm in `into_any_element` (constructing the GPUI element).
4. Add the Lua constructor to `lua/lumen/stdlib.lua` if it is a pure table.

The render pipeline never calls into Lua *during* GPUI's GPU draw. Lua is only
called to produce the element tree description; GPUI then renders that tree
on the GPU with no further Lua involvement.

---

### `runtime/src/bridge/state.rs` — LuaState

`LuaState` is the reactive primitive. It wraps an `Rc<RefCell<LuaValue>>` with
a notifier list (`Vec<Rc<dyn Fn()>>`).

`state:set(val)` from Lua:
1. Updates the stored value.
2. Calls each per-state notifier (Rc snapshot taken first so the RefCell is
   free during calls).
3. Calls `context::notify_all_views()` — marks all `LuaView` entities dirty in
   GPUI, causing a re-render on the next frame.

`state:subscribe(fn)` stores the callback in the Lua registry
(`lua.create_registry_value`) so it lives beyond the current Lua stack frame.
On change, `vm::with_lua` retrieves it by key and calls it. The registry key
is `'static` (unlike `LuaFunction` which is lifetime-bound), making this safe
to store in closures.

---

### `runtime/src/bridge/window.rs` — LuaView & LuaWindowHandle

`LuaView` is the GPUI `Render` implementor. It holds:
- `render_key: Option<LuaRegistryKey>` — the stored Lua render function.
- `bg`, `fg`, `font_size` — theme values from `shell.window({...})`.

On each GPUI render tick (only after `cx.notify()` was called):
1. Retrieve the stored render function from the Lua registry.
2. Call it with no arguments; expect a Lua table back.
3. Parse the table via `LuaElement::from_lua_table`.
4. Wrap in a full-size `div` with the window's background/text colors.
5. Return the GPUI element tree for GPU rendering.

`LuaWindowHandle` is the userdata value returned to Lua by `shell.window()`.
Its only method is `:render(fn)` which stores the function in the view entity
(reached via `context::current_cx`) and calls `cx.notify()`.

`open_shell_window` creates a `WindowKind::LayerShell` window using GPUI's
built-in layer shell support (no external Wayland library needed). The
`exclusive_zone` reserves screen space so other windows don't overlap the bar.

---

### `runtime/src/api/` — Lua globals

| Module | Registers |
|---|---|
| `api/shell.rs` | `shell.window`, `shell.state`, `shell.interval`, `shell.once`, `shell.exec`, `shell.watch_file`, `shell.quit` |
| `api/ui.rs` | `ui` table (Rust-native components go here; pure-Lua constructors are in stdlib) |
| `api/services.rs` | `shell.services.battery`, `.audio`, `.network`, `.compositor` |

**mlua error handling:** `mlua::Error` is `!Send + !Sync`, so it cannot be
converted to `anyhow::Error` with bare `?`. All API registration functions
return `LuaResult<()>`. The boundary in `api/mod.rs` converts with
`.map_err(|e| anyhow::anyhow!("{e}"))`.

---

## `crates/services/` — system integrations

Each service follows the same pattern:
```rust
pub struct FooService;
impl FooService {
    pub fn start(cx: &mut App) -> Entity<FooState> {
        let entity = cx.new(|_| FooState::default());
        cx.spawn(async move |cx| {
            // Subscribe to D-Bus / IPC / sysfs events
            // On change: entity.update(cx, |state, cx| { *state = new; cx.notify(); });
        }).detach();
        entity
    }
}
```

Services are started in `api/services.rs` during app init. Their initial values
are written into Lua tables on `shell.services.*`. Live updates are a TODO —
the next step is to create a `LuaState` per service and call `state.set()`
when the GPUI entity updates, which will automatically trigger re-renders.

Compositor auto-detection (`services/src/compositor/mod.rs`) checks
`$HYPRLAND_INSTANCE_SIGNATURE` and `$NIRI_SOCKET` environment variables.

---

## `lua/lumen/` — the Lua standard library

Loaded at startup before the user config runs. Extends the `ui` table (created
empty by Rust) with pure-Lua constructors:

| Constructor | Returns |
|---|---|
| `ui.hbox(props)` / `ui.hstack` | `{ type="hbox", ... }` |
| `ui.vbox(props)` / `ui.vstack` | `{ type="vbox", ... }` |
| `ui.text(content)` | `{ type="text", ... }` |
| `ui.label(content)` | alias for `ui.text` |
| `ui.spacer()` | `{ type="spacer" }` |
| `ui.icon(name)` | `{ type="icon", ... }` |
| `ui.bar_layout(left, center, right)` | convenience hbox with spacers |

Widget modules in `lua/lumen/widgets/` are loaded lazily via `require()` using
`package.preload` registered at startup. Adding a new widget module requires:
1. Create `lua/lumen/widgets/my_widget.lua`.
2. Add an entry to `LUA_MODULES` in `crates/assets/src/lib.rs`.

---

## Wayland layer shell

GPUI (from the `andre-brandao/zed` fork, branch `fix/target_dispay`) has
first-class layer shell support via `gpui::layer_shell::*`. No external crate
is needed. Key types:

```rust
WindowKind::LayerShell(LayerShellOptions {
    layer: Layer::Top,          // Top, Bottom, Overlay, Background
    anchor: Anchor::TOP | ...,  // which edges to stick to
    exclusive_zone: Some(px(h)),// pixels to reserve (pushes other windows away)
    keyboard_interactivity: KeyboardInteractivity::None,
    ..
})
```

---

## Known TODOs / next development steps

### High priority
1. **Live service updates** — wrap each `Entity<ServiceState>` in a `LuaState`;
   observe the entity and call `state.set()` when it changes. This makes
   battery/network/audio truly reactive from Lua.
2. **`shell.watch_file`** — implement using `inotify`. Crucial for live config
   reload without restarting the process.
3. **SVG icons** — implement `LuaElement::Icon::into_any_element` to load from
   `assets/icons/*.svg` using GPUI's image rendering pipeline.

### Medium priority
4. **`ui.button(props)`** — clickable element with `on_click` callback. Requires
   `keyboard_interactivity` on overlay-layer windows.
5. **Style props on text** — `bold`, `italic`, `color` per-text-node.
6. **Padding on `vbox`** — `vbox` layout ignores padding today.
7. **Multi-monitor** — `shell.window` currently opens on the primary display.
   Add a `display = "all" | "primary" | index` prop that iterates `cx.displays()`.

### Lower priority
8. **gpui-component integration** — add rich components (Table, Select, Chart,
   CodeEditor) as native Lua userdata via `crates/ui`.
9. **Nix lib helpers** — flesh out `nix/lib.nix` `mkBar` etc. to generate
   valid Lua, not just string snippets.
10. **Async Lua** — add `shell.async(fn)` using GPUI's async executor for
    non-blocking shell command output streaming.

---

## Dependency notes

- **GPUI**: `git = "https://github.com/andre-brandao/zed", branch = "fix/target_dispay"`.
  This fork adds layer shell support not yet merged into mainline Zed. Track
  upstream `zed-industries/zed` for eventual merge.
- **mlua**: `version = "0.10", features = ["lua54", "vendored"]`. Vendored means
  no system Lua required. `!Send` because we use the default non-send build;
  this is fine since GPUI and Lua both live on the main thread.
- **No smithay / wayland-client**: layer shell is handled entirely inside GPUI.
