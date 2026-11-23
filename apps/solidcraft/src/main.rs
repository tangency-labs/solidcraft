//! SolidCraft desktop app.
//!
//! Usage: `solidcraft [--control <port>] [--sample] [--crash-log] [--safe-mode] [design.sc | part.step]`
//!
//! `--safe-mode` (or `SOLIDCRAFT_SAFE_MODE=1`) starts with default settings, no recovery offer and
//! the software renderer; see `startup` and docs/book/src/guide/troubleshooting.md.
//!
//! A `.step`/`.stp` file opens as a new design holding the file's bodies (an Import feature); a
//! `.3mf`/`.stl`/`.obj`/`.glb`/`.gltf` or OpenUSD (`.usd`/`.usda`/`.usdc`/`.usdz`) file as a new
//! design holding mesh bodies.
//!
//! `SOLIDCRAFT_SETTINGS_DIR` selects an explicit absolute settings directory without legacy adoption.
//! `--control <port>` (or `SOLIDCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"engine.execute","params":{"command":"solid.box","params":{…}}}`.
//! Without them the channel still opens, on the default port, unless Preferences turns it off.
//! See `solidcraft_ui_egui::control` and docs/control-protocol.md.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(unsafe_code)]

#[cfg(not(target_os = "macos"))]
#[path = "../../../assets/icons/app/shared/source/app_icon.rs"]
mod app_icon;
#[cfg(test)]
mod clipboard_tests;
mod come_forward;
mod control_server;
#[cfg(target_os = "linux")]
mod desktop_chooser;
#[cfg(target_os = "linux")]
mod desktop_theme;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod desktop_transparency;
mod device_lost;
#[cfg(target_os = "windows")]
mod native_crash;
#[cfg(target_os = "macos")]
mod native_crash_macos;
#[cfg(target_os = "windows")]
mod native_frame;
#[cfg(any(target_os = "macos", test))]
mod native_menu;
mod nav_bridge;
#[cfg(target_os = "macos")]
mod open_documents;
mod open_with;
mod single_instance;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod space_mouse;
mod startup;
#[cfg(unix)]
mod system_quit;
#[cfg(any(target_os = "windows", test))]
mod window_fit;

use solidcraft_engine::{Session, io};
use solidcraft_ui_egui::{Services, SolveApp};

/// Windows: put the restored window back inside the monitor's work area if it is out of reach
/// (a gone monitor, another scale, a taskbar edge). Sizes are in points, the work area in pixels.
#[cfg(target_os = "windows")]
fn fit_window_to_work_area(ctx: &egui::Context, frame: &eframe::Frame) {
    use window_fit::Area;
    let Some((x, y, w, h)) = native_frame::work_area(frame) else { return };
    let (outer, inner, scale, maximized) = ctx.input(|i| {
        let v = i.viewport();
        (v.outer_rect, v.inner_rect, v.native_pixels_per_point, v.maximized.unwrap_or(false) || v.fullscreen.unwrap_or(false))
    });
    let (Some(outer), Some(inner), Some(scale)) = (outer, inner, scale.filter(|s| s.is_finite() && *s > 0.0)) else { return };
    if maximized {
        return;
    }
    let work = Area::new(x as f32 / scale, y as f32 / scale, w as f32 / scale, h as f32 / scale);
    let now = Area::new(outer.min.x, outer.min.y, outer.width(), outer.height());
    let fitted = window_fit::fit(now, work, (960.0, 600.0));
    if fitted != now {
        // The frame (title bar, borders) keeps its size; only the inner size follows.
        let inner_size = egui::vec2((inner.width() - (now.w - fitted.w)).max(1.0), (inner.height() - (now.h - fitted.h)).max(1.0));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner_size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(fitted.x, fitted.y)));
    }
}

/// `SOLIDCRAFT_PROFILE_STARTUP=1` prints the time since launch at each startup step.
fn mark(step: &str) {
    static START: std::sync::OnceLock<(std::time::Instant, bool)> = std::sync::OnceLock::new();
    let (start, on) = START.get_or_init(|| (std::time::Instant::now(), solidcraft_engine::geom::env::var_os("PROFILE_STARTUP").is_some()));
    if *on {
        solidcraft_engine::note!("startup {:>6.0} ms  {step}", start.elapsed().as_secs_f64() * 1000.0);
    }
}

struct App {
    solve: SolveApp,
    /// The setup still to run, a step a frame, behind the starting page (#1210).
    setup: solidcraft_ui_egui::starting::Starting<App>,
    /// The viewport's shader compile, running beside the window while the start page shows (it no
    /// longer waits for it); the app waits for it when it closes (#1210).
    gpu_job: Option<solidcraft_ui_egui::starting::Worker<solidcraft_ui_egui::WgpuBuilt>>,
    /// The settings folder whose start marker the first frame clears (`startup::started`).
    starting: Option<std::path::PathBuf>,
    /// A newly selected API is confirmed only once the first frame has been presented.
    backend_attempt: Option<(std::path::PathBuf, startup::BackendAttempt)>,
    /// The crash guard's attempt file, cleared once the first frame has been presented
    /// (`startup::BackendGuard`).
    backend_guard: Option<startup::BackendGuard>,
    /// A tracked full-graphics launch: `graphics.json`, where it reports that it proved itself, closed
    /// normally or lost its device (`startup::SafeGraphics`). `None` in safe graphics mode.
    safe_graphics_record: Option<std::path::PathBuf>,
    /// When this full-graphics launch reached "app up"; cleared once it proved itself.
    full_graphics_up: Option<std::time::Instant>,
    /// Frames the window has begun, the starting page's included: the first is fitted to the work
    /// area before the window shows, the second asks for the foreground.
    window_frames: u32,
    /// Frames begun once the setup was done; the second one means the first was drawn (the start
    /// page is up).
    frames: u32,
    /// Startup is over: the start page was drawn and the 3D view's pipelines are compiled (or
    /// failed). Until then a crash still counts as a failed start.
    up: bool,
    /// Started over the control channel: the window doesn't ask for the foreground.
    take_focus_unwanted: bool,
    /// Coming forward after a close that asks about unsaved designs.
    come_forward: come_forward::ComeForward,
    /// The graphics device: lost, it can't draw again (`App::watch_device`, #4172).
    gpu: Option<LostWatch>,
    /// Started by the setup's last step.
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    space_mouse: Option<space_mouse::SpaceMouse>,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    /// A device's own app moving the view (Preferences › Navigation, off by default).
    nav_bridge: nav_bridge::NavBridge,
    /// Where the next file dialog opens: it follows the open design (#7756).
    dialog_folder: DialogFolder,
    /// Settings go to `app.ron` in the frame they change (#8275).
    prefs_writer: PrefsWriter,
    /// The window's egui context: pointer motion between frames is hit-tested against its last
    /// pass (`repaint_on_window_event`).
    ctx: egui::Context,
    /// Preferences › Layout's `windows_smooth_window_drag`, as winit was given it.
    #[cfg(target_os = "windows")]
    window_move: native_frame::WindowMove,
}

/// Settings written to the settings file soon after they change (#8275). eframe saves them
/// only every 30 s and when the window closes normally, so a change was lost whenever the app
/// did not get that far: ended from the Task Manager, stopped by a sign-out or shutdown, or
/// crashed or hung in the graphics driver on the way out.
///
/// Some settings change continuously (a panel's width while its edge is dragged), so the file
/// is written at most once per [`PrefsWriter::INTERVAL`], and a change made in between is written
/// when that interval ends.
#[derive(Default)]
struct PrefsWriter {
    /// The settings written (or read at start) last.
    written: Option<String>,
    /// The previous frame had input that can change settings: what it changed shows in this
    /// frame's settings.
    input_before: bool,
    /// When the settings were last written by this writer.
    last_write: Option<std::time::Instant>,
    /// A change waits until then to be written.
    pending_until: Option<std::time::Instant>,
}

impl PrefsWriter {
    /// The least time between two writes of the settings file.
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

    /// The settings as they were read at start: they are on disk already.
    fn read(prefs: String) -> Self {
        Self { written: Some(prefs), ..Self::default() }
    }

    /// Whether this frame's settings are worth comparing: input that can change them came in this
    /// frame or the one before, or a waiting change's time has come. Pointer motion alone isn't
    /// such input; eframe's own save still catches anything else.
    fn due(&mut self, input: bool, now: std::time::Instant) -> bool {
        let due = input || self.input_before || self.pending_until.is_some_and(|at| now >= at);
        self.input_before = input;
        due
    }

    /// Write `prefs` and flush them to disk if they differ from the settings written last and the
    /// last write is at least [`Self::INTERVAL`] old. Returns how long a change waits otherwise:
    /// the caller asks for a frame then, so an idle app still writes it.
    fn store(&mut self, storage: &mut dyn eframe::Storage, prefs: String, now: std::time::Instant) -> Option<std::time::Duration> {
        if self.written.as_deref() == Some(prefs.as_str()) {
            self.pending_until = None;
            return None;
        }
        if let Some(wait) = self.last_write.map(|at| Self::INTERVAL.saturating_sub(now.saturating_duration_since(at))).filter(|w| !w.is_zero()) {
            self.pending_until = Some(now + wait);
            return Some(wait);
        }
        storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, prefs.clone());
        storage.flush();
        self.written = Some(prefs);
        self.last_write = Some(now);
        self.pending_until = None;
        None
    }
}

/// A window position in physical pixels, in egui's points: what egui-winit makes of a
/// `CursorMoved`. None for a scale or position that is not a usable number.
fn pointer_in_points(x: f64, y: f64, pixels_per_point: f32) -> Option<egui::Pos2> {
    let pos = egui::pos2(x as f32, y as f32) / pixels_per_point;
    (pixels_per_point > 0.0 && pos.is_finite()).then_some(pos)
}

/// Input that can change a setting: a click, a key, typed or pasted text, a wheel or a pinch.
/// Pointer motion, which comes every frame while the mouse moves, cannot by itself.
fn changes_settings(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::PointerButton { .. }
            | egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Cut
            | egui::Event::Ime(_)
            | egui::Event::MouseWheel { .. }
            | egui::Event::Zoom(_)
            | egui::Event::Rotate(_)
            | egui::Event::AccessKitActionRequest(_)
    )
}

