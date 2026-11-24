# The SolidCraft package, built from source. ../flake.nix calls it for the current checkout
# (`solidcraft`) and, through ./channels.nix, for the pinned upstream releases
# (`solidcraft-stable`, `solidcraft-prerelease`).
#
#   src          the source tree
#   version      the package version
#   rustVersion  the Rust floor in that source's Cargo.toml
#   cargoLock    for a source in this repository: { lockFile = ./Cargo.lock; }
#   cargoHash    for a fetched source, whose Cargo.lock Nix cannot read without
#                import-from-derivation
pkgs:
{
  src,
  version,
  rustVersion,
  cargoLock ? null,
  cargoHash ? null,
}:
let
  inherit (pkgs) lib;
  runtime = import ./runtime.nix pkgs;
in
assert lib.assertMsg ((cargoLock == null) != (cargoHash == null))
  "solidcraft: pass exactly one of cargoLock or cargoHash";
assert lib.assertMsg (lib.versionAtLeast pkgs.rustc.version rustVersion)
  "SolidCraft ${version} requires Rust ${rustVersion} or newer, but this nixpkgs has ${pkgs.rustc.version}; update the pinned nixpkgs input, and do not make it follow an older one.";
pkgs.rustPlatform.buildRustPackage (
  {
    pname = "solidcraft";
    inherit version src;

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
    buildInputs = runtime.runtimeLibs;

    postInstall = ''
      ${lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
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
    # binary needs the same LD_LIBRARY_PATH the shell exports. File dialogs go through the
    # desktop portal and fall back to zenity without one (#2177), so zenity comes along,
    # after the system's own on PATH. macOS needs no wrapper.
    # SOLIDCRAFT_VERSION stays unset: it marks a release-workflow build, which offers to
    # download installer updates; Nix updates the package instead.
    postFixup = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
      wrapProgram $out/bin/solidcraft \
        --prefix LD_LIBRARY_PATH : "${runtime.libraryPath}" \
        --suffix PATH : "${lib.makeBinPath [ pkgs.zenity ]}" \
        --set-default XDG_DATA_DIRS /usr/local/share:/usr/share \
        --prefix XDG_DATA_DIRS : "$out/share:${runtime.driverShareDir}"
    '';

    meta = {
      description = "Rust-native parametric 3D CAD app";
      homepage = "https://github.com/bherbruck/solidcraft";
      license = with lib.licenses; [
        mit
        asl20
      ];
      mainProgram = "solidcraft";
    };
  }
  // (if cargoLock != null then { inherit cargoLock; } else { inherit cargoHash; })
)
