{
  description = "Nur — GPU-accelerated Lua-scriptable desktop shell";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay     = { url = "github:oxalica/rust-overlay"; inputs.nixpkgs.follows = "nixpkgs"; };
    crane            = { url = "github:ipetkov/crane"; };
  };

  outputs = { self, nixpkgs, rust-overlay, crane }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
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

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || pkgs.lib.hasInfix "/lua/" path
              || pkgs.lib.hasSuffix "/lua" path;
          };

          nativeBuildInputs = with pkgs; [ pkg-config makeWrapper ];
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
          ];

          runtimePath = pkgs.lib.makeBinPath (with pkgs; [
            bash
            bluez
            networkmanager
            playerctl
            power-profiles-daemon
            wireplumber
          ]);

          commonArgs = {
            pname = "nur";
            version = "0.1.0";
            inherit src nativeBuildInputs buildInputs;
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            cargoExtraArgs = "-p nur";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        in craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          postInstall = ''
            mkdir -p $out/share/nur
            cp -r lua $out/share/nur/
            wrapProgram $out/bin/nur \
              --prefix PATH : ${runtimePath} \
              --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath buildInputs}
          '';

          meta = with pkgs.lib; {
            description = "GPU-accelerated Lua-scriptable Wayland desktop shell";
            homepage = "https://github.com/y0usaf/nur";
            license = licenses.agpl3Only;
            mainProgram = "nur";
            platforms = platforms.linux;
          };
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

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.nur}/bin/nur";
        };
        nur = self.apps.${system}.default;
      });

      overlays.default = final: prev: {
        nur = self.packages.${prev.stdenv.hostPlatform.system}.nur;
      };

      devShells = forAllSystems (system: {
        default = mkDevShell system;
      });

      homeManagerModules.default = { pkgs, ... }@args:
        import ./nix/module.nix (args // {
          nurPackage = self.packages.${pkgs.stdenv.hostPlatform.system}.nur;
        });

      lib = import ./nix/lib.nix { inherit (nixpkgs) lib; };
    };
}
