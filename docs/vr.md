# VR headset view

Status: the first step of #2172. The window is shown in a VR headset as a flat window floating
in front of you, and the design in its view is shown in real depth. Mouse and keyboard work as
on the desktop. Controllers, panels placed in the room and a VR keyboard are later steps.

Off by default. Nothing of it runs, and no VR software is touched, unless it is asked for.

## Using it

1. Start the VR software (SteamVR, or Meta Quest Link) and put the headset on its stand or on.
2. Start SolidCraft with `--vr`, or with `SOLIDCRAFT_VR=1`, or turn on Preferences › Graphics ›
   **Show in a VR headset** and restart. `SOLIDCRAFT_VR=0` turns a saved choice off for one run.
3. Put the headset on. The window floats about 1.2 m in front of the seated position; the design
   view in it is a pane of glass with the model behind and in front of it.

Orbit, pan and zoom with the mouse as always: the model turns and grows behind the glass. The
mouse pointer is drawn for the headset, because it shows the window's picture and not the
system's pointer: on the window over the UI and over empty space, and on the model's surface,
at the model's depth, where it points at the model, so it does not seem to sink into it. The
window on the desktop still takes the mouse and the keyboard, so it has to have the focus;
maximized, the pointer cannot leave it by accident.

The window is as readable as its UI is large: Preferences › Appearance › UI scale applies in the
headset as on the desktop, and is the setting to raise for a headset with fewer pixels.

Preferences › Graphics › **In the headset, show the model in the room** (off by default, applies
at once) lets the model and its grid fill the room around the window. Off, the model shows only
through the design view, cut exactly at its edges however the head is held, as on the desktop.

To put the window back in front of you, use the VR software's own recentre (SteamVR: the
dashboard's reset-view button). The window is fixed in the runtime's seated space, so that
recentre moves it too.

Preferences › Graphics says what this run is doing: showing in the headset, waiting for it, or
why the VR view did not start. A start that cannot (no VR software running, no headset, no
Vulkan, the loader missing) leaves the app exactly as it starts without VR.

What is different while it runs:

- The app draws with Vulkan on the graphics card the headset is on, whatever Graphics API is
  saved.
- The window does not wait for the monitor's refresh; the headset's frames set the pace.
- Pictures of the model made off the window's thread (thumbnails, file previews) are drawn by
  the CPU.
- Ambient occlusion and supersampling apply to the window only, not to the eyes' pictures.

Known limits of this step:

- What the UI draws over the design view (dimensions, handles, the view cube) is on the
  window's plane, where the desktop camera puts it. It matches the model exactly at the depth
  of the camera's target and drifts from it nearer and farther.
- The mouse feels odd on the model. A click picks along the desktop camera's ray, not along a
  line from the headset, and the pointer on the model shows what that ray hit. With the head
  away from the desktop camera's viewpoint, the pointer does not follow the mouse as expected.
  This needs a proper fix, which is not designed yet.
- While the view is being moved, and over things that are not a point on the model (an axis, a
  profile), the pointer is on the window's plane and can be seen through model in front of it.
- Panels in front of the model cover it, in the room as through the design view.
- The window on the desktop has to stay open and not minimized: the headset shows what that
  window draws, at the pace it draws.
- After the VR view has ended in a run, the window keeps drawing without waiting for the
  monitor's refresh until the next start.
- Linux is built but untested.

### Trying sizes

