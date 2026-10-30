//! SolidCraft desktop app.
//!
//! Usage: `solidcraft [--control <port>] [--sample] [--crash-log] [--safe-mode] [design.sc | part.step]`
//!
//! `--safe-mode` (or `SOLIDCRAFT_SAFE_MODE=1`) starts with default settings, no recovery offer and
//! the software renderer; see `startup` and docs/book/src/guide/troubleshooting.md.
//!
//! A `.step`/`.stp` file opens as a new design holding the file's bodies (an Import feature); a
//! `.3mf`/`.stl`/`.obj`/`.glb`/`.gltf` file as a new design holding mesh bodies.
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
mod cloud;
mod come_forward;
mod control_server;
#[cfg(target_os = "linux")]
mod desktop_theme;
mod device_lost;
#[cfg(target_os = "windows")]
mod native_crash;
#[cfg(target_os = "macos")]
mod native_crash_macos;
#[cfg(target_os = "windows")]
mod native_frame;
#[cfg(target_os = "macos")]
mod native_menu;
mod nav_bridge;
#[cfg(target_os = "macos")]
mod open_documents;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
mod space_mouse;
mod startup;
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
        eprintln!("startup {:>6.0} ms  {step}", start.elapsed().as_secs_f64() * 1000.0);
    }
}

