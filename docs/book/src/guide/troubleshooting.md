# Troubleshooting: SolidCraft won't start

## 0.10.0 closes a few seconds after it starts (Windows)

SolidCraft 0.10.0 crashes at startup on Windows PCs that draw with Direct3D 12 and the system
shader compiler. That covers older Intel graphics, PCs without a graphics driver (the
"Microsoft Basic Display Adapter"), virtual machines and Remote Desktop sessions. **Install
0.10.1 or later**, which fixes it. Versions before 0.10.0 don't have this problem.

## Start in safe mode

Safe mode starts with default settings (your own settings stay as they are, for the next normal
start), skips the "Recover unsaved design?" offer and draws with the software renderer.

- Windows: run `solidcraft.exe --safe-mode` from a terminal in the install folder, or add
  ` --safe-mode` to the end of a shortcut's *Target*.
- macOS: `open -a SolidCraft --args --safe-mode`
- Linux: `solidcraft --safe-mode` (Flatpak: `flatpak run io.tangencylabs.solidcraft --safe-mode`)

Setting `SOLIDCRAFT_SAFE_MODE=1` does the same. From 0.10.2, if two launches in a row stop
before the window opens, the next one starts in safe mode by itself. `SOLIDCRAFT_RENDERER=cpu`
uses only the software renderer and keeps your settings.

## A design won't open or hangs while opening

- **A design that stopped the app while it was being reopened is skipped once.** If SolidCraft
  crashes or hangs while reopening the designs from the last session, the next start does not
  reopen them: it says which design the last session stopped on and opens the start page
  instead, so the app stays usable. Open the design yourself if you want it; the skip clears
  itself either way.
- **Open the previous save.** Every save keeps the version it replaced next to it as
  `<file>.bak` (see [Files](files.md)). Copy it to a new name ending in `.sc`, for
  example `bracket-previous.sc`, and open that. Work saved after the previous save is
  not in it.
- **Check the crash reports.** If SolidCraft crashed while the design was open, the newest report
  in the `crashes` folder (see the table below) usually holds a `design.sc` with the
  design as it was. `solidcraft-cli crashes` lists the reports.
- **Unsaved designs** are in the `recovery` folder, not next to the file.
- **Report it.** GitHub doesn't accept `.sc` (or `.solidcraft`) attachments, so zip the file
  (`design.sc.zip`) and attach it to an issue, with the crash report if there is one.

## Reset the settings

SolidCraft keeps its settings and its data in the folders below. All installed versions share
them. No version changes anything outside them: no system settings, drivers or registry
entries. Quit SolidCraft, then **rename** (don't delete) the folders. SolidCraft starts with
defaults and makes new ones. Rename them back to restore your settings, or send them to us
with a bug report.

| System | Settings (`app.ron`, `graphics.json`) | Data (`recovery`, `crashes`, `control`, thumbnails) |
|---|---|---|
| Windows | `%APPDATA%\SolidCraft` | `%LOCALAPPDATA%\SolidCraft` |
| macOS | `~/Library/Application Support/SolidCraft` | (the same folder) |
| Linux | `~/.local/share/solidcraft` | (the same folder); thumbnails in `$XDG_CACHE_HOME/SolidCraft` or `/tmp/SolidCraft` |
| Linux (Flatpak) | `~/.var/app/io.tangencylabs.solidcraft/data/solidcraft` | (the same folder) |

On Windows, paste `%APPDATA%` or `%LOCALAPPDATA%` into the Explorer address bar to open the
folder. Releases before the rename used `SolveCraft` folders next to these; SolidCraft only
reads them, so you can leave them alone.

- The **`recovery`** folder holds autosaves of designs that were never saved. Rename the data
  folder rather than deleting it, so you can still open those autosaves (`.sc` files, or `.solidcraft` from earlier releases)
  with File > Open.
- The **`crashes`** folder holds crash reports. Please attach the newest one to a bug report.
  GitHub does not accept `.sc` files: attach a design as a `.zip` instead (Help >
  Crash Reports… > Report…, then **Save design (.zip)…**, or rename
  `name.sc` to `name.sc.zip`), and only if it can be shared publicly.

From 0.10.2, SolidCraft also checks the settings files at start. A file it can't read is renamed
to `app.ron.bad-<time>` or `graphics.json.bad-<time>`, and SolidCraft starts with defaults
instead.

## "No graphics adapter" or nothing happens at all

SolidCraft needs Direct3D 12 or Vulkan on Windows, Metal on macOS, and Vulkan or OpenGL on
Linux. Without a graphics driver it falls back to the system's software renderer (WARP on
Windows, llvmpipe or lavapipe on Linux), which is slow but works. From 0.10.2, if no window can
be opened at all, SolidCraft tries the software renderer once and otherwise shows a message
saying what failed (earlier versions closed without a word).

