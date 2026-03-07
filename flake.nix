{
  description = "Nur — GPU-accelerated Lua-scriptable desktop shell";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay     = { url = "github:oxalica/rust-overlay"; inputs.nixpkgs.follows = "nixpkgs"; };
    crane            = { url = "github:ipetkov/crane"; };
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib  = (crane.mkLib pkgs).overrideToolchain toolchain;

        src = craneLib.cleanCargoSource ./.;

        nativeBuildInputs = with pkgs; [ pkg-config ];
        buildInputs = with pkgs; [
          wayland
          libxkbcommon
          vulkan-loader
          vulkan-headers
          fontconfig
          freetype
          openssl
          pipewire
          libxcb
          libx11
          libxcursor
          libxi
          libxkbcommon
        ];

        commonArgs = {
          inherit src nativeBuildInputs buildInputs;
          # mlua vendored build needs cc
          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        nur = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          postInstall = ''
            mkdir -p $out/share/nur
            cp -r lua $out/share/nur/
          '';
        });
      in {
        packages.default = nur;
        packages.nur      = nur;

        devShells.default = craneLib.devShell {
          packages = buildInputs ++ nativeBuildInputs ++ (with pkgs; [
            rust-analyzer
            cargo-watch
            lua-language-server
            lua5_4
          ]);
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
        };
      }
    ) // {
      # Home-manager module, available as:
      #   inputs.nur.homeManagerModules.default
      homeManagerModules.default = import ./nix/module.nix;

      # Nix helper functions for generating Lua configs:
      #   inputs.nur.lib.mkBar { ... }
      lib = import ./nix/lib.nix { inherit (nixpkgs) lib; };
    };
}