While the sizes are tried out on real headsets, four environment variables (metres) replace the
defaults for a run: `SOLIDCRAFT_VR_WIDTH` (the window's width, 1.6), `SOLIDCRAFT_VR_DISTANCE`
(to the window, 1.2), `SOLIDCRAFT_VR_DROP` (its middle below eye level, 0.1) and
`SOLIDCRAFT_VR_DEPTH` (the camera's target behind the glass, 0; negative brings the model out in
front of the window). A value out of reach or not a number is ignored.

### The OpenXR loader

On Windows the OpenXR loader (`openxr_loader.dll`, Khronos, Apache-2.0 OR MIT) is a library an
app brings along, not part of the system. SolidCraft looks for it beside the program first, then
on the search path. Releases do not ship it yet: copy it beside `solidcraft.exe` to try the VR
view. Without it Preferences says that the loader is missing, which is ours to fix and not the
VR software's.

## How it works

The code is in three places:

- `crates/xr`: the view maths (`view`), the decisions a session makes from what the runtime
  reports (`plan`), and the OpenXR session itself (`Runtime`, `Session`). It knows nothing of the
  renderer or the UI.
- `apps/solidcraft/src/vr.rs`: the frame, drawn with the viewport's renderer and the window's
  UI.
- Small hooks in the shared crates: `Renderer::draw_second_view` (viewport-gpu), `Cutout`
  (viewport-egui) and `stereo::Stereo` (ui-egui), all idle unless a host switches them on.

### One device for the window and the headset

An OpenXR runtime composes the pictures it allocated, so it has to create the Vulkan instance
and device (`XR_KHR_vulkan_enable2`). That is why VR is chosen at start: `vr::open` connects to
the runtime, has it create the instance and the device with the features and limits the window
would have asked for, wraps both for wgpu (`wgpu-hal`'s `from_raw` constructors) and hands them
to eframe as an existing device. The window, the viewport and the headset then share one device,
and the model is uploaded once.

Those calls have no safe API. They are in one file, `crates/xr/src/session/interop.rs`, one
function each with its `SAFETY` note; the rest of the crate denies `unsafe`.

### A frame

The runtime shows two layers:

1. Behind, a projection layer: each eye's picture of the room, which holds the model and a
   backdrop in the view's background colours, fixed in the room.
2. In front, a quad layer: a picture of the window, see-through where the design view is, so
   the model behind shows there.

The window's picture is a second copy of the frame's UI. A frame compositor (`UiCopy`) stands in
front of whichever compositor the window has (the view's blur, when on) and draws the frame's
primitives once more into a texture, with `Cutout` switched on: the design view's paint callback
then clears its rectangle to transparent instead of drawing the scene, which erases the
background painted under it and leaves everything painted over it. The window on the desktop is
drawn as ever, by the compositor it had or the ordinary pass.

The model is placed so that the design view is a pane of glass onto it
(`view::place_model`): the desktop camera's target in the pane's middle, its right and up along
the window's, and the height it shows at its target filling the pane's height. So the mouse
moves the model exactly as on the desktop, and what the camera has at its target's distance lies
in the window's plane.

Each eye then draws the scenes the window already uploaded, with its own view and projection
(`Renderer::draw_second_view`), into an offscreen frame of the scene's own kind (depth, MSAA,
order-independent transparency). That draw writes the camera uniform, the culled placements and
the glass order for the eye and submits at once; the window's next `prepare` writes its own again
before it draws, and its cached still picture is not touched.

A last pass puts the eye's frame over the backdrop into the runtime's sRGB picture. It keeps the
model only where that pixel's line of sight passes through the design view's rectangle on the
window, which is exact at its edges whatever the head's tilt. (The draw before it is limited to
that rectangle's bounds in the eye's picture, `view::pane_scissor`, which only saves work.) With
the model in the room neither applies. The pointer on the model is the window's arrow laid in the
room at the model point under it (`view::outline_in_room`), drawn over each eye's picture at the
size it has on the window.

The frame's steps, at the end of the window's UI: `Session::poll` (begin, end or close the
session as the runtime asks), `begin_frame` (waits for the headset's next frame, which paces the
window, and says where the eyes will be), the window's picture from the last frame's UI copy,
the two eyes, `end_frame`. A frame that fails ends the session, says so on the status line, and
the window draws on.

### Decisions tested without a headset

- The picture format: sRGB only, BGRA before RGBA (`plan::pick_format`). A runtime reads a plain
  8-bit format as linear light, which bands.
- The eyes' size: the runtime's recommendation within the view's and the device's limits and a
  pixel budget (`plan::eye_size`).
- The window's pictures grow in steps, so resizing the window does not make new ones at every
  pixel (`plan::window_picture_size`).
- What each session state asks for (`plan::Lifecycle`): ready begins the frames, stopping ends
  them, exiting and a pending loss close the session for good.
- The view maths (`view`): the window's place, the model's placement, each eye's view and
  projection, and the pane's rectangle in an eye's picture. Poses and fields of view a runtime
  reports when it has nothing to report give no picture.

What cannot be tested here is composition: whether the runtime shows the pictures. That needs a
headset.

## Hardware seen so far

| Headset | Runtime | Vulkan | Eye | Notes |
|---|---|---|---|---|
| Meta Quest 3S | Oculus 1.208.0 | `XR_KHR_vulkan_enable2` | 1824 × 1872 | from the `xr-info` probe (#7275) |
| Steam Frame (streaming) | SteamVR/OpenXR 2.18.4 | `XR_KHR_vulkan_enable2` | 2644 × 2644 | from the `xr-info` probe (#13469) |

Neither offers `XR_KHR_vulkan_swapchain_format_list`; formats are enumerated and picked.
