# Vendored third-party crates

Copies of third-party crates that SolidCraft builds with local fixes, wired in with
`[patch.crates-io]` in the workspace `Cargo.toml`. They keep their own licence; changed lines
are marked `SolidCraft:` in the source.

## egui 0.37.0 (MIT OR Apache-2.0, Emil Ernerfeldt and contributors)

`vendor/egui` is egui's whole repository at tag 0.37.0 (commit `841a328ba2`, https://github.com/emilk/egui),
added with `git subtree add --squash`, so SolidCraft can carry its own changes to egui, eframe and
egui-wgpu. Nothing of it is reorganised: SolidCraft's changes, listed below and marked `SolidCraft:`
in the source, and the images left out (below) are the only difference from upstream, so a
`git subtree pull` of a newer egui meets nothing but them. It keeps its own licences (both texts are
in it), tooling and rules: SolidCraft's checks do not apply to it, and the workspace excludes
`vendor/`.

Every egui crate SolidCraft builds is patched to it in the workspace `Cargo.toml`: `ecolor`,
`eframe`, `egui`, `egui-wgpu`, `egui-winit`, `egui_glow`, `emath`, `epaint` and
`epaint_default_fonts`. egui's own crates build and test there, for example
`cargo test --manifest-path vendor/egui/Cargo.toml -p egui -p eframe --lib`.

### Images upstream keeps in Git LFS

