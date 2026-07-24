<div align="center">

# nur — نور

*A GPU-accelerated, Lua-scriptable Wayland desktop shell.*

Write `~/.config/nur/init.lua` to define bars, overlays, and widgets.
Rust handles rendering, window management, and system service integration via
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) (Zed's UI framework) and Vulkan/Blade.

</div>

---

> **Winding down.** nur's successor path changed twice: first
> [moonshell](https://github.com/y0usaf/moonshell) (the lean standalone
> rewrite), which has itself now merged into
> [tomoe](https://github.com/y0usaf/tomoe) as its in-process shell
> subsystem — see tomoe's `FUSION.md`. The `shell.*`/`ui.*` Lua
> contract, stdlib, and widgets live on there; nur remains the
> GPUI-based reference until the fused shell reaches widget parity
> (FUSION F3).

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
# Run from the flake package
nix run

# Or enter the dev shell for local development
nix develop
cargo run --bin nur
```

Nur loads your config from `~/.config/nur/init.lua` by default.

---

<div align="center">

## Example config

</div>

```lua
local Clock = require("nur.widgets.clock")
local clock = Clock.new({ format = "%H:%M" })

local win = shell.window({
    position = "top",
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

## CLI

</div>

Run `nur` without a subcommand to start the shell daemon. Once it is running,
the same binary can control it over `$XDG_RUNTIME_DIR/nur.sock`:

```bash
nur reload                    # re-run the active config
nur quit                      # stop the running shell
nur eval 'os.date()'          # evaluate Lua in the running VM
nur msg 'toggle-calendar'     # send a string to shell.on_msg(...)
```

Use `NUR_CONFIG=/path/to/init.lua nur` to start with a non-default config.

---

<div align="center">

## Lua API

</div>

| Function | Description |
|---|---|
| `shell.window(opts)` | Open a layer-shell window |
| `shell.state(val)` | Create a reactive state value |
| `shell.interval(ms, fn)` | Run a callback on a timer |
| `shell.once(ms, fn)` | Run a callback once after a delay |
| `shell.exec(cmd)` | Run a shell command synchronously (use during init only) |
| `shell.exec_async(cmd, fn)` | Run a shell command without blocking the UI |
| `shell.watch_file(path, fn)` | Watch a file for changes (calls `fn(content)`) |
| `shell.quit()` | Exit the shell |

### Services

Services are reactive `LuaState` values — call `:get()` to read the current
value. Re-renders trigger automatically when the underlying data changes.

```lua
shell.services.applications:get() -- { apps = { { name, exec, icon, comment, keywords, categories }, ... } }
shell.services.battery:get()    -- { percent, charging }
shell.services.audio:get()      -- { volume, muted }
shell.services.network:get()    -- { connected, ssid, strength }
shell.services.compositor:get() -- { workspaces, active_workspace, active_window }
shell.services.sysinfo:get()    -- { cpu_percent, memory_percent, memory_used_gb,
                                --   memory_total_gb, temperature, gpu_percent }
shell.services.power_profiles:get() -- { active, profiles }
shell.services.mpris:get()          -- { player_name, status, title, artist, album, art_url,
                                      --   length, position, volume }
shell.services.bluetooth:get()      -- { enabled, discovering, devices }
shell.services.notifications:get()  -- { count, dnd, notifications }
shell.services.systemtray:get()     -- { items }
```

Action-capable services also expose methods such as `:set_volume(...)`,
`:set_profile(...)`, `:play_pause()`, `:connect(addr)`, `:dismiss(id)`,
`:activate(id, x, y)`, `:search(query)`, and `:launch(exec)`.

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
examples/           -- small sample configs
nix/                -- Nix package, overlay, and home-manager module
```

---

<div align="center">

## NixOS / home-manager

</div>

```nix
{
  inputs.nur.url = "github:y0usaf/nur";

  outputs = { nur, ... }: {
    # Home-manager:
    imports = [ nur.homeManagerModules.default ];

    programs.nur = {
      enable = true;
      # Optional: if omitted, the module uses inputs.nur.packages.${pkgs.system}.nur.
      config = builtins.readFile ./init.lua;
    };

    # Or as an overlay:
    nixpkgs.overlays = [ nur.overlays.default ];
  };
}
```

---

<div align="center">

## License

[GNU Affero General Public License v3.0](LICENSE)

</div>
