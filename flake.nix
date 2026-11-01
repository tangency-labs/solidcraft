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

      # Linux only. Why this shell exists. winit opens the window by dlopen'ing the Wayland client libraries,
      # wgpu renders through the EGL loader (libEGL.so.1), and rfd's file dialogs reach
      # xdg-desktop-portal through libdbus-1; a plain `cargo run` under NixOS finds none of them,
      # because there is no /usr/lib and nothing puts them on LD_LIBRARY_PATH:
      #   winit EventLoopError: The wayland library could not be loaded
      #   eframe: WGPU error: Failed to create surface for any enabled backend: {}
      #   rfd::backend::xdg_desktop_portal::portal::libdbus: Can't connect to a portal: libdbus-1.so not found
      # The Mesa drivers themselves — and the EGL vendor / Vulkan ICD descriptions that name them —
      # come from the system in /run/opengl-driver (hardware.graphics.enable), not from here, since
      # that is the driver stack the compositor runs on. Verified on an Intel LNL iGPU under
      # Wayland: wgpu picks the GL backend through this loader and draws.
      # The X11 libraries are the ones the app actually uses on a Wayland desktop: with `DISPLAY`
      # set and `SOLIDCRAFT_WAYLAND` unset, winit runs through XWayland (file drag and drop exists
      # only there). The Wayland ones are for `SOLIDCRAFT_WAYLAND=1`, and both are needed under
      # `xvfb-run` on a machine with no display at all.
      runtimeLibs =
        pkgs:
        pkgs.lib.optionals pkgs.stdenv.isLinux (
          (with pkgs; [
            wayland
            libxkbcommon
            libglvnd
            vulkan-loader
            dbus
          ])
          ++ (with pkgs; [
            # xorg.libX11 and friends are deprecated aliases in current nixpkgs.
            libx11
            libxcursor
            libxi
            libxrandr
            libxrender
            libxext
            libxcb
          ])
        );
      # macOS needs none of the above: winit talks to AppKit, wgpu renders through Metal, and the
      # OS ships its GPU drivers, so the darwin package and shells skip the wrapper and paths.
      driverLibDir = pkgs: if pkgs.stdenv.isLinux then "/run/opengl-driver/lib" else "";
      driverShareDir = pkgs: if pkgs.stdenv.isLinux then "/run/opengl-driver/share" else "";
      libraryPath =
        pkgs:
        if pkgs.stdenv.isLinux then "${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}:${driverLibDir pkgs}" else "";

      shellHook = pkgs: ''
        ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
          if [ ! -d ${driverLibDir pkgs} ]; then
            echo "SolidCraft: ${driverLibDir pkgs} is missing — on NixOS enable hardware.graphics; on other distributions use a matching nixGL driver wrapper." >&2
          fi
        ''}
        command -v cc >/dev/null || echo "solidcraft: no C linker on PATH, cargo cannot link — use nix develop .#rust" >&2
        echo "solidcraft: cargo run --release -p solidcraft -- --sample"
        ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
          export XDG_DATA_DIRS="${driverShareDir pkgs}:''${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
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
          LD_LIBRARY_PATH = libraryPath pkgs;
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
          LD_LIBRARY_PATH = libraryPath pkgs;
          # Under target/, which .gitignore already covers.
          CARGO_TARGET_DIR = "target/nix";
          shellHook = ''
            ${shellHook pkgs}
            echo "solidcraft: $(rustc --version), building into target/nix"
          '';
        };
      });

      packages = forAllSystems (pkgs: rec {
        default = solidcraft;
        solidcraft = assert pkgs.lib.assertMsg
          (pkgs.lib.versionAtLeast pkgs.rustc.version (pkgs.lib.importTOML ./Cargo.toml).workspace.package.rust-version)
          "SolidCraft requires the Rust version advertised in Cargo.toml; update the pinned nixpkgs input.";
          pkgs.rustPlatform.buildRustPackage {
          pname = "solidcraft";
          version = (pkgs.lib.importTOML ./Cargo.toml).workspace.package.version;
          src = self;

          cargoLock.lockFile = ./Cargo.lock;
          # The CLI draws the same packaging icons as the other Linux distributions.
          cargoBuildFlags = [
            "-p"
            "solidcraft"
            "-p"
            "solidcraft-cli"
          ];
          cargoInstallFlags = [
            "-p"
            "solidcraft"
            "-p"
            "solidcraft-cli"
          ];
          # `cargo xtask ci` is the test gate (it wants a wasm target and a GPU); the sandbox has
          # neither.
          doCheck = false;

          nativeBuildInputs = [ pkgs.makeWrapper ];
          buildInputs = runtimeLibs pkgs;

          postInstall = ''
            ${pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              install -Dm644 assets/platforms/linux/io.tangencylabs.solidcraft.desktop \
                $out/share/applications/io.tangencylabs.solidcraft.desktop
              install -Dm644 assets/platforms/linux/io.tangencylabs.solidcraft.mime.xml \
                $out/share/mime/packages/io.tangencylabs.solidcraft.xml
              for size in 16 24 32 48 64 128 256 512; do
                icon=$out/share/icons/hicolor/''${size}x''${size}/apps/io.tangencylabs.solidcraft.png
                mkdir -p "$(dirname "$icon")"
                cp assets/icons/app/linux/exports/hicolor/''${size}x''${size}/apps/io.tangencylabs.solidcraft.png "$icon" >/dev/null
              done
            ''}
          '';

          # Linux only: the app reaches the window through dlopen'd libraries, so the packaged
          # binary needs the same LD_LIBRARY_PATH the shell exports. macOS needs no wrapper.
          postFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            wrapProgram $out/bin/solidcraft \
              --prefix LD_LIBRARY_PATH : "${libraryPath pkgs}" \
              --set-default XDG_DATA_DIRS /usr/local/share:/usr/share \
              --prefix XDG_DATA_DIRS : "$out/share:${driverShareDir pkgs}"
          '';

          meta = {
            description = "Rust-native parametric 3D CAD app";
            homepage = "https://github.com/bherbruck/solidcraft";
            license = with pkgs.lib.licenses; [
              mit
              asl20
            ];
            mainProgram = "solidcraft";
          };
        };
      });

      apps = forAllSystems (pkgs: let
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.solidcraft;
        desktop = { type = "app"; program = "${package}/bin/solidcraft"; };
      in {
        default = desktop;
        solidcraft = desktop;
        cli = { type = "app"; program = "${package}/bin/solidcraft-cli"; };
      });
    };
}