struct App {
    solve: SolveApp,
    /// The settings folder whose start marker the first frame clears (`startup::started`).
    starting: Option<std::path::PathBuf>,
    /// Frames begun; the second one means the first was drawn (startup is over).
    frames: u32,
    /// Started over the control channel: the window doesn't ask for the foreground.
    take_focus_unwanted: bool,
    /// Coming forward after a close that asks about unsaved designs.
    come_forward: come_forward::ComeForward,
    /// The graphics device: lost, it can't draw again (`App::watch_device`, #4172).
    gpu: Option<LostWatch>,
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    space_mouse: space_mouse::SpaceMouse,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    /// A device's own app moving the view (Preferences › Navigation, off by default).
    nav_bridge: nav_bridge::NavBridge,
    /// Where the next file dialog opens: it follows the open design (#7756).
    dialog_folder: DialogFolder,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.frames < 2 {
            self.frames += 1;
            // Before the window is shown: its title bar must be within reach (window_fit.rs).
            #[cfg(target_os = "windows")]
            if self.frames == 1 {
                fit_window_to_work_area(ctx, _frame);
            }
            mark(if self.frames == 1 { "first frame begins" } else { "second frame begins (first presented)" });
            // A second frame right away, which means the first was presented: the app is up.
            ctx.request_repaint();
            if self.frames == 2 {
                // The window is shown only once its first frame is presented, and Windows then
                // keeps a window of a process started from a shortcut or an installer behind the
                // others. Ask for the foreground once, now that it is on screen; never again, so
                // later frames don't take the focus from whatever the user moved to. Automation
                // over the control channel keeps the focus where it is.
                if !self.take_focus_unwanted {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                solidcraft_engine::crash::phase(solidcraft_engine::crash::RUNNING);
                // A DMG install has no installer step: put solidcraft-cli on PATH, quietly.
                #[cfg(target_os = "macos")]
                std::thread::spawn(solidcraft_ui_egui::cli_link::install_on_launch);
                // Automation runs (a control port was asked for) leave the user's AI apps alone.
                if !self.take_focus_unwanted {
                    solidcraft_ui_egui::ai_connect::begin(&mut self.solve);
                    solidcraft_ui_egui::agent_chat::begin(&mut self.solve);
                }
                if let Some(dir) = self.starting.take() {
                    startup::started(&dir);
                }
            }
        }
        self.watch_device();
        // The design on screen is the one whose commands a crash report replays.
        self.solve.session.crash_journal = solidcraft_engine::crash::enabled();
        // The OS closing the window (Alt+F4, Cmd+Q, the window manager): ask about unsaved
        // designs first; the window closes once they are answered (`quit_requested`).
        if ctx.input(|i| i.viewport().close_requested()) && !self.solve.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            solidcraft_ui_egui::documents::request_quit(&mut self.solve);
            // Closed from the taskbar or a window switcher while another app had the focus: the
            // "save changes?" prompt is only seen once the window comes forward.
            if !self.solve.quit_requested {
                self.come_forward.start(std::time::Instant::now());
            }
        }
        self.come_forward.tick(ctx, _frame);
        #[cfg(target_os = "macos")]
        for f in if self.solve.file_picker_busy() { Vec::new() } else { open_documents::take() } {
            self.solve.open_path_async(&f);
        }
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            menu.tick(&mut self.solve, ctx);
        }
        self.solve.logic(ctx);
        self.dialog_folder.design(self.solve.session.path.as_deref());
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        if !self.solve.file_picker_busy() {
            self.space_mouse.tick(&mut self.solve, ctx);
        }
        // Every frame, so switching the bridge off reaches the reader while a picker is open too.
        self.nav_bridge.tick(&mut self.solve, ctx);
        if self.solve.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            menu.raw_input_hook(&mut self.solve, ctx, raw);
        }
        #[cfg(target_os = "macos")]
        macos_fullscreen_content(ctx, raw);
        // Before the app's own hook, which adds the control channel's events.
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        self.space_mouse.raw_input_hook(self.solve.preferences.space_mouse_enabled, raw);
        // The 3D mouse's buttons, before the app reads the frame: Esc and held modifiers join its input.
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        solidcraft_ui_egui::space_mouse::buttons_hook(&mut self.solve, ctx, self.space_mouse.buttons(), raw);
        self.solve.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        content_ui(&mut self.solve, ui);
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, self.solve.prefs());
    }
    fn on_exit(&mut self) {
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
        solidcraft_ui_egui::open_elsewhere::close(&mut self.solve);
        // Before a restart below starts the next instance, which would otherwise get nothing from
        // the 3D mouse driver (`solidcraft_3dx_client::stop`).
        #[cfg(target_os = "macos")]
        solidcraft_3dx_client::stop();
        solidcraft_engine::crash::end_session();
        if let Some(lost_at) = self.gpu.as_ref().and_then(|g| g.lost_at) {
            device_lost::restart(self.gpu.as_ref().is_some_and(|g| g.restart_on_software), lost_at);
        }
        // An update asked to restart into the new version or to run its installer.
        if let Some((program, args)) = solidcraft_ui_egui::update::take_launch(&mut self.solve) {
            let mut command = std::process::Command::new(&program);
            command.args(&args);
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

/// The window's graphics device, watched for its loss.
struct LostWatch {
    lost: eframe::egui_wgpu::DeviceLost,
    /// Where the next start runs once this device is lost ([`device_lost::restart_on_software`]).
    restart_on_software: bool,
    /// When the loss was seen: the app is closing to start again.
    lost_at: Option<u64>,
}

impl App {
    /// The graphics device was lost (#4172): the window can't draw again, so close, keeping
    /// unsaved changes for recovery as a forced quit does (the window can't ask about them), and
    /// start again once closed ([`device_lost::restart`]).
    fn watch_device(&mut self) {
        let Some(gpu) = self.gpu.as_mut().filter(|g| g.lost_at.is_none() && g.lost.is_lost()) else { return };
        let reason = gpu.lost.reason().unwrap_or_default();
        log::error!("graphics: the device was lost ({reason}); closing to start again");
        eprintln!("SolidCraft graphics: the device was lost ({reason}); starting again");
        solidcraft_engine::crash::phase(&format!("graphics device lost: {reason}"));
        gpu.lost_at = Some(device_lost::now_secs());
        self.solve.quit_requested = true;
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
        if let (Some(folder), Ok(mut state)) = (file.parent().filter(|f| !f.as_os_str().is_empty()), self.0.lock()) {
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
    ui.painter().rect_filled(above, 0.0, solidcraft_ui_egui::theme::Tokens::get().app_bar);
    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| app.ui(ui));
}

/// A transparent macOS title bar still intercepts clicks at the top in fullscreen (#3643).
/// `contentLayoutRect` reports the whole content view once that bar auto-hides, so reserve
/// the native titled-window height even then. AppKit supplies the height in logical points;
/// egui's UI zoom is separate from the display's Retina scale.
#[cfg(target_os = "macos")]
fn macos_fullscreen_content(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let frame = objc2_app_kit::NSWindow::frameRectForContentRect_styleMask(Default::default(), objc2_app_kit::NSWindowStyleMask::Titled, mtm);
    fullscreen_content_insets(raw, frame.size.height as f32, ctx.zoom_factor());
}

#[cfg(any(target_os = "macos", test))]
fn fullscreen_content_insets(raw: &mut egui::RawInput, title_height: f32, zoom: f32) {
    let fullscreen = raw.viewports.get(&raw.viewport_id).and_then(|v| v.fullscreen).unwrap_or(false);
    let mut insets = raw.safe_area_insets.unwrap_or_default();
    if fullscreen && title_height.is_finite() && title_height > 0.0 && zoom.is_finite() && zoom > 0.0 {
        insets.0.top = insets.0.top.max(title_height / zoom);
    }
    // Explicitly reset on leaving fullscreen: None would retain the preceding frame's inset.
    raw.safe_area_insets = Some(insets);
}

fn services(cc: &eframe::CreationContext<'_>, folder: &DialogFolder) -> Services {
    let parent = rfd::AsyncFileDialog::new().set_parent(cc);
    let open_parent = parent.clone();
    let program_parent = parent.clone();
    let file_parent = parent.clone();
    let (open_folder, file_folder, save_folder) = (folder.clone(), folder.clone(), folder.clone());
    Services {
        pick_open: Some(Box::new(move |first: &[&str]| {
            // The file type the command reads comes first, so the picker opens on it (#1801).
            let mut dialog = open_folder.start(open_parent.clone());
            if !first.is_empty() {
                let label = first.iter().map(|e| e.to_uppercase()).collect::<Vec<_>>().join(", ");
                dialog = dialog.add_filter(label, &case_variants(first));
            }
            // The groups come from the format registry, so a new format joins this dialog by itself.
            let mut everything = io::formats::open_extensions(true);
            everything.extend(["dxf", "svg"]);
            dialog = dialog.add_filter(solidcraft_ui_egui::i18n::tr("file.open.drawing_filter"), &case_variants(&everything));
            for (label, exts) in io::formats::import_filters(true) {
                dialog = dialog.add_filter(label, &case_variants(&exts));
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
        graphics_path: graphics_path().map(|p| p.to_string_lossy().to_string()),
        window_system: window_system_now(),
        // Linux: the desktop's dark or light setting, which the window is not told on every
        // desktop (#7944). Windows and macOS: the window's own theme.
        #[cfg(target_os = "linux")]
        desktop_dark: Some(desktop_theme::start(&cc.egui_ctx)),
        #[cfg(not(target_os = "linux"))]
        desktop_dark: None,
        http_get: Some(std::sync::Arc::new(http_get)),
        http_download: Some(std::sync::Arc::new(http_download)),
        // Already an installed app: only the browser build offers to install.
        install_app: None,
        can_restore: None,
        restore_file: None,
        download_design: None,
        easyprint_upload: Some(std::sync::Arc::new(easyprint_upload)),
        browser_files: None,
        agent_http: Some(std::sync::Arc::new(agent_http)),
        secrets: Some(std::sync::Arc::new(Keychain)),
        open_url: Some(Box::new(|url| {
            if let Err(e) = cloud::open_browser(url) {
                log::warn!("could not open a browser: {e}");
            }
        })),
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

/// API keys in the system keychain (service "SolidCraft agent chat", one entry per agent).
struct Keychain;

impl Keychain {
    fn entry(id: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("SolidCraft agent chat", id).map_err(|e| e.to_string())
    }
}

impl solidcraft_ui_egui::agent_chat::backend::Secrets for Keychain {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        match Self::entry(id)?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        Self::entry(id)?.set_password(secret).map_err(|e| e.to_string())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        match Self::entry(id)?.delete_credential() {
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

/// Compatibility shim (docs/compatibility.md): on the first launch after the rename, copy the
/// settings of earlier releases into the new folder. They kept `graphics.json` (and, on Linux
/// and macOS, the recovery folder) under the old product name, and eframe's `app.ron` under the
/// window's old app id. In the Flatpak sandbox, earlier releases kept everything under an
/// old app id's `~/.var/app` folder; those copies come first, newest first. The old folders
/// are never changed.
fn adopt_legacy_settings() {
    let (Some(current), Some(legacy)) = (settings_dir().ok().flatten(), eframe::storage_dir("SolveCraft")) else { return };
    let legacy_eframe = eframe::storage_dir(solidcraft_engine::io::LEGACY_APP_ID);
    let old_flatpak = solidcraft_engine::io::legacy_flatpak_paths(&current);
    if let Err(e) = adopt_settings(&current, &old_flatpak, &legacy, legacy_eframe.as_deref()) {
        log::warn!("settings: could not copy the previous release's settings into {}: {e}", current.display());
    }
}

fn adopt_settings(
    current: &std::path::Path,
    old_flatpak: &[std::path::PathBuf],
    legacy: &std::path::Path,
    legacy_eframe: Option<&std::path::Path>,
) -> std::io::Result<()> {
    let fresh = !current.exists();
    for old in old_flatpak {
        solidcraft_engine::io::adopt_legacy_dir(current, old)?;
    }
    solidcraft_engine::io::adopt_legacy_dir(current, legacy)?;
    if let Some(old) = legacy_eframe.map(|d| d.join("app.ron")).filter(|p| fresh && p.is_file()) {
        std::fs::create_dir_all(current)?;
        let new = current.join("app.ron");
        if !new.exists() {
            std::fs::copy(old, new)?;
        }
    }
    Ok(())
}

/// `graphics.json`, read before the window opens.
fn graphics() -> Option<serde_json::Value> {
    graphics_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
}

/// The graphics backend preference (`graphics.json`'s `backend`: `auto`, `dx12`, `vulkan`, `gl`).
fn backend_preference() -> Option<String> {
    graphics().and_then(|v| v.get("backend").and_then(serde_json::Value::as_str).map(str::to_string))
}

/// The viewport scene's sample count: the app's anti-aliasing (the window's pass is 1x), clamped
/// by the backend's policy (OpenGL renders 1x, see `effective_msaa_samples`). A count the device
/// cannot render with falls back to the supported default.
fn scene_samples(backend: eframe::wgpu::Backend, samples: u32, supported: &[u32]) -> u32 {
    let requested = eframe::egui_wgpu::effective_msaa_samples(backend, samples.max(1));
    if requested <= 1 || supported.is_empty() || supported.contains(&requested) {
        requested
    } else {
        supported.iter().copied().filter(|&n| n <= solidcraft_ui_egui::prefs::Prefs::default().msaa).max().unwrap_or(1)
    }
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

const DEPTH_BITS: u8 = 24;
/// A 32-bit float depth buffer, drawn with reversed depth (1 near, 0 far) so its precision follows
/// the distance from the eye (#1434). Only where every adapter offers it with a stencil
/// ([`startup::float_depth_everywhere`]); else [`DEPTH_BITS`]. Not in safe mode.
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
    Ok(egui::ViewportBuilder::default()
        .with_title("SolidCraft")
        .with_icon(icon)
        .with_inner_size([1600.0, 1000.0])
        .with_min_inner_size([960.0, 600.0])
        .with_drag_and_drop(true)
        .with_decorations(!custom_titlebar)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_app_id(APP_ID))
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
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--sample" => sample = true,
            "--theme-file" => theme_file = args.next(),
            "--max-file-size" => max_file_size = Some(args.next()),
            s if s.starts_with("--max-file-size=") => max_file_size = Some(s.strip_prefix("--max-file-size=").map(str::to_string)),
            "--safe-mode" => safe_mode = true,
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
                    "usage: solidcraft [--control PORT] [--sample] [--theme-file THEME.json] [--crash-log] [--safe-mode] [--max-file-size SIZE] [design.sc | part.step]"
                );
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    // `--max-file-size 2GB`: the largest files this run reads, over Preferences, never saved (#4086).
    if let Some(value) = max_file_size {
        let applied = startup::max_file_size(value.as_deref()).and_then(|bytes| solidcraft_engine::io::limits::set_override(Some(bytes)));
        if let Err(error) = applied {
            eprintln!("SolidCraft: {error}");
            return Err(eframe::Error::AppCreation(std::io::Error::other(error).into()));
        }
    }
    // Settings that don't read are moved aside; launches that never got a window are counted,
    // and after a few in a row this one starts in safe mode (docs/book/src/guide/troubleshooting.md).
    let mut notes: Vec<String> = Vec::new();
    let mut failed_starts = 0;
    if let Some(dir) = &settings {
        notes.extend(startup::check_settings(dir, &[solidcraft_ui_egui::prefs::STORAGE_KEY, solidcraft_ui_egui::prefs::LEGACY_STORAGE_KEY]));
        failed_starts = startup::begin(dir);
    }
    if failed_starts >= startup::FAILED_STARTS_FOR_SAFE_MODE && !safe_mode {
        safe_mode = true;
        notes.push(format!("The last {failed_starts} launches stopped before the window opened, so this one is in safe mode."));
    }
    let cpu_renderer = safe_mode || startup::cpu_renderer_env(std::env::var(startup::RENDERER_ENV).ok().as_deref());
    // The launch before this one closed because its graphics device was lost (`App::watch_device`).
    let after_loss = device_lost::recent(std::env::var(device_lost::LOST_ENV).ok().as_deref(), device_lost::now_secs());
    if after_loss {
        notes.push(device_lost::note(cpu_renderer));
    }
    if safe_mode {
        notes.push("Safe mode: default settings (yours are kept for the next normal start), no recovery offer, software rendering.".into());
    }
    let fallback_note = std::env::var(startup::FALLBACK_NOTE_ENV).ok().map(|s| s.chars().take(4096).collect::<String>());
    if let Some(note) = &fallback_note {
        log::warn!("{note}");
        eprintln!("SolidCraft: {note}");
        notes.push(note.clone());
    }
    let saved_samples = if safe_mode { None } else { msaa() };
    let wgpu_options = wgpu_options(cpu_renderer, if safe_mode { None } else { backend_preference() }.as_deref());
    let depth_bits = if !safe_mode && startup::float_depth_everywhere(&wgpu_options) { FLOAT_DEPTH_BITS } else { DEPTH_BITS };
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
    let custom_titlebar = CUSTOM_TITLEBAR && !startup::system_title_bar(cfg!(target_os = "windows"), started);
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
            let mut app = SolveApp::new(Session::default(), services(cc, &dialog_folder));
            // Cloud Open and Save, when a CloudCraft service is configured.
            if let Some(backend) = cloud::service(cc.egui_ctx.clone()) {
                app.cloud.connect(backend);
            }
            // The Share dialog's backend. It asks for a client each request,
            // so a session collected later is picked up without a restart.
            app.share.connect(collabcraft_share::CollabCraft::from_fn(cloud::client).for_app("solidcraft"));
            app.apply_off_thread = Some(solidcraft_ui_egui::dialogs::APPLY_AT_ONCE);
            app.custom_titlebar = custom_titlebar;
            #[cfg(target_os = "windows")]
            {
                // Windows: the taskbar button shows the mark even when slow first frames keep the
                // taskbar waiting for the window's icon.
                if !native_frame::set_class_icon(cc) {
                    eprintln!("SolidCraft window: no class icon; the taskbar may show a generic one");
                }
                // The OS hit test offers the resize border, at the window's DPI, from the first
                // message (#1381).
                app.native_resize_border = custom_titlebar && native_frame::install(cc);
                if custom_titlebar && !app.native_resize_border {
                    eprintln!("SolidCraft window: no native resize border; egui edge zones instead");
                }
            }
            app.integrated_titlebar = cfg!(target_os = "macos");
            app.cpu_meshing = safe_mode;
            app.graphics_fallback = fallback_note.clone();
            if let Some(p) = cc.storage.and_then(|s| solidcraft_ui_egui::prefs::stored(|k| s.get_string(k))).filter(|_| !safe_mode) {
                app.load_prefs(&p);
            }
            if let Some(path) = &theme_file
                && let Err(e) = solidcraft_ui_egui::prefs::follow_theme_file_for_run(&mut app, path)
            {
                app.set_status(format!("--theme-file: {e}"), true);
            }
            mark("app state and prefs");
            let gpu = cc.wgpu_render_state.as_ref().map(|rs| LostWatch {
                lost: rs.device_lost.clone(),
                restart_on_software: device_lost::restart_on_software(after_loss, cpu_renderer),
                lost_at: None,
            });
            if !safe_mode {
                app.preferences.graphics_adapter =
                    graphics().and_then(|v| v.get("adapter").and_then(serde_json::Value::as_str).map(str::to_string)).unwrap_or_default();
            }
            if let Some(rs) = &cc.wgpu_render_state {
                app.services.graphics_adapters = rs
                    .available_adapters
                    .iter()
                    .map(|adapter| {
                        let info = adapter.get_info();
                        (info.name.clone(), format!("{} ({:?}, {:?})", info.name, info.device_type, info.backend))
                    })
                    .collect();
                let info = rs.adapter.get_info();
                let samples = scene_msaa_request(saved_samples);
                let supported = scene_sample_choices(depth_bits, STENCIL_BITS, |depth| {
                    eframe::egui_wgpu::supported_msaa_samples(&rs.adapter, &rs.device, rs.target_format, depth)
                });
                let scene_samples = scene_samples(info.backend, u32::from(samples), &supported);
                eprintln!("SolidCraft graphics: {:?}, MSAA {scene_samples}x (requested {}x)", rs.adapter.get_info().backend, samples);
                solidcraft_engine::crash::phase("viewport pipelines (shader compile)");
                let scene_samples = app.set_wgpu_split(rs, depth_bits, STENCIL_BITS, scene_samples);
                app.msaa_supported = supported;
                app.preferences.msaa = u32::from(samples);
                solidcraft_ui_egui::gpu::install_picture_renderer(rs);
                // The graphics driver is loaded now; it may have put its own exception filter
                // in front of ours.
                #[cfg(target_os = "windows")]
                native_crash::reinstall();
                // Reflect a confirmed probe fallback or an unsupported saved count in Graphics.
                solidcraft_ui_egui::prefs::check_msaa(&mut app, u32::from(samples));
                solidcraft_engine::crash::set_environment(&format!(
                    "Renderer: {:?} {} ({}), MSAA {scene_samples}x",
                    info.backend, info.name, info.driver
                ));
            }
            mark("viewport pipelines");
            // `--control`/the environment name a port; otherwise the preference (on by default)
            // opens the default one (docs/control-protocol.md).
            if let Some(listen_on) = solidcraft_engine::control_auth::plan(control_port, app.preferences.agent_control) {
                let rx = control_server::start(listen_on, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            // The agent chat's MCP bridge reaches the app through an endpoint of its own, started
            // on a free port the first time the chat connects (docs/agent-chat.md).
            let ctx = cc.egui_ctx.clone();
            app.agent_chat.control_starter = Some(Box::new(move |tx| control_server::listen_agent(tx, ctx.clone())));
            if sample {
                if let Err(e) = app.session.run_script(&solidcraft_engine::sample::script()) {
                    app.set_status(format!("sample: {e}"), true);
                }
                app.session.doc_mut().name = "Sample Plate".into();
                app.session.mark_saved();
                app.session.undo.clear();
                // Built straight into the session, so nothing asked for a fit: frame it at the
                // home view as any opened design is (#9195). The fit waits for the first layout.
                app.cam.set_view(solidcraft_engine::render::StandardView::Iso);
                app.fit_view();
            }
            // Lock files next to open designs: a design open on another computer says so (#5467),
            // the files given at launch included.
            solidcraft_ui_egui::open_elsewhere::start(&mut app);
            // A launch from Finder: its files arrived before the window.
            #[cfg(target_os = "macos")]
            let files = {
                open_documents::set_ui(&cc.egui_ctx);
                [files, open_documents::take()].concat()
            };
            // No design given: the designs open at the last quit, when the preference asks (#5542).
            // Not when driven over the control channel, whose clients expect a fresh design.
            let files =
                if files.is_empty() && !sample && control_port.is_none() { solidcraft_ui_egui::documents::designs_to_reopen(&app) } else { files };
            for f in &files {
                app.open_path_async(f);
            }
            // No file and no sample: the empty design takes Units of new designs (#1205).
            // An opened file and the sample part keep the units they were saved or built with.
            if files.is_empty() && !sample {
                solidcraft_ui_egui::prefs::apply_new_design(&mut app);
            }
            // No design given: the start page (not when driven over the control channel, whose
            // clients expect the design view; `ui.home` shows it there).
            if files.is_empty() && !sample && control_port.is_none() {
                let rev = app.session.revision;
                app.home.show(rev);
            }
            // Autosave, and offer what crashed sessions left behind.
            if let Some(dir) = solidcraft_engine::recovery::default_dir() {
                let every = if app.autosave_minutes > 0.0 { app.autosave_minutes * 60.0 } else { f64::MAX / 4.0 };
                solidcraft_ui_egui::recovery_ui::start(&mut app, &dir, std::time::Duration::from_secs_f64(every.min(1e9)));
                // Safe mode keeps autosaving but offers nothing: the entries wait for a normal start.
                if safe_mode {
                    app.recoverable.clear();
                }
            }
            // Tell the user about crashes since the last start (after the recovery offer is set up).
            solidcraft_ui_egui::crash_ui::announce(&mut app, &crashed);
            if !notes.is_empty() {
                app.set_status(notes.join(" "), true);
            }
            let nav_bridge = nav_bridge::NavBridge::start(&cc.egui_ctx);
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            let space_mouse = space_mouse::SpaceMouse::start(&cc.egui_ctx, nav_bridge.claim());
            #[cfg(target_os = "macos")]
            let native_menu = match native_menu::NativeMenu::new(&app, &cc.egui_ctx) {
                Ok(menu) => Some(menu),
                Err(error) => {
                    app.set_status(format!("System menu: {error}"), true);
                    None
                }
            };
            #[cfg(target_os = "macos")]
            {
                app.native_file_menu = native_menu.is_some();
            }
            mark("app created");
            solidcraft_engine::crash::phase("first frame");
            Ok(Box::new(App {
                solve: app,
                starting,
                frames: 0,
                take_focus_unwanted: control_port.is_some(),
                come_forward: come_forward::ComeForward::default(),
                gpu,
                #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
                space_mouse,
                #[cfg(target_os = "macos")]
                native_menu,
                nav_bridge,
                dialog_folder,
            }))
        }),
    );
    control_server::remove_token_file();
    match result {
        Err(e) if !created.load(std::sync::atomic::Ordering::SeqCst) => no_window(e, cpu_renderer, started, settings.as_deref()),
        r => r,
    }
}

/// egui-wgpu is about to start a step of getting the graphics device and the window's surface up:
/// the crash journal keeps it, and the adapter once it is picked.
fn startup_step(step: &'static str, detail: &str) {
    if step == eframe::egui_wgpu::STEP_ADAPTER_CHOSEN {
        solidcraft_engine::crash::set_environment(&format!("Renderer: {detail}"));
    }
    solidcraft_engine::crash::phase(step);
}

/// The renderer settings: with `cpu`, the software adapter (WARP, lavapipe, llvmpipe). The
/// backends come from `startup::choose_backends` (on Windows D3D12 first, and only the backend
/// in use is started); `preference` is `graphics.json`'s `backend`.
fn wgpu_options(cpu: bool, preference: Option<&str>) -> eframe::egui_wgpu::WgpuConfiguration {
    let mut options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup {
        let env_set = std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| !v.trim().is_empty());
        // A retry applies only to this launch. WGPU_BACKEND always has priority.
        let retry = std::env::var(startup::RETRY_BACKEND_ENV).ok();
        let preference = if cpu { preference } else { retry.as_deref().or(preference) };
        if let Some(backends) = startup::choose_backends(cfg!(target_os = "windows"), env_set, cpu, preference, startup::has_hardware_adapter) {
            eprintln!("SolidCraft graphics: starting {backends:?}");
            setup.instance_descriptor.backends = backends;
        }
        let intel_mac = cfg!(all(target_os = "macos", target_arch = "x86_64"));
        let indirect = std::env::var(startup::INDIRECT_VALIDATION_ENV).ok();
        setup.instance_descriptor.flags = startup::window_instance_flags(setup.instance_descriptor.flags, intel_mac, indirect.as_deref());
        // Never a `dxcompiler.dll` another program put on PATH (#6756).
        setup.instance_descriptor.backend_options.dx12.shader_compiler = solidcraft_gpu::dx12_compiler();
        let environment = std::env::var("WGPU_ADAPTER_NAME").ok();
        let adapter = graphics().and_then(|v| v.get("adapter").and_then(serde_json::Value::as_str).map(str::to_string));
        startup::configure_adapter(setup, cpu, environment, adapter);
        // A float depth buffer with a stencil, wherever the adapter has it (see `FLOAT_DEPTH_BITS`).
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            d.required_features |= adapter.features() & startup::FLOAT_DEPTH_STENCIL;
            d
        });
    }
    options
}

/// A failed renderer returns here before app creation. A fresh process avoids reusing winit's
/// event loop or a failed device; each retry advances through the APIs, then software once.
fn no_window(error: eframe::Error, tried_cpu: bool, failed: eframe::wgpu::Backends, settings: Option<&std::path::Path>) -> eframe::Result {
    log::error!("the window could not open: {error}");
    eprintln!("SolidCraft: the window could not open: {error}");
    // This failure was handled, so neither the marker nor the journal should call it a crash.
    if let Some(dir) = settings {
        startup::started(dir);
    }
    solidcraft_engine::crash::end_session();
    let explicit = std::env::var(startup::WGPU_BACKEND_ENV).is_ok_and(|v| !v.trim().is_empty());
    if let Some(next) = startup::fallback_backend(cfg!(target_os = "windows"), explicit, tried_cpu, failed)
        && let Ok(exe) = std::env::current_exe()
    {
        let previous = std::env::var(startup::FALLBACK_NOTE_ENV).unwrap_or_default();
        let note = startup::fallback_note(&previous, failed, next, &error.to_string());
        log::warn!("{note}");
        eprintln!("SolidCraft: {note}");
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
/// XWayland when it's there. Except on niri, whose XWayland (xwayland-satellite) breaks window
/// management (maximize/restore), so it stays on native Wayland and gives up file drops.
/// `SOLIDCRAFT_WAYLAND=1` keeps native Wayland, `SOLIDCRAFT_WAYLAND=0` forces XWayland.
#[cfg(all(unix, not(target_os = "macos")))]
fn prefer_x11(display: bool, wayland_display: bool, desktop: Option<&str>, wayland_override: Option<&str>) -> bool {
    match wayland_override.map(str::trim) {
        Some("0") => return display,
        Some(_) => return false,
        None => {}
    }
    let niri = desktop.is_some_and(|d| d.split(':').any(|name| name.trim().eq_ignore_ascii_case("niri")));
    display && !(niri && wayland_display)
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod backend_tests {
    use super::prefer_x11;

    #[test]
    fn xwayland_by_default_native_wayland_on_niri_and_the_override_wins() {
        // XWayland whenever there's an X display, for file drag-and-drop.
        assert!(prefer_x11(true, true, Some("GNOME"), None));
        assert!(prefer_x11(true, false, None, None));
        assert!(!prefer_x11(false, true, Some("KDE"), None));
        // niri (alone or in a desktop list) under Wayland stays native.
        assert!(!prefer_x11(true, true, Some("niri"), None));
        assert!(!prefer_x11(true, true, Some("niri:GNOME"), None));
        // niri without a Wayland socket (nested X session) still uses X11.
        assert!(prefer_x11(true, false, Some("niri"), None));
        // SOLIDCRAFT_WAYLAND: anything but 0 keeps Wayland, 0 forces XWayland (when present).
        assert!(!prefer_x11(true, true, Some("GNOME"), Some("1")));
        assert!(!prefer_x11(true, true, Some("GNOME"), Some("")));
        assert!(prefer_x11(true, true, Some("niri"), Some("0")));
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

    #[test]
    fn fullscreen_bar_clears_native_chrome_and_undo_redo_keep_working() {
        use egui::{Event, PointerButton, Pos2, RawInput, Rect, ViewportId, vec2};
        use serde_json::json;
        use solidcraft_ui_egui::scenario::handle_at;

        // AppKit points are independent of Retina scale and of the user's UI zoom.
        for (zoom, retina) in [(1.0, 1.0), (1.0, 2.0), (1.5, 2.0), (2.0, 2.0)] {
            let ctx = egui::Context::default();
            solidcraft_ui_egui::fonts::setup_bundled_fonts(&ctx);
            ctx.set_zoom_factor(zoom);
            let mut app = super::SolveApp::new(super::Session::default(), Default::default());
            app.integrated_titlebar = true;
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
                super::fullscreen_content_insets(&mut raw, 32.0, zoom);
                let _ = ctx.run_ui(raw, |ui| super::content_ui(app, ui));
            };
            // Enter, leave, and enter again: no stale inset when returning to a normal window.
            for fullscreen in [false, true, false, true] {
                frame(&mut app, fullscreen, vec![]);
                frame(&mut app, fullscreen, vec![]);
                let top = if fullscreen { 32.0 / zoom } else { 0.0 };
                for name in ["appbar:file", "appbar:undo", "appbar:redo", "appbar:help"] {
                    let at = handle_at(name).unwrap();
                    assert!((at.y - top - 17.0).abs() < 0.1, "{name}: {at:?}, fullscreen={fullscreen}, zoom={zoom}");
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
    fn fullscreen_inset_preserves_existing_safe_areas_and_resets_on_exit() {
        use egui::epaint::MarginF32;
        use egui::{RawInput, SafeAreaInsets, ViewportId};
        let mut raw = RawInput::default();
        raw.viewports.get_mut(&ViewportId::ROOT).unwrap().fullscreen = Some(true);
        raw.safe_area_insets = Some(SafeAreaInsets(MarginF32 { top: 40.0, left: 5.0, right: 6.0, bottom: 7.0 }));
        super::fullscreen_content_insets(&mut raw, 32.0, 2.0);
        assert_eq!(raw.safe_area_insets.unwrap().0, MarginF32 { top: 40.0, left: 5.0, right: 6.0, bottom: 7.0 });

        raw.viewports.get_mut(&ViewportId::ROOT).unwrap().fullscreen = Some(false);
        raw.safe_area_insets = None;
        super::fullscreen_content_insets(&mut raw, 32.0, 1.0);
        assert_eq!(raw.safe_area_insets, Some(SafeAreaInsets::default()));
    }
}

#[cfg(test)]
mod msaa_tests {
    #[test]
    fn first_launch_adopts_the_previous_release_settings() {
        let d = std::env::temp_dir().join(format!("solidcraft-adopt-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (current, legacy, legacy_eframe) = (d.join("solidcraft"), d.join("solvecraft"), d.join("ai.storyteller.solvecraft"));
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&legacy_eframe).unwrap();
        std::fs::write(legacy.join("graphics.json"), r#"{"msaa":8}"#).unwrap();
        std::fs::write(legacy_eframe.join("app.ron"), "(prefs)").unwrap();
        super::adopt_settings(&current, &[], &legacy, Some(&legacy_eframe)).unwrap();
        assert_eq!(std::fs::read_to_string(current.join("graphics.json")).unwrap(), r#"{"msaa":8}"#);
        assert_eq!(std::fs::read_to_string(current.join("app.ron")).unwrap(), "(prefs)");
        assert!(legacy.join("graphics.json").is_file() && legacy_eframe.join("app.ron").is_file());
        // Later launches leave the current settings alone.
        std::fs::write(current.join("app.ron"), "(mine)").unwrap();
        std::fs::remove_file(current.join("graphics.json")).unwrap();
        super::adopt_settings(&current, &[], &legacy, Some(&legacy_eframe)).unwrap();
        assert_eq!(std::fs::read_to_string(current.join("app.ron")).unwrap(), "(mine)");
        assert!(!current.join("graphics.json").exists());
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
