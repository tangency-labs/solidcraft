# What the app needs at run time outside its own binary, shared by the packages and the
# development shells in ../flake.nix.
pkgs:
let
  inherit (pkgs) lib;
  isLinux = pkgs.stdenv.hostPlatform.isLinux;
in
rec {
  # Linux only. winit opens the window by dlopen'ing the Wayland client libraries, wgpu renders
  # through the EGL loader (libEGL.so.1), and rfd's file dialogs reach xdg-desktop-portal through
  # libdbus-1; a plain `cargo run` under NixOS finds none of them, because there is no /usr/lib and
  # nothing puts them on LD_LIBRARY_PATH:
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
  runtimeLibs = lib.optionals isLinux (
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
  driverLibDir = if isLinux then "/run/opengl-driver/lib" else "";
  driverShareDir = if isLinux then "/run/opengl-driver/share" else "";
  libraryPath = if isLinux then "${lib.makeLibraryPath runtimeLibs}:${driverLibDir}" else "";
}
