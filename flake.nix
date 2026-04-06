{
  description = "Nur — GPU-accelerated Lua-scriptable desktop shell";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay     = { url = "github:oxalica/rust-overlay"; inputs.nixpkgs.follows = "nixpkgs"; };
    crane            = { url = "github:ipetkov/crane"; };
  };

  outputs = { self, nixpkgs, rust-overlay, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      mkPkgs = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };

      mkNur = system:
        let
          pkgs        = mkPkgs system;
          toolchain   = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib    = (crane.mkLib pkgs).overrideToolchain toolchain;

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
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          postInstall = ''
            mkdir -p $out/share/nur
            cp -r lua $out/share/nur/
          '';
        });

      mkDevShell = system:
        let
          pkgs        = mkPkgs system;
          toolchain   = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib    = (crane.mkLib pkgs).overrideToolchain toolchain;

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
        in craneLib.devShell {
          packages = buildInputs ++ nativeBuildInputs ++ (with pkgs; [
            rust-analyzer
            cargo-watch
            lua-language-server
            lua5_4
          ]);
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
        };
    in {
      packages = forAllSystems (system: {
        default = mkNur system;
        nur     = mkNur system;
      });

      devShells = forAllSystems (system: {
        default = mkDevShell system;
      });

      homeManagerModules.default = import ./nix/module.nix;

      lib = import ./nix/lib.nix { inherit (nixpkgs) lib; };
    };
}
