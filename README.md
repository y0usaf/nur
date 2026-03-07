# nur

A GPU-accelerated, Lua-scriptable Wayland desktop shell.

Write `~/.config/nur/init.lua` to define bars, overlays, and widgets.
Rust handles rendering, window management, and system service integration via
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's UI
framework) and Vulkan/Blade.

---

## Features

- **Lua 5.4 config** — hot-reloadable shell definition with a simple reactive API
- **GPU-accelerated rendering** — GPUI + Blade (Vulkan) for every frame
- **Wayland-native** — wlr-layer-shell for docked bars and overlays
- **Reactive state** — `shell.state()` values automatically trigger re-renders
- **Built-in services** — battery, audio, network, compositor IPC (Hyprland & Niri)
- **Widget library** — clock, battery, workspaces — all pure Lua, no GTK/Qt

---

## Quick start

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

## Example config

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

## Configuration

Config is loaded from (in order):

1. `$NUR_CONFIG`
2. `$XDG_CONFIG_HOME/nur/init.lua`
3. `~/.config/nur/init.lua`

---

## Lua API

| Function | Description |
|---|---|
| `shell.window(opts)` | Open a layer-shell window |
| `shell.state(val)` | Create a reactive state value |
| `shell.interval(ms, fn)` | Run a callback on a timer |
| `shell.once(fn)` | Run a callback after init |
| `shell.exec(cmd)` | Run a shell command asynchronously |
| `shell.watch_file(path, fn)` | Watch a file for changes |
| `shell.quit()` | Exit the shell |

### Services

```lua
shell.services.battery   -- { percent, charging }
shell.services.audio     -- { volume, muted }
shell.services.network   -- { connected, ssid, interface }
shell.services.compositor -- { workspaces, active_workspace }
```

---

## Architecture

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

## Project layout

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

## NixOS / home-manager

```nix
{
  programs.nur = {
    enable = true;
    config = builtins.readFile ./init.lua;
  };
}
```

---

## License

[GNU Affero General Public License v3.0](LICENSE)
