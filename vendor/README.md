# Vendored third-party crates

Copies of third-party crates that SolidCraft builds with local fixes, wired in with
`[patch.crates-io]` in the workspace `Cargo.toml`. They keep their own licence; changed lines
are marked `SolidCraft:` in the source.

## egui-wgpu 0.36.2 (MIT OR Apache-2.0, Emil Ernerfeldt and contributors)

Original package sources from https://github.com/emilk/egui/tree/0.36.2/crates/egui-wgpu,
with both original licence texts retained. No images or bundled documentation are copied.

SolidCraft changes:
- `src/setup.rs`, `src/lib.rs`: let a named adapter selector defer to the normal wgpu
  request when the requested card is unavailable or incompatible with the window surface.
- `src/lib.rs`: use 1x MSAA on the selected OpenGL adapter before creating pipelines,
  exposing the effective sample count so host paint callbacks can match it.
- `src/winit.rs`: use that same effective count for depth and resolve attachments.
- `src/renderer.rs`: when the GPU refuses a staging buffer for a frame's meshes (device lost
  or out of memory), log a warning and draw nothing that frame instead of panicking (#1469).
- Other backends retain requested MSAA, without another adapter or device probe.
- `src/lib.rs`: clamp MSAA to the largest sample count the device supports for the surface and
  depth formats (`supported_msaa_samples`, `RenderState::msaa_supported`): a saved 8x on a
  device without it failed pipeline validation, a panic at start.
- `src/setup.rs`: the default device requests `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` when
  the adapter has it, so 2x and 8x work where the adapter supports them (WebGPU only
  guarantees 1x and 4x).
- `src/setup.rs`: the default device asks for the adapter's largest 2D texture (at least 8192)
  instead of exactly 8192.
- `src/lib.rs`, `src/winit.rs`: `set_startup_hook` is told before each step of getting the
  device and the window's surface up (instance, surface, adapter request, the chosen adapter,
  device, renderer, surface configure, first present), so the app's crash journal can say which
  step a crash inside the graphics driver happened in.
- `src/lib.rs`, `src/winit.rs`: a window larger than the device's largest texture gets a surface
  clamped to it (`clamp_surface_extent`, aspect ratio kept) and egui is drawn at the matching
  smaller scale, instead of a `Surface::configure` validation panic (#869). The resize path and
  an unsupported surface no longer unwrap.
- `src/winit.rs`: acquire a drawable before preparing paint callbacks, so a skipped
  surface frame cannot discard initialization commands for cached viewport meshes.
- `src/lib.rs`, `src/winit.rs`: surfaces are configured under `capture_errors` (error scopes):
  a configuration the device refuses (D3D12 "Failed to wait for GPU to come idle before
  reconfiguring the Surface", #1935) is logged, the frame is skipped and the configure is tried
  again, instead of the device's uncaptured-error handler panicking. A streak of refusals warns
  once (debug after), waits 16 ms doubling up to 1 s between tries and recreates the surface
  every `SURFACE_RECREATE_AFTER` refusals (`ConfigureRetry`). Each surface keeps the deadline of
  its next try: until it is due, no configure or recreate happens, whether the repaint comes
  from input, an animation, a resize (its size is kept for the retry) or an outdated surface,
  and the frame is skipped with a repaint asked for when it is due. The streak and the deadline
  survive a recreate; a configure the device takes ends the streak, a recreate it asked for
  included. A recreate for a lost surface goes ahead as before.
- `src/lib.rs`, `src/winit.rs`: the device's lost callback marks `RenderState::device_lost`
  (`DeviceLost`), and the painter draws nothing on a lost device. Before a frame's uploads it
  submits nothing (`DeviceLost::probe`), so a device the driver lost is found by that
  submission rather than by the first queue write, which panicked inside wgpu-core
  (`PendingWrites::activate`, #4172).
- `src/lib.rs`: creation of the egui renderer uses validation, internal and out-of-memory
  error scopes, awaiting every result before returning it. A refused UI pipeline (D3D12
  `egui_pipeline` shader compilation, #6648) returns `WgpuError::CreateRendererError` through
  eframe so startup can retry another backend instead of invoking wgpu's panic handler.
