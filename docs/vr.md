# VR headset view

Status: the first step of #2172. The window is shown in a VR headset as a flat window floating
in front of you, and the design in its view is shown in real depth. Mouse and keyboard work as
on the desktop. Controllers, panels placed in the room and a VR keyboard are later steps.

Off by default. Nothing of it runs, and no VR software is touched, unless it is asked for.

## Using it

1. Start the VR software (SteamVR, or Meta Quest Link) and put the headset on its stand or on.
2. In Preferences › Graphics › 3D viewing, press **Launch VR**: SolidCraft restarts in the VR
   view and opens your designs again. **Start with VR** beside it opens the VR view at every
   start. `--vr` or `SOLIDCRAFT_VR=1` ask for it for one run; `SOLIDCRAFT_VR=0` turns a saved
   Start with VR off for one run. **Stop VR** ends it at once, without a restart. `SOLIDCRAFT_VR=0` turns a saved choice off for one run.
3. Put the headset on. The window floats about 1.2 m in front of the seated position; the design
   view in it shows the model in depth, behind and in front of the window.

The design view is the monitor's own picture: the same size, the same place, the same camera.
Each eye gets that picture with every point moved sideways by how deep it lies, so what the
camera turns about is in the window's plane, what is farther lies behind the window and what is
nearer comes out of it. It belongs to the window: moving the head does not move the model
against the window, and everything the UI draws over the design (joints, dimensions, handles) is
where it is on the monitor. Orbit, pan and zoom with the mouse as always.

The mouse pointer is drawn for the headset, because it shows the window's picture and not the
system's pointer: on the window over the UI and over empty space, and at the model's depth where
it points at the model, so it does not seem to sink into it. The window on the desktop still
takes the mouse and the keyboard, so it has to have the focus; maximized, the pointer cannot
leave it by accident.

The window is as readable as its UI is large: Preferences › Appearance › UI scale applies in the
headset as on the desktop, and is the setting to raise for a headset with fewer pixels.

Preferences › Graphics › 3D viewing › **Extend viewport** (off by default, applies
at once) makes the model a thing in the room instead: it stands at the window, fills the room
around it with its grid, and is seen from wherever the head is, so moving the head looks around
it. It is then no longer the monitor's picture: it has the size and the perspective the head's
place gives it, and what the UI draws over the design does not follow it.

To put the window back in front of you, use the VR software's own recentre (SteamVR: the
dashboard's reset-view button). The window is fixed in the runtime's seated space, so that
recentre moves it too.

The VR settings have a section of their own, 3D viewing, at the end of Preferences › Graphics,
shown where the app can open a headset at all. It also says what this run is doing: showing in the headset, waiting for it, or
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

- What the UI draws over the design view (dimensions, handles, joints, the view cube) is flat on
  the window, in the place it has on the monitor. The model point it belongs to is in that same
  place, but nearer or farther when it is not at the depth of the camera's target.
- A parallel (orthographic) camera has no distance of its own, so its depth is the depth a
  viewer at the window would see, and a scene much deeper than the view is wide is flattened to
  what eyes follow with ease (zoomed in on a part of a large design).
- With the model in the room, the mouse feels odd on the model: a click picks along the desktop
  camera's ray, not along a line from the headset.
- While the view is being moved, and over things that are not a point on the model (an axis, a
  profile), the pointer is on the window's plane and can be seen through model in front of it.
- Panels in front of the design view cover the model, also where it comes out of the window.
- The window on the desktop has to stay open and not minimized: the headset shows what that
  window draws, at the pace it draws.
- After the VR view has ended in a run, the window keeps drawing without waiting for the
  monitor's refresh until the next start.
- Linux is built but untested.

### Trying sizes

