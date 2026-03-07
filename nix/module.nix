# Home-manager module for Nur.
#
# Add to your flake.nix:
#   inputs.nur.url = "github:you/nur";
#
# In home.nix:
#   imports = [ inputs.nur.homeManagerModules.default ];
#   programs.nur = {
#     enable = true;
#     config = '' ... lua ... '';
#   };

{ config, lib, pkgs, ... }:

let
  cfg = config.programs.nur;
in {
  options.programs.nur = {
    enable = lib.mkEnableOption "Nur GPU-accelerated desktop shell";

    package = lib.mkOption {
      type        = lib.types.package;
      description = "The nur package. Override to use a local build.";
    };

    config = lib.mkOption {
      type        = lib.types.lines;
      default     = "";
      description = ''
        Lua configuration written to $XDG_CONFIG_HOME/nur/init.lua.
        Use inputs.nur.lib helpers to generate this from Nix expressions.
      '';
      example = lib.literalExpression ''
        '''
          local Clock = require("nur.widgets.clock")
          local clock = Clock.new({ format = "%H:%M" })

          local bar = shell.window({ position = "top", height = 32 })
          bar:render(function()
            return ui.bar_layout({}, { clock:render() }, {})
          end)
        '''
      '';
    };

    systemd = {
      enable = lib.mkOption {
        type    = lib.types.bool;
        default = true;
        description = "Whether to install a systemd user service for nur.";
      };
      target = lib.mkOption {
        type    = lib.types.str;
        default = "graphical-session.target";
        description = "systemd target to bind the nur service to.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."nur/init.lua" = lib.mkIf (cfg.config != "") {
      text = cfg.config;
    };

    systemd.user.services.nur = lib.mkIf cfg.systemd.enable {
      Unit = {
        Description = "Nur desktop shell";
        After       = [ cfg.systemd.target ];
        PartOf      = [ cfg.systemd.target ];
      };
      Service = {
        Type      = "simple";
        ExecStart = "${cfg.package}/bin/nur";
        Restart   = "on-failure";
        RestartSec = "3s";
        Environment = [
          "WAYLAND_DISPLAY=%E{WAYLAND_DISPLAY}"
          "XDG_RUNTIME_DIR=%E{XDG_RUNTIME_DIR}"
        ];
      };
      Install.WantedBy = [ cfg.systemd.target ];
    };
  };
}