impl eframe::App for App {
    /// A frame's logic. The first frames show the starting page while the setup runs behind it, a
    /// step a frame (#1210); then the app's own.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.window_frames < 2 {
            self.window_frames += 1;
            // Before the window is shown: its title bar must be within reach (window_fit.rs).
            #[cfg(target_os = "windows")]
            if self.window_frames == 1 {
                fit_window_to_work_area(ctx, _frame);
            }
            mark(if self.window_frames == 1 { "first frame begins" } else { "second frame begins (first presented)" });
            ctx.request_repaint();
            // The window is shown only once its first frame is presented, and Windows then keeps
            // a window of a process started from a shortcut or an installer behind the others. Ask
            // for the foreground once, now that it is on screen; never again, so later frames don't
            // take the focus from whatever the user moved to. Automation over the control channel
            // keeps the focus where it is.
            if self.window_frames == 2 && !self.take_focus_unwanted {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        // The system asked the app to stop (shutdown, SIGTERM): close as a forced quit does,
        // keeping unsaved changes for recovery, since nobody is there to answer "save changes?".
        // Before the starting page returns, so a signal during the setup closes the app too.
        #[cfg(unix)]
        if system_quit::requested() {
            self.solve.quit_requested = true;
        }
        // The window shows with its first frame: the starting page, before the slow setup.
        let mut setup = std::mem::take(&mut self.setup);
        let starting = setup.logic(self, ctx);
        self.setup = setup;
        if starting {
            self.watch_device();
            self.solve.apply_preferences(ctx);
            // Closed from the starting page: there is nothing to save yet, so the window closes,
            // and a close the user asked for is no failed start. One the app made itself (a lost
            // device) still is.
            if ctx.input(|i| i.viewport().close_requested())
                && let Some(dir) = self.starting.take()
            {
                startup::closed_while_starting(&dir, self.gpu.as_ref().is_none_or(|g| g.lost_at.is_none()));
            }
            if self.solve.ready_to_quit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        if self.frames < 2 {
            self.frames += 1;
            // A second frame right away, which means the first was presented: the start page is up.
            ctx.request_repaint();
            if self.frames == 2 {
                mark("start page up (first app frame presented)");
                // The device is up and presented a frame: its API did not crash the process.
                if let Some(guard) = self.backend_guard.take() {
                    guard.clear();
                }
                // A DMG install has no installer step: put solidcraft-cli on PATH, quietly.
                #[cfg(target_os = "macos")]
                std::thread::spawn(solidcraft_ui_egui::cli_link::install_on_launch);
                // Automation runs (a control port was asked for) leave the user's AI apps alone.
                if !self.take_focus_unwanted {
                    solidcraft_ui_egui::ai_connect::begin(&mut self.solve);
                    solidcraft_ui_egui::agent_chat::begin(&mut self.solve);
                }
                if !self.setup.settled() {
                    solidcraft_engine::crash::phase("viewport pipelines (shader compile)");
                }
            }
        }
        // Startup is over once the 3D view's shader compile, running beside the start page, is
        // done too: a driver that crashes in it still counts as a failed start (safe mode, the
        // backend retry), and a new graphics API is confirmed only once it drew the view.
        if !self.up && self.frames >= 2 && self.setup.settled() {
            self.up = true;
            mark("app up (3D view ready)");
            solidcraft_engine::crash::phase(solidcraft_engine::crash::RUNNING);
            if let Some((path, attempt)) = self.backend_attempt.take()
                && let Err(reason) = startup::confirm_backend_start(&path, &attempt)
            {
                log::warn!("graphics: could not confirm the startup choice: {reason}");
                self.solve.set_status(reason, true);
            }
            if let Some(dir) = self.starting.take() {
                startup::started(&dir);
            }
            // The minute that proves full graphics runs from here (`prove_full_graphics`).
            if self.safe_graphics_record.is_some() {
                self.full_graphics_up = Some(std::time::Instant::now());
            }
        }
        self.watch_device();
        self.prove_full_graphics(ctx);
        // The design on screen is the one whose commands a crash report replays.
        self.solve.session.crash_journal = solidcraft_engine::crash::enabled();
        // The OS closing the window (Alt+F4, Cmd+Q, the window manager): ask about unsaved
        // designs first; the window closes once they are answered (`quit_requested`).
        if ctx.input(|i| i.viewport().close_requested()) && !self.solve.ready_to_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.solve.quit_requested {
                solidcraft_ui_egui::documents::request_quit(&mut self.solve);
            }
            // Closed from the taskbar or a window switcher while another app had the focus: the
            // "save changes?" prompt is only seen once the window comes forward.
            if !self.solve.quit_requested {
                self.come_forward.start(std::time::Instant::now());
            }
        }
        self.come_forward.tick(ctx, _frame);
        #[cfg(target_os = "macos")]
        for f in if self.solve.file_picker_busy() { Vec::new() } else { open_documents::take() } {
            self.solve.open_chosen_file(&f);
        }
        // Files other launches handed over (single_instance.rs).
        if !self.solve.file_picker_busy() {
            for f in single_instance::take() {
                self.solve.open_chosen_file(&f);
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            menu.tick(&mut self.solve, ctx);
        }
        self.solve.logic(ctx);
        // Whether the window draws while it is dragged by its title bar: winit has it at once.
        #[cfg(target_os = "windows")]
        self.window_move.follow(self.solve.preferences.windows_smooth_window_drag);
        // The file chooser that opened is not the desktop's own: the user has to be told (#10159).
        #[cfg(target_os = "linux")]
        if let Some(reason) = desktop_chooser::take_reason() {
            let text = self.solve.tr(reason).to_string();
            self.solve.set_status(text, false);
        }
        self.dialog_folder.design(self.solve.session.path.as_deref());
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        if !self.solve.file_picker_busy()
            && let Some(space_mouse) = &mut self.space_mouse
        {
            space_mouse.tick(&mut self.solve, ctx);
        }
        // Every frame, so switching the bridge off reaches the reader while a picker is open too.
        self.nav_bridge.tick(&mut self.solve, ctx);
        if self.solve.ready_to_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.solve.quit_requested {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    /// Pointer motion over a still 3D view that lights up nothing new paints no frame: the view
    /// would draw the same picture again (`viewport::follow_pointer_without_frame`, which keeps
    /// the hover at the pointer). The motion still reaches the next frame. Any other event keeps
    /// eframe's answer, as does every event of another window (eframe asks about the main
    /// window's alone).
    fn repaint_on_window_event(&mut self, event: &winit::event::WindowEvent) -> Option<bool> {
        let winit::event::WindowEvent::CursorMoved { position, .. } = event else {
            return None;
        };
        let pos = pointer_in_points(position.x, position.y, self.ctx.pixels_per_point())?;
        solidcraft_ui_egui::viewport::follow_pointer_without_frame(&mut self.solve, &self.ctx, pos).then_some(false)
    }
    /// Raw mouse motion (not tied to a window) paints no frame while the 3D view is quiet: the
    /// window's own pointer motion decides then, above.
    fn repaint_on_device_event(&mut self, event: &winit::event::DeviceEvent) -> Option<bool> {
        let quiet = matches!(event, winit::event::DeviceEvent::MouseMotion { .. }) && self.solve.viewport.quiet_motion.is_some();
        quiet.then_some(false)
    }
    /// The frame's input before egui reads it: the release of a press handed to a window move
    /// or resize, the macOS menu and full-screen fixes, the 3D mouse, then the app's own hook.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // First: the release of a press the last frame handed to a window move or resize (#13334).
        solidcraft_ui_egui::window_chrome::release_press_taken_by_os(ctx, raw);
        // A native dialog or lost focus may have kept a release (#16460).
        solidcraft_ui_egui::window_chrome::release_stale_input(ctx, raw);
        // Wayland shows the last program's cursor until the window sets its own again (#16581).
        solidcraft_ui_egui::window_chrome::reapply_cursor_on_return(ctx, raw);
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            menu.raw_input_hook(&mut self.solve, ctx, raw);
        }
        #[cfg(target_os = "macos")]
        macos_fullscreen_title_bar();
        // Before the app's own hook, which adds the control channel's events.
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        if let Some(space_mouse) = &mut self.space_mouse {
            space_mouse.raw_input_hook(self.solve.preferences.space_mouse_enabled, raw);
            // The 3D mouse's buttons, before the app reads the frame: Esc and held modifiers join its input.
            solidcraft_ui_egui::space_mouse::buttons_hook(&mut self.solve, ctx, space_mouse.buttons(), raw);
        }
        self.solve.raw_input_hook(raw);
    }
    /// A frame's UI: the starting page while the setup runs (in the saved language and fonts),
    /// then the app's, whose changed settings are written soon after (#8275).
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.setup.done() {
            let _language = self.solve.prepare_text(ui.ctx());
            self.setup.ui(ui);
            return;
        }
        if let Some(window) = frame.winit_window() {
            let monitors = || {
                let primary = window.primary_monitor();
                window
                    .available_monitors()
                    .map(|monitor| {
                        let position = monitor.position();
                        let size = monitor.size();
                        solidcraft_ui_egui::panel_runtime::NativeMonitor {
                            name: monitor.name(),
                            bounds: egui::Rect::from_min_size(
                                egui::pos2(position.x as f32, position.y as f32),
                                egui::vec2(size.width as f32, size.height as f32),
                            ),
                            scale: monitor.scale_factor() as f32,
                            primary: primary.as_ref() == Some(&monitor),
                        }
                    })
                    .collect()
            };
            self.solve.refresh_panel_monitors(monitors);
            solidcraft_ui_egui::view_windows::refresh_displays(&mut self.solve, monitors);
        }
        solidcraft_ui_egui::view_windows::prepare(&mut self.solve, ui.ctx());
        content_ui(&mut self.solve, ui);
        solidcraft_ui_egui::view_windows::show(&mut self.solve, ui.ctx(), frame.wgpu_render_state());
        self.solve.retain_view_pictures(frame.wgpu_render_state());
        // After the frame's UI, so a setting changed in it (Preferences' OK) is written now.
        let now = std::time::Instant::now();
        if self.prefs_writer.due(ui.input(|i| i.events.iter().any(changes_settings)), now)
            && let Some(storage) = frame.storage_mut()
            && let Ok(prefs) = self.prefs_for_storage(storage)
            && let Some(wait) = self.prefs_writer.store(storage, prefs, now)
        {
            ui.ctx().request_repaint_after(wait);
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let prefs = match self.prefs_for_storage(storage) {
            Ok(prefs) => prefs,
            Err(reason) => {
                log::warn!("Settings were not written: {reason}");
                return;
            }
        };
        storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, prefs.clone());
        self.prefs_writer.written = Some(prefs);
        #[cfg(feature = "usage-stats")]
        storage.set_string(solidcraft_ui_egui::usage_stats::STORAGE_KEY, self.solve.usage_stats_for_storage());
    }
    /// The app is closing: work still running beside the window ends first, then the session.
    fn on_exit(&mut self) {
        // Closed from the start page before the 3D view was ready: as a close from the starting
        // page, a close the user asked for is no failed start (#1210).
        if let Some(dir) = self.starting.take() {
            startup::closed_while_starting(&dir, self.gpu.as_ref().is_none_or(|g| g.lost_at.is_none()));
        }
        // A full-graphics launch closing normally did not fail (a lost device took the record).
        if self.gpu.as_ref().is_none_or(|g| g.lost_at.is_none())
            && let Some(path) = self.safe_graphics_record.take()
            && let Err(reason) = startup::update_safe_graphics(&path, startup::SafeGraphics::closed_normally)
        {
            log::warn!("graphics: safe graphics record not saved: {reason}");
        }
        // A shader compile from the start still running ends before the GPU is torn down: a
        // driver call cut off as the process exits can crash it (#1210).
        if let Some(job) = self.gpu_job.take()
            && !job.wait(std::time::Duration::from_secs(10))
        {
            log::warn!("the viewport's shader compile was still running as the app closed");
        }
        // The agent chat's agent and everything it started end with the app.
        solidcraft_ui_egui::agent_chat::shutdown(&mut self.solve);
        // A save still being written ends before the app does.
        solidcraft_ui_egui::save_job::tick(&mut self.solve, true);
        // Pictures still being drawn off the window thread (Home cards, file previews) end
        // before the GPU is torn down: one drawn while the process exits can crash the driver.
        if !solidcraft_engine::capture::close(std::time::Duration::from_secs(5)) {
            log::warn!("a picture was still being drawn as the app closed");
        }
        solidcraft_ui_egui::recovery_ui::close(&mut self.solve);
        // A GPU path tracer setup still running did not stop the app (#9236).
        solidcraft_ui_egui::render_workspace::close(&mut self.solve);
        solidcraft_ui_egui::open_elsewhere::close(&mut self.solve);
        // Before a restart (`Drop for App`) starts the next instance, which would otherwise get
        // nothing from the 3D mouse driver (`solidcraft_3dx_client::stop`).
        #[cfg(target_os = "macos")]
        solidcraft_3dx_client::stop();
        solidcraft_engine::crash::end_session();
        // The next start (after a lost device, or into an update) begins once the app is dropped
        // (`Drop for App`), when the settings are on disk.
    }
}

impl Drop for App {
    /// eframe drops its settings storage first, which waits for `app.ron` to be written, then the
    /// app, then the window and its GPU surface. So the next start begins here: started from
    /// `on_exit`, it read `app.ron` while the old process was still writing it, found it
    /// unreadable and moved it aside, resetting every setting (#9044).
    fn drop(&mut self) {
        // A successor started below with design files would otherwise hand them back to this
        // closing window and end (#19340).
        single_instance::stop();
        if let Some(gpu) = &self.gpu
            && let Some(lost_at) = gpu.lost_at
        {
            device_lost::restart(gpu.restart_on_software, lost_at, &gpu.reason);
            // The window's GPU surface is still to be dropped, and on a lost device that may
            // never return (#9044).
            device_lost::end();
        }
        // Restart only after eframe has flushed settings. Graphics restarts also reopen
        // the saved designs collected by the normal quit flow.
        let graphics_restart = solidcraft_ui_egui::update::take_graphics_restart(&mut self.solve);
        if let Some((program, args)) = solidcraft_ui_egui::update::take_launch(&mut self.solve) {
            let mut command = if graphics_restart {
                graphics_restart_command(&program, &args, &self.solve.preferences.open_designs)
            } else {
                let mut command = std::process::Command::new(&program);
                command.args(&args);
                command
            };
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // Not tied to this process: its own process group, and no console (the PowerShell
                // that waits for an installer is hidden; the app itself is detached).
                let hidden_shell = program.file_stem().is_some_and(|n| n.eq_ignore_ascii_case("powershell"));
                command.creation_flags(0x0000_0200 | if hidden_shell { 0x0800_0000 } else { 0x0000_0008 });
            }
            if let Err(e) = command.spawn() {
                log::error!("update: starting {}: {e}", program.display());
            }
        }
    }
}

/// A UI restart applies the saved API instead of inheriting this session's recovery
/// overrides. Explicit developer overrides remain authoritative.
fn graphics_restart_command(program: &std::path::Path, args: &[String], designs: &[String]) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    command.args(args).args(designs);
    for variable in [
        startup::RETRY_BACKEND_ENV,
        startup::FALLBACK_NOTE_ENV,
        startup::RENDERER_ENV,
        startup::SAFE_MODE_ENV,
        device_lost::LOST_ENV,
        device_lost::REASON_ENV,
    ] {
        command.env_remove(variable);
    }
    command
}

/// Re-enter an AppImage through its outer executable only when this process actually lives
/// in that image's mount. An inherited APPIMAGE from another application is not our launcher.
fn graphics_restart_program(
    exe: Option<&std::path::Path>,
    appimage: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    let exe = exe?;
    if let (Some(image), Some(directory)) = (appimage, appdir) {
        let image = std::path::Path::new(image);
        let directory = std::path::Path::new(directory);
        if image.is_absolute() && directory.is_absolute() && exe.starts_with(directory) && is_file(image) {
            return Some(image.to_path_buf());
        }
    }
    Some(exe.to_path_buf())
}

/// Keep launch-scoped settings through a UI restart. Safe mode and the sample are one-shot;
/// original file arguments are replaced by the current saved tabs at shutdown.
fn graphics_restart_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut retained = Vec::new();
    let mut args = args.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--control" | "--theme-file" | "--max-file-size" => {
                if let Some(value) = args.next() {
                    retained.extend([argument, value]);
                }
            }
            value if value.starts_with("--max-file-size=") => retained.push(argument),
            _ => {}
        }
    }
    retained
}

/// The window's graphics device, watched for its loss.
struct LostWatch {
    lost: eframe::egui_wgpu::DeviceLost,
    /// Where the next start runs once this device is lost ([`device_lost::restart_on_software`]).
    restart_on_software: bool,
    /// When the loss was seen: the app is closing to start again.
    lost_at: Option<u64>,
    /// Why, as wgpu said it: passed to the next start ([`device_lost::REASON_ENV`]).
    reason: String,
}

impl App {
    fn prefs_for_storage(&self, storage: &dyn eframe::Storage) -> Result<String, String> {
        self.solve.try_prefs().or_else(|reason| {
            log::warn!("Panel layout changes were not saved: {reason}");
            let previous = solidcraft_ui_egui::prefs::stored(|key| storage.get_string(key));
            self.solve.prefs_with_stored_panel_layout(previous.as_deref())
        })
    }

    /// The graphics device was lost (#4172): the window can't draw again, so close, keeping
    /// unsaved changes for recovery as a forced quit does (the window can't ask about them), and
    /// start again once closed ([`device_lost::restart`]).
    fn watch_device(&mut self) {
        let Some(gpu) = self.gpu.as_mut().filter(|g| g.lost_at.is_none() && g.lost.is_lost()) else { return };
        let reason = gpu.lost.reason().unwrap_or_default();
        log::error!("graphics: the device was lost ({reason}); closing to start again");
        solidcraft_engine::note!("SolidCraft graphics: the device was lost ({reason}); starting again");
        solidcraft_engine::crash::phase(&format!("graphics device lost: {reason}"));
        gpu.lost_at = Some(device_lost::now_secs());
        gpu.reason = reason;
        self.solve.quit_requested = true;
        // A full-graphics launch that lost its device failed, even after it proved itself.
        if let Some(path) = self.safe_graphics_record.take()
            && let Err(reason) = startup::update_safe_graphics(&path, startup::SafeGraphics::lost_device)
        {
            log::warn!("graphics: safe graphics record not saved: {reason}");
        }
    }

    /// A full-graphics launch that has run [`startup::FULL_GRAPHICS_PROOF`] since "app up"
    /// without losing its device clears the failures before it (`startup::SafeGraphics::proved`).
    fn prove_full_graphics(&mut self, ctx: &egui::Context) {
        let Some(up) = self.full_graphics_up else { return };
        let Some(left) = startup::FULL_GRAPHICS_PROOF.checked_sub(up.elapsed()).filter(|left| !left.is_zero()) else {
            self.full_graphics_up = None;
            if let Some(path) = &self.safe_graphics_record
                && let Err(reason) = startup::update_safe_graphics(path, startup::SafeGraphics::proved)
            {
                log::warn!("graphics: safe graphics record not saved: {reason}");
            }
            return;
        };
        ctx.request_repaint_after(left);
    }
}

/// Where the next file dialog opens (#7756): the folder of the file last chosen in one, or of the
/// design last opened or saved, whichever came last. Linux dialogs otherwise open in the home
/// folder every time; macOS and Windows remember a folder of their own, which this agrees with.
#[derive(Clone, Default)]
struct DialogFolder(std::sync::Arc<std::sync::Mutex<DialogFolderState>>);

#[derive(Default)]
struct DialogFolderState {
    folder: Option<std::path::PathBuf>,
    /// The open design's path when last looked at, to notice another design opened or saved.
    design: Option<String>,
}

impl DialogFolder {
    /// A file was chosen in a dialog: the next one opens in its folder.
    fn chosen(&self, file: &std::path::Path) {
        if let Some(folder) = file.parent().filter(|f| !f.as_os_str().is_empty()) {
            self.remember(folder);
        }
    }

    /// A folder was chosen: the next dialog opens in that folder, not in its parent.
    fn chosen_folder(&self, folder: &std::path::Path) {
        if !folder.as_os_str().is_empty() {
            self.remember(folder);
        }
    }

    fn remember(&self, folder: &std::path::Path) {
        if let Ok(mut state) = self.0.lock() {
            state.folder = Some(folder.to_path_buf());
        }
    }

    /// The open design's path, every frame: a design opened or saved somewhere else moves the
    /// next dialog to its folder; the same design again leaves a folder chosen since alone.
    fn design(&self, path: Option<&str>) {
        let Ok(mut state) = self.0.lock() else { return };
        if state.design.as_deref() == path {
            return;
        }
        state.design = path.map(str::to_string);
        if let Some(folder) = path.and_then(|p| std::path::Path::new(p).parent()).filter(|f| !f.as_os_str().is_empty()) {
            state.folder = Some(folder.to_path_buf());
        }
    }

    /// A dialog's answer as a path, its folder remembered for the next dialog.
    fn answer(&self, picked: Option<rfd::FileHandle>) -> Option<String> {
        let picked = picked?;
        self.chosen(picked.path());
        Some(picked.path().to_string_lossy().into_owned())
    }

    /// A folder picker's answer as a path. The picked folder itself is remembered, so the next
    /// dialog opens there. [`answer`] remembers a file's parent, which would open one level above
    /// a folder the user just chose.
    fn answer_folder(&self, picked: Option<rfd::FileHandle>) -> Option<String> {
        let picked = picked?;
        self.chosen_folder(picked.path());
        Some(picked.path().to_string_lossy().into_owned())
    }

    fn folder(&self) -> Option<std::path::PathBuf> {
        self.0.lock().ok().and_then(|state| state.folder.clone())
    }

    /// The dialog, opening in the remembered folder when there is one.
    fn start(&self, dialog: rfd::AsyncFileDialog) -> rfd::AsyncFileDialog {
        match self.folder() {
            Some(folder) => dialog.set_directory(folder),
            None => dialog,
        }
    }
}

/// Linux file dialogs match patterns by case, so each extension goes in as lower and upper
/// case, plus FreeCAD's own mixed casing.
fn case_variants(exts: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for e in exts {
        let mut forms = vec![e.to_ascii_lowercase(), e.to_ascii_uppercase()];
        if e.eq_ignore_ascii_case("fcstd") {
            forms.push("FCStd".into());
        }
        for v in forms {
            if !all.contains(&v) {
                all.push(v);
            }
        }
    }
    all
}

/// egui's root Ui spans the whole viewport, including native safe areas. Lay out the app
/// inside the unobscured rectangle so its application bar can receive native pointer input.
fn content_ui(app: &mut SolveApp, ui: &mut egui::Ui) {
    let content = ui.ctx().content_rect();
    let above = egui::Rect::from_min_max(ui.max_rect().min, egui::pos2(ui.max_rect().right(), content.top()));
    ui.painter().rect_filled(above, 0.0, solidcraft_theme::Tokens::get().app_bar);
    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| app.ui(ui));
}

