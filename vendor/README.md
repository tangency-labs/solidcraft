# Vendored third-party crates

Copies of third-party crates that SolidCraft builds with local fixes, wired in with
`[patch.crates-io]` in the workspace `Cargo.toml`. They keep their own licence; changed lines
are marked `SolidCraft:` in the source.

## accesskit_macos 0.26.3 (MIT OR Apache-2.0, The AccessKit contributors, https://github.com/AccessKit/accesskit)

The published package (it carries no licence files; `Cargo.toml` names the licence). eframe's
accessibility support (`accesskit_winit` 0.32) uses it on macOS. Drop this copy when eframe
moves to accesskit_macos 0.27.2 or later, which has the fix.

Changes:
- `src/subclass.rs` is accesskit_macos 0.27.2's (AccessKit/accesskit#835). The adapter swaps
  the window's view to a subclass of its own; key-value observing then subclasses it again when
  AppKit's Touch Bar observes the first responder (`nextResponder`). 0.26.3 reset the view's
  class when the adapter was dropped, which threw away KVO's subclass, so AppKit's removal of
  its observer on quit raised `NSRangeException` and the app crashed on Macs with a Touch Bar
  (#7535). Dropping the adapter now leaves the class alone when KVO subclassed it, and the
  subclass's methods fall back to the view's own without the adapter. 0.27.2 also refuses
  (panics) to subclass a view that is already observed; eframe creates the adapter right after
  the window, before anything observes it.
- `tests/integration/` is 0.27.2's tests of the subclassing adapter, with its `[[test]]` and
  `libtest-mimic` dev-dependency in `Cargo.toml` (AppKit needs the main thread, where libtest
  doesn't run tests). On a Mac: `cargo test --manifest-path vendor/accesskit_macos/Cargo.toml`
  (it writes a `Cargo.lock` there; don't commit it).

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
- `src/lib.rs`, `src/winit.rs`: a window other than the main one (an immediate viewport, drawn
  and presented inside the main window's frame) is configured with Mailbox, else Immediate,
  when the configured mode waits for vsync and the surface offers one (`viewport_present_mode`).
  Waiting on both surfaces in one frame halved the frame rate, and on Wayland a window the
  compositor wasn't showing stalled the main window behind its present.
- `src/lib.rs`, `src/winit.rs`: the device's lost callback marks `RenderState::device_lost`
  (`DeviceLost`), and the painter draws nothing on a lost device. Before a frame's uploads it
  submits nothing (`DeviceLost::probe`), so a device the driver lost is found by that
  submission rather than by the first queue write, which panicked inside wgpu-core
  (`PendingWrites::activate`, #4172).
- `src/lib.rs`, `src/winit.rs`: a surface configure that meets a lost device is not taken as
  configured (`configure_captured`): wgpu reports device-loss errors to no error scope, and on
  D3D12 a driver reset found by the configure has already dropped the surface's configuration.
  The next acquire failed with "Surface is not configured for presentation" and the device's
  default handler panicked (#7007). An unsupported surface is a refused configure too. The
  acquire runs under `capture_errors`, and the `Validation` status it then returns reconfigures
  the surface instead of skipping every later frame.
- `src/lib.rs`, `src/winit.rs`: the painter holds an acquired frame as `AcquiredFrame`, which
  destroys a second handle to the frame's texture once the frame is presented or dropped. On a
  lost device wgpu's present and discard keep the frame's image in the surface, and dropping
  the surface (the window closing for the restart) then panicked in wgpu-hal ("Trying to
  destroy a SwapchainAcquireSemaphore that is still in use by a SurfaceTexture", #7148); the
  destroy takes the image back whatever the device's state.
- `src/lib.rs`: creation of the egui renderer uses validation, internal and out-of-memory
  error scopes, awaiting every result before returning it. A refused UI pipeline (D3D12
  `egui_pipeline` shader compilation, #6648) returns `WgpuError::CreateRendererError` through
  eframe so startup can retry another backend instead of invoking wgpu's panic handler.

## winit 0.30.13 (Apache-2.0, © The winit contributors, https://github.com/rust-windowing/winit)

Original package sources from the 0.30.13 release, with the upstream licence retained. Example
and test targets are dropped (their sources are not vendored); the new gesture module's unit
tests stay in `src/platform_impl/linux/wayland/seat/pointer/pointer_gesture.rs`
(`cargo test -p winit --lib -- pointer_gesture`).

SolidCraft changes:
- `src/platform_impl/linux/wayland/seat/pointer/pointer_gesture.rs` (new): bind
  `zwp_pointer_gestures_v1` (versions 1..=3) when the compositor has it, create one pinch
  gesture object per pointer and destroy it before the pointer is released, and turn
  begin/update/end into `WindowEvent::PinchGesture` and `WindowEvent::RotationGesture` for
  the window the gesture began over. Two fingers only; the protocol's scale is relative to
  the gesture's beginning, so the event carries the change since the previous update;
  rotation is negated to winit's counterclockwise degrees; a non-finite scale is dropped, and
  so are updates of a gesture whose begin we never saw; surfaces that are not one of our
  windows (decorations, subsurfaces, other clients) do not take part. Backport of the gesture
  part of rust-windowing/winit#4338 (winit 0.31's Wayland gesture support; the 0.30 backport
  rust-windowing/winit#4778 was closed unmerged). A trackpad pinch otherwise produces no
  input at all on Wayland: egui-winit already maps `PinchGesture` to `egui::Event::Zoom`, and
  `navigation::handle` already zooms the camera from that, so the Wayland backend is the only
  missing piece. eframe 0.36 requires winit 0.30, so this cannot come from a newer winit.
- `src/platform_impl/linux/wayland/seat/mod.rs`, `state.rs`, `seat/pointer/mod.rs`: the
  global bound with the connection, the gesture object of each seat's pointer, and the
  module wiring.
- `Cargo.toml`: enable the `unstable` feature of `wayland-protocols` on the Linux dependency
  (the `wp::pointer_gestures` module); drop the example and test targets.
- `src/event.rs`, `src/changelog/unreleased.md`: the platform notes of both events and the
  changelog entry.