The vendored tree is plain files; SolidCraft uses no Git LFS. Upstream keeps some images in Git
LFS (screenshot-test references, demo and media pictures, example screenshots, and the `images`
example's `ferris.gif`), and a subtree import brings them as LFS pointer files whose contents only
egui's own LFS storage has. They are left out of `vendor/egui`, with the `filter=lfs` lines of its
`.gitattributes`. None is in a crate SolidCraft builds; the images those crates use
(`crates/eframe/data`, `crates/egui/assets`) are plain files upstream too. Without them egui's
screenshot tests have nothing to compare against and its `images` example does not build.

### Updating egui

```sh
GIT_LFS_SKIP_SMUDGE=1 git subtree pull --prefix=vendor/egui https://github.com/emilk/egui <tag> --squash
# Leave out the LFS images again (if the pull stops on a conflict, run these to resolve it):
git grep -lz 'git-lfs.github.com/spec' -- vendor/egui | xargs -0 -r git rm -q --
sed -i -e '/filter=lfs/d' -e '/LFS:$/d' -e '/!filter !diff !merge/d' vendor/egui/.gitattributes
git add vendor/egui/.gitattributes
git commit -m "vendor/egui: leave out the images upstream keeps in Git LFS"
git grep -l 'git-lfs.github.com/spec' -- vendor/egui   # prints nothing
```

`GIT_LFS_SKIP_SMUDGE=1` keeps Git LFS, where it is installed, from fetching the images while the
pull checks them out.

Conflicts can only come from SolidCraft's own changes and the images left out. Land the pull request
that does this with a merge commit, not a squash or a rebase: `git subtree` finds the last import by
its squash commit, and a squash merge rewrites it away. SolidCraft's changes can go back to a fork
of egui the same way, with `git subtree split --prefix=vendor/egui` or `git subtree push
--prefix=vendor/egui <egui fork> <branch>`.

The shared viewport crate `solidcraft-viewport-egui` draws through egui-wgpu and relies on the
fixes below (above all, the drawable acquired before paint callbacks are prepared: without it the
cached offscreen picture is never drawn). Another workspace that uses that crate patches the egui
crates to this tree the same way, with
`[patch.crates-io] egui-wgpu = { path = ".../vendor/egui/crates/egui-wgpu" }` and the same for the others.

SolidCraft changes to egui and eframe:
- `crates/egui/src/context.rs`, `crates/epaint/src/mutex.rs`: a `Context` read taken on a thread
  that already holds a read of the same context goes ahead of a waiting writer
  (`RwLock::read_recursive`). parking_lot holds new readers back behind a waiting writer, so a
  closure given to `Context::data` that called the context again (`ctx.viewport_id()`) froze the
  window for good when a worker's `request_repaint` came between the two reads
  (#21091). The context tracks the locks each thread holds for this.
  `Context::record_nested_locks` reports nested ones so tests can keep them out; it builds only
  with the `lock_nesting_report` feature (`crates/egui/Cargo.toml`), which SolidCraft's crates
  enable from their dev-dependencies. epaint's `RwLock::is_locked_exclusive` lets the egui test
  see the writer queue. Tests:
  `a_nested_read_does_not_wait_on_a_waiting_writer` (egui) and
  `the_window_never_locks_its_context_inside_its_own_lock_while_a_check_runs` (`crates/ui-egui`).
- `crates/egui/src/data/input/raw_input.rs`, `crates/egui/src/input_state/mod.rs`,
  `crates/eframe/src/epi.rs`, `crates/eframe/src/native/{winit,wgpu,glow}_integration.rs`:
  backport of emilk/egui#8768 (open upstream, against egui's `main`). `RawInput::repaint_on_pointer_move`
  (default true) lets pointer motion alone ask for no further frame, and
  `App::repaint_on_window_event` / `App::repaint_on_device_event` let a native app answer
  `Some(false)` to process a winit event without painting. The motion is still delivered with
  the next frame. Tests: `pointer_motion_repaints_unless_asked_not_to` (egui) and
  `the_app_can_override_the_repaint_an_event_asks_for` (eframe). SolidCraft only: the two hooks
  are asked about the root viewport's events alone, since the event does not name its window;
  another window's events keep eframe's answer.
- `crates/egui-winit/src/lib.rs`: while no pointer button is held, queued pointer motion joins
  the motion of the same kind at the end of the queue (`PointerMoved` keeps the last position,
  `MouseMoved` sums), so frames skipped for pointer motion do not pile up one event per platform
  report. Test: `queued_pointer_motion_collapses_between_other_events`.
- `crates/egui/src/context.rs`, `crates/egui/src/lib.rs`: `Context::widget_hits_at(pos)`, the
  hit test the next pass would start with, run between passes against the last pass's widgets
  (`begin_pass` and it share `prev_pass_hits`); `WidgetHits` is exported for it. Test:
  `widget_hits_at_matches_the_next_pass`.
- `crates/eframe/src/native/file_storage.rs`, `crates/eframe/src/lib.rs`: `save_to_disk` writes
  the settings (`app.ron`) to a temporary file beside them, syncs it and renames it over them, so
  the file always holds one whole save. Two windows saving at once each truncated and wrote the
  same file, and a shorter save left the tail of a longer one behind it (#4164). A rename the
  system refuses is tried again for up to 150 ms (Windows refuses while another process has the
  file open). `lib.rs` re-exports `save_to_disk`, hidden, for SolidCraft's tests
  (`settings_saved_at_once_from_two_windows_stay_whole` in `apps/solidcraft/src/startup.rs`).
- `crates/eframe/src/epi.rs`, `crates/eframe/src/native/run.rs`,
  `crates/eframe/src/native/winit_integration.rs`: `NativeOptions::event_loop_ready` hands the app
  the event loop's proxy before the loop starts (dropped when the options are cloned, like the other
  native hooks), and `UserEvent::RunLogicNow { viewport_id }` runs that viewport's logic-only pass at
  once. The control channel (#743) serves a request with it without waiting for the compositor's
  next frame: on Wayland a window that gets no frames (off-screen, on another workspace) otherwise
  stalled each request for up to a second. Normal redraws still paint.

- `crates/egui/src/data/input/pointer_button.rs`, `crates/egui-winit/src/lib.rs`,
  `crates/eframe/src/web/input.rs`: mice with more than five buttons. `PointerButton` has
  `Extra3` to `Extra13` (buttons 6 to 16; `NUM_POINTER_BUTTONS` is 16), `PointerButton::ALL`,
  `number` and `from_number`. egui-winit turns winit's `MouseButton::Other(code)` into them by
  each platform's code (`other_button_number`) and the web runner its `button` 5 and up, where
  upstream drops them. Tests: `buttons_are_numbered_in_order` (egui) and
  `further_mouse_buttons_reach_egui_by_number` (egui-winit).

The SolidCraft 3D view uses both to skip frames for pointer motion that changes nothing it
draws (`viewport::follow_pointer_without_frame` in `crates/ui-egui`).

SolidCraft changes to egui-wgpu (paths relative to `crates/egui-wgpu`; these lived in a
separate copy at `vendor/egui-wgpu` before egui was vendored as a whole):
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
  submits a command buffer holding a debug marker (`DeviceLost::probe`), so a device the
  driver lost is found by opening that command list rather than by the first queue write,
  which panicked inside wgpu-core (`PendingWrites::activate`, #4172). An empty submission
  only signalled a fence, which a reset D3D12 device let through (#6528).
  A reset that lands after the probe is still met by a queue write or the present: the painter
  runs the frame under `DeviceLost::survive`, which takes that panic for the loss once the probe
  then finds the device lost, and lets any other panic carry on (#10240).
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
- `src/lib.rs`, `src/winit.rs`: `stalled_frames` counts the frames in a row whose texture took
  at least `STALLED_ACQUIRE` (950 ms) to acquire. wgpu waits at most a second for the next
  image and D3D12 then goes on as if it had come, so a hung GPU shows only as frames that each
  wait that second; the app restarts on another renderer after a few of them (#3321). Only the
  main window counts. `refused_for` says how long the main window has skipped its frames on a
  refused configure, which never reach the acquire.

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
  missing piece. eframe 0.37 requires winit 0.30, so this cannot come from a newer winit.
- `src/platform_impl/linux/wayland/seat/mod.rs`, `state.rs`, `seat/pointer/mod.rs`: the
  global bound with the connection, the gesture object of each seat's pointer, and the
  module wiring.
- `Cargo.toml`: enable the `unstable` feature of `wayland-protocols` on the Linux dependency
  (the `wp::pointer_gestures` module); drop the example and test targets.
- `src/event.rs`, `src/changelog/unreleased.md`: the platform notes of both events and the
  changelog entry.
- `src/platform_impl/windows/event_loop.rs`, `window_state.rs`, `mod.rs`, `src/platform/windows.rs`
  (`set_quiet_window_move`): a process-wide switch with which Windows' modal move and size loop
  (`WM_ENTERSIZEMOVE` … `WM_EXITSIZEMOVE`, entered when the title bar is dragged) no longer makes
  the app render once per move step. It is off by default, which is winit's own behaviour, so
  anything built on this copy is unchanged until it opts in; SolidCraft turns it on only when
  Preferences > Layout's `windows_smooth_window_drag` is on (off by default). It is read at each
  message, so a change applies at once, and a paint or a move held back before it is turned off is
  still sent when the loop ends. With it on, while the window only moves (its size unchanged):
  `Moved` is coalesced and sent once, with the final position, at `WM_EXITSIZEMOVE`; `WM_PAINT`
  is validated without `RedrawRequested`, and one paint is requested when the loop ends; relative
  raw mouse motion is not sent as device events. Off, as in winit, every `WM_WINDOWPOSCHANGED`
  becomes `Moved`, egui-winit asks for a repaint on `Moved`, and the redraw arrives as `WM_PAINT`
  inside the loop, so each step runs a full frame that blocks on the swapchain (acquire waits of
  50-110 ms were measured on D3D12), and a frame that asked for the next one (eframe repaints on
  raw motion while it believes the press that started the drag is still held) keeps a paint loop
  spinning for the whole drag: the window moves at 10-40 fps. A resize, a snap or a DPI change
  inside the loop (a size change) paints live either way.
- `src/platform_impl/windows/event_loop.rs` (`raw_motion`): relative raw mouse motion is summed
  while more raw input is waiting in the queue (`GetQueueStatus(QS_RAWINPUT)`) and sent once
  per burst, so a high-rate mouse (8 kHz: about 7,000 `WM_INPUT` a second) no longer makes three
  device events (`Motion` x, `Motion` y, `MouseMotion`) per report on the window thread. The
  summed motion is exact; motion is flushed before a button or wheel change in the same report
  stream, so the order of events is kept.
- `src/platform_impl/windows/event_loop.rs`, `dark_mode.rs`: on Windows 11, `WM_DPICHANGED`
  applies the rect Windows suggests (`takes_suggested_rect`), for a window that isn't maximized
  or fullscreen. The nudge loop took the monitor the window was still on for the new one and
  pushed the rect back, so a window moved to a monitor with another scale stayed on the
  boundary and its size drifted (#9336). Same behaviour as rust-windowing/winit#4341 (winit 0.31
  only; the 0.30 backport rust-windowing/winit#4637 was closed), except that the maximized path
  stays as it was. On this path a size the application writes through the `InnerSizeWriter` of
  `ScaleFactorChanged` is not applied; nothing in SolidCraft writes one. The version query in
  `dark_mode.rs` passes the structure size to `RtlGetVersion`, as its documentation requires,
  since this choice depends on the build number it returns. eframe 0.37 requires
  winit 0.30, so this cannot come from a newer winit.
- `src/platform_impl/windows/monitor.rs` (`MonitorHandle::name`, `native_identifier`, `size`): a
  monitor that has gone away (unplugged, a dock disconnected, a display change on resume) makes
  `GetMonitorInfoW` fail with OS error 1461 (`ERROR_INVALID_MONITOR_HANDLE`), and these unwrapped
  that error, so asking for the monitor list or going full screen ended the app (#20645). Now
  `name` is `None`, `native_identifier` is empty and `size` is 0x0, the same kind of fallback
  winit already uses in `position` ((0, 0)), `scale_factor` (96 DPI) and `video_modes` (none).
- `src/platform_impl/windows/window.rs` (`set_fullscreen`): going full screen on such a monitor
  would have sized the window 0x0, so it fills the monitor the window is on instead, and never
  applies a zero-area size (#20645).

## gpu-allocator 0.28.0 (MIT OR Apache-2.0, Traverse Research and contributors)

Original package sources from https://crates.io/crates/gpu-allocator/0.28.0 (repository
https://github.com/Traverse-Research/gpu-allocator), with both original licence texts retained.
The examples, README and lock file are not copied; the manifest's example targets are removed
with them.

SolidCraft changes:
- `src/vulkan/mod.rs`: `MemoryLocation::CpuToGpu` prefers `HOST_VISIBLE | HOST_COHERENT` memory
  instead of `HOST_VISIBLE | HOST_COHERENT | DEVICE_LOCAL`, so wgpu's staging and
  mapped-for-writing buffers are in system RAM rather than VRAM mapped through Resizable BAR.
  On an NVIDIA RTX 2000 Ada laptop GPU (driver 595, Linux) CPU writes into the BAR-mapped type
  ran at about 35 MB/s; with this change the UI upload of 0.37 MB per frame takes 0.06 ms
  instead of 10.6 ms and orbiting a large assembly goes from 25 to 164 frames per second (#3595).
