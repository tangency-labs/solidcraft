{
  description = "SolidCraft — native app development shell and package for Linux and macOS";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # The run-time libraries, driver paths and package live in nix/; see nix/runtime.nix for
      # why a NixOS shell needs them at all.
      runtime = pkgs: import ./nix/runtime.nix pkgs;

      shellHook = pkgs: ''
        ${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
          if [ ! -d ${(runtime pkgs).driverLibDir} ]; then
            echo "SolidCraft: ${(runtime pkgs).driverLibDir} is missing — on NixOS enable hardware.graphics; on other distributions use a matching nixGL driver wrapper." >&2
          fi
        ''}
        command -v cc >/dev/null || echo "solidcraft: no C linker on PATH, cargo cannot link — use nix develop .#rust" >&2
        echo "solidcraft: cargo run --release -p solidcraft -- --sample"
        ${pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
          export XDG_DATA_DIRS="${(runtime pkgs).driverShareDir}:''${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
        ''}
      '';
    in
    {
      devShells = forAllSystems (pkgs: {
        # The shell to work in. It brings no toolchain of its own, Rust or C: the ones already on
        # PATH (a system package, rustup, nixpkgs) keep their build cache and their linker, where a
        # second rustc — or mkShell's second gcc, with another glibc behind it — would make every
        # `cargo build` produce binaries from somewhere else.
        default = pkgs.mkShellNoCC {
          name = "solidcraft";
          LD_LIBRARY_PATH = (runtime pkgs).libraryPath;
          shellHook = shellHook pkgs;
        };

        # For a machine with no Rust at all: `nix develop .#rust`. Its own target directory, so
        # switching between this toolchain and the system one does not thrash the cache.
        rust = pkgs.mkShell {
          name = "solidcraft-rust";
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
          ];
          LD_LIBRARY_PATH = (runtime pkgs).libraryPath;
          # Under target/, which .gitignore already covers.
          CARGO_TARGET_DIR = "target/nix";
          shellHook = ''
            ${shellHook pkgs}
            echo "solidcraft: $(rustc --version), building into target/nix"
          '';
        };
      });

      packages = forAllSystems (
        pkgs:
        let
          workspace = (pkgs.lib.importTOML ./Cargo.toml).workspace.package;
          # The upstream releases pinned in nix/sources.json (refreshed by nix/update.sh).
          channels = import ./nix/channels.nix pkgs;
        in
        rec {
          default = solidcraft;
          # This checkout: `main` when the flake input follows the default branch.
          solidcraft = import ./nix/package.nix pkgs {
            src = self;
            inherit (workspace) version;
            rustVersion = workspace.rust-version;
            cargoLock.lockFile = ./Cargo.lock;
          };
          # The newest release marked Latest.
          solidcraft-stable = channels.stable;
          # The newest published release of any kind: a nightly or release candidate when one is
          # newer than the latest stable release.
          solidcraft-prerelease = channels.prerelease;
        }
      );

      apps = forAllSystems (
        pkgs:
        let
          packages = self.packages.${pkgs.stdenv.hostPlatform.system};
          app = package: {
            type = "app";
            program = "${package}/bin/solidcraft";
          };
        in
        {
          default = app packages.solidcraft;
          solidcraft = app packages.solidcraft;
          stable = app packages.solidcraft-stable;
          prerelease = app packages.solidcraft-prerelease;
          cli = {
            type = "app";
            program = "${packages.solidcraft}/bin/solidcraft-cli";
          };
        }
      );

      # `programs.solidcraft.enable = true;` in Home Manager; see docs/nix.md.
      homeManagerModules.default = import ./nix/hm-module.nix self;
    };
}
