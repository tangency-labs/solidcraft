# VR viewer

Status: the first step of #2172, a simple viewer. The design stands in the room in front of you
in a VR headset, and you hold, turn and scale it with the controllers, and point at its parts to
name, select, hide and isolate them, cut it with a plane held in the hand and pull an assembly
apart. The window on the desktop works as ever. Moving parts and joints are later steps (see
Next).

Off by default. Nothing of it runs, and no VR software is touched, unless it is asked for.

## Using it

1. Start the VR software (SteamVR, or Meta Quest Link). The [OpenXR loader](#the-openxr-loader)
   must be beside the program.
2. Start SolidCraft with `--vr`, or with `SOLIDCRAFT_VR=1`, or turn on Preferences › XR
   › **Start in VR** and restart. `SOLIDCRAFT_VR=0` turns a saved choice off for one run.
3. Put the headset on. Once a design with a model is open in the design view, the model appears
   about 0.6 m in front of you and a little below your eyes, about 0.4 m across, upright, with
   the side the window's camera shows toward you.

The defaults are for a controller with a trigger, a grip, a stick and two buttons (most have
that); every one is a setting (Preferences › XR › VR, below), applied at once.

| Input | What it does |
|---|---|
| Point | The ray stops at the part it points at (or at the belt, when that is nearer), with a dot there. The part lights up (in the window too) and a sign names it. |
| Trigger (Select; a pinch with tracked hands) | Selects the part pointed at, or takes it out of the selection; pointing at nothing, clears it. On the belt: opens a group, or does an item. |
| Grip (Grab; a closed hand) | One hand holds the model like a thing in the hand; both hands scale it (and move, or turn, as set). |
| Stick, while one hand holds the model | Right stick: up and down push the model along the holding hand's ray or pull it back, left and right turn it about the vertical. Left stick: left and right roll it about your line of sight, up and down tip it away or toward you. Written beside the sticks (Preferences can give the holding hand's stick the move and roll instead). On the Browser's window the stick scrolls it; otherwise the sticks are free. |
| Right B (or the Menu button) | The ring menu, held. |
| Left X, left Y | Undo, redo. |

**The ring menu.** Hold the ring menu button: the slots stand in a ring around the hand, facing
you, drawn as the desktop's radial menu draws them. Move the hand toward one (it lights and grows,
with a tick) and let go to take it; let go in the middle to take nothing. A group opens a whole
ring of its own around the slot the hand reached, with a gap on the side it came from; the ring
before turns grey, and moving back through the gap returns to it.

What each slot holds is a setting (Preferences › XR › VR › Ring menu): up to eight slots in the
first ring, each an action or one of four groups, and up to eight actions in each group, with the
group's name and icon. The defaults:

| Slot | What it does |
|---|---|
| Show/Hide | Hides what is pointed at, or the selection when nothing is (the whole selection when the part pointed at is in it); when all of that is hidden, shows it again. |
| Isolate | Shows only that, the rest hidden. |
| Show all | Shows everything again. |
| Section / Section off | A section plane at the tip of that controller: what lies on your side of it is cut away. Select with that hand leaves it in the model. Only the headset is cut. |
| View ▸ | Put back (the model in front of you, at its first size), Explode / Put together (its first exploded view, or every part moved away from the middle; nothing saved), Real room. While parts are moved and their positions not captured, Explode says so and changes nothing: capture or undo the moves first. |
| Edit ▸ | Undo, Redo. |

Also available for a slot: Hide (only), and every action above.

**The belt.** Below and in front of you, curved toward you, and left where it is put: Browser,
Designs (each open design, and New), File (Save, Close), Room (the real room around the model, Put
back) and Edit (Undo, Redo). Point at a group (or touch it with the controller) and Select: its
items grow out above it; Select on one does it. Save and Close ask nothing in the headset: a
design that has no file yet, or whose moved parts are not captured, is saved on the desktop
(Save As and the capture question are there), and one with unsaved changes is closed on the
desktop (it asks there whether to save them); in the headset the sign at the controller says so
and nothing changes. Grab the belt (or the bar under it) to move it: it goes and turns with the
hand as a held part does, its long side always level, and the stick pulls it closer or pushes it
away. Put back puts it in front of you again. The model, the controllers and the menu cover it
where they are nearer.

**The Browser.** Select on Browser opens the window's own Browser beside the belt (Select again
closes it): every row the desktop Browser has, with its folds, eyes and selecting. The ray is its
pointer and Select its click; with the ray on it, the stick scrolls it. Pointing at a row lights
its part.

**Joints** are drawn as the window draws them (their badges, where their eyes and the Joints
folder show them), over the model.

**The real room** (passthrough): where the VR software offers it (an alpha-blended view), the
model stands in the real room instead of the drawn one, with no backdrop or floor grid.

