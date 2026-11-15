# SolidCraft with Nix

The flake supports `x86_64-linux`, `aarch64-linux` and `aarch64-darwin`. Enable Nix's `nix-command`
and `flakes` experimental features. The committed `flake.lock` pins nixpkgs and
its Rust toolchain; package evaluation checks that compiler against the Rust
floor in `Cargo.toml`.

## Run, build or install

Run the current app without installing it:

```sh
nix run github:bherbruck/solidcraft#solidcraft -- --sample
```

Inside a checkout, `nix run .` and `nix run .#solidcraft` launch the desktop app.
`nix run .#cli -- --version` launches the CLI. The legacy `.#solidcraft` app name
also works. These commands use the flake's exact wrapped binary, rather than an
older executable already on your shell's `PATH`.

Build without installing:

```sh
nix build github:bherbruck/solidcraft#solidcraft
./result/bin/solidcraft --version
./result/bin/solidcraft --sample
```

The result contains `solidcraft`, `solidcraft-cli` and the deprecated old-name CLI alias
(see [compatibility.md](compatibility.md)). **Building does not install a desktop-menu entry.** For a user
profile installation, run:

```sh
nix profile install github:bherbruck/solidcraft#solidcraft
solidcraft --sample
```

On Linux, this installs the SolidCraft desktop entry, MIME definition and generated icon
sizes alongside the commands. On macOS the package contains the binaries only; for a bundled
`SolidCraft.app`, use the release `.dmg`. NixOS/Home Manager can instead put the flake's
`packages.${system}.solidcraft` in `environment.systemPackages`/`home.packages`.
If a non-NixOS desktop does not discover profile-installed launchers, ensure that
its session includes `~/.nix-profile/share` in `XDG_DATA_DIRS`, then log out and
back in. The desktop/application ID is unchanged (see
[compatibility.md](compatibility.md#package-ids)).

Check which executable a plain terminal command would run:

```sh
type -a solidcraft
readlink -f "$(command -v solidcraft)"
solidcraft --version
nix run github:bherbruck/solidcraft#solidcraft -- --version
```

An unrelated old executable on `PATH` is not updated by `nix build`.
Use the flake app or `./result/bin/solidcraft` to verify the package you just built.

## Graphics and file dialogs

The app wrapper includes Wayland, X11, xkbcommon, EGL, Vulkan-loader and D-Bus
libraries. It exposes `/run/opengl-driver/lib` and
`/run/opengl-driver/share` so NixOS's graphics drivers and Vulkan manifests are
discoverable. Enable `hardware.graphics.enable = true` on NixOS and keep the
system's driver stack consistent with the compositor. File pickers also need a
working session D-Bus and the desktop's `xdg-desktop-portal` service.

On macOS (`aarch64-darwin`), none of this applies: winit talks to AppKit, wgpu renders through
Metal, and the GPU drivers ship with the operating system, so the darwin package carries no
library wrapper and no extra search paths.

On another distribution, graphics drivers outside the Nix store may need a
matching [nixGL wrapper](https://github.com/nix-community/nixGL). Follow its
GPU-specific instructions; a wrapper for a different driver is not a substitute.
For Intel/Mesa, after building:

```sh
nix run github:nix-community/nixGL#nixGLIntel -- ./result/bin/solidcraft --sample
```

If a window is black, first verify that you launched the newly wrapped package
and that your system exposes a functioning GPU driver. `WGPU_BACKEND=vulkan`
selects Vulkan for diagnosis; a loader without a usable driver still cannot render.
OpenGL rendering uses 1x anti-aliasing to avoid the multisampled black-frame failure.
Vulkan and other backends keep the requested sample count; saved preferences are preserved.
For older builds, including 0.3.0, set Anti-aliasing to 1x and restart when using OpenGL.
If that window cannot show the settings dialog, edit only the `msaa` field to `1` in
`~/.local/share/solidcraft/graphics.json`; preserve any other settings.

## Wayland, XWayland, niri and Hyprland

On Linux the app runs through XWayland whenever an X display is available, because
winit only supports dragging files from a file manager into the window on X11. On
niri and Hyprland (detected from `XDG_CURRENT_DESKTOP` with a Wayland socket present)
it stays on native Wayland instead: niri's XWayland (xwayland-satellite) misbehaves
for window management such as maximize/restore, and Hyprland's common
`force_zero_scaling` plus `GDK_SCALE` makes an XWayland UI ~2.5× oversized under
fractional scale (#10480). The trade-off: **on native Wayland, dropping files onto the
window does not work**; use File > Open or pass the file on the command line.
This applies to every build, not only the Nix package.

Choose it in Preferences › Graphics › **Window system** (Automatic, Wayland or X11; it applies
at the next start). The pane says which one is running and, on native Wayland, links to
winit's issue about dropping files there. Native Wayland is the better choice with
display scaling whenever the compositor doesn't let X11 apps scale themselves: XWayland
windows are then drawn at 100 % and stretched, so the interface looks soft and small. That's
the case with **fractional scaling** (a monitor at 125 % or 140 %), and on GNOME 47 (RHEL and
Rocky Linux 10) at any scale, 200 % included; see
[the troubleshooting page](book/src/guide/troubleshooting.md#the-interface-looks-blurry-on-linux-with-display-scaling).
On Hyprland under XWayland you can also set `WINIT_X11_SCALE_FACTOR` to the compositor
scale (for example `1.6`) if you keep Window system on X11.

The environment overrides the preference: `SOLIDCRAFT_WAYLAND=1` (always native Wayland) or
`SOLIDCRAFT_WAYLAND=0` (XWayland whenever an X display is available).

## Development shells

`nix develop` supplies runtime libraries and uses the Rust/C toolchain already on
your `PATH`. `nix develop .#rust` supplies the pinned toolchain, C linker and
`target/nix` build directory. Both retain the GUI and CLI's existing Cargo package
names (`solidcraft`, `solidcraft-cli`).