On Windows, if the graphics API cannot create the window's renderer, SolidCraft tries the
remaining APIs in order: Direct3D 12 → Vulkan → OpenGL, then software rendering. The status
message and **Preferences › Graphics** say which API failed and why. The saved graphics
preference stays unchanged. With an explicit `WGPU_BACKEND`, a failed start shows the error
and exits without switching; change or remove that variable to try another API.

Things to try:

- Install the graphics driver from the GPU maker (Intel, AMD, NVIDIA) rather than Windows Update.
- In a virtual machine, turn on 3D acceleration. Over Remote Desktop, try it on the machine itself.
- Run `solidcraft --crash-log` to list recent crash reports, and attach the newest one to an
  issue.
- On an Intel Mac (Intel Iris graphics, macOS 12), "Parent device is lost" at start comes from
  the Metal compiler refusing wgpu's indirect-call validation shader. SolidCraft leaves that
  validation off on Intel Macs. To change it, set `WGPU_VALIDATION_INDIRECT_CALL=1` (on) or `0`
  (off) before starting; a value you set is always kept.

## The desktop stutters or freezes while SolidCraft builds bodies

Some graphics drivers hang on the shader SolidCraft uses to build bodies on the GPU, and the
system then resets the GPU. SolidCraft already builds bodies on the CPU on the AMD FirePro
D300/D500/D700 (Mac Pro 2013). On another GPU that does this, start SolidCraft with
`SOLIDCRAFT_CPU_TESS=1` set to build bodies on the CPU, and open an issue with the GPU's name.

## The interface looks blurry on Linux with display scaling

