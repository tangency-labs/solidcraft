# VR viewer

Status: the first step of #2172, a simple viewer. The design stands in the room in front of you
in a VR headset, and you hold, turn and scale it with the controllers. The window on the desktop
works as ever. Picking, sections, assemblies and the rest are later steps (see Next).

Off by default. Nothing of it runs, and no VR software is touched, unless it is asked for.

## Using it

1. Start the VR software (SteamVR, or Meta Quest Link). The [OpenXR loader](#the-openxr-loader)
   must be beside the program.
2. Start SolidCraft with `--vr`, or with `SOLIDCRAFT_VR=1`, or turn on Preferences › VR headset
   › **Start in VR** and restart. `SOLIDCRAFT_VR=0` turns a saved choice off for one run.
3. Put the headset on. Once a design with a model is open in the design view, the model appears
   about 0.6 m in front of you and a little below your eyes, about 0.4 m across, upright, with
   the side the window's camera shows toward you.

| Controller | What it does |
|---|---|
| One grip held | You hold the model like a thing in your hand: it moves and turns with your hand. |
| Both grips held | The model grows or shrinks as your hands move apart or together, and moves with the point between them. It does not turn. Brought near its real size, it snaps to real size with a tick in both hands, and stays there until your hands take it clearly away. |
| Stick click (either hand) | The model goes back in front of you, at its first size. Resetting the view in the VR software does the same. |
| Trigger | Nothing yet; the ray brightens while it is held. |

The room never moves, only the model does. The floor is drawn where the VR software says it is
(`XR_EXT_local_floor`, else the play area), as a grid fixed under you; the design's own grid and
ground shadow are left out in the headset, because they would move with the model. The model
starts in front of you, and is put there again when another design is opened or switched to.
The grip buzzes briefly when it takes hold. There is no stick movement: moving the model is the
only way around, so nobody gets sick.

The window on the desktop goes on as ever: whoever sits at it sees the design, opens files and
runs commands with mouse and keyboard, and the headset shows the result. Preferences › VR headset
says what the session is doing, or why VR did not start; in a run without VR it says whether
OpenXR was found (the loader beside the program and a registered VR runtime, looked up as files
and settings only, so no VR software is started). Leaving VR in the VR software ends the
session; the window goes on. A start that cannot (no VR software running, no headset, no Vulkan,
the loader missing, safe mode or the software renderer) leaves the app exactly as it starts
without VR.

The buttons are the VR software's to rebind (SteamVR: Settings › Controllers › Manage controller
bindings › SolidCraft). SolidCraft suggests bindings for Valve Index, Meta Touch, Windows Mixed
Reality and HTC Vive controllers, and one button for any other; SteamVR binds the Steam Frame's
controllers from the Index ones.

What is different in a run that started VR, for the whole run:

- The window draws on the Vulkan device the VR runtime created, and while the headset shows the
  design it waits for the headset's frames instead of the monitor's.
- Pictures of the model drawn off the window's thread (thumbnails, file previews) are drawn by
  the CPU: the runtime shares the device's queue, which Vulkan lets only one thread at a time use.

Known limits of this first step:

- The headset shows what the window's design view drew. While the window is minimized or
  covered it draws nothing new: the headset keeps the last picture and gets about ten frames a
  second, so keep the window open.
- The controllers are drawn over the model, not into it.
- A VR runtime that stops answering holds the window too (OpenXR's frame wait has no time limit).

### The OpenXR loader

On Windows the OpenXR loader (`openxr_loader.dll`, Khronos, Apache-2.0 OR MIT) is a library an
app brings along, not part of the system. SolidCraft looks for it beside the program first, then
on the search path. Releases do not ship it yet: copy it beside `solidcraft.exe` to try VR
(SteamVR has one in `SteamVR\bin\win64`). Without it Preferences says that the loader is
missing, which is ours to fix and not the VR software's.

## How it works

- `crates/xr`: the OpenXR session (`Runtime`, `Session`), the decisions it makes from what the
  runtime reports (`plan`), the view maths (`view`) and the controllers (`input`, and
  `session/controls.rs`: `Session::attach_controls`, `hands`, `pulse`). It knows nothing of the
  renderer or the UI.
- `crates/vr`: plain maths, tested without a headset. `room.rs` places the model (`Transform`:
  one turn, one scale, one move; where it first appears), `grab.rs` is the one- and two-handed
  grab, `marks.rs` the triangles drawn for each controller.
- `apps/solidcraft/src/vr.rs`: whether a run starts in VR, and the frame.
- Small hooks in the shared crates, idle unless a host switches them on:
  `Renderer::draw_second_view` (viewport-gpu) and `stereo::Stereo` (ui-egui).

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

At the end of the window's UI: `Session::poll` (begin, end or close the session as the runtime
asks), `begin_frame` (waits for the headset's next frame, which paces the window, and says where
the eyes will be), `hands` (the controllers then), the grab, the two eyes, `end_frame`. A frame
that fails ends the session, says so on the status line, and the window draws on.

Each eye draws the scenes the window's design view already uploaded, with its own view and
projection and without the ground grid and shadow (`Renderer::draw_second_view`), into an
offscreen frame of the scene's own kind (depth, MSAA, order-independent transparency). That draw
writes the camera uniform, the culled placements and the glass order for the eye and submits at
once; the window's next `prepare` writes its own again before it draws, and its cached still
picture is not touched. A last pass puts it over the backdrop and the floor into the runtime's
sRGB picture, and the controllers' boxes and rays over that. The pictures go to the runtime as
one projection layer.

### Decisions tested without a headset

- The picture format: sRGB only, BGRA before RGBA (`plan::pick_format`). A runtime reads a plain
  8-bit format as linear light, which bands.
- The eyes' size: the runtime's recommendation within the view's and the device's limits and a
  pixel budget (`plan::eye_size`).
- What each session state asks for (`plan::Lifecycle`): ready begins the frames, stopping ends
  them, exiting and a pending loss close the session for good.
- The view maths (`view`): each eye's view and projection of the placed model. Poses and fields
  of view a runtime reports when it has nothing to report give no picture.
- The controllers (`input`): every suggested binding is well formed and names each control once;
  what a runtime reports is kept to its range.
- The model's first place, the grab and the controllers' marks (`crates/vr`).

What cannot be tested here is composition: whether the runtime shows the pictures. That needs a
headset.

## Next

Viewing first, then assemblies, each step tried in the headset on its own:

- Pick with the ray: the part under it highlighted and named, the trigger selects it (the
  desktop's selection too); the ray stopping at what it hits.
- Hide, isolate, show all; a section plane held in the hand; exploding an assembly.
- Assemblies: grab a part and it moves as its joints allow, with a buzz at limits and contact;
  play and scrub motion studies; make joints by picking on two parts.
- Comfort: a steady frame rate, the display's refresh rate.

Modeling in the headset is not planned here: direct modeling (#15584) is being built for the
desktop, and VR would drive its commands once it lands.

## Hardware seen so far

| Headset | Runtime | Vulkan | Eye | Notes |
|---|---|---|---|---|
| Meta Quest 3S | Oculus 1.208.0 | `XR_KHR_vulkan_enable2` | 1824 × 1872 | from the `xr-info` probe (#7275) |
| Steam Frame (streaming) | SteamVR/OpenXR 2.18.4 | `XR_KHR_vulkan_enable2` | 2644 × 2644 | from the `xr-info` probe (#13469); the viewer tried on it |

Neither offers `XR_KHR_vulkan_swapchain_format_list`; formats are enumerated and picked.
