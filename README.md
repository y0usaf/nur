<div align="center">

# nur — نور

*A GPU-accelerated, Lua-scriptable Wayland desktop shell.*

Write `~/.config/nur/init.lua` to define bars, overlays, and widgets.
Rust handles rendering, window management, and system service integration via
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's UI framework) and Vulkan/Blade.

</div>

---

<div align="center">

## Features

</div>

- **Lua 5.4 config** — hot-reloadable shell definition with a simple reactive API
- **GPU-accelerated rendering** — GPUI + Blade (Vulkan) for every frame
- **Wayland-native** — wlr-layer-shell for docked bars and overlays
- **Reactive state** — `shell.state()` values automatically trigger re-renders
- **Built-in services** — battery, audio, network, compositor IPC (Hyprland & Niri)
- **Widget library** — clock, battery, workspaces — all pure Lua, no GTK/Qt

---

<div align="center">

## Quick start

</div>

```bash
# Enter the dev shell (sets up Wayland/Vulkan library paths)
nix develop

# Build
cargo build --bin nur

# Install an example config
cp examples/simple-bar/init.lua ~/.config/nur/init.lua

# Run
./target/debug/nur
```

---

<div align="center">

## Example config

</div>

```lua
local W = require("nur.widgets.clock")
local clock = W.Clock.new({ format = "%H:%M" })

local win = shell.window({
    anchor = "top",
    height = 32,
    bg = "#1e1e2e",
    fg = "#cdd6f4",
})

win:render(function()
    return ui.bar_layout(
        { ui.text("nur") },
        { clock:render() },
        {}
    )
end)
```

---

<div align="center">

## Configuration

</div>

Config is loaded from (in order):

1. `$NUR_CONFIG`
2. `$XDG_CONFIG_HOME/nur/init.lua`
3. `~/.config/nur/init.lua`

---

<div align="center">

## Lua API

</div>

| Function | Description |
|---|---|
| `shell.window(opts)` | Open a layer-shell window |
| `shell.state(val)` | Create a reactive state value |
| `shell.interval(ms, fn)` | Run a callback on a timer |
| `shell.once(fn)` | Run a callback after init |
| `shell.exec(cmd)` | Run a shell command synchronously (use during init only) |
| `shell.exec_async(cmd, fn)` | Run a shell command without blocking the UI |
| `shell.watch_file(path, fn)` | Watch a file for changes (calls `fn(content)`) |
| `shell.quit()` | Exit the shell |

### Services

Services are reactive `LuaState` values — call `:get()` to read the current
value. Re-renders trigger automatically when the underlying data changes.

```lua
shell.services.battery:get()    -- { percent, charging }
shell.services.audio:get()      -- { volume, muted }
shell.services.network:get()    -- { connected, ssid, strength }
shell.services.compositor:get() -- { workspaces, active_workspace, active_window }
shell.services.sysinfo:get()    -- { cpu_percent, memory_percent, memory_used_gb,
                                --   memory_total_gb, temperature, gpu_percent }
```

---

<div align="center">

## Architecture

</div>

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full technical picture.

```
~/.config/nur/init.lua
         |
crates/runtime      -- Lua VM, API surface, Lua<->GPUI bridge
         |
crates/services     -- battery, audio, network, compositor IPC
         |
GPUI (Zed fork)     -- reactive rendering, event loop, window management
         |
Blade (Vulkan)      -- GPU draw calls
         |
wlr-layer-shell     -- Wayland protocol for docked windows
```

---

<div align="center">

## Project layout

</div>

```
crates/nur/         -- binary entry point
crates/runtime/     -- Lua VM lifecycle and all Lua<->Rust bridging
crates/services/    -- system integrations (no Lua dependency)
crates/assets/      -- embedded Lua stdlib and resources
lua/nur/            -- pure-Lua stdlib, utils, and widget modules
examples/           -- example configs
nix/                -- NixOS/home-manager module
```

---

<div align="center">

## NixOS / home-manager

</div>

```nix
{
  programs.nur = {
    enable = true;
    config = builtins.readFile ./init.lua;
  };
}
```

---

<div align="center">

## License

[GNU Affero General Public License v3.0](LICENSE)

</div>