**Settings** (Preferences › XR › VR, applied at once, also with the headset on): which inputs
(trigger, grip, A/X, B/Y, stick click, menu) do Select, Grab, the ring menu, Put back and Section,
and on which hands; what the stick does; whether, holding the model with one hand, its stick
pulls it closer or pushes it away and the other stick turns it; what both hands do (scale and
move; scale, move and turn with both hands equal, the default, where hands closer than 1 cm
turn nothing; or the first hand holds and turns while the second scales); the real-size snap; when the scale (1:1, 1:4) is shown; the hold time, the ring
menu's size, its items' size and growth, the wait before a group opens; the signs' size, the
ray's length, the vibration's strength, where the model first appears, and the stick speeds;
the belt's height, distance and curve, where the Browser's window stands and how big it is, and
whether joints are drawn.
"Restore VR defaults" puts them back.

Where the VR software hands out the models of the controllers actually held
(`XR_EXT_render_model`, SteamVR does), they are drawn in place of the boxes, their stick, trigger
and buttons moving as the runtime reports them; elsewhere a box stands for each controller.

The room never moves, only the model does. The floor is drawn where the VR software says it is
(`XR_EXT_local_floor`, else the play area), as a grid fixed under you; the design's own grid and
ground shadow are left out in the headset, because they would move with the model. The model
starts in front of you, and is put there again when another design is opened or switched to.
The grip buzzes briefly when it takes hold. There is no stick movement: moving the model is the
only way around, so nobody gets sick. A hand holding the model does not point: moving the model
neither lights nor selects anything.

A sign at the controller says what an item or the stick did. Hiding, isolating and showing are
the desktop's Show/Hide, one undo step each.

A part is what the Browser selects on a click in an assembly: the outermost placement the body
is in, or the body itself where it belongs to the design's own component. Hidden and ghosted
bodies are passed through; with a section on, what it cuts away is too, and the cut face stops
the ray.

The window on the desktop goes on as ever: whoever sits at it sees the design, opens files and
runs commands with mouse and keyboard, and the headset shows the result. Preferences › XR
says what the session is doing, or why VR did not start; in a run without VR it says whether
OpenXR was found (the loader beside the program and a registered VR runtime, looked up as files
and settings only, so no VR software is started). Leaving VR in the VR software ends the
session; the window goes on. A start that cannot (no VR software running, no headset, no Vulkan,
the loader missing, safe mode or the software renderer) leaves the app exactly as it starts
without VR.

The buttons are the VR software's to rebind (SteamVR: Settings › Controllers › Manage controller
bindings › SolidCraft). SolidCraft suggests bindings for Valve Index, Meta Touch, Windows Mixed
Reality and HTC Vive controllers, tracked hands (`XR_EXT_hand_interaction`: pinch and grasp), and
one button for any other; SteamVR binds the Steam Frame's controllers from the Index ones.

What is different in a run that started VR, for the whole run:

- The window draws on the Vulkan device the VR runtime created, and while the headset shows the
  design it waits for the headset's frames instead of the monitor's.
- Pictures of the model drawn off the window's thread (thumbnails, file previews) are drawn by
  the CPU: the runtime shares the device's queue, which Vulkan lets only one thread at a time use.

Known limits of this first step:

- The headset shows what the window's design view drew. While the window is minimized or
  covered it draws nothing new: the headset keeps the last picture and gets about ten frames a
  second, so keep the window open.
- The controllers are drawn over the model, not into it; a hand behind the model shows through
  it. The ray stops at the part it points at, so it never crosses the model.
- The signs are drawn over everything. Edges and points are not picked, only parts.
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
  grab, `marks.rs` the triangles drawn for each controller and where a sign stands.
- `crates/ui-egui/src/vr_pick.rs`: what a ray points at, from the same pick index and rules as
  the mouse (`view_pick::nearest_body`), the part a body belongs to as the Browser selects it,
  lighting it (`viewport.hover_bodies`, alongside the Browser's own hover) and selecting it.
- `apps/solidcraft/src/vr.rs`: whether a run starts in VR, and the frame; `vr/sign.rs` paints a
  sign's name with an egui context of its own, in the app's fonts and the tooltip's colours.
- Small hooks in the shared crates, idle unless a host switches them on:
  `Renderer::draw_second_view` (viewport-gpu) and `stereo::Stereo` (ui-egui).

### One device for the window and the headset

An OpenXR runtime composes the pictures it allocated, so it has to create the Vulkan instance
and device (`XR_KHR_vulkan_enable2`). That is why VR is chosen at start: `vr::open` connects to
the runtime, has it create the instance and the device with the features and limits the window
would have asked for, wraps both for wgpu (`wgpu-hal`'s `from_raw` constructors) and hands them
to eframe as an existing device. The window, the viewport and the headset then share one device,
and the model is uploaded once.

Those calls have no safe API. They are in `crates/xr/src/session/interop.rs`, one function each
with its `SAFETY` note, and so are the controllers' models (`XR_EXT_render_model`), which the
`openxr` crate does not wrap, in its child `interop/render_models.rs`; the rest of the crate
denies `unsafe`.

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

- See-through for the rest instead of hiding; the explode as a motion instead of a jump.
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
