# OpenXR VR view

Status: Phase 0 is `solidcraft-cli xr-info` (PR #7275), which reports the OpenXR
runtime, headset and Vulkan support. It passes on both runtimes tested so far, Oculus
and SteamVR. Phase 1 - the stereo view this note describes - is not implemented. Vulkan
only. The questions that still need an answer are at the end.

## Goal

`solidcraft --vr` opens an OpenXR session, renders the open design's viewport to both
eyes, follows the headset's pose, and returns to the desktop viewport when the session
ends, with the design untouched. View only: no controller input, no spatial UI, no
runtime switching mid-session (that is Phase 2 in #2172).

VR adds two render targets and a camera per eye to the existing viewport. It does not
add a second renderer: the scene description, the tessellation plans and the draw calls
stay where they are.

## What the hardware says

Four reports from real hardware, collected with `xr-info` through the discussion on
#2172, cover two runtimes and three headsets:

| | Quest 3S / Oculus | Quest 3 / Oculus | Quest 3 / SteamVR | Steam Frame / SteamVR |
|---|---|---|---|---|
| Runtime version | 1.208.0 | 1.208.0 | 2.18.4 | 2.18.4 |
| Recommended per-eye image | 1824 x 1872 | 2064 x 2272 | 2528 x 2780 | 2644 x 2644 |
| Largest per-eye image | 3648 x 3744 | 4128 x 4544 | 8192 x 8192 | 8192 x 8192 |
| Largest swapchain image | 4096 x 4096 | 4096 x 4096 | 8192 x 8192 | 8192 x 8192 |
| Composition layers | 16 | 16 | 16 | 16 |
| Recommended / max sample count | 1 / 4 | 1 / 4 | 1 / 1 | 1 / 1 |
| `XR_KHR_vulkan_enable2` | present | present | present | present |
| `XR_KHR_vulkan_swapchain_format_list` | absent | absent | absent | absent |
| `XR_KHR_composition_layer_depth` | present | present | present | present |

The Quest 3S report is kept as a test fixture
(`apps/solidcraft-cli/tests/fixtures/quest3s-oculus-1.208.0.json`).

What follows from the four:

- **Instance adoption is possible everywhere.** `XR_KHR_vulkan_enable2` is present on
  both runtimes, so the path below is not a Meta or a Valve special case.
- **The runtime chooses the swapchain image count.** OpenXR does not take one in
  `XrSwapchainCreateInfo`; the app asks afterwards. "Two swapchains" means one per eye,
  not two images.
- **Format selection is ours on every runtime.** `XR_KHR_vulkan_swapchain_format_list`
  is absent on Oculus and on SteamVR alike, so no runtime ranks formats. The core
  `xrEnumerateSwapchainFormats` still lists what is legal; picking is our job.
- **Sizing is per system, not per headset.** The same Quest 3 asks for 2064 x 2272
  through Oculus and 2528 x 2780 through SteamVR, and the largest swapchain image is
  4096 on Oculus against 8192 on SteamVR. Any constant here would be wrong on one of
  them.
- **Sample counts are per runtime as well.** Both recommend 1. Oculus supports up to 4;
  SteamVR reports a maximum of 1, so there is no MSAA through it at all.

No Windows Mixed Reality report yet. Nothing in this note depends on one: the two
runtimes that do report differ in numbers, not in shape, and the probe runs there
unchanged.

## Adopting the runtime's Vulkan objects

wgpu normally creates the Vulkan instance and device itself. OpenXR requires the
opposite: the runtime owns them, because it must compose the images it allocated.
`wgpu-hal` has an explicit path for this, and `wgpu` wraps it:

1. `Instance::graphics_requirements::<Vulkan>(system)` returns the minimum and maximum
   Vulkan API version the runtime accepts.
2. `Instance::create_vulkan_instance(...)` (unsafe) has the runtime create a
   `VkInstance` from our `VkInstanceCreateInfo`. `XR_KHR_vulkan_enable2` exists for
   exactly this.
3. `wgpu_hal::vulkan::Instance::from_raw(entry, vk_instance, ...)` (unsafe) wraps it.
   `entry` is the same `ash::Entry` the instance was created through.
4. `wgpu::Instance::from_hal::<Vulkan>(hal_instance)` (unsafe) gives the safe `wgpu`
   instance everything else uses.
5. `xrGetVulkanGraphicsDevice2KHR` names the `VkPhysicalDevice` to use;
   `Instance::enumerate_adapters` (unsafe) lists the hal adapters, and we keep the one
   whose `raw_physical_device()` matches.
6. `Adapter::open(features, limits, memory_hints)` (unsafe) creates the device and
   queue, and `wgpu::Instance::create_adapter_from_hal` +
   `wgpu::Adapter::create_device_from_hal` (both unsafe) wrap them, so wgpu-core owns
   the device and its lifetime.
7. `Instance::create_session::<Vulkan>(system, &SessionCreateInfo { instance,
   physical_device, device, queue_family_index, queue_index })` (unsafe) opens the
   session with the handles from step 6.

`create_device_from_hal` needs the same feature and limit set the device was created
with, plus the queue family and index. Those are the three things to get right; a
mismatch is a validation error, not a silently wrong picture.

The unsafe surface is a handful of calls, each wrapped in one function with a `SAFETY`
note, matching the pattern the crate already uses for `crates/threedx-client` and the
macOS open-document handler. The alternative - letting OpenXR create the device with
`xrCreateVulkanDeviceKHR` and wrapping it with `Adapter::device_from_raw` - moves the
same unsafety and leaves wgpu-core owning less, so this design keeps wgpu as the device
owner.

Both wgpu interop entry points sit behind wgpu's `wgpu-core` feature. The desktop app
already enables it through `egui-wgpu`, so no new feature flag is needed there.

## Swapchains

Per eye, once per session:

- **Format**: `session.enumerate_swapchain_formats()` returns the legal `VkFormat`s.
  Prefer `B8G8R8A8_SRGB`, then `R8G8B8A8_SRGB`, then the first format offered, and map
  it to the matching `wgpu::TextureFormat`. If the viewport pipeline cannot write the
  chosen format, fail with a clear message instead of rendering wrong colours.
- **Size**: the view's `recommended_width` x `recommended_height`, clamped to the
  view's maximum and to the system maximum. All three come from the runtime and differ
  per system: 1824 x 1872 recommended on Quest 3S, 2528 x 2780 through SteamVR, with
  system maxima of 4096 and 8192. On Oculus the per-view maximum (4128 x 4544) exceeds
  the largest swapchain image, so the system maximum is the binding limit there. The
  recommended size is what the runtime composes at; the maximum is headroom for
  supersampling, not a target.
- **Count**: the runtime decides. `Swapchain::enumerate_images()` returns the images;
  wrap each one once, at session start, and reuse them.
- **Wrapping**: `wgpu_hal::Device::texture_from_raw(image, &desc, Some(no_op_drop),
  TextureMemory::External)` (unsafe), then `wgpu::Device::create_texture_from_hal`
  (unsafe). Both ownership defaults are wrong here: without the drop callback wgpu-hal
  destroys the image on drop, and without `TextureMemory::External` it frees memory the
  runtime allocated. Either would corrupt the session on the first resize.

The swapchain is recreated when the runtime reports a new recommended size (a
resolution change from the headset's settings), when the session restarts, or after a
lost session.

## The frame loop

One iteration, with the calls `openxr` 0.22 exposes:

```rust
// Blocks until the runtime wants a frame.
let state = frame_waiter.wait()?;  // FrameState: predicted_display_time, period, should_render
if !state.should_render {
    continue;
}

frame_stream.begin()?;  // xrBeginFrame
let (flags, views) = session.locate_views(
    ViewConfigurationType::PRIMARY_STEREO,
    state.predicted_display_time,
    &stage_space,
)?;  // pose and field of view per eye

for (eye, view) in views.iter().enumerate() {
    let image = swapchain[eye].acquire_image()?;  // xrAcquireSwapchainImage
    swapchain[eye].wait_image(Duration::from_secs(1))?;
    render_eye(view, image);  // the existing viewport, one camera
    swapchain[eye].release_image()?;
}

frame_stream.end(
    state.predicted_display_time,
    EnvironmentBlendMode::OPAQUE,
    &[&projection_layer],  // xrEndFrame
)?;
```

`views` carries the pose and field of view per eye; `flags` says whether the pose is
valid and oriented, and a frame without `VALID` must not be drawn as if the tracking
data were fresh. The view matrices come from the pose and the fov, not from the desktop
camera, so exiting VR returns to exactly the view the user left.

## Session lifecycle

`session.begin(PRIMARY_STEREO)` returns a session state, not a bool: `READY`,
`RUNNING`, `STOPPING`, `EXITING`, `LOSS_PENDING`, `IDLE`. Only `RUNNING` draws.
`EXITING` means the user pressed the system button or the runtime asked to quit: the
loop ends and the desktop viewport comes back. `LOSS_PENDING` means the device was
lost; the session is recreated, and the design stays as it is.

Rendering happens on the thread that owns the device, which in the desktop app is the
render thread. The session is created and polled on that same thread; the event loop
only forwards "enter VR" and "leave VR".

## Failure modes

| What happened | What the user sees |
|---|---|
| No OpenXR runtime | "No VR software is running" plus what to start; the desktop viewport stays |
| Runtime present, no headset | The same, naming the headset |
| No `XR_KHR_vulkan_enable(2)` | "This VR runtime does not support Vulkan"; no D3D12 fallback in Phase 1 |
| No usable swapchain format | The format list the runtime offered, and a clean end to the session |
| Session lost | A notice, then the desktop viewport; the design is untouched |
| Device lost | The same, through the crash-free path: no panic |

Every one of these is a `Result`, not a panic. A VR session that fails must leave the
desktop app exactly as it was.

## Where the code goes

- `crates/xr` (L5, beside `gpu` and `ui-egui`): the session, the swapchains and the
  frame loop. It depends on `render` for the view math and on `openxr`; it does not
  depend on `ui-egui`.
- `apps/solidcraft`: the `--vr` flag, the thread that runs the loop, and the viewport
  draw that already exists.
- `apps/solidcraft-cli`: stays Phase 0. A `--plan` mode that prints the swapchain
  configuration it would use is the cheap way to make this note executable, and it is
  testable against the fixture.

The crate denies `unsafe_code` and allows it in the named functions above, as
`apps/solidcraft` and `crates/threedx-client` do.

## Testing without a headset

The parts that decide behaviour are pure and testable:

- Format choice: a function over the enumerated formats and a preference list.
- Size choice: recommended against maximum, clamped, from the fixture.
- View math: pose and fov to projection matrices, against known values.
- Session state machine: `READY`/`RUNNING`/`EXITING`/`LOSS_PENDING` transitions,
  including "no frame while `EXITING`".
- The report and verdict, already covered by `xr-info`'s tests and the Quest 3S
  fixture.

What cannot be tested here is composition: whether the runtime shows the images,
whether tracking is smooth, whether the chosen format is accepted. `xr-info` reports
what a runtime exposes; it never composes a frame. That needs a headset running the
renderer, and no report so far has done it.

## Open decisions

1. **Depth submission.** `XR_KHR_composition_layer_depth` is available on both
   runtimes. Submitting a depth swapchain lets the runtime reproject late frames, at the
   cost of a second swapchain per eye and a projection that matches the runtime's
   conventions exactly. Phase 1 can ship without it.
2. **One pass or two.** Rendering both eyes in one pass with `VK_KHR_multiview` halves
   the draw calls but constrains the pipeline and the shaders; two passes reuse the
   existing pipeline unchanged. Phase 1 should start with two passes.
3. **Where `--vr` sits.** A CLI flag on the app (this note's assumption) or a toolbar
   toggle. The issue asks for a flag in Phase 1 and a toggle in Phase 2; the flag is the
   smaller change and keeps the toggle honest.
4. **Loader packaging.** Phase 0's kit ships `openxr_loader.dll` beside the binary by
   hand. A released app needs a decision: vendor it, fetch it in the build, or document
   it as a runtime dependency.
5. **MSAA.** Both runtimes recommend 1x. Oculus supports up to 4x; SteamVR reports a
   maximum of 1, so 4x is not available there at all. Start at 1x: it is what both
   runtimes ask for, and 4x would be an Oculus-only path until there is a measurement.