While the sizes are tried out on real headsets, four environment variables (metres) replace the
defaults for a run: `SOLIDCRAFT_VR_WIDTH` (the window's width, 1.6), `SOLIDCRAFT_VR_DISTANCE`
(to the window, 1.2), `SOLIDCRAFT_VR_DROP` (its middle below eye level, 0.1) and, with the model
in the room, `SOLIDCRAFT_VR_DEPTH` (the camera's target behind the window, 0; negative brings
the model out in front of it). A value out of reach or not a number is ignored.

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
and device (`XR_KHR_vulkan_enable2`). That is why VR is chosen at start (Launch VR restarts the app): `vr::open` connects to
the runtime, has it create the instance and the device with the features and limits the window
would have asked for, wraps both for wgpu (`wgpu-hal`'s `from_raw` constructors) and hands them
to eframe as an existing device. The window, the viewport and the headset then share one device,
and the model is uploaded once.

Those calls have no safe API. They are in one file, `crates/xr/src/session/interop.rs`, one
function each with its `SAFETY` note; the rest of the crate denies `unsafe`.

### A frame

The runtime shows these layers, from the back:

1. A projection layer: each eye's picture of the room, a backdrop in the view's background
   colours, fixed in the room (and the model, when it is in the room).
2. Two quad layers at the window's place, one shown to the left eye only and one to the right:
   that eye's picture of the window.

An eye's picture of the window is the design view's picture for that eye with the frame's UI
over it, put together by the app. Each eye has one whole picture, so no two flat layers lie in
one another's place. A first version showed the design view's two pictures as layers of their
own under one window layer for both eyes; on SteamVR what the UI draws over the design view (a
floating ribbon, joints) then flickered, as the runtime did not keep the layers in their order.

The UI is a second copy of the frame's UI. A frame compositor (`UiCopy`) stands in front of
whichever compositor the window has (the view's blur, when on) and draws the frame's primitives
once more into a texture, with `Cutout` switched on: the design view's paint callback then
clears its rectangle to transparent instead of drawing the scene, which erases the background
painted under it and leaves everything painted over it. The window on the desktop is drawn as
ever, by the compositor it had or the ordinary pass. With the model in the room nothing is put
under the UI, and the runtime blends the window over the room where the design view is.

The design view's picture for an eye is the window camera's own (`view::stereo_view_proj`): its
view-projection with the clip-space x of each point moved by the point's depth behind the
camera's target, to the left for the left eye and to the right for the right. Height, depth and
size are untouched, so it is the monitor's picture, drawn at the monitor's size in pixels. How
far a point moves follows from the distance between the eyes and the width the view has in the
room; a perspective camera's far points come to rest where the eyes look straight ahead. A
parallel camera has no distance, so it gets the depth a viewer at the window would see, within
`view::PARALLEL_REACH` (`Depth::parallel`, from the scene's box, which `stereo::Stereo` passes
on with the frame).

Both are part of the window's flat layers, which the runtime places itself. The design cannot
drift against the window when the app draws slower than the headset shows, which an eye's
picture of the room does: the runtime can only turn that one to follow the head.

Each picture draws the scenes the window already uploaded (`Renderer::draw_second_view`) into
an offscreen frame of the scene's own kind (depth, MSAA, order-independent transparency). That
draw writes the camera uniform, the culled placements and the glass order for the eye and
submits at once; the window's next `prepare` writes its own again before it draws, and its cached
still picture is not touched. A last pass (`Gpu::compose`) puts it over the view's background
into the design view's rectangle of the runtime's sRGB picture, the pointer over that where it
points at the model (the window's arrow at the place the model point has in that eye's
picture), and the UI over all of it.

With the model in the room the window's pictures hold the UI only. The model is placed so that the design view
is a pane of glass onto it (`view::place_model`): the desktop camera's target in the pane's
middle, its right and up along the window's, and the height it shows at its target filling the
pane's height. Each eye draws it with its own view and projection into its picture of the room,
and the pointer is the window's arrow laid in the room at the model point under it
(`view::outline_in_room`).

The frame's steps, at the end of the window's UI: `Session::poll` (begin, end or close the
session as the runtime asks), `begin_frame` (waits for the headset's next frame, which paces the
window, and says where the eyes will be), the window's picture from the last frame's UI copy,
the window's picture for each eye, the two eyes' pictures of the room, `end_frame`. A frame that fails ends the session, says so on the status line, and
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
- The view maths (`view`): the window's place, the design view's place on it, each eye's copy of
  the camera's picture and how deep it may be, the model's placement in the room and each eye's
  view of it. Poses and fields of view a runtime reports when it has nothing to report give no
  picture.

What cannot be tested here is composition: whether the runtime shows the pictures. That needs a
headset.

## Hardware seen so far

| Headset | Runtime | Vulkan | Eye | Notes |
|---|---|---|---|---|
| Meta Quest 3S | Oculus 1.208.0 | `XR_KHR_vulkan_enable2` | 1824 × 1872 | from the `xr-info` probe (#7275) |
| Steam Frame (streaming) | SteamVR/OpenXR 2.18.4 | `XR_KHR_vulkan_enable2` | 2644 × 2644 | from the `xr-info` probe (#13469) |

Neither offers `XR_KHR_vulkan_swapchain_format_list`; formats are enumerated and picked.
