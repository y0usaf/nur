# Nix helper functions for generating Nur Lua configurations.
#
# This lets NixOS/home-manager users configure their entire desktop in Nix
# without writing Lua by hand — while still allowing raw Lua via `extraConfig`.
#
# Usage in flake.nix:
#   let nur = inputs.nur.lib; in
#   programs.nur.config = nur.mkConfig {
#     bar = nur.mkBar {
#       left   = [ (nur.mkWorkspaces {}) ];
#       center = [ (nur.mkClock {})      ];
#       right  = [ (nur.mkBattery {})    ];
#     };
#   };

{ lib }:

rec {
  # Combine sections into a complete config string.
  mkConfig = { bar ? "", extraConfig ? "" }: ''
    ${bar}
    ${extraConfig}
  '';

  # A top/bottom/left/right bar with left/center/right widget slots.
  mkBar = {
    position  ? "top",
    height    ? 32,
    exclusive ? true,
    left      ? [],
    center    ? [],
    right     ? [],
  }: let
    renderSlot = widgets:
      lib.concatStringsSep "\n" (map (w: "${w}") widgets);
  in ''
    do
      local bar = shell.window({
        position  = "${position}",
        height    = ${toString height},
        exclusive = ${lib.boolToString exclusive},
      })
      bar:render(function()
        return ui.bar_layout(
          { ${renderSlot left}   },
          { ${renderSlot center} },
          { ${renderSlot right}  }
        )
      end)
    end
  '';

  # Clock widget — returns a Lua expression string (usable in a slot list).
  mkClock = { format ? "%H:%M", interval ? 1000 }: ''
    (function()
      local Clock = require("nur.widgets.clock")
      return Clock.new({ format = "${format}", interval = ${toString interval} }):render()
    end)()
  '';

  # Battery widget.
  mkBattery = {}: ''
    (function()
      local Battery = require("nur.widgets.battery")
      return Battery.new():render()
    end)()
  '';

  # Workspace indicators.
  mkWorkspaces = {}: ''
    (function()
      local Workspaces = require("nur.widgets.workspaces")
      return Workspaces.new():render()
    end)()
  '';

  # Raw Lua expression passthrough — escape hatch for anything not covered.
  mkRaw = lua: lua;
}
