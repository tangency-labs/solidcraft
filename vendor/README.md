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
- `crates/egui/src/viewport.rs`, `crates/egui-winit/src/lib.rs`, `winit/src/platform/macos.rs`,
  `winit/src/platform_impl/macos/window_delegate.rs`: `ViewportCommand::TitlebarButtonsHidden(bool)`
  hides or shows the traffic lights of an open macOS window (winit's `with_titlebar_buttons_hidden`
  only worked at creation, and changing it by recreating the window loses the native frame). No
  effect on other systems.

- `crates/egui/src/data/input/pointer_button.rs`, `crates/egui-winit/src/lib.rs`,
  `crates/eframe/src/web/input.rs`: mice with more than five buttons. `PointerButton` has
  `Extra3` to `Extra13` (buttons 6 to 16; `NUM_POINTER_BUTTONS` is 16), `PointerButton::ALL`,
  `number` and `from_number`. egui-winit turns winit's `MouseButton::Other(code)` into them by
  each platform's code (`other_button_number`) and the web runner its `button` 5 and up, where
  upstream drops them. Tests: `buttons_are_numbered_in_order` (egui) and
  `further_mouse_buttons_reach_egui_by_number` (egui-winit).
- `crates/egui/src/data/input/viewport_info.rs`, `crates/egui-winit/src/lib.rs`
  (`update_viewport_info`): `ViewportInfo::native_window`, the handle of the native window a
  viewport is shown in (the `HWND` on Windows, `None` elsewhere). Windows asks a window what is
  under the pointer by that handle, and a window's title bar is laid out by its viewport: with
  the handle each window, the main one and every pop-out, publishes its own bar for one window
  procedure to answer from (`crates/ui-kit/src/window_chrome/caption.rs`, #13996).

- `crates/egui/src/widgets/text_edit/{builder.rs,mod.rs}`, `crates/egui/src/widgets/mod.rs`,
  `crates/egui/src/atomics/container_atom.rs`, `crates/egui/src/lib.rs`: a vertically centred
  `TextEdit` lifts its single row of text so the middle of a capital's ink lands in the middle of
  the field, not the middle of the font's row box (SolidCraft #23381). A row box is not where
  capitals sit: the ascent exceeds the cap height and a descent hangs below the baseline, so
  centring the row box puts the text low in the frame — 1.5 pt with egui's own font. The lift
  comes from `cap_ink_rise`; it lays out one fixed probe glyph, so it follows the font, its size
  and the pixels-per-point, and the baseline never moves while one types. `galley_cap_ink_rise`
  applies it to a laid-out galley (zero for multi-row galleys, which centre as a block); both are
  exported for SolidCraft's ui-kit to centre hand-drawn pills the same way. The atom layout
  applies the lift to every [`AtomKind::Text`] atom whose cross-axis alignment is centred, so
  `TextEdit`'s hint, prefix and suffix atoms share its lifted baseline and centred labels in
  other atom containers read centred too. Only `Align::Center` rows are touched; top- and
  bottom-aligned edits are unchanged. Tests: `crates/egui/tests/text_edit_centering.rs`.

The SolidCraft 3D view uses both to skip frames for pointer motion that changes nothing it
draws (`viewport::follow_pointer_without_frame` in `crates/ui-egui`).

SolidCraft changes to egui-wgpu (paths relative to `crates/egui-wgpu`; these lived in a
separate copy at `vendor/egui-wgpu` before egui was vendored as a whole):
- `src/lib.rs`, `src/winit.rs`: a window larger than the device's largest texture gets a surface
  clamped to it (`clamp_surface_extent`, aspect ratio kept) and egui is drawn at the matching
  smaller scale, instead of a `Surface::configure` validation panic (#869). The resize path and
  an unsupported surface no longer unwrap.
- `src/winit.rs`: acquire a drawable before preparing paint callbacks, so a skipped
  surface frame cannot discard initialization commands for cached viewport meshes.
- `src/renderer.rs`, `src/winit.rs`: `FrameCompositor`, an optional owner of a whole frame
  (`Renderer::frame_compositor`, unset unless the blur behind panels is on, #8876), and
  `Renderer::render_with_mesh_offset`, which continues an uploaded frame in another render pass;
  the blur (`crates/ui-kit/src/blur_native.rs`) draws through them.
- `src/renderer.rs`: `Callback::downcast_ref` (and `CallbackTrait: Any`), so tests can count the
  kinds of paint callbacks a frame emitted (`crates/ui-egui/src/gpu.rs`).

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
- `src/platform_impl/windows/drop_handler.rs`, `src/platform_impl/linux/x11/event_processor.rs`:
  while files are dragged over a window, send `WindowEvent::CursorMoved` with where the drag is
  (Windows: `IDropTarget`'s `DragEnter`, `DragOver` and `Drop`, screen to client coordinates;
  X11: each `XdndPosition`, root to window coordinates). winit 0.30 gives `HoveredFile` and
  `DroppedFile` no position, and neither platform sends the window pointer motion while a drag
  runs, so the app could not tell where files were dropped (the design tabs open files dropped
  on them at that place). macOS and Wayland are unchanged.
- `src/platform_impl/windows/definitions.rs`: `IDropTarget`'s `pt` is a `POINTL` passed by value,
  as COM declares it; winit 0.30 declared a pointer, harmless only while nothing read it.
- `src/platform_impl/windows/event_loop.rs` (`WM_NCCALCSIZE`), `window_state.rs`
  (`WindowFlags::adjust_rect`): a resizable undecorated window with the undecorated shadow keeps
  the system frame on the left, right and bottom (its invisible resize borders and drop shadow)
  and has none at the top. This replaces winit's client area moved down by 1 px, which blurred
  text (#3927) and with which Windows 11 offered no snap layouts on a restored window's maximize
  button, although the hit test answered `HTMAXBUTTON` (#13996). A window that is all client
  area gets no snap layouts either: Windows 11 shows them only over a window that keeps a frame
  of its own, so the frame is kept here and the app's window procedure leaves this answer alone.
  `adjust_rect` grows an inner size by the same frame, so requested and saved sizes and DPI
  changes keep the client area's size. The two places must agree, which is why the calculation
  is here and not in the app. A window that cannot be resized (the preview of a view dragged out
  of the window) has no resize borders to keep and is winit's own, as is a maximized one. The
  hit test itself is the app's (`apps/solidcraft/src/native_frame.rs`): nothing of it is in this
  copy.
- `src/platform_impl/windows/event_loop.rs`, `util.rs`, `mod.rs`, `src/platform/windows.rs`
  (`set_touchpad_wheels`): a process-wide switch, off by default, which is winit's own behaviour.
  Windows sends a precision touchpad's two-finger scroll as the wheel messages a mouse sends, so
  winit reported it as lines, and in the SolidCraft presets it zoomed (vertical) or did nothing
  (horizontal) (#15353). On, the event loop thread is registered with
  `RegisterTouchpadCapableThread` (looked up by name; absent or failing means off), touchpad
  `WM_POINTER` contacts go to `DefWindowProc` instead of becoming touches, and a touchpad's wheel
  message is reported as `PixelDelta`: 40 px per `WHEEL_DELTA` (120), times the window scale
  factor, which is egui's default line speed, so a pinch keeps its speed. Mouse wheels stay
  `LineDelta`. The registration is undone when the event loop drops.
- `src/platform_impl/windows/keyboard_layout.rs`, `window_state.rs`, `event_loop.rs`
  (`get_agnostic_mods_async`, `effective_modifiers`): while unfocused or gaining focus, read
  modifier keys with `GetAsyncKeyState` (keeping AltGr filtering) and hold any pre-focus
  modifiers in `WindowState::inherited_modifiers` until the thread's synchronous `GetKeyState`
  observes them or the key is physically released. Clicking into an unfocused window while holding
  Ctrl/Shift/Alt from another application otherwise saw stale `GetKeyState == 0` until the
  modifier was released and pressed again (#20352). Test:
  `effective_modifiers_tracks_unfocused_and_inherited_keys` (Windows).
- `src/window.rs` (`WindowAttributes::with_owned_by`), `src/platform_impl/windows/window.rs`,
  `linux/x11/window.rs`, `linux/wayland/window/mod.rs`, `macos/window_delegate.rs`, `macos/window.rs`:
  a top-level window can name a live owner (#20252). Windows gives it a Win32 owner, X11 a
  transient hint and Wayland an XDG toplevel parent. On macOS the owner keeps a list of the windows
  it owns and puts them right above itself whenever it becomes the key window
  (`raise_owned_windows`), which is what an owner does there: they come forward with it. When
  it is minimized it takes them off the screen, and it brings those back when it is restored
  (`hide_owned_windows`, `show_owned_windows`). When it moves, they move by what its top left
  corner moved, as a child window does (`move_owned_windows`). While it is full screen they are
  full-screen auxiliary windows that move to the active space, so they show in its space above
  it and raising them does not leave that space; one on another screen stays there
  (`owned_windows_join_full_screen`, undone by `owned_windows_leave_full_screen`). They are not AppKit child
  windows. A child window is kept on its parent's screen, so one dragged to another screen
  jumped back when the drag ended, and one opened on another screen did not come forward with
  its parent (#22313). A window with an explicit parent
  (`WindowAttributes::with_parent_window`) is a child window as in winit.

## occt-capi (MIT OR Apache-2.0; Open CASCADE itself LGPL-2.1-only WITH OCCT-exception-1.0)

`vendor/occt-capi` is the Open CASCADE (OCCT) C ABI module behind `solidcraft-kernel`'s opt-in
`backend-occt` feature, an experiment that checks the kernel interface fits a second kernel
(`docs/kernel-support.md`). Its build files and two shim files come from text-to-cad-rs
(`crates/occt-capi-sys`); `vendor/occt-capi/README.md` says which and what changed. It is not a
`[patch]`: `solidcraft-kernel` depends on it by path, only with the feature, and no build we ship
turns that on. OCCT is downloaded and built into the module at build time and is never stored here;
the licence terms are in `vendor/occt-capi/README.md`.