/// macOS full screen: AppKit moves the title bar (the traffic lights) into a window of its own
/// over the top of the app's window, invisible (alpha 0) until the pointer reaches the top of the
/// screen, when it slides down over the application bar like any full-screen app's. The bar keeps
/// its place under it: no strip is reserved, so the bar starts right below the menu bar or the
/// notch. That invisible window must not take the clicks meant for File, Undo and Redo under it
/// (#3643): while it is fully transparent it ignores the mouse, and it takes clicks again as soon
/// as it shows. The pointer moving over it reaches the app then, so the next frame restores it.
#[cfg(target_os = "macos")]
fn macos_fullscreen_title_bar() {
    use objc2_app_kit::{NSApplication, NSWindowButton, NSWindowStyleMask};
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    for window in NSApplication::sharedApplication(mtm).windows().iter() {
        if !window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            continue;
        }
        // In full screen the close button lives in the title bar's own window.
        let Some(bar) = window.standardWindowButton(NSWindowButton::CloseButton).and_then(|b| b.window()) else { continue };
        if bar == window {
            continue;
        }
        let ignore = hidden_title_bar_ignores_clicks(bar.alphaValue());
        if bar.ignoresMouseEvents() != ignore {
            bar.setIgnoresMouseEvents(ignore);
        }
    }
}

/// The full-screen title bar's window lets clicks through while it can't be seen.
#[cfg(any(target_os = "macos", test))]
fn hidden_title_bar_ignores_clicks(alpha: f64) -> bool {
    alpha.is_nan() || alpha <= 0.0
}

fn services(cc: &eframe::CreationContext<'_>, folder: &DialogFolder) -> Services {
    let parent = rfd::AsyncFileDialog::new().set_parent(cc);
    // The file chooser that opens may not be the desktop's own: the user is told when it does
    // not (#10159).
    #[cfg(target_os = "linux")]
    let chooser_ctx = cc.egui_ctx.clone();
    let open_parent = parent.clone();
    let program_parent = parent.clone();
    let file_parent = parent.clone();
    let folder_parent = parent.clone();
    let (open_folder, file_folder, save_folder, browse_folder) = (folder.clone(), folder.clone(), folder.clone(), folder.clone());
    Services {
        pick_open: Some(Box::new(move |first: &[&str]| {
            // The chooser that opens may not be the desktop's own (#10159).
            #[cfg(target_os = "linux")]
            desktop_chooser::note_opening(&chooser_ctx);
            // The file type the command reads comes first, so the picker opens on it (#1801).
            let mut dialog = open_folder.start(open_parent.clone());
            if !first.is_empty() {
                let label = first.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join(", ");
                dialog = dialog.add_filter(label, &case_variants(first));
            }
            // The groups come from the format registry, so a new format joins this dialog by itself.
            // Installed importer plugins read more kinds of file: they join "everything" and add
            // a group of their own, named after the plugin.
            let plugins = io::formats::plugin_open_filters();
            let mut everything: Vec<&str> = io::formats::open_extensions(true);
            for (_, exts) in &plugins {
                everything.extend(exts.iter().map(String::as_str).filter(|e| !io::formats::open_extensions(true).contains(e)));
            }
            dialog = dialog.add_filter(solidcraft_ui_text::tr("file.open.drawing_filter"), &case_variants(&everything));
            for (label, exts) in io::formats::import_filters(true) {
                dialog = dialog.add_filter(label, &case_variants(&exts));
            }
            for (label, exts) in &plugins {
                let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
                dialog = dialog.add_filter(label.as_str(), &case_variants(&exts));
            }
            let future = dialog.add_filter("All files", &["*"]).pick_file();
            let folder = open_folder.clone();
            Box::pin(async move { folder.answer(future.await) })
        })),
        // One kind of file (the agent chat's attachments): only the types it names, as given and in
        // lower and upper case (Linux dialogs match case; macOS allows just the listed types).
        pick_file: Some(Box::new(move |label: &str, exts: &[&str]| {
            let mut all: Vec<String> = Vec::new();
            for e in exts {
                for v in [e.to_string(), e.to_ascii_lowercase(), e.to_ascii_uppercase()] {
                    if !all.contains(&v) {
                        all.push(v);
                    }
                }
            }
            let all: Vec<&str> = all.iter().map(String::as_str).collect();
            // Any other file attaches by reference, so the list is not the only choice.
            let future = file_folder.start(file_parent.clone()).add_filter(label, &all).add_filter("All files", &["*"]).pick_file();
            let folder = file_folder.clone();
            Box::pin(async move { folder.answer(future.await) })
        })),
        pick_program: Some(Box::new(move || {
            let dialog = program_parent.clone().set_title("Choose your slicer");
            let dialog = if cfg!(target_os = "windows") { dialog.add_filter("Programs", &["exe"]) } else { dialog };
            let future = dialog.pick_file();
            Box::pin(async move { future.await.map(|p| p.path().to_string_lossy().into_owned()) })
        })),
        pick_folder: Some(Box::new(move || {
            let future = browse_folder.start(folder_parent.clone()).set_title("Choose a folder").pick_folder();
            let folder = browse_folder.clone();
            Box::pin(async move { folder.answer_folder(future.await) })
        })),
        pick_save: Some(Box::new(move |name: &str, exts: &[&str]| {
            let dialog = save_folder.start(parent.clone()).set_file_name(name);
            let dialog = if exts == [io::DESIGN_EXT, "json"] {
                if name.ends_with(".json") {
                    dialog.add_filter("Plain JSON design", &["json"]).add_filter("SolidCraft container", &[io::DESIGN_EXT])
                } else {
                    dialog.add_filter("SolidCraft container", &[io::DESIGN_EXT]).add_filter("Plain JSON design", &["json"])
                }
            } else {
                dialog.add_filter("File", exts)
            };
            let future = dialog.save_file();
            let folder = save_folder.clone();
            Box::pin(async move { folder.answer(future.await) })
        })),
        graphics_adapters: Vec::new(),
        graphics_backend: None,
        graphics_backend_override: std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| !v.trim().is_empty()),
        graphics_restart: graphics_restart_program(
            std::env::current_exe().ok().as_deref(),
            std::env::var_os("APPIMAGE").as_deref(),
            std::env::var_os("APPDIR").as_deref(),
            std::path::Path::is_file,
        )
        .map(|program| (program, graphics_restart_args(std::env::args().skip(1)))),
        graphics_path: graphics_path().map(|p| p.to_string_lossy().to_string()),
        window_system: window_system_now(),
        // Linux: the desktop's dark or light setting, which the window is not told on every
        // desktop (#7944). Windows and macOS: the window's own theme.
        #[cfg(target_os = "linux")]
        desktop_dark: Some(desktop_theme::start(&cc.egui_ctx)),
        #[cfg(not(target_os = "linux"))]
        desktop_dark: None,
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        reduce_transparency: Some(desktop_transparency::cached(&cc.egui_ctx)),
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        reduce_transparency: None,
        http_get: Some(std::sync::Arc::new(http_get)),
        http_download: Some(std::sync::Arc::new(http_download)),
        // Already an installed app: only the browser build offers to install.
        install_app: None,
        persist_design: None,
        can_restore: None,
        restore_file: None,
        download_design: None,
        easyprint_upload: Some(std::sync::Arc::new(easyprint_upload)),
        browser_files: None,
        agent_http: Some(std::sync::Arc::new(agent_http)),
        secrets: Some(std::sync::Arc::new(Keychain("SolidCraft agent chat"))),
        tickets_http: Some(std::sync::Arc::new(tickets_http)),
        #[cfg(feature = "usage-stats")]
        usage_http: Some(std::sync::Arc::new(usage_http)),
        ticket_secrets: Some(std::sync::Arc::new(Keychain("SolidCraft tickets"))),
        show_in_folder: Some(Box::new(|path| {
            let folder = std::path::Path::new(path).parent().unwrap_or_else(|| std::path::Path::new("."));
            let mut command = std::process::Command::new(if cfg!(target_os = "windows") {
                "explorer"
            } else if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            });
            command.arg(folder);
            #[cfg(target_os = "windows")]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let _ = command.spawn();
        })),
        open_path: Some(Box::new(open_with::open_with_default_app)),
        open_bytes: None,
    }
}

/// HTTPS GET for the update check (docs/updates.md): certificates from the operating system, a
/// 15 s limit, at most 1 MB, and no headers beyond what GitHub requires (a User-Agent without
/// the version or anything about this machine, and the API's media type).
fn http_get(url: &str) -> Result<Vec<u8>, solidcraft_ui_egui::update::FetchError> {
    use solidcraft_ui_egui::update::FetchError;
    let mut response = http_call(url, std::time::Duration::from_secs(15))?;
    response.body_mut().with_config().limit(1 << 20).read_to_vec().map_err(|e| FetchError::Failed(e.to_string()))
}

/// HTTPS download of an update package to `to` (docs/updates.md), reporting progress; stops when
/// `progress` returns false. The caller verifies the file.
fn http_download(url: &str, to: &std::path::Path, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<(), solidcraft_ui_egui::update::FetchError> {
    use solidcraft_ui_egui::update::FetchError;
    use std::io::{Read, Write};
    let fail = |e: String| FetchError::Failed(e);
    let mut response = http_call(url, std::time::Duration::from_secs(30 * 60))?;
    let total = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let mut body = response.body_mut().with_config().limit(solidcraft_ui_egui::update::install::MAX_PACKAGE).reader();
    let mut file = std::fs::File::create(to).map_err(|e| fail(format!("{}: {e}", to.display())))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = body.read(&mut buf).map_err(|e| fail(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| fail(format!("{}: {e}", to.display())))?;
        done += n as u64;
        if !progress(done, total) {
            return Err(fail("cancelled".into()));
        }
    }
    file.sync_all().map_err(|e| fail(format!("{}: {e}", to.display())))?;
    Ok(())
}

/// HTTP for the agent chat's API agents (OpenAI-compatible endpoints, docs/agent-chat.md): to the
/// URL the user configured, which may be plain HTTP on this computer (Ollama, LM Studio). No
/// redirects (a key must not follow one elsewhere), at most 16 MB back.
fn agent_http(r: &solidcraft_ui_egui::agent_chat::backend::HttpRequest) -> Result<(u16, Vec<u8>), String> {
    use std::time::Duration;
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(r.timeout_s.clamp(1, 3600))))
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent("SolidCraft")
        .max_redirects(0)
        .build()
        .into();
    let mut response = if r.method == "POST" {
        let mut req = agent.post(&r.url);
        for (k, v) in &r.headers {
            req = req.header(k, v);
        }
        req.send(r.body.as_deref().unwrap_or_default())
    } else {
        let mut req = agent.get(&r.url);
        for (k, v) in &r.headers {
            req = req.header(k, v);
        }
        req.call()
    }
    .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.body_mut().with_config().limit(16 << 20).read_to_vec().map_err(|e| e.to_string())?;
    Ok((status, body))
}

/// Open with EasyPrint: upload the model to Printables' import endpoint on a worker and answer on
/// `done` with the address to open. Through [`agent_http`]: no redirects, a timeout, no cookies.
fn easyprint_upload(name: &str, bytes: Vec<u8>, done: std::sync::mpsc::Sender<Result<String, String>>) {
    use solidcraft_engine::easyprint;
    let name = name.to_string();
    std::thread::spawn(move || {
        let (content_type, body) = easyprint::multipart(&name, &bytes);
        let request = solidcraft_ui_egui::agent_chat::backend::HttpRequest {
            method: "POST",
            url: easyprint::IMPORT_URL.into(),
            headers: vec![("Content-Type".into(), content_type), ("Accept".into(), "application/json".into())],
            body: Some(body),
            timeout_s: 180,
        };
        let _ = done.send(agent_http(&request).and_then(|(status, body)| easyprint::parse_import_response(status, &body)));
    });
}

/// Secrets in the system keychain: API keys (service "SolidCraft agent chat", one entry per
/// agent) and the ticket server's sign-in (service "SolidCraft tickets").
struct Keychain(&'static str);

impl Keychain {
    fn entry(&self, id: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(self.0, id).map_err(|e| e.to_string())
    }
}

/// HTTP to the ticket server for problem reports (docs/problem-reports.md): on a worker thread, HTTPS
/// only (a local test server aside), no redirects (the token must not follow one), at most 16 MB
/// back. The answer comes on `done`.
fn tickets_http(r: solidcraft_ui_egui::agent_chat::backend::HttpRequest, done: std::sync::mpsc::Sender<solidcraft_ui_egui::tickets::Reply>) {
    let spawned = std::thread::Builder::new().name("tickets-http".into()).spawn(move || {
        // HTTPS, or plain HTTP to this computer (a test server): the host compared exactly.
        let reply = if solidcraft_ui_egui::tickets::api::allowed_url(&r.url) { tickets_call(&r) } else { Err("only HTTPS is allowed".into()) };
        let _ = done.send(reply);
    });
    // Without a worker the sender is dropped with the closure: the report sees no answer and
    // waits in the queue.
    drop(spawned);
}

/// Sends a usage statistics request on a worker thread, to its endpoints only
/// (`usage_stats::allowed_url`); anything else is refused. The answer comes on `done`.
#[cfg(feature = "usage-stats")]
fn usage_http(r: solidcraft_ui_egui::agent_chat::backend::HttpRequest, done: std::sync::mpsc::Sender<solidcraft_ui_egui::tickets::Reply>) {
    let spawned = std::thread::Builder::new().name("usage-stats-http".into()).spawn(move || {
        let reply =
            if solidcraft_ui_egui::usage_stats::allowed_url(&r.url) { tickets_call(&r) } else { Err("not the usage statistics endpoint".into()) };
        let _ = done.send(reply);
    });
    drop(spawned);
}

fn tickets_call(r: &solidcraft_ui_egui::agent_chat::backend::HttpRequest) -> Result<(u16, Vec<u8>), String> {
    use std::time::Duration;
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(r.timeout_s.clamp(1, 3600))))
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent(concat!("SolidCraft/", env!("CARGO_PKG_VERSION")))
        .max_redirects(0)
        .build()
        .into();
    let body = r.body.as_deref().unwrap_or_default();
    macro_rules! with_headers {
        ($req:expr) => {{
            // Each call has its own agent, so a kept-alive connection is never used again. Asked
            // to close, the server ends a reply that has no length by closing; otherwise the
            // reply is read until the server gives up and the call ends on `timeout: global`
            // (#17932).
            let mut req = $req.header("Connection", "close");
            for (k, v) in &r.headers {
                req = req.header(k, v);
            }
            req
        }};
    }
    let mut response = match r.method {
        "GET" => with_headers!(agent.get(&r.url)).call(),
        "POST" => with_headers!(agent.post(&r.url)).send(body),
        "PUT" => with_headers!(agent.put(&r.url)).send(body),
        "PATCH" => with_headers!(agent.patch(&r.url)).send(body),
        "DELETE" => with_headers!(agent.delete(&r.url)).call(),
        other => return Err(format!("unsupported method {other}")),
    }
    .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.body_mut().with_config().limit(16 << 20).read_to_vec().map_err(|e| e.to_string())?;
    Ok((status, body))
}

impl solidcraft_ui_egui::agent_chat::backend::Secrets for Keychain {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        match self.entry(id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.entry(id)?.set_password(secret).map_err(|e| e.to_string())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        match self.entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// One GET to GitHub: a 200 answer, or why not (rate limits included).
fn http_call(url: &str, timeout: std::time::Duration) -> Result<ureq::http::Response<ureq::Body>, solidcraft_ui_egui::update::FetchError> {
    use solidcraft_ui_egui::update::FetchError;
    use std::time::Duration;
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .https_only(true)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .timeout_connect(Some(Duration::from_secs(5)))
        .user_agent("SolidCraft-update-check")
        .max_redirects(3)
        .build()
        .into();
    let response = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| FetchError::Failed(e.to_string()))?;
    let status = response.status().as_u16();
    let header = |k: &str| response.headers().get(k).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
    if status == 429 || (status == 403 && (header("x-ratelimit-remaining") == Some(0) || header("retry-after").is_some())) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let retry_after_s = header("retry-after").or_else(|| header("x-ratelimit-reset").map(|reset| reset.saturating_sub(now))).unwrap_or(3600);
        return Err(FetchError::RateLimited { retry_after_s });
    }
    if status != 200 {
        return Err(FetchError::Failed(format!("GitHub answered HTTP {status}")));
    }
    Ok(response)
}