On Linux, SolidCraft runs through XWayland whenever an X display is available, because file
drag and drop from a file manager only works there (niri and Hyprland stay on native Wayland
when Automatic is selected). A compositor that doesn't let X11 apps scale themselves draws an
XWayland window at 100 % and stretches it to the monitor's scale, so text and icons look soft.
That happens with fractional scaling (125 %, 140 %), and on GNOME 47 (RHEL and Rocky Linux 10)
at any scale, 200 % included. The diagnostics in a bug report show it as about 1 physical pixel
per point at a scale of 200 %. On Hyprland with `force_zero_scaling` and `GDK_SCALE` set, an
XWayland UI can instead look ~2.5× oversized relative to the desktop (#10480).

Run SolidCraft on native Wayland instead: Preferences › Graphics › **Window system** ›
Wayland, which applies at the next start (Automatic already picks Wayland on niri and Hyprland).
Versions without that setting (0.21.0 and earlier) take `SOLIDCRAFT_WAYLAND=1` in the
environment:

```
SOLIDCRAFT_WAYLAND=1 solidcraft
```

On native Wayland, dropping files onto the window doesn't work; use File > Open or pass the file
on the command line.
[Wayland, XWayland, niri and Hyprland](https://github.com/tangency-labs/solidcraft/blob/main/docs/nix.md#wayland-xwayland-niri-and-hyprland)
has the details.

## Trackpad pinch does nothing on Linux

A Wayland compositor reports touchpad gestures only to native Wayland windows. Through
XWayland there is no gesture protocol, so a pinch never reaches SolidCraft at all (two-finger
scrolling still works there, because X11 reports it as a wheel). Run SolidCraft on native
Wayland (see above) and pinch zooms the view at the cursor, as it already did on macOS and
the browser.

## SolidCraft closes right away on Windows

A graphics driver can crash the app while it starts, before SolidCraft can show anything (an
old Intel Vulkan driver, `igvk64.dll`, did). Windows' Event Viewer names the driver under
*Windows Logs › Application* ("Faulting module name").

From 0.11.1, SolidCraft on Windows starts Direct3D 12 first and loads no other graphics API
when Direct3D 12 has a graphics card; Vulkan and then OpenGL are tried only on a computer
without one. Earlier versions started Vulkan, Direct3D 12 and OpenGL together and preferred
Vulkan.

Choose **Preferences > Graphics > Graphics API** to select an API for the next start.
The page shows the current API separately from the saved choice. **Automatic** keeps
the Windows order above. **Vulkan** enables GPU path tracing when the graphics device
supports it. **Direct3D 12** and **OpenGL** remain available for compatibility.

Use **Restart now** to apply the saved choice. SolidCraft asks about unsaved designs
before it closes and reopens saved designs after restarting. Canceling that save prompt
keeps the app open; the selected API remains saved for the next start. Canceling
Preferences restores the setting from when the dialog opened.

A newly selected API gets one guarded startup attempt. If the app stops before its first
frame is presented, the next normal launch restores the previous choice. A handled
startup failure restores it before retrying. A failure to save the guard uses software
rendering for that launch. The guard does not detect a blank image or a hang after the
first frame; normal device-loss recovery still applies.

Safe mode uses the software renderer without consuming a pending API choice. An explicit
launch override still takes precedence and is shown in Graphics. Changing the API in
Preferences does not require commands, environment variables or editing settings files.

## GPU path tracing is unavailable in a Direct3D 12 session

On Windows, GPU path tracing needs a Vulkan session. SolidCraft does not load Vulkan
behind a Direct3D 12 window, because initializing another installed driver can crash
the process.

In Render, select **GPU path tracing**, then **Restart with Vulkan** in the backend
message. Alternatively, choose **Vulkan** in **Preferences > Graphics > Graphics API**
and use **Restart now**. Both use the saved setting and the startup recovery described
above. CPU image methods remain available.

A successful Vulkan window does not guarantee that every GPU supports the path tracer.
If shader setup or a device capability still fails, Render reports that reason.
The API switch does not reduce shader compiler memory use.

With OpenGL on Windows, the window has the Windows title bar above SolidCraft's application bar
instead of using the application bar as its title bar. Some OpenGL drivers (older Intel HD and
UHD graphics) otherwise show the picture a title bar's height lower than the window, so clicks
land above what they point at.

## SolidCraft restarted, saying the graphics device was reset

When the graphics driver stops responding, the system resets it (on Windows, "Display driver
stopped responding and has recovered"), and the graphics device SolidCraft draws with is gone
for good: the window can't draw anything more. SolidCraft then closes, keeps your unsaved
changes for recovery as it does after a crash, and starts again; the new window says what
happened, with what the graphics driver reported, and offers the changes back. The old window
closes on its own; SolidCraft doesn't wait for the driver to release it. If the device is reset
again within ten minutes of that restart, the next start uses software rendering, which doesn't
depend on the graphics driver. Updating the graphics driver from the GPU maker usually stops the
resets. Laptops that lose the device on waking from sleep (often with an NVIDIA GPU that doesn't
drive the screen) can keep SolidCraft on the integrated GPU instead: Preferences › Graphics.

## SolidCraft closed while the GPU path tracer was being set up

Choosing **GPU path tracing** in the Render workspace first has the graphics driver compile the
path tracer, which can take minutes. A few drivers crash while compiling it, and the crash takes
SolidCraft with it; nothing inside SolidCraft can catch a crash inside the driver. SolidCraft
remembers that setup (`gpu-path-setup.json` in the settings folder) and doesn't run it again by
itself on that graphics adapter and driver: the Render workspace says what happened and offers
**Try again**. CPU ray tracing still works. A driver update also tries the setup again, and so
does deleting that file.

## A design saved by 0.10 won't open in an older version

Designs saved by a newer SolidCraft use a newer file format. Older versions may refuse them with
"this design was saved by a newer SolidCraft" and don't change the file: open them with the current
version. Recent versions open them with a notice, and a feature they can't read shows an error
on the timeline; update to build it.

## Save As says the file can't be found (Windows)

If saving into *Documents* (or *Pictures*, *Desktop* and other protected folders) fails with a
"File not found" message from the Windows save dialog and nothing is saved, Windows Security's
*Controlled folder access* is probably blocking SolidCraft. Windows usually also shows an
"Unauthorized changes blocked" notification. Either save to a folder that isn't protected, or
allow SolidCraft: *Windows Security › Virus & threat protection › Ransomware protection ›
Controlled folder access › Allow an app through Controlled folder access*, then add
`solidcraft.exe`.