/// The per-user settings folder: eframe's storage (`app.ron`) and the anti-aliasing preference.
fn settings_dir() -> Result<Option<std::path::PathBuf>, String> {
    let selected = std::env::var_os("SOLIDCRAFT_SETTINGS_DIR");
    startup::settings_directory(selected.as_deref(), eframe::storage_dir("SolidCraft"))
}

/// The anti-aliasing preference, kept next to eframe's storage (it is needed before the
/// window opens).
fn graphics_path() -> Option<std::path::PathBuf> {
    settings_dir().ok().flatten().map(|d| d.join("graphics.json"))
}

/// Compatibility shim (docs/compatibility.md): on the first launch after the app id changed, copy
/// the settings of earlier releases into the new folder. In the Flatpak sandbox, earlier releases
/// kept everything under an old app id's `~/.var/app` folder; those copies come first, newest
/// first. The old folders are never changed.
fn adopt_legacy_settings() {
    let Some(current) = settings_dir().ok().flatten() else { return };
    if let Err(e) = adopt_settings(&current, &solidcraft_engine::io::legacy_flatpak_paths(&current)) {
        log::warn!("settings: could not copy the previous release's settings into {}: {e}", current.display());
    }
}

fn adopt_settings(current: &std::path::Path, old_flatpak: &[std::path::PathBuf]) -> std::io::Result<()> {
    for old in old_flatpak {
        solidcraft_engine::io::adopt_legacy_dir(current, old)?;
    }
    Ok(())
}

/// `graphics.json`, read before the window opens.
fn graphics() -> Option<serde_json::Value> {
    graphics_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
}

fn restore_preferences(app: &mut SolveApp, stored_ui: Option<&str>, saved_graphics: Option<&serde_json::Value>, safe_mode: bool) {
    if let Some(p) = stored_ui.filter(|_| !safe_mode) {
        app.load_prefs(p);
    }
    // Even safe mode edits the real graphics.json. Keep its choices in Preferences so an
    // anti-aliasing change cannot erase them; safe mode's renderer still uses its own defaults.
    restore_graphics_preferences(app, saved_graphics);
}

/// Restore startup choices after loading the separately saved UI preferences.
fn restore_graphics_preferences(app: &mut SolveApp, saved: Option<&serde_json::Value>) {
    // Preserve legacy comma lists until the user explicitly selects a different API.
    app.preferences.graphics_backend = saved
        .and_then(|v| v.get("backend").and_then(serde_json::Value::as_str))
        .filter(|choice| !choice.trim().is_empty())
        .unwrap_or("auto")
        .to_string();
    app.preferences.graphics_adapter = saved.and_then(|v| v.get("adapter").and_then(serde_json::Value::as_str)).unwrap_or_default().to_string();
    // graphics.json chooses the window before app.ron is loaded. Reflect that saved choice,
    // not the current session's window system (which an environment override may change).
    app.preferences.window_system = saved
        .and_then(|v| v.get("window_system").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|choice| matches!(*choice, "wayland" | "x11"))
        .unwrap_or("auto")
        .to_string();
}

/// The viewport scene's sample count, by the rule the web app shares
/// (`solidcraft_ui_egui::gpu::scene_samples`).
fn scene_samples(backend: eframe::wgpu::Backend, samples: u32, supported: &[u32]) -> u32 {
    solidcraft_ui_egui::gpu::scene_samples(backend, samples, supported)
}

/// Keep the default until the installed renderer's startup check proves a fallback is needed.
fn scene_msaa_request(saved: Option<u16>) -> u16 {
    saved.unwrap_or(MSAA)
}

/// Query the scene's colour/depth intersection, rather than the depthless window's counts.
fn scene_sample_choices(depth_bits: u8, stencil_bits: u8, query: impl FnOnce(Option<eframe::wgpu::TextureFormat>) -> Vec<u32>) -> Vec<u32> {
    query(eframe::egui_wgpu::depth_format_from_bits(depth_bits, stencil_bits))
}

/// An explicitly saved anti-aliasing choice, absent until Graphics changes it.
fn msaa() -> Option<u16> {
    graphics()
        .and_then(|v| v.get("msaa").and_then(serde_json::Value::as_u64))
        .and_then(|m| u16::try_from(m).ok())
        .filter(|m| [1, 2, 4, 8].contains(m))
}

/// Windows and Linux: no OS title bar; the application bar is the title bar
/// (`solidcraft_ui_egui::titlebar`). macOS keeps its traffic lights over the integrated bar.
const CUSTOM_TITLEBAR: bool = !cfg!(target_os = "macos");

/// Linux: the window manager's own decorations (server-side, or the compositor's frame) can be
/// asked for instead of the application bar ([`titlebar_choice`]). Elsewhere always `true`.
fn app_titlebar_wanted() -> bool {
    if !cfg!(all(unix, not(target_os = "macos"))) {
        return true;
    }
    let setting = graphics().and_then(|v| v.get("titlebar").and_then(serde_json::Value::as_str).map(str::to_string));
    titlebar_choice(solidcraft_engine::geom::env::var("TITLEBAR").as_deref(), setting.as_deref())
}

/// Linux: whether the application bar is the title bar. `SOLIDCRAFT_TITLEBAR` (else
/// `graphics.json`'s `titlebar`) set to `system` gives the window the desktop's own title bar and
/// buttons; `app` (or anything else, or nothing) keeps the application bar as the title bar.
fn titlebar_choice(env: Option<&str>, setting: Option<&str>) -> bool {
    let choice = env.map(str::trim).filter(|v| !v.is_empty()).or(setting.map(str::trim));
    !choice.is_some_and(|v| v.eq_ignore_ascii_case("system"))
}

const DEPTH_BITS: u8 = 24;
/// A 32-bit float depth buffer, drawn with reversed depth (1 near, 0 far) so its precision follows
/// the distance from the eye (#1434). Only where the device the window got has it with a stencil
/// ([`startup::FLOAT_DEPTH_STENCIL`], asked for wherever the adapter offers it); else
/// [`DEPTH_BITS`]. Not in safe mode.
const FLOAT_DEPTH_BITS: u8 = 32;
/// The section cap marks where it shows in the stencil.
const STENCIL_BITS: u8 = 8;
const MSAA: u16 = 4;

/// `custom_titlebar`: no OS decorations, the application bar is the title bar.
fn app_viewport(custom_titlebar: bool) -> Result<egui::ViewportBuilder, eframe::Error> {
    // Let macOS use the bundle's layered icon; eframe's default bitmap would replace it.
    #[cfg(target_os = "macos")]
    let icon = egui::IconData::default();
    #[cfg(not(target_os = "macos"))]
    let icon =
        egui::IconData { rgba: app_icon::rgba(64).map_err(|e| eframe::Error::AppCreation(std::io::Error::other(e).into()))?, width: 64, height: 64 };
    Ok(solidcraft_ui_egui::window_chrome::viewport("SolidCraft", APP_ID, custom_titlebar).with_icon(icon))
}

/// The window's app id: it names the desktop entry, so it is the packages' reverse-DNS id.
const APP_ID: &str = solidcraft_engine::io::APP_ID;

/// Disconnects from the 3D mouse driver when dropped, as `main` returns or a panic unwinds out
/// of it (`solidcraft_3dx_client::stop`).
#[cfg(target_os = "macos")]
struct DriverClient;

#[cfg(target_os = "macos")]
impl Drop for DriverClient {
    fn drop(&mut self) {
        solidcraft_3dx_client::stop();
    }
}

fn main() -> eframe::Result {
    #[cfg(target_os = "macos")]
    objc2_foundation::NSProcessInfo::processInfo().setProcessName(objc2_foundation::ns_string!("SolidCraft"));
    mark("main");
    // The kernel's fix set, named to solidcore-ops before any call runs on it.
    solidcraft_engine::kernel::start();
    // A large importer plugin compiles once per version, not at the first file of every session.
    solidcraft_engine::plugin::configure_compile_cache();
    solidcraft_ui_egui::i18n::install();
    solidcraft_look::install_tables();
    solidcraft_ui_egui::workspace::install_tabs();
    solidcraft_ui_egui::navigation_slots::install();
    // On OCLP-patched Macs, wgpu picks a Metal Shading Language standard that the system's
    // compiler refuses (`invalid value 'metal3.2'`), because the patched OS reports a newer
    // macOS version than the GPU's Metal compiler supports. Re-launching with
    // SYSTEM_VERSION_COMPAT=1 makes the OS report the real (older) version so wgpu selects
    // a standard the compiler accepts. The variable must be set before the process starts, so
    // we detect OCLP here and re-exec immediately, before any macOS framework call (#3080, #4605).
    #[cfg(target_os = "macos")]
    if let Err(e) = startup::relaunch_with_version_compat() {
        log::warn!("startup: OCLP check failed (continuing normally): {e}");
    }
    // SOLIDCRAFT_PROFILE=1: the profiler records from the start (diagnostics.profile reads it).
    solidcraft_engine::timing::profile_from_env();
    // Validate before legacy adoption, crash markers or any user preference read.
    let settings = settings_dir().map_err(|error| eframe::Error::AppCreation(error.into()))?;
    if std::env::var_os("SOLIDCRAFT_SETTINGS_DIR").is_none() {
        adopt_legacy_settings();
    }
    let mut safe_mode = startup::env_on(std::env::var(startup::SAFE_MODE_ENV).ok().as_deref());
    let mut control_port: Option<u16> = solidcraft_engine::geom::env::var("CONTROL_PORT").and_then(|p| p.parse().ok());
    let mut files = Vec::new();
    let mut sample = false;
    // `--theme-file PATH`: follow that theme file for this run without saving it (#7573).
    let mut theme_file: Option<String> = None;
    let mut max_file_size = None;
    // `--new-instance`: open this launch as its own app even when SolidCraft already runs.
    let mut new_instance = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--sample" => sample = true,
            "--new-instance" => new_instance = true,
            "--theme-file" => theme_file = args.next(),
            "--max-file-size" => max_file_size = Some(args.next()),
            s if s.starts_with("--max-file-size=") => max_file_size = Some(s.strip_prefix("--max-file-size=").map(str::to_string)),
            "--safe-mode" => safe_mode = true,
            "--view" => solidcraft_ui_egui::new_window::set_start_view(&args.next().unwrap_or_default()),
            "--version" | "-V" => {
                println!("solidcraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--crash-log" => {
                match solidcraft_engine::crash::default_dir() {
                    Some(dir) => {
                        println!("crash reports: {}", dir.display());
                        for r in solidcraft_engine::crash::list(&dir).iter().take(5) {
                            println!("  {}  {}  {}", r.id, r.kind, r.message);
                        }
                        println!("more: solidcraft-cli crashes");
                    }
                    None => println!("no crash folder on this system"),
                }
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "usage: solidcraft [--control PORT] [--sample] [--theme-file THEME.json] [--crash-log] [--safe-mode] [--new-instance] [--max-file-size SIZE] [--view model|sketch|sheet|render|animation] [design.sc | part.step]"
                );
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    // Files given while SolidCraft already runs open in that window, and this launch ends (#18055).
    // Not with a control port, the sample or safe mode, which want a window of their own; no answer
    // within a moment starts normally.
    if !files.is_empty()
        && !new_instance
        && !safe_mode
        && !sample
        && control_port.is_none()
        && let Some(endpoint) = settings.as_deref().and_then(single_instance::Endpoint::in_settings)
    {
        let absolute: Vec<String> = files.iter().map(|f| single_instance::absolute(f)).collect();
        if single_instance::forward(&endpoint, &absolute) {
            return Ok(());
        }
    }
    // The log on disk, which a problem report attaches the end of (`log_file`); the run
    // before's log is kept beside it. After the hand-off above: a launch that only passes its
    // files to the running window must not move that window's log aside.
    if let Some(dir) = solidcraft_engine::recovery::data_dir() {
        solidcraft_ui_egui::log_file::install(&dir.join("logs"));
    }
    log::info!("SolidCraft {} ({}) started", solidcraft_ui_egui::help::VERSION, solidcraft_ui_egui::help::COMMIT);
    // `--max-file-size 2GB`: the largest files this run reads, over Preferences, never saved (#4086).
    if let Some(value) = max_file_size {
        let applied = startup::max_file_size(value.as_deref()).and_then(|bytes| solidcraft_engine::io::limits::set_override(Some(bytes)));
        if let Err(error) = applied {
            solidcraft_engine::note!("SolidCraft: {error}");
            return Err(eframe::Error::AppCreation(std::io::Error::other(error).into()));
        }
    }
    // Settings that don't read are moved aside; launches that never got a window are counted,
    // and after a few in a row this one starts in safe mode (docs/book/src/guide/troubleshooting.md).
    let mut notes: Vec<String> = Vec::new();
    let mut failed_starts = 0;
    if let Some(dir) = &settings {
        notes.extend(startup::check_settings(dir, &[solidcraft_ui_egui::prefs::STORAGE_KEY]));
        failed_starts = startup::begin(dir);
    }
    if failed_starts >= startup::FAILED_STARTS_FOR_SAFE_MODE && !safe_mode {
        safe_mode = true;
        notes.push(format!("The last {failed_starts} launches stopped before the window opened, so this one is in safe mode."));
    }
    let mut cpu_renderer = safe_mode || startup::cpu_renderer_env(std::env::var(startup::RENDERER_ENV).ok().as_deref());
    // Persist the one-start guard before any adapter, device or depth-format probe.
    // Recovery launches and explicit overrides must not consume a pending UI choice.
    let backend_override = std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| !v.trim().is_empty());
    let retry_backend = std::env::var(startup::RETRY_BACKEND_ENV).ok();
    let backend_path = settings.as_ref().map(|dir| dir.join("graphics.json"));
    let backend_launch = match backend_path
        .as_ref()
        .map(|path| startup::prepare_backend_start(path, !cpu_renderer && !backend_override && retry_backend.is_none()))
        .transpose()
    {
        Ok(launch) => launch.unwrap_or_default(),
        Err(reason) => {
            // A choice that could not be guarded must not load the requested driver.
            cpu_renderer = true;
            notes.push(reason);
            startup::BackendLaunch::default()
        }
    };
    let backend_path_for_safe = backend_path.clone();
    // The crash guard (#18683): an API whose driver crashed the process while the last launch
    // started it is skipped from now on (until the next version), and each API this launch
    // probes or starts is named on disk first.
    let (backend_guard, skipped) = match (&settings, &backend_path) {
        (Some(dir), Some(path)) => {
            let (guard, skipped) = startup::BackendGuard::begin(dir, path, solidcraft_ui_egui::help::VERSION);
            (Some(guard), skipped)
        }
        _ => (None, startup::SkippedBackends::default()),
    };
    if !skipped.crashed.is_empty() || !skipped.failed.is_empty() {
        solidcraft_engine::note!("SolidCraft graphics: skipping {:?} (crashed) and {:?} (no hardware adapter)", skipped.crashed, skipped.failed);
    }
    let backend_attempt = backend_path.zip(backend_launch.attempt);
    let backend_note = backend_launch.note;
    // The launch before this one closed because its graphics device was lost (`App::watch_device`).
    let after_loss = device_lost::recent(std::env::var(device_lost::LOST_ENV).ok().as_deref(), device_lost::now_secs());
    if after_loss {
        let reason = std::env::var(device_lost::REASON_ENV).ok();
        if let Some(reason) = device_lost::reason(reason.as_deref()) {
            log::warn!("graphics: the last launch lost its device ({reason})");
        }
        notes.push(device_lost::note(cpu_renderer, reason.as_deref()));
    }
    // Safe graphics mode: after a launch that failed to come up or lost its device, or when asked
    // for, this one starts on the plain path (`startup::SafeGraphics`). Decided and saved before
    // any graphics device is created, so a launch that dies in the driver counts.
    let tracked = control_port.is_none();
    let previous_failed = failed_starts > 0 || after_loss;
    let mut failed_backend = None;
    let safe_graphics = match backend_path_for_safe.as_ref() {
        Some(path) => match startup::update_safe_graphics(path, |record| (record.begin(previous_failed, tracked, safe_mode), record.backend.clone()))
        {
            Ok((reason, backend)) => {
                // After a failure, the API the failed full launch used is avoided.
                if matches!(reason, Some(startup::SafeGraphicsReason::Failed | startup::SafeGraphicsReason::Sticky)) {
                    failed_backend = backend;
                }
                reason
            }
            Err(reason) => {
                log::warn!("graphics: safe graphics record not saved: {reason}");
                (safe_mode || (tracked && previous_failed)).then_some(startup::SafeGraphicsReason::Failed)
            }
        },
        None => safe_mode.then_some(startup::SafeGraphicsReason::Asked),
    };
    if let Some(reason) = safe_graphics {
        solidcraft_engine::note!("SolidCraft graphics: safe graphics mode ({reason:?})");
    }
    let safe = safe_graphics.is_some();
    if safe_mode {
        notes.push("Safe mode: default settings (yours are kept for the next normal start), no recovery offer, software rendering.".into());
    }
    let fallback_note = std::env::var(startup::FALLBACK_NOTE_ENV).ok().map(|s| s.chars().take(4096).collect::<String>()).or(backend_note);
    if let Some(note) = &fallback_note {
        log::warn!("{note}");
        solidcraft_engine::note!("SolidCraft: {note}");
        notes.push(note.clone());
    }
    let saved_samples = if safe_mode { None } else { msaa() };
    let wgpu_options = if safe {
        safe_wgpu_options(cpu_renderer, failed_backend.as_deref(), skipped.crashed)
    } else {
        wgpu_options(cpu_renderer, backend_launch.backend.as_deref(), skipped, backend_guard.as_ref())
    };
    // Named on disk before the window loads the drivers; cleared once the
    // first frame is presented (`App::backend_guard`) or startup returns.
    if let Some(guard) = &backend_guard
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &wgpu_options.wgpu_setup
    {
        guard.attempt(setup.instance_descriptor.backends);
    }
    // A tracked full launch notes the API it starts, for the safe launch after a failure.
    if !safe
        && tracked
        && !cpu_renderer
        && let Some(path) = backend_path_for_safe.as_ref()
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &wgpu_options.wgpu_setup
    {
        let backend = startup::backend_name(setup.instance_descriptor.backends).map(str::to_string);
        if let Err(reason) = startup::update_safe_graphics(path, |record| record.backend = backend) {
            log::warn!("graphics: safe graphics record not saved: {reason}");
        }
    }
    // Crashes leave a report in the crash folder (solidcraft_engine::crash, docs/crash-reports.md).
    let crashed = solidcraft_engine::crash::default_dir()
        .map(|dir| {
            let build = solidcraft_engine::crash::Build {
                version: solidcraft_ui_egui::help::VERSION,
                commit: solidcraft_ui_egui::help::COMMIT,
                built: solidcraft_ui_egui::help::BUILD_DATE,
            };
            solidcraft_engine::crash::install(dir, build)
        })
        .unwrap_or_default();
    // The recovery folder also holds the reopen guard marker (#5064); read once, reused below.
    let recovery_dir = solidcraft_engine::recovery::default_dir();
    // A crash in native code (a graphics driver) says where it happened: the exception or signal
    // and the module on Windows and macOS, and on every system the startup step the app was in.
    #[cfg(target_os = "windows")]
    native_crash::install();
    #[cfg(target_os = "macos")]
    native_crash_macos::install();
    // Windows with only OpenGL to start: the system title bar instead of ours (#2063).
    let started = match &wgpu_options.wgpu_setup {
        eframe::egui_wgpu::WgpuSetup::CreateNew(setup) => setup.instance_descriptor.backends,
        _ => eframe::wgpu::Backends::all(),
    };
    let custom_titlebar = CUSTOM_TITLEBAR && app_titlebar_wanted() && !startup::system_title_bar(cfg!(target_os = "windows"), started);
    eframe::egui_wgpu::set_startup_hook(startup_step);
    solidcraft_engine::crash::phase("window");
    let options = eframe::NativeOptions {
        viewport: app_viewport(custom_titlebar)?,
        // The window's pass (egui's) has no depth and one sample: the viewport draws its scene
        // offscreen with its own depth and MSAA, then blits it, so the UI pass pays for neither.
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 1,
        // Kept with the other settings, not in a folder named after the app id. Safe mode reads
        // and writes a scratch copy, leaving the user's settings as they were.
        persistence_path: if safe_mode {
            let scratch = std::env::temp_dir().join(format!("solidcraft-safe-mode-{}.ron", std::process::id()));
            let _ = std::fs::remove_file(&scratch);
            Some(scratch)
        } else {
            settings.as_ref().map(|d| d.join("app.ron"))
        },
        persist_window: !safe_mode,
        wgpu_options,
        ..Default::default()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut options = options;
    #[cfg(all(unix, not(target_os = "macos")))]
    let x11 = {
        // SOLIDCRAFT_WAYLAND wins over Preferences › Graphics › Window system (#3083).
        let environment = solidcraft_engine::geom::env::var("WAYLAND");
        let preference = window_system_preference();
        let wayland_override = environment.as_deref().or(window_system_override(preference.as_deref()));
        let wayland_display = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let x11 = prefer_x11(
            std::env::var_os("DISPLAY").is_some(),
            wayland_display,
            std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
            wayland_override,
        );
        // Without a Wayland socket winit opens on X11 anyway.
        let _ = WINDOW_SYSTEM.set(if x11 || !wayland_display { "x11" } else { "wayland" });
        x11
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    if x11 {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // Files opened from Finder and the Dock arrive as events, not arguments.
    #[cfg(target_os = "macos")]
    open_documents::install();
    let created = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let created_in_app = created.clone();
    let starting = settings.clone();
    let instance_dir = settings.clone();
    let attempt_in_app = backend_attempt.clone();
    let guard_in_app = backend_guard.clone();
    // A tracked full-graphics launch reports how it went (`startup::SafeGraphics`).
    let graphics_path_in_app = backend_path_for_safe.clone().filter(|_| !safe && tracked);
    // A panic on the window's thread ends the app without `on_exit`; this disconnects from the
    // 3D mouse driver then too.
    #[cfg(target_os = "macos")]
    let _driver_client = DriverClient;
    mark("run_native");
    let result = eframe::run_native(
        "SolidCraft",
        options,
        Box::new(move |cc| {
            created_in_app.store(true, std::sync::atomic::Ordering::SeqCst);
            solidcraft_engine::crash::phase("app setup");
            mark("window and device ready");
            solidcraft_ui_egui::fonts::setup_bundled_fonts(&cc.egui_ctx);
            solidcraft_ui_egui::fonts::load_system_fonts_in_background(&cc.egui_ctx);
            mark("fonts");
            let dialog_folder = DialogFolder::default();
            // The file chooser that opens may not be the desktop's own: the session bus is asked
            // now, so the answer is there when a file is opened (#10159).
            #[cfg(target_os = "linux")]
            desktop_chooser::start(&cc.egui_ctx);
            let mut app = SolveApp::new(Session::default(), services(cc, &dialog_folder));
            app.apply_off_thread = Some(solidcraft_ui_egui::dialogs::APPLY_AT_ONCE);
            app.custom_titlebar = custom_titlebar;
            #[cfg(target_os = "windows")]
            {
                // Windows: the taskbar button shows the mark even when slow first frames keep the
                // taskbar waiting for the window's icon.
                if !native_frame::set_class_icon(cc) {
                    solidcraft_engine::note!("SolidCraft window: no class icon; the taskbar may show a generic one");
                }
                // The OS hit test offers the resize border, at the window's DPI, from the first
                // message (#1381).
                app.native_resize_border = custom_titlebar && native_frame::install(cc);
                if custom_titlebar && !app.native_resize_border {
                    solidcraft_engine::note!("SolidCraft window: no native resize border; egui edge zones instead");
                }
            }
            app.integrated_titlebar = cfg!(target_os = "macos");
            app.cpu_meshing = safe_mode || safe;
            app.safe_graphics = safe;
            app.graphics_fallback = fallback_note.clone();
            let stored_ui = cc.storage.and_then(|s| solidcraft_ui_egui::prefs::stored(|k| s.get_string(k)));
            restore_preferences(&mut app, stored_ui.as_deref(), graphics().as_ref(), safe_mode);
            // Plugin approvals live in the plugins folder's grant store, shared with the command
            // line and headless MCP; safe mode keeps to its own empty preferences.
            if !safe_mode {
                app.share_plugin_grants();
            }
            // The administrator's policy holds in safe mode too.
            app.policy = solidcraft_ui_egui::policy::load();
            solidcraft_ui_egui::policy::enforce(&mut app);
            #[cfg(feature = "usage-stats")]
            app.load_usage_stats(cc.storage.and_then(|s| s.get_string(solidcraft_ui_egui::usage_stats::STORAGE_KEY)).as_deref());
            if let Some(path) = &theme_file
                && let Err(e) = solidcraft_ui_egui::prefs::follow_theme_file_for_run(&mut app, path)
            {
                app.set_status(format!("--theme-file: {e}"), true);
            }
            // Before the first frame: a drag can start as soon as the window shows.
            #[cfg(target_os = "windows")]
            let window_move = {
                let mut window_move = native_frame::WindowMove::default();
                window_move.follow(app.preferences.windows_smooth_window_drag);
                window_move
            };
            mark("app state and prefs");
            let gpu = cc.wgpu_render_state.as_ref().map(|rs| LostWatch {
                lost: rs.device_lost.clone(),
                restart_on_software: device_lost::restart_on_software(after_loss, cpu_renderer),
                lost_at: None,
                reason: String::new(),
            });
            let gpu_setup = cc.wgpu_render_state.as_ref().map(|rs| {
                app.services.graphics_adapters = rs
                    .available_adapters
                    .iter()
                    .map(|adapter| {
                        let info = adapter.get_info();
                        (info.name.clone(), format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend))
                    })
                    .collect();
                let info = rs.adapter.get_info();
                app.services.graphics_backend = match info.backend {
                    eframe::wgpu::Backend::Vulkan => Some("vulkan"),
                    eframe::wgpu::Backend::Dx12 => Some("dx12"),
                    eframe::wgpu::Backend::Gl => Some("gl"),
                    eframe::wgpu::Backend::Metal => Some("metal"),
                    _ => None,
                };
                if !app.preferences.graphics_adapter.is_empty() && !info.name.eq_ignore_ascii_case(app.preferences.graphics_adapter.as_str()) {
                    solidcraft_engine::note!(
                        "SolidCraft graphics: preferred adapter {:?} was not used; using {:?} instead",
                        app.preferences.graphics_adapter,
                        info.name
                    );
                }
                let samples = scene_msaa_request(saved_samples);
                // Read from the device itself: no second instance lists every adapter for it.
                let float_depth = !safe && rs.device.features().contains(startup::FLOAT_DEPTH_STENCIL);
                let depth_bits = if float_depth { FLOAT_DEPTH_BITS } else { DEPTH_BITS };
                let supported = scene_sample_choices(depth_bits, STENCIL_BITS, |depth| {
                    eframe::egui_wgpu::supported_msaa_samples(&rs.adapter, &rs.device, rs.target_format, depth)
                });
                // Safe graphics draws the scene without anti-aliasing; the saved choice is kept.
                let scene_samples = if safe { 1 } else { scene_samples(info.backend, u32::from(samples), &supported) };
                GpuSetup { rs: rs.clone(), depth_bits, samples, supported, scene_samples }
            });
            // `--control`/the environment name a port for this run; otherwise the preference (off
            // by default) opens its port, and turning it off or on later takes effect at once
            // (docs/control-protocol.md).
            let ctx = cc.egui_ctx.clone();
            solidcraft_ui_egui::control_switch::launch(&mut app, Box::new(move |listen| control_server::start(listen, ctx.clone())), control_port);
            // The agent chat's MCP bridge reaches the app through an endpoint of its own, started
            // on a free port the first time the chat connects (docs/agent-chat.md).
            let ctx = cc.egui_ctx.clone();
            app.agent_chat.control_starter = Some(Box::new(move |tx| control_server::listen_agent(tx, ctx.clone())));
            // Later launches with a file hand it to this window instead of opening another app. Not
            // when driven over the control channel or by `--new-instance`.
            if !new_instance && !sample && control_port.is_none() {
                single_instance::start(instance_dir.as_deref(), &cc.egui_ctx);
            }
            // A launch from Finder: its files arrived before the window.
            #[cfg(target_os = "macos")]
            let files = {
                open_documents::set_ui(&cc.egui_ctx);
                [files, open_documents::take()].concat()
            };
            // No design given: the designs open at the last quit, when the preference asks (#5542).
            // Not when driven over the control channel, whose clients expect a fresh design.
            // A design that stopped the last session while it was being reopened is skipped this
            // time, so one bad file cannot brick every launch (#5064).
            app.reopen_guard_dir = recovery_dir.clone();
            let files = if files.is_empty() && !sample && control_port.is_none() {
                let (reopen, skipped) = solidcraft_ui_egui::documents::reopen_plan(&app, recovery_dir.as_deref());
                if !skipped.is_empty() {
                    let names: Vec<String> = skipped
                        .iter()
                        .map(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()))
                        .collect();
                    notes.push(format!(
                        "The last session stopped while reopening {}. It was not reopened this time; open it yourself if you need it.",
                        names.join(", ")
                    ));
                }
                reopen
            } else {
                files
            };
            // A shutdown or restart closes the app cleanly instead of killing it (#7345).
            #[cfg(unix)]
            {
                let ctx = cc.egui_ctx.clone();
                if let Err(error) = system_quit::install(move || ctx.request_repaint()) {
                    log::warn!("shutdown signals: {error}");
                }
            }
            let nav_bridge = nav_bridge::NavBridge::start(&cc.egui_ctx);
            // The rest is slow on some computers: it runs after the window shows, a step a frame,
            // behind the starting page (#1210).
            // The 3D view's shader compile starts with the first step and runs beside the app: the
            // start page does not wait for it (#1210).
            let opening = sample || !files.is_empty();
            // Driven over the control channel, the app still waits for it before it takes commands:
            // clients take pictures of the view as soon as they are connected.
            let mut setup = solidcraft_ui_egui::starting::Starting::default();
            if let Some(gpu_setup) = gpu_setup {
                if control_port.is_some() {
                    setup = setup.until("", move |a: &mut App, ctx| view_step(a, ctx, &gpu_setup));
                } else {
                    setup = setup.beside(move |a: &mut App, ctx| view_step(a, ctx, &gpu_setup));
                }
            }
            let setup = setup
                .then(if opening { "starting.design" } else { "" }, move |a: &mut App, _ctx| {
                    solidcraft_engine::crash::phase("design");
                    let app = &mut a.solve;
                    if sample {
                        if let Err(e) = app.session.run_script(&solidcraft_engine::sample::script()) {
                            app.set_status(format!("sample: {e}"), true);
                        }
                        app.session.doc_mut().name = "Sample Plate".into();
                        app.session.mark_saved();
                        app.session.undo.clear();
                        // Built straight into the session, so nothing asked for a fit: frame it at the
                        // home view as any opened design is (#9195). The fit waits for the first layout.
                        app.cam.set_view(solidcraft_viewport::StandardView::Iso);
                        app.fit_view();
                    }
                    // Lock files next to open designs: a design open on another computer says so (#5467),
                    // the files given at launch included.
                    solidcraft_ui_egui::open_elsewhere::start(app);
                    for f in &files {
                        app.open_chosen_file(f);
                    }
                    // No file and no sample: the empty design takes Units of new designs (#1205).
                    // An opened file and the sample part keep the units they were saved or built with.
                    if files.is_empty() && !sample {
                        solidcraft_ui_egui::prefs::apply_new_design(app);
                    }
                    // No design given: the start page (not when driven over the control channel, whose
                    // clients expect the design view; `ui.home` shows it there).
                    if app.preferences.home_on_launch(!files.is_empty(), sample, control_port.is_some()) {
                        let rev = app.session.revision;
                        app.home.show(rev);
                    }
                })
                .then("starting.recovery", move |a: &mut App, _ctx| {
                    solidcraft_engine::crash::phase("recovery");
                    let app = &mut a.solve;
                    // Autosave, and offer what crashed sessions left behind.
                    if let Some(dir) = recovery_dir.as_ref() {
                        let every = if app.autosave_minutes > 0.0 { app.autosave_minutes * 60.0 } else { f64::MAX / 4.0 };
                        solidcraft_ui_egui::recovery_ui::start(app, dir, std::time::Duration::from_secs_f64(every.min(1e9)));
                        // Safe mode keeps autosaving but offers nothing: the entries wait for a normal start.
                        if safe_mode {
                            app.recoverable.clear();
                        }
                    }
                    // Tell the user about crashes since the last start (after the recovery offer is set up).
                    solidcraft_ui_egui::crash_ui::announce(app, &crashed);
                    if !notes.is_empty() {
                        app.set_status(notes.join(" "), true);
                    }
                    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                    {
                        a.space_mouse = Some(space_mouse::SpaceMouse::start(_ctx, a.nav_bridge.claim()));
                    }
                    #[cfg(target_os = "macos")]
                    {
                        let app = &mut a.solve;
                        let native_menu = match native_menu::NativeMenu::new(app, _ctx) {
                            Ok(menu) => Some(menu),
                            Err(error) => {
                                app.set_status(format!("System menu: {error}"), true);
                                None
                            }
                        };
                        app.native_file_menu = native_menu.is_some();
                        a.native_menu = native_menu;
                    }
                    mark("app created");
                    solidcraft_engine::crash::phase("first frame");
                });
            Ok(Box::new(App {
                // Before `app` moves: the settings as read, already on disk.
                prefs_writer: PrefsWriter::read(app.prefs()),
                solve: app,
                setup,
                gpu_job: None,
                starting,
                backend_attempt: attempt_in_app,
                backend_guard: guard_in_app,
                safe_graphics_record: graphics_path_in_app,
                full_graphics_up: None,
                window_frames: 0,
                frames: 0,
                up: false,
                take_focus_unwanted: control_port.is_some(),
                come_forward: come_forward::ComeForward::default(),
                gpu,
                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                space_mouse: None,
                #[cfg(target_os = "macos")]
                native_menu: None,
                nav_bridge,
                dialog_folder,
                ctx: cc.egui_ctx.clone(),
                #[cfg(target_os = "windows")]
                window_move,
            }))
        }),
    );
    control_server::remove_token_file();
    single_instance::stop();
    // Startup returned: whatever happened, no driver crashed the process.
    if let Some(guard) = &backend_guard {
        guard.clear();
    }
    match result {
        Err(e) if !created.load(std::sync::atomic::Ordering::SeqCst) => {
            no_window(e, cpu_renderer, safe, started, skipped.crashed | skipped.failed, settings.as_deref(), backend_attempt.as_ref())
        }
        r => r,
    }
}

/// What the 3D view's setup step needs from the window's graphics device.
struct GpuSetup {
    rs: eframe::egui_wgpu::RenderState,
    /// The scene's depth buffer bits (a float one where every adapter has it).
    depth_bits: u8,
    /// The anti-aliasing the settings ask for.
    samples: u16,
    /// The sample counts the device draws the scene with.
    supported: Vec<u32>,
    /// The scene's sample count, from both.
    scene_samples: u32,
}

/// The 3D view's setup (#1210): the shader compile runs on a [`Worker`] beside the window, whose
/// start page is usable meanwhile; a design opened before it is done says in its view that the
/// view is being prepared. The pipelines go in use the frame after it is done. `true` once it is
/// over: the view has its pipelines, or says it can't draw (the compile failed).
///
/// [`Worker`]: solidcraft_ui_egui::starting::Worker
fn view_step(a: &mut App, ctx: &egui::Context, gpu: &GpuSetup) -> bool {
    use solidcraft_ui_egui::starting::{Poll, Worker};
    let gpu_meshing = !a.solve.cpu_meshing;
    let (scene_samples, depth_bits) = (gpu.scene_samples, gpu.depth_bits);
    let build = move |rs: &eframe::egui_wgpu::RenderState| {
        let built = SolveApp::build_wgpu_split(rs, depth_bits, STENCIL_BITS, scene_samples, gpu_meshing);
        solidcraft_ui_egui::gpu::install_picture_renderer(rs);
        built
    };
    let Some(job) = &a.gpu_job else {
        solidcraft_engine::note!("SolidCraft graphics: {:?}, MSAA {scene_samples}x (requested {}x)", gpu.rs.adapter.get_info().backend, gpu.samples);
        solidcraft_engine::crash::phase("viewport pipelines (shader compile)");
        // Home's pictures, drawn off the window thread, wait for the renderer the compile installs.
        solidcraft_engine::capture::coming(true);
        a.solve.viewport.gpu_preparing = true;
        let rs = gpu.rs.clone();
        match Worker::spawn("viewport pipelines", ctx.clone(), move || build(&rs)) {
            Ok(job) => a.gpu_job = Some(job),
            Err(e) => {
                log::warn!("viewport pipelines: no thread for them ({e}); compiling on the window's thread");
                gpu_ready(a, gpu, build(&gpu.rs));
                return true;
            }
        }
        return false;
    };
    match job.poll() {
        Poll::Running => return false,
        Poll::Done(built) => gpu_ready(a, gpu, built),
        // The compile panicked (its crash report is written): the view says it can't draw, rather
        // than staying empty.
        Poll::Failed => {
            a.solve.gpu_unavailable();
            solidcraft_engine::capture::coming(false);
        }
    }
    a.gpu_job = None;
    mark("viewport pipelines");
    true
}

/// The viewport's pipelines, compiled beside the window, in use.
fn gpu_ready(a: &mut App, gpu: &GpuSetup, built: solidcraft_ui_egui::WgpuBuilt) {
    let app = &mut a.solve;
    app.adopt_wgpu(&gpu.rs, built);
    let scene_samples = app.viewport.gpu.map_or(gpu.scene_samples, |g| g.samples);
    app.msaa_supported = gpu.supported.clone();
    app.preferences.msaa = u32::from(gpu.samples);
    // The graphics driver is loaded now; it may have put its own exception filter in front of ours.
    #[cfg(target_os = "windows")]
    native_crash::reinstall();
    // Reflect a confirmed probe fallback or an unsupported saved count in Graphics.
    solidcraft_ui_egui::prefs::check_msaa(app, u32::from(gpu.samples));
    let info = gpu.rs.adapter.get_info();
    solidcraft_engine::crash::set_environment(&format!("Renderer: {:?} {} ({}), MSAA {scene_samples}x", info.backend, info.name, info.driver));
}

/// egui-wgpu is about to start a step of getting the graphics device and the window's surface up:
/// the crash journal keeps it until the app runs (a window opened later goes through the same
/// steps), and the adapter once it is picked.
fn startup_step(step: &'static str, detail: &str) {
    if step == eframe::egui_wgpu::STEP_ADAPTER_CHOSEN {
        solidcraft_engine::crash::set_environment(&format!("Renderer: {detail}"));
    }
    solidcraft_engine::crash::startup_step(step);
}

/// The renderer settings: with `cpu`, the software adapter (WARP, lavapipe, llvmpipe). The
/// backends come from `startup::choose_backends` (on Windows Vulkan first, and only the backend
/// in use is started); `preference` is `graphics.json`'s `backend`. `skipped`: the APIs that
/// crashed while starting or whose window failed to start; `guard` names each API on disk before
/// it is probed.
fn wgpu_options(
    cpu: bool,
    preference: Option<&str>,
    skipped: startup::SkippedBackends,
    guard: Option<&startup::BackendGuard>,
) -> eframe::egui_wgpu::WgpuConfiguration {
    // One frame in flight, as eframe's own default (`NativeOptions::default`): egui-wgpu's
    // `WgpuConfiguration::default()` is HIGH_THROUGHPUT (two frames queued), which added a frame
    // of input-to-display latency to every pan, orbit and drag.
    let mut options = eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY);
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        let env_set = std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| !v.trim().is_empty());
        // A retry applies only to this launch. WGPU_BACKEND always has priority.
        let retry = std::env::var(startup::RETRY_BACKEND_ENV).ok();
        let preference = if cpu { preference } else { retry.as_deref().or(preference) };
        // Each API probed is named on disk first: a driver that crashes in the probe is caught too.
        let probe = |backends| {
            if let Some(guard) = guard {
                guard.attempt(backends);
            }
            startup::has_hardware_adapter(backends)
        };
        if let Some(backends) =
            startup::choose_backends(cfg!(target_os = "windows"), env_set, cpu, preference, skipped.crashed, skipped.failed, probe)
        {
            solidcraft_engine::note!("SolidCraft graphics: starting {backends:?}");
            setup.instance_descriptor.backends = backends;
        }
        let intel_mac = cfg!(all(target_os = "macos", target_arch = "x86_64"));
        let indirect = std::env::var(startup::INDIRECT_VALIDATION_ENV).ok();
        setup.instance_descriptor.flags = startup::window_instance_flags(setup.instance_descriptor.flags, intel_mac, indirect.as_deref());
        // Never a `dxcompiler.dll` another program put on PATH (#6756).
        setup.instance_descriptor.backend_options.dx12.shader_compiler = solidcraft_viewport_gpu::dx12_compiler();
        let environment = std::env::var("WGPU_ADAPTER_NAME").ok();
        let adapter = graphics().and_then(|v| v.get("adapter").and_then(serde_json::Value::as_str).map(str::to_string));
        startup::configure_adapter(setup, cpu, environment, adapter);
        // A float depth buffer with a stencil (see `FLOAT_DEPTH_BITS`), indirect draws that start
        // at an instance (instanced meshes drawn a call per pipeline), and timestamp queries for
        // the GPU time the frame-rate readout shows, wherever the adapter has them.
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            d.required_features |=
                adapter.features() & (startup::FLOAT_DEPTH_STENCIL | solidcraft_viewport_gpu::INDIRECT_DRAWS | startup::TIMESTAMP_QUERY);
            d
        });
    }
    options
}

/// Safe graphics mode's renderer settings (`startup::SafeGraphics`): wgpu's own backend and adapter
/// choice, as v0.1.0 had, with no API order, no retries, no adapter preference and no float depth.
/// After a failure, the API the failed launch used is left out (`failed`, [`startup::safe_backends`]),
/// and so is every API that crashed while starting (`crashed`), the software renderer's included.
/// Kept: one frame in flight (eframe's own default), the D3D12 shader compiler that never loads a
/// `dxcompiler.dll` from PATH (#6756), and the Intel Mac validation fix (#3080); with `cpu`, the
/// software adapter.
fn safe_wgpu_options(cpu: bool, failed: Option<&str>, crashed: eframe::wgpu::Backends) -> eframe::egui_wgpu::WgpuConfiguration {
    let mut options = eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY);
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        let intel_mac = cfg!(all(target_os = "macos", target_arch = "x86_64"));
        let indirect = std::env::var(startup::INDIRECT_VALIDATION_ENV).ok();
        setup.instance_descriptor.flags = startup::window_instance_flags(setup.instance_descriptor.flags, intel_mac, indirect.as_deref());
        setup.instance_descriptor.backend_options.dx12.shader_compiler = solidcraft_viewport_gpu::dx12_compiler();
        if cpu {
            startup::configure_adapter(setup, true, None, None);
        }
        if std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| v.trim().is_empty()) || std::env::var_os(startup::WGPU_BACKEND_ENV).is_none() {
            // WGPU_BACKEND always has priority; otherwise the failed API (not for the software
            // renderer, which picks its own adapter) and the crashed ones are left out.
            let failed = if cpu { None } else { failed };
            setup.instance_descriptor.backends = startup::safe_backends(setup.instance_descriptor.backends, failed, crashed);
        }
    }
    solidcraft_engine::note!("SolidCraft graphics: safe graphics mode, wgpu's own backend choice");
    options
}

/// A failed renderer returns here before app creation. A fresh process avoids reusing winit's
/// event loop or a failed device; each retry advances through the APIs, then software once.
fn no_window(
    error: eframe::Error,
    tried_cpu: bool,
    safe_graphics: bool,
    failed: eframe::wgpu::Backends,
    skipped: eframe::wgpu::Backends,
    settings: Option<&std::path::Path>,
    backend_attempt: Option<&(std::path::PathBuf, startup::BackendAttempt)>,
) -> eframe::Result {
    log::error!("the window could not open: {error}");
    solidcraft_engine::note!("SolidCraft: the window could not open: {error}");
    // This failure was handled, so neither the marker nor the journal should call it a crash.
    if let Some(dir) = settings {
        startup::started(dir);
    }
    solidcraft_engine::crash::end_session();
    // A rejected UI choice goes back to its previous API before ordinary fallback.
    // This is separate from startup::started: a handled failure is not confirmation.
    if let Some((path, attempt)) = backend_attempt {
        match startup::rollback_backend_start(path, attempt) {
            Ok(()) => {
                if let Ok(exe) = std::env::current_exe() {
                    let note = startup::backend_reverted_note(&attempt.requested, &attempt.previous);
                    let mut command = std::process::Command::new(exe);
                    command.args(std::env::args_os().skip(1)).env_remove(startup::RETRY_BACKEND_ENV).env(startup::FALLBACK_NOTE_ENV, note);
                    match command.status() {
                        Ok(status) if status.success() => return Ok(()),
                        Ok(_) => return Err(error),
                        Err(reason) => log::error!("could not restart with the previous graphics API: {reason}"),
                    }
                }
            }
            Err(reason) => log::warn!("could not restore the previous graphics API: {reason}"),
        }
    }
    let explicit = std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| !v.trim().is_empty());
    // An API whose graphics setup failed (a device that would not start) is skipped by the
    // automatic order from now on, so the next launch of this version starts on the API that
    // works without failing on this one first. Window-system errors are not the API's fault.
    if matches!(error, eframe::Error::Wgpu(_))
        && !explicit
        && !tried_cpu
        && !safe_graphics
        && startup::backend_name(failed).is_some()
        && let Some(dir) = settings
    {
        startup::record_failed_backend(&dir.join("graphics.json"), failed, solidcraft_ui_egui::help::VERSION);
    }
    // Safe graphics tries no other graphics API, only the software renderer as the last resort.
    let next = if safe_graphics && !explicit && !tried_cpu {
        Some("cpu")
    } else {
        startup::fallback_backend(cfg!(target_os = "windows"), explicit, tried_cpu, failed, skipped)
    };
    if let Some(next) = next
        && let Ok(exe) = std::env::current_exe()
    {
        let previous = std::env::var(startup::FALLBACK_NOTE_ENV).unwrap_or_default();
        let note = startup::fallback_note(&previous, failed, next, &error.to_string());
        log::warn!("{note}");
        solidcraft_engine::note!("SolidCraft: {note}");
        let mut command = std::process::Command::new(exe);
        command.args(std::env::args_os().skip(1)).env(startup::FALLBACK_NOTE_ENV, note);
        if next == "cpu" {
            command.env(startup::RENDERER_ENV, "cpu").env_remove(startup::RETRY_BACKEND_ENV);
        } else {
            command.env(startup::RETRY_BACKEND_ENV, next);
        }
        match command.status() {
            Ok(status) if status.success() => return Ok(()),
            // The final try showed its own message.
            Ok(_) => return Err(error),
            Err(e) => log::error!("could not start again on {next}: {e}"),
        }
    }
    let message = if explicit {
        format!("SolidCraft could not open its window with WGPU_BACKEND.\n\n{error}\n\nChange or remove WGPU_BACKEND to try another graphics API.")
    } else {
        startup::no_window_message(&error.to_string(), settings)
    };
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("SolidCraft could not start")
        .set_description(message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    Err(error)
}

/// Linux: the window system this run opened with (`wayland` or `x11`), for Preferences (#3083).
#[cfg(all(unix, not(target_os = "macos")))]
static WINDOW_SYSTEM: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// The window system this run uses, where there is a choice (Linux).
fn window_system_now() -> Option<&'static str> {
    #[cfg(all(unix, not(target_os = "macos")))]
    return WINDOW_SYSTEM.get().copied();
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    None
}

/// Preferences › Graphics › Window system (`graphics.json`'s `window_system`), read before the
/// window opens.
#[cfg(all(unix, not(target_os = "macos")))]
fn window_system_preference() -> Option<String> {
    graphics().and_then(|v| v.get("window_system").and_then(serde_json::Value::as_str).map(str::to_string))
}

/// The window system preference as the `SOLIDCRAFT_WAYLAND` value it stands for: `wayland` is
/// `1`, `x11` is `0`, and `auto` (or anything else) leaves the choice to [`prefer_x11`].
#[cfg(all(unix, not(target_os = "macos")))]
fn window_system_override(preference: Option<&str>) -> Option<&'static str> {
    match preference.map(str::trim) {
        Some("wayland") => Some("1"),
        Some("x11") => Some("0"),
        _ => None,
    }
}

/// Whether to run through XWayland rather than native Wayland. winit has no file drag-and-drop
/// on Wayland (only on X11), so dropping files from the file manager showed a "no" cursor: use
/// XWayland when it's there. Except on compositors where XWayland is a worse default: niri
/// (xwayland-satellite breaks maximize/restore) and Hyprland (fractional scale with
/// `force_zero_scaling` plus `GDK_SCALE` makes the UI ~2.5× oversized, #10480). Those stay on
/// native Wayland and give up file drops. `SOLIDCRAFT_WAYLAND=1` keeps native Wayland,
/// `SOLIDCRAFT_WAYLAND=0` forces XWayland.
#[cfg(all(unix, not(target_os = "macos")))]
fn prefer_x11(display: bool, wayland_display: bool, desktop: Option<&str>, wayland_override: Option<&str>) -> bool {
    match wayland_override.map(str::trim) {
        Some("0") => return display,
        Some(_) => return false,
        None => {}
    }
    display && !(native_wayland_desktop(desktop) && wayland_display)
}

/// Desktops that keep Automatic on native Wayland when a Wayland socket is present.
#[cfg(all(unix, not(target_os = "macos")))]
fn native_wayland_desktop(desktop: Option<&str>) -> bool {
    desktop.is_some_and(|d| {
        d.split(':').any(|name| {
            let name = name.trim();
            name.eq_ignore_ascii_case("niri") || name.eq_ignore_ascii_case("Hyprland")
        })
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
const CHOOSER_NO_SESSION_BUS: &str = "file.open.portal_fallback.no_session_bus";
#[cfg(all(unix, not(target_os = "macos")))]
const CHOOSER_NO_PORTAL_SERVICE: &str = "file.open.portal_fallback.no_portal_service";
#[cfg(all(unix, not(target_os = "macos")))]
const CHOOSER_XWAYLAND: &str = "file.open.portal_fallback.xwayland";

/// Whether the chooser that opens is NOT the desktop's own, and why.
///
/// `rfd` opens the file chooser through the XDG desktop portal; when the portal is not there it
/// falls back to zenity, which opens a window that does not belong to SolidCraft and that the
/// application never mentions. The application has to know that: this decision is what the warning
/// is built on.
///
/// * `dbus_session`: the session bus is there (`DBUS_SESSION_BUS_ADDRESS`).
/// * `portal_service`: the file chooser portal answers on it.
/// * `wayland_display`: a Wayland session runs (`WAYLAND_DISPLAY`).
/// * `x11_display`: the window is an X11 one (`DISPLAY`, or XWayland).
///
/// `None` means the desktop's own chooser is the one that opens and there is nothing to say;
/// `Some` carries the reason to report.
#[cfg(all(unix, not(target_os = "macos")))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_chooser_fallback(dbus_session: bool, portal_service: bool, wayland_display: bool, x11_display: bool) -> Option<&'static str> {
    // Without a session bus there is no bus to reach the portal on, so the chooser is not the
    // desktop's own whatever else is there.
    if !dbus_session {
        return Some(CHOOSER_NO_SESSION_BUS);
    }
    // The bus is there but the portal does not answer on it: `rfd` falls back to zenity.
    if !portal_service {
        return Some(CHOOSER_NO_PORTAL_SERVICE);
    }
    // Under XWayland the window is an X11 one while the session is a Wayland one, and the portal
    // refuses to parent its chooser to an X11 window: `rfd` falls back to zenity.
    if wayland_display && x11_display {
        return Some(CHOOSER_XWAYLAND);
    }
    None
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod backend_tests {
    use super::prefer_x11;

    #[test]
    fn xwayland_by_default_native_wayland_on_niri_hyprland_and_the_override_wins() {
        // XWayland whenever there's an X display, for file drag-and-drop.
        assert!(prefer_x11(true, true, Some("GNOME"), None));
        assert!(prefer_x11(true, false, None, None));
        assert!(!prefer_x11(false, true, Some("KDE"), None));
        // niri / Hyprland (alone or in a desktop list) under Wayland stays native.
        assert!(!prefer_x11(true, true, Some("niri"), None));
        assert!(!prefer_x11(true, true, Some("niri:GNOME"), None));
        assert!(!prefer_x11(true, true, Some("Hyprland"), None));
        assert!(!prefer_x11(true, true, Some("hyprland"), None));
        // Without a Wayland socket (nested X session) still uses X11.
        assert!(prefer_x11(true, false, Some("niri"), None));
        assert!(prefer_x11(true, false, Some("Hyprland"), None));
        // SOLIDCRAFT_WAYLAND: anything but 0 keeps Wayland, 0 forces XWayland (when present).
        assert!(!prefer_x11(true, true, Some("GNOME"), Some("1")));
        assert!(!prefer_x11(true, true, Some("GNOME"), Some("")));
        assert!(prefer_x11(true, true, Some("niri"), Some("0")));
        assert!(prefer_x11(true, true, Some("Hyprland"), Some("0")));
        assert!(!prefer_x11(false, true, Some("niri"), Some("0")));
    }

    /// Preferences › Graphics › Window system stands for the same values (#3083).
    #[test]
    fn the_window_system_preference_maps_to_the_override() {
        use super::window_system_override as over;
        assert_eq!(
            (over(Some("wayland")), over(Some("x11")), over(Some("auto")), over(None), over(Some("junk"))),
            (Some("1"), Some("0"), None, None, None)
        );
        // Wayland chosen on GNOME keeps native Wayland; X11 chosen on niri goes through XWayland.
        assert!(!prefer_x11(true, true, Some("GNOME"), over(Some("wayland"))));
        assert!(prefer_x11(true, true, Some("niri"), over(Some("x11"))));
    }

    /// The file chooser must say when it is NOT the desktop's own: `rfd` falls back to zenity
    /// when the XDG portal is not there, and the user gets a foreign window with no explanation.
    #[test]
    fn the_file_picker_says_when_it_is_not_the_desktops_own_chooser() {
        use super::desktop_chooser_fallback;
        // The normal case: the portal is there, the desktop's own chooser opens, nothing to say.
        assert_eq!(desktop_chooser_fallback(true, true, true, false), None);
        // No session bus: the portal can not be reached at all, so `rfd` falls back to zenity.
        assert!(
            desktop_chooser_fallback(false, true, true, false).is_some(),
            "without a D-Bus session bus the file chooser is not the desktop's own: the user has to be told"
        );
        // The bus is there but the portal service is not: same fallback, same warning.
        assert!(
            desktop_chooser_fallback(true, false, true, false).is_some(),
            "without the desktop portal service the file chooser is not the desktop's own: the user has to be told"
        );
        // XWayland: the parent window is an X11 one and the portal refuses it, so `rfd` falls back.
        assert!(
            desktop_chooser_fallback(true, true, true, true).is_some(),
            "under XWayland the portal rejects the parent window: the user has to be told"
        );
    }

    #[test]
    fn the_desktops_title_bar_only_when_asked_for() {
        use super::titlebar_choice;
        // Nothing set: the application bar is the title bar, as before.
        assert!(titlebar_choice(None, None));
        // graphics.json's `titlebar`.
        assert!(!titlebar_choice(None, Some("system")));
        assert!(titlebar_choice(None, Some("app")));
        // SOLIDCRAFT_TITLEBAR wins over the file; empty counts as unset; case and spaces don't matter.
        assert!(!titlebar_choice(Some(" System "), Some("app")));
        assert!(titlebar_choice(Some("app"), Some("system")));
        assert!(!titlebar_choice(Some(""), Some("system")));
        // Anything unknown keeps the application bar.
        assert!(titlebar_choice(Some("ssd"), None));
    }
}

#[cfg(test)]
mod tickets_http_tests {
    use super::tickets_call;
    use solidcraft_ui_egui::agent_chat::backend::HttpRequest;
    use std::io::{Read, Write};

    /// A server that answers without a length and keeps the connection open unless the request
    /// asked to close it: the reply ends only when the connection does (#17932).
    #[test]
    fn a_ticket_reply_without_a_length_ends_without_waiting_for_the_timeout() {
        let Ok(listener) = std::net::TcpListener::bind("127.0.0.1:0") else { return };
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // The whole request, body too: a connection closed with part of the request unread is
            // reset, and the client then gets an error instead of the reply (os error 22 on macOS)
            // whenever the body arrives after the headers were read.
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            let whole = |request: &[u8]| {
                let Some(head) = request.windows(4).position(|w| w == b"\r\n\r\n") else { return false };
                let headers = String::from_utf8_lossy(&request[..head]).to_ascii_lowercase();
                let length = headers.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok());
                request.len() >= head + 4 + length.unwrap_or(0)
            };
            while !whole(&request) {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0, "the request ended early");
                request.extend_from_slice(&buf[..n]);
            }
            let reply = b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\n\r\n{\"id\":\"up1\"}";
            stream.write_all(reply).unwrap();
            let close = String::from_utf8_lossy(&request).to_ascii_lowercase().contains("connection: close");
            if !close {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        });
        let request = HttpRequest {
            method: "POST",
            url: format!("http://127.0.0.1:{port}/api/uploads"),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(b"{}".to_vec()),
            timeout_s: 3,
        };
        let started = std::time::Instant::now();
        let reply = tickets_call(&request);
        assert_eq!(reply, Ok((201, br#"{"id":"up1"}"#.to_vec())), "after {:?}", started.elapsed());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "the reply ended with the connection, not the timeout");
        server.join().unwrap();
    }
}

#[cfg(test)]
mod prefs_writer_tests {
    /// In-process MCP-to-UI integration, without opening a native window or socket session.
    #[test]
    fn app_management_mcp_tools_reach_the_real_ui_without_dialogs() {
        use serde_json::{Value, json};
        struct Desktop(solidcraft_ui_egui::scenario::Harness);
        impl solidcraft_mcp::Backend for Desktop {
            fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
                let reply = self.0.call(method, params);
                if reply["ok"] == true {
                    Ok(reply["result"].clone())
                } else {
                    Err(reply["error"].as_str().unwrap_or("unexpected control reply").into())
                }
            }
            fn has_ui(&self) -> bool {
                true
            }
            fn describe(&self) -> String {
                "in-process desktop UI harness".into()
            }
        }
        fn call(b: &mut Desktop, tool: &str, args: Value, fails: bool) -> Value {
            let reply = solidcraft_mcp::call_tool(b, tool, &args);
            assert_eq!(reply["isError"] == true, fails, "{tool}: {reply}");
            assert!(b.0.app.docs.closing.is_none() && b.0.app.docs.quitting.is_none());
            assert!(!b.0.app.prefs_window.shell.open);
            reply
        }
        let mut b = Desktop(solidcraft_ui_egui::scenario::Harness::new());
        b.0.app.run("parameters.add", json!({"name": "width", "expression": "12 mm"})).unwrap();
        let original = b.0.app.session.document_id.clone();
        call(&mut b, "documents", json!({"action": "new"}), false);
        assert_eq!(b.0.app.docs.count(), 2);
        call(&mut b, "documents", json!({"action": "switch", "index": 0}), false);
        assert_eq!(b.0.app.session.document_id, original);
        call(&mut b, "documents", json!({"action": "close"}), true);
        call(&mut b, "quit_app", json!({}), true);
        assert!(!b.0.app.quit_requested && b.0.app.session.is_dirty());
        let before = b.0.app.preferences.theme.clone();
        call(&mut b, "preferences", json!({"open": "General", "theme": "light"}), true);
        assert_eq!(b.0.app.preferences.theme, before);
        call(&mut b, "preferences", json!({"theme": "light", "default_units": "cm"}), false);
        assert_eq!(b.0.app.preferences.theme, "light");
        let prefs = call(&mut b, "preferences", json!({}), false);
        assert!(prefs["content"][0]["text"].as_str().unwrap().contains("cm"));
        call(&mut b, "agent_cursor", json!({"show": true, "speed": "instant", "follow_camera": true}), false);
        assert!(b.0.app.ui.agent_cursor.show && b.0.app.ui.agent_cursor.follow_camera);
        assert_eq!(b.0.call("ui.agent_cursor", json!({}))["result"]["speed"], "instant");
        if let Ok(dir) = std::env::var("SOLIDCRAFT_APP_TOOLS_EVIDENCE") {
            // Real egui UI paint, not a native-window or geometry-rendering claim.
            let ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_bundled_fonts(&ctx);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1000.0));
            let mut textures = std::collections::HashMap::new();
            let mut shapes = Vec::new();
            for _ in 0..4 {
                let raw = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
                let output = ctx.run_ui(raw, |ui| {
                    b.0.app.logic(ui.ctx());
                    b.0.app.ui(ui);
                });
                solidcraft_ui_kit::test_textures_update(&mut textures, &output.textures_delta);
                shapes = output.shapes;
            }
            std::fs::create_dir_all(&dir).unwrap();
            solidcraft_ui_kit::test_raster(&ctx, shapes, 1.0, screen, &textures)
                .save(std::path::Path::new(&dir).join("app-tools-no-dialog.png"))
                .unwrap();
        }
        call(&mut b, "documents", json!({"action": "close", "force": true}), false);
        assert_eq!(b.0.app.docs.count(), 1);
        assert_ne!(b.0.app.session.document_id, original);
        call(&mut b, "quit_app", json!({}), false);
        assert!(b.0.app.quit_requested);
    }

    use super::PrefsWriter;
    use solidcraft_ui_egui::prefs::{NavigatorStyle, STORAGE_KEY};
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    /// eframe's settings file: what was flushed to disk is what the next start reads.
    #[derive(Default)]
    struct Disk {
        pending: HashMap<String, String>,
        flushed: HashMap<String, String>,
        flushes: usize,
    }

    impl eframe::Storage for Disk {
        fn get_string(&self, key: &str) -> Option<String> {
            self.pending.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.pending.insert(key.into(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.pending.remove(key);
        }
        fn flush(&mut self) {
            self.flushed = self.pending.clone();
            self.flushes += 1;
        }
    }

    fn new_app() -> solidcraft_ui_egui::SolveApp {
        solidcraft_ui_egui::SolveApp::new(solidcraft_engine::Session::default(), solidcraft_ui_egui::Services::default())
    }

    /// #8275: Preferences' View navigator and Display quality, changed and confirmed, are on disk
    /// in that frame, so the next start has them even when this one never closes normally (ended
    /// from the Task Manager, a sign-out, a hang or crash in the graphics driver on the way out).
    #[test]
    fn changed_settings_reach_the_disk_in_the_frame_they_change() {
        let mut app = new_app();
        let mut disk = Disk::default();
        let mut writer = PrefsWriter::read(app.prefs());
        let now = Instant::now();
        assert_eq!(writer.store(&mut disk, app.prefs(), now), None);
        assert_eq!(disk.flushes, 0, "the settings read at start are not written again");
        app.preferences.navigator_style = NavigatorStyle::Custom;
        app.preferences.display_quality = "fine".into();
        assert!(writer.due(true, now), "a frame with input is checked");
        assert_eq!(writer.store(&mut disk, app.prefs(), now), None);
        assert_eq!(disk.flushes, 1, "flushed at once, not left for eframe's next save");
        assert_eq!(writer.store(&mut disk, app.prefs(), now + Duration::from_secs(5)), None);
        assert_eq!(disk.flushes, 1, "unchanged settings are not written again");

        // The next start reads them.
        let mut next = new_app();
        let stored = solidcraft_ui_egui::prefs::stored(|k| disk.flushed.get(k).cloned()).expect("settings on disk");
        next.load_prefs(&stored);
        assert_eq!(next.preferences.navigator_style, NavigatorStyle::Custom);
        assert_eq!(next.preferences.display_quality, "fine");
        assert!(disk.flushed.contains_key(STORAGE_KEY));
    }

    /// A frame without input is checked only right after one with input (whose clicks the UI
    /// handled in that frame); after that, idle frames cost nothing. Pointer motion, which comes
    /// every frame while the mouse moves, doesn't count as input.
    #[test]
    fn frames_are_checked_with_input_and_once_after_it() {
        let now = Instant::now();
        let mut writer = PrefsWriter::default();
        assert!(!writer.due(false, now));
        assert!(writer.due(true, now));
        assert!(writer.due(false, now), "the frame after input");
        assert!(!writer.due(false, now));

        let click = egui::Event::PointerButton {
            pos: egui::Pos2::ZERO,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert!(super::changes_settings(&click));
        assert!(super::changes_settings(&egui::Event::Text("5".into())));
        assert!(!super::changes_settings(&egui::Event::PointerMoved(egui::Pos2::ZERO)));
        assert!(!super::changes_settings(&egui::Event::MouseMoved(egui::Vec2::X)));
    }

    /// A window position in physical pixels is egui's points at the window's scale; a scale or
    /// position that is not a usable number answers nothing, so eframe's repaint stands.
    #[test]
    fn pointer_motion_is_read_in_points() {
        assert_eq!(super::pointer_in_points(300.0, 150.0, 2.0), Some(egui::pos2(150.0, 75.0)));
        assert_eq!(super::pointer_in_points(10.0, 20.0, 1.0), Some(egui::pos2(10.0, 20.0)));
        assert_eq!(super::pointer_in_points(10.0, 20.0, 0.0), None);
        assert_eq!(super::pointer_in_points(f64::NAN, 20.0, 1.0), None);
        assert_eq!(super::pointer_in_points(1e300, 20.0, 1.0), None);
    }

    /// A setting that changes in every frame (for example while a slider is dragged) doesn't
    /// write the file in every frame: at most once a second, and the last change is on disk at
    /// most a second after it was made, with a frame asked for then so an idle app writes it.
    #[test]
    fn a_continuous_change_is_written_once_a_second_and_its_end_soon_after() {
        let mut app = new_app();
        let mut disk = Disk::default();
        let mut writer = PrefsWriter::read(app.prefs());
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        let mut asked = None;
        // 1.5 s of dragging at 60 frames a second, with input in every frame.
        for n in 0..94u32 {
            let now = start + frame * n;
            app.preferences.ui_scale = 1.25 + n as f32 / 100.0;
            assert!(writer.due(true, now));
            asked = writer.store(&mut disk, app.prefs(), now);
        }
        assert_eq!(disk.flushes, 2, "the first change at once, then one a second later");
        let wait = asked.expect("the last value waits, and a frame is asked for when it can be written");
        assert!(wait <= PrefsWriter::INTERVAL);

        // The drag ends: no more input. Frames before the wait is over don't even look.
        let end = start + frame * 93;
        assert!(writer.due(false, end + frame), "the frame right after input");
        assert!(!writer.due(false, end + frame * 2), "an idle frame before the wait is over");
        // The frame asked for writes the last value.
        let then = end + wait;
        assert!(writer.due(false, then));
        assert_eq!(writer.store(&mut disk, app.prefs(), then), None);
        assert_eq!(disk.flushes, 3);
        let mut next = new_app();
        let stored = solidcraft_ui_egui::prefs::stored(|k| disk.flushed.get(k).cloned()).expect("settings on disk");
        next.load_prefs(&stored);
        assert_eq!(next.preferences.ui_scale, 1.25 + 93.0 / 100.0, "the value the drag ended at");
        assert!(!writer.due(false, then + frame), "nothing waits any more");
    }
}

#[cfg(test)]
mod dialog_folder_tests {
    use super::DialogFolder;
    use std::path::{Path, PathBuf};

    /// #7756: a file dialog opens where the last one left off, not in the home folder.
    #[test]
    fn the_next_dialog_opens_in_the_folder_of_the_file_chosen_last() {
        let folder = DialogFolder::default();
        assert_eq!(folder.folder(), None, "before any choice the system decides");
        folder.chosen(Path::new("/work/prints/bracket.stl"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/prints")));
        // A bare file name has no folder to remember.
        folder.chosen(Path::new("bracket.step"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/prints")));
    }

    /// A folder picker remembers the folder itself. Remembering its parent would open the next
    /// dialog one level above the folder the user chose.
    #[test]
    fn a_folder_picker_remembers_the_picked_folder_itself() {
        let folder = DialogFolder::default();
        folder.chosen(Path::new("/work/project/assets"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/project")), "a file's parent");
        folder.chosen_folder(Path::new("/work/project/assets"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/project/assets")));
        folder.chosen_folder(Path::new(""));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/project/assets")), "an empty path leaves the last folder");
    }

    /// Save As starts beside the open design (#7756), and a folder chosen since wins until another
    /// design is opened or saved.
    #[test]
    fn the_open_design_moves_the_folder_only_when_it_changes() {
        let folder = DialogFolder::default();
        folder.design(Some("/work/lid/lid.solidcraft"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/lid")));
        folder.chosen(Path::new("/exports/lid.3mf"));
        folder.design(Some("/work/lid/lid.solidcraft"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/exports")), "the same design again leaves the export folder");
        folder.design(None);
        assert_eq!(folder.folder(), Some(PathBuf::from("/exports")), "a new, unsaved design has no folder of its own");
        folder.design(Some("/work/base/base.solidcraft"));
        assert_eq!(folder.folder(), Some(PathBuf::from("/work/base")));
    }
}

#[cfg(test)]
mod viewport_tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_preserves_the_bundle_icon_instead_of_setting_a_runtime_bitmap() {
        let viewport = super::app_viewport(super::CUSTOM_TITLEBAR).expect("viewport");
        assert_eq!(viewport.icon.as_deref(), Some(&eframe::egui::IconData::default()));
        assert_eq!(viewport.app_id.as_deref(), Some(super::APP_ID));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn desktop_viewport_supplies_the_original_mark_to_the_window() {
        let viewport = super::app_viewport(super::CUSTOM_TITLEBAR).expect("viewport");
        let icon = viewport.icon.expect("running window must have an icon");
        assert_eq!((icon.width, icon.height), (64, 64));
        assert_eq!(icon.rgba, super::app_icon::rgba(64).expect("original mark"));
        assert_eq!(viewport.app_id.as_deref(), Some(super::APP_ID));
    }

    /// Full screen keeps the application bar at the top of the window, with no strip reserved
    /// above it, and Undo and Redo in the bar keep working (#3643).
    #[test]
    fn fullscreen_bar_starts_at_the_top_and_undo_redo_keep_working() {
        use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportId, vec2};
        use serde_json::json;
        use solidcraft_ui_egui::scenario::handle_at;

        for (zoom, retina) in [(1.0, 1.0), (1.0, 2.0), (1.5, 2.0), (2.0, 2.0)] {
            let ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_bundled_fonts(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut app = super::SolveApp::new(super::Session::default(), Default::default());
            app.integrated_titlebar = true;
            app.preferences.language = "en".into();
            for (command, params) in [
                ("sketch.create", json!({"plane":"XY"})),
                ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[40,30]})),
                ("sketch.finish", json!({})),
                ("solid.extrude", json!({"distance":15})),
            ] {
                app.run(command, params).unwrap();
            }
            let mut time = 0.0;
            let mut frame = |app: &mut super::SolveApp, fullscreen, events| {
                time += 0.05;
                let mut raw = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                };
                let viewport = raw.viewports.get_mut(&ViewportId::ROOT).unwrap();
                viewport.fullscreen = Some(fullscreen);
                viewport.native_pixels_per_point = Some(retina);
                ctx.run_ui(raw, |ui| super::content_ui(app, ui)).textures_delta.clear();
            };
            // Enter, leave, and enter again: nothing stale when returning to a normal window.
            for fullscreen in [false, true, false, true] {
                frame(&mut app, fullscreen, vec![]);
                frame(&mut app, fullscreen, vec![]);
                for name in ["appbar:file", "appbar:undo", "appbar:redo", "appbar:help"] {
                    let at = handle_at(name).unwrap();
                    assert!((at.y - 17.0).abs() < 0.1, "{name}: {at:?}, fullscreen={fullscreen}, zoom={zoom}");
                }
                for (handle, bodies) in [("appbar:undo", 0), ("appbar:redo", 1)] {
                    let at = handle_at(handle).unwrap();
                    frame(&mut app, fullscreen, vec![Event::PointerMoved(at)]);
                    for pressed in [true, false] {
                        frame(
                            &mut app,
                            fullscreen,
                            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() }],
                        );
                    }
                    assert_eq!(app.session.model.state().bodies.len(), bodies, "{handle}, fullscreen={fullscreen}, zoom={zoom}");
                }
            }
        }
    }

    #[test]
    fn hidden_fullscreen_title_bar_lets_clicks_through_only_while_unseen() {
        assert!(super::hidden_title_bar_ignores_clicks(0.0));
        assert!(super::hidden_title_bar_ignores_clicks(f64::NAN));
        assert!(!super::hidden_title_bar_ignores_clicks(0.01));
        assert!(!super::hidden_title_bar_ignores_clicks(1.0));
    }
}

#[cfg(test)]
mod msaa_tests {
    /// Safe graphics mode leaves the backend and adapter to wgpu (no API order, no adapter
    /// preference, no float depth feature); its software fallback still picks the CPU adapter.
    #[test]
    fn safe_graphics_renderer_is_wgpus_own_choice() {
        let eframe::egui_wgpu::WgpuSetup::CreateNew(default) = eframe::egui_wgpu::WgpuConfiguration::default().wgpu_setup else { panic!() };
        let eframe::egui_wgpu::WgpuSetup::CreateNew(safe) = super::safe_wgpu_options(false, None, eframe::wgpu::Backends::empty()).wgpu_setup else {
            panic!()
        };
        assert_eq!(safe.instance_descriptor.backends, default.instance_descriptor.backends, "wgpu's own backends");
        assert!(safe.native_adapter_selector.is_none(), "no adapter preference");
        assert_eq!(safe.power_preference, default.power_preference);
        let eframe::egui_wgpu::WgpuSetup::CreateNew(cpu) = super::safe_wgpu_options(true, None, eframe::wgpu::Backends::empty()).wgpu_setup else {
            panic!()
        };
        assert!(cpu.native_adapter_selector.is_some(), "the last-resort software renderer");
        if std::env::var_os("WGPU_BACKEND").is_none() {
            let eframe::egui_wgpu::WgpuSetup::CreateNew(after) =
                super::safe_wgpu_options(false, Some("vulkan"), eframe::wgpu::Backends::empty()).wgpu_setup
            else {
                panic!()
            };
            assert!(!after.instance_descriptor.backends.contains(eframe::wgpu::Backends::VULKAN), "a failed Vulkan launch: no Vulkan");
            // An API that crashed the process while starting (#18683) is left out of the instance
            // in safe graphics mode and for the software renderer too: its driver is never loaded.
            for cpu in [false, true] {
                let crashed = eframe::wgpu::Backends::VULKAN;
                let eframe::egui_wgpu::WgpuSetup::CreateNew(guarded) = super::safe_wgpu_options(cpu, None, crashed).wgpu_setup else { panic!() };
                assert!(!guarded.instance_descriptor.backends.contains(crashed), "software renderer {cpu}: no Vulkan");
                assert!(!guarded.instance_descriptor.backends.is_empty());
            }
        }
    }

    /// Safe mode uses default UI settings but edits the real graphics.json for the next start.
    #[test]
    fn safe_mode_preserves_graphics_choices_when_changing_antialiasing() {
        use serde_json::{Value, json};
        use solidcraft_ui_egui::scenario::Harness;

        for choice in ["wayland", "x11"] {
            let mut h = Harness::new();
            let path = h.dir.join("graphics.json");
            let saved = json!({"window_system": choice, "adapter": "Selected GPU", "backend": "vulkan", "msaa": 8});
            std::fs::write(&path, saved.to_string()).unwrap();
            h.app.services.graphics_path = Some(path.to_string_lossy().into_owned());
            let defaults = h.app.preferences.clone();
            let stored_ui = json!({"preferences": {"window_system": "auto", "graphics_adapter": "Stale GPU", "msaa": 2,
                "display_quality": "fine"}})
            .to_string();
            super::restore_preferences(&mut h.app, Some(&stored_ui), Some(&saved), true);

            assert_eq!(h.app.preferences.window_system, choice);
            assert_eq!(h.app.preferences.graphics_adapter, "Selected GPU");
            assert_eq!(h.app.preferences.msaa, defaults.msaa, "safe mode ignores saved anti-aliasing");
            assert_eq!(h.app.preferences.display_quality, defaults.display_quality, "safe mode ignores saved UI settings");
            assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).unwrap(), saved, "startup does not write settings");

            assert_eq!(h.call("ui.prefs", json!({"open": "Graphics"}))["ok"], true);
            assert_eq!(h.call("ui.prefs", json!({"msaa": 1}))["ok"], true);
            assert_eq!(h.call("ui.prefs", json!({"button": "ok"}))["ok"], true);
            let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(after, json!({"window_system": choice, "adapter": "Selected GPU", "backend": "vulkan", "msaa": 1}));

            let mut normal = Harness::new();
            super::restore_preferences(&mut normal.app, Some(&stored_ui), Some(&after), false);
            assert_eq!(normal.app.preferences.window_system, choice);
            assert_eq!(normal.app.preferences.graphics_adapter, "Selected GPU");
            assert_eq!(normal.app.preferences.display_quality, "fine", "normal startup still restores UI settings");
        }
    }

    #[test]
    fn missing_or_invalid_startup_window_system_uses_automatic() {
        use serde_json::json;

        for saved in [None, Some(json!(null)), Some(json!({})), Some(json!({"window_system": 7})), Some(json!({"window_system": "unknown"}))] {
            let mut app = super::SolveApp::new(super::Session::default(), super::Services::default());
            app.preferences.window_system = "wayland".into();
            super::restore_graphics_preferences(&mut app, saved.as_ref());
            assert_eq!(app.preferences.window_system, "auto", "{saved:?}");
        }
    }

    /// A manual graphics.json choice wins over stale app.ron preferences and survives an
    /// unrelated graphics edit (#10116), including on a fresh launch with no saved UI state.
    #[test]
    fn startup_window_system_survives_changing_antialiasing() {
        use serde_json::{Value, json};
        use solidcraft_ui_egui::scenario::Harness;

        for choice in ["wayland", "x11", "auto", " wayland ", " x11 "] {
            for stale in [None, Some("auto"), Some("wayland"), Some("x11")] {
                let mut h = Harness::new();
                if let Some(stale) = stale {
                    h.app.load_prefs(&json!({"preferences": {"window_system": stale}}).to_string());
                }
                let path = h.dir.join("graphics.json");
                std::fs::write(&path, json!({"window_system": choice, "backend": "vulkan", "adapter": "Selected GPU", "msaa": 4}).to_string())
                    .unwrap();
                h.app.services.graphics_path = Some(path.to_string_lossy().into_owned());
                let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                super::restore_graphics_preferences(&mut h.app, Some(&saved));

                let result = h.call("ui.prefs", json!({"msaa": 2}));
                assert_eq!(result["ok"], true, "{result}");
                let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                assert_eq!(after["window_system"], choice.trim(), "saved choice {choice:?}, stale UI choice {stale:?}");
                assert_eq!(result["result"]["window_system"], choice.trim());
                assert_eq!(after["msaa"], 2);
                assert_eq!(after["backend"], "vulkan");
                assert_eq!(after["adapter"], "Selected GPU");
            }
        }
    }

    #[test]
    fn first_launch_adopts_the_previous_release_settings() {
        let d = std::env::temp_dir().join(format!("solidcraft-adopt-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (current, old) = (d.join("solidcraft"), d.join("ai.storyteller.solidcraft"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("graphics.json"), r#"{"msaa":8}"#).unwrap();
        super::adopt_settings(&current, std::slice::from_ref(&old)).unwrap();
        assert_eq!(std::fs::read_to_string(current.join("graphics.json")).unwrap(), r#"{"msaa":8}"#);
        assert!(old.join("graphics.json").is_file());
        // Later launches leave the current settings alone.
        std::fs::write(current.join("graphics.json"), r#"{"msaa":2}"#).unwrap();
        super::adopt_settings(&current, &[old]).unwrap();
        assert_eq!(std::fs::read_to_string(current.join("graphics.json")).unwrap(), r#"{"msaa":2}"#);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saved_multisampling_uses_the_selected_backend_policy() {
        use eframe::wgpu::Backend;
        for requested in [1, 2, 4, 8] {
            assert_eq!(eframe::egui_wgpu::effective_msaa_samples(Backend::Gl, requested), 1);
            for backend in [Backend::Vulkan, Backend::Dx12, Backend::Metal] {
                assert_eq!(eframe::egui_wgpu::effective_msaa_samples(backend, requested), requested);
            }
        }
    }

    #[test]
    fn scene_sample_choices_include_the_scene_depth_format() {
        use eframe::wgpu::TextureFormat;
        for (bits, expected) in [(24, TextureFormat::Depth24PlusStencil8), (32, TextureFormat::Depth32FloatStencil8)] {
            // A synthetic colour target supports 8x, but the scene depth supports only 1x/4x.
            let supported = super::scene_sample_choices(bits, 8, |depth| match depth {
                Some(format) if format == expected => vec![1, 4],
                None => vec![1, 4, 8],
                other => panic!("unexpected scene depth: {other:?}"),
            });
            assert_eq!(supported, [1, 4]);
            assert_eq!(super::scene_samples(eframe::wgpu::Backend::Dx12, 8, &supported), 4);
        }
    }

    #[test]
    fn scene_msaa_keeps_four_samples_by_default_and_explicit_choices() {
        use eframe::wgpu::Backend;
        let default = super::scene_msaa_request(None);
        assert_eq!(default, 4);
        for backend in [Backend::Dx12, Backend::Vulkan, Backend::Metal] {
            assert_eq!(super::scene_samples(backend, u32::from(default), &[1, 4]), 4);
        }
        for saved in [1, 2, 4, 8] {
            assert_eq!(super::scene_msaa_request(Some(saved)), saved);
        }
    }

    #[test]
    fn the_scene_follows_the_backend_policy_too() {
        use eframe::wgpu::Backend;
        assert_eq!(super::scene_samples(Backend::Gl, 4, &[1, 4]), 1);
        assert_eq!(super::scene_samples(Backend::Gl, 4, &[]), 1);
        assert_eq!(super::scene_samples(Backend::Vulkan, 4, &[1, 4]), 4);
        assert_eq!(super::scene_samples(Backend::Dx12, 8, &[1, 2, 4]), 4);
    }
}
