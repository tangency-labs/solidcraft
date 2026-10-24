//! SolidCraft Android launcher.
//!
//! Starts the app's egui UI from a `NativeActivity` through eframe/wgpu, and supplies the two
//! Android services a desktop app cannot have on its own:
//! - the soft keyboard (`ime_bridge` + the hidden `EditText` in `SolidCraftActivity`), so numeric
//!   fields, the command bar and dialogs can be typed into;
//! - file open/save through the Storage Access Framework (`saf_bridge` + the same Activity).
//!
//! Desktop-only host services (native dialogs, keychain, updater, SpaceMouse) stay absent.

// The launcher is Android-only: `android-activity` and the JNI bridge do not exist for other
// targets. The crate is a workspace member, so a host build (`cargo clippy`/`cargo test` on a
// desktop) must see an empty crate rather than unresolved Android dependencies.
#![cfg(target_os = "android")]

mod ime_bridge;
mod saf_bridge;

use saf_bridge::saf;
mod control_server;
mod system_fonts;

use android_activity::AndroidApp;
use egui::Modifiers;
use jni::{JavaVM, jni_sig, jni_str, objects::JObject};
use solidcraft_engine::Session;
use solidcraft_ui_egui::{Services, SolveApp};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const ANDROID_SHELL_MARKER: &str = "SOLIDCRAFT_ANDROID_SHELL_V3_VIEWPORT";

/// Depth/stencil the egui pass allocates; the viewport callback is built for the same formats.
const DEPTH_BITS: u8 = 24;
const STENCIL_BITS: u8 = 8;
/// The tablet's Mali-G68 renders this CAD scene slowly with MSAA; keep it off until the port works.
const MSAA: u16 = 1;

// ---------------------------------------------------------------- SAF state and helpers

pub type SafInbox = ();

#[derive(Default)]
pub struct SafState {
    next_id: AtomicU64,
    mirror: saf::SaveMirror,
}

fn saf_next_id(state: &SafState) -> u64 {
    state.next_id.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
}

/// Open the system picker and block until the user answers. The picker runs on the Java UI thread
/// while `android_main` runs on its own Rust thread, so waiting does not freeze the UI.
fn saf_open_paths(bridge: &saf::Bridge, state: &SafState) -> Vec<String> {
    let id = saf_next_id(state);
    match bridge.pick_blocking(id, None, saf::PICK_TIMEOUT) {
        Ok(saf::PickAnswer::Paths(paths)) => paths,
        Ok(saf::PickAnswer::Cancelled) => Vec::new(),
        Err(error) => {
            log::error!("Android open picker: {error}");
            Vec::new()
        }
    }
}

/// Save dialog: the staged app-private destination, watched so the app's own write is mirrored to
/// the chosen SAF document on the following frames.
fn saf_save_path(bridge: &saf::Bridge, state: &SafState, name: &str) -> Option<String> {
    let id = saf_next_id(state);
    match bridge.pick_blocking(id, Some(name), saf::PICK_TIMEOUT) {
        Ok(saf::PickAnswer::Paths(paths)) => {
            let path = paths.into_iter().next();
            if let Some(path) = &path {
                state.mirror.track(id, path);
            }
            path
        }
        Ok(saf::PickAnswer::Cancelled) => None,
        Err(error) => {
            log::error!("Android save picker: {error}");
            None
        }
    }
}

fn saf_poll(bridge: &saf::Bridge, state: &SafState) {
    state.mirror.sync(bridge);
}

fn services(bridge: saf::Bridge, state: Arc<SafState>) -> Services {
    let open_bridge = bridge.clone_ref();
    let open_state = Arc::clone(&state);
    let save_bridge = bridge.clone_ref();
    let save_state = Arc::clone(&state);
    Services {
        pick_open: Some(Box::new(move |_exts: &[&str]| {
            let paths = saf_open_paths(&open_bridge, &open_state);
            Box::pin(async move { paths.into_iter().next() })
        })),
        pick_save: Some(Box::new(move |name: &str, _exts: &[&str]| {
            let path = saf_save_path(&save_bridge, &save_state, name);
            Box::pin(async move { path })
        })),
        ..Services::default()
    }
}

// ---------------------------------------------------------------- Android IME visibility

/// Show or hide the soft keyboard through the Activity's `imeShow`/`imeHide`, which own the hidden
/// `EditText` the characters come from.
fn ime_visibility(app: &AndroidApp, show: bool) {
    let app_clone = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: AndroidApp owns the live VM pointer for the lifetime of the activity.
        let jvm = unsafe { JavaVM::from_raw(app_clone.vm_as_ptr() as *mut _) };
        let result = jvm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity_raw = app_clone.activity_as_ptr() as jni::sys::jobject;
            // SAFETY: AndroidApp retains this global Activity reference; as_cast_raw borrows it.
            let activity = unsafe { env.as_cast_raw::<JObject>(&activity_raw)? };
            if show {
                env.call_method(&*activity, jni_str!("imeShow"), jni_sig!("()V"), &[])?;
            } else {
                env.call_method(&*activity, jni_str!("imeHide"), jni_sig!("()V"), &[])?;
            }
            Ok(())
        });
        if let Err(error) = result {
            log::error!("Android IME visibility: {error}");
        }
    }));
}

/// Finish the Activity (its `finish()`), so a window that went away shows the launcher instead of a
/// black screen.
fn finish_activity(app: &AndroidApp) {
    let app_clone = app.clone();
    app.run_on_java_main_thread(Box::new(move || {
        // SAFETY: AndroidApp owns the live VM pointer for the lifetime of the activity.
        let jvm = unsafe { JavaVM::from_raw(app_clone.vm_as_ptr() as *mut _) };
        let result = jvm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity_raw = app_clone.activity_as_ptr() as jni::sys::jobject;
            // SAFETY: AndroidApp retains this global Activity reference; as_cast_raw borrows it.
            let activity = unsafe { env.as_cast_raw::<JObject>(&activity_raw)? };
            env.call_method(&*activity, jni_str!("finish"), jni_sig!("()V"), &[])?;
            Ok(())
        });
        if let Err(error) = result {
            log::error!("Android finish(): {error}");
        }
    }));
}

/// Reconstruct egui's modifier state on Android.
///
/// winit's Android backend delivers `KeyboardInput` events but never emits `ModifiersChanged`
/// (it discards the `KeyEvent`'s meta state), so egui's modifier state stays empty and
/// `Ctrl`/`Alt` shortcuts are ignored while plain keys continue to work. Physical modifier keys
/// do arrive as `Event::Key` events, so we track them and stamp the resulting state onto this
/// frame's events, suppressing the text character that would otherwise be emitted alongside a
/// Ctrl/Command shortcut.
fn apply_android_modifiers(state: &mut egui::Modifiers, raw: &mut egui::RawInput) {
    let before = *state;
    let mut kept: Vec<egui::Event> = Vec::with_capacity(raw.events.len());
    for mut event in raw.events.drain(..) {
        match &mut event {
            egui::Event::Key { key, physical_key, pressed, modifiers, .. } => {
                let tracked = physical_key.unwrap_or(*key);
                match tracked {
                    egui::Key::ShiftLeft | egui::Key::ShiftRight => state.shift = *pressed,
                    egui::Key::ControlLeft | egui::Key::ControlRight => state.ctrl = *pressed,
                    egui::Key::AltLeft | egui::Key::AltRight => state.alt = *pressed,
                    _ => {}
                }
                state.mac_cmd = false;
                state.command = state.ctrl;
                *modifiers = *state;
                kept.push(event);
            }
            egui::Event::PointerButton { modifiers, .. } => {
                *modifiers = *state;
                kept.push(event);
            }
            egui::Event::MouseWheel { modifiers, .. } => {
                *modifiers = *state;
                kept.push(event);
            }
            egui::Event::Text(_) if state.ctrl || state.command || state.mac_cmd => {}
            egui::Event::WindowFocused(false) => {
                *state = egui::Modifiers::default();
                kept.push(event);
            }
            _ => kept.push(event),
        }
    }
    raw.events = kept;
    if *state != before {
        raw.events.push(egui::Event::ModifiersChanged(*state));
    }
}

// ---------------------------------------------------------------- runtime switches

/// Read an `env.txt` (`KEY=VALUE` per line) and set those variables, so a renderer, a backend or the
/// control server can be switched on the device without rebuilding the APK.
///
/// The app's own data dir is not writable from `adb push` (a release build is not debuggable), so
/// its external files dir is read too.
fn apply_env_overrides(dir: &std::path::Path) {
    let external = std::path::Path::new("/sdcard/Android/data/ai.storyteller.solidcraft/files");
    let text = std::fs::read_to_string(dir.join("env.txt")).or_else(|_| std::fs::read_to_string(external.join("env.txt"))).ok();
    let Some(text) = text else { return };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            let shown = if key.contains("TOKEN") { "<hidden>" } else { value };
            log::info!("env override: {key}={shown}");
            // SAFETY: called before any thread is spawned and before eframe reads the environment.
            unsafe { std::env::set_var(key, value) };
        }
    }
}

// ---------------------------------------------------------------- shell

struct Shell {
    solve: SolveApp,
    app: AndroidApp,
    bridge: saf::Bridge,
    state: Arc<SafState>,
    ime: ime_bridge::ime::ImeBridge,
    modifiers: Modifiers,
    keyboard_wanted: bool,
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.solve.logic(ctx);
        saf_poll(&self.bridge, &self.state);
        // The soft keyboard follows egui's focus: any focused TextEdit wants it.
        let wanted = ctx.egui_wants_keyboard_input();
        if wanted != self.keyboard_wanted {
            self.keyboard_wanted = wanted;
            ime_visibility(&self.app, wanted);
        }
        if self.solve.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        apply_android_modifiers(&mut self.modifiers, raw);
        self.ime.poll(raw);
        self.solve.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        system_fonts::install_system_fonts(ui.ctx());
        self.solve.ui(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(solidcraft_ui_egui::prefs::STORAGE_KEY, self.solve.prefs());
    }

    fn on_exit(&mut self) {
        solidcraft_ui_egui::agent_chat::shutdown(&mut self.solve);
        solidcraft_ui_egui::recovery_ui::close(&mut self.solve);
        solidcraft_engine::crash::end_session();
    }
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Debug).with_tag("SolidCraft"));
    log::info!("{ANDROID_SHELL_MARKER}");
    system_fonts::install_font_env(&app);
    let bridge = saf::Bridge::new(&app);
    if let Ok(dir) = bridge.files_dir() {
        apply_env_overrides(&dir);
    }
    let state: Arc<SafState> = Arc::new(SafState::default());
    let ime = ime_bridge::ime::ImeBridge::new(&app);
    let shell_app = app.clone();
    let app_finish = app.clone();
    let shell_bridge = bridge.clone_ref();
    let shell_state = Arc::clone(&state);
    solidcraft_engine::crash::phase("window");
    let options = eframe::NativeOptions {
        depth_buffer: DEPTH_BITS,
        stencil_buffer: STENCIL_BITS,
        multisampling: MSAA,
        android_app: Some(app),
        ..Default::default()
    };
    let result = eframe::run_native(
        "SolidCraft",
        options,
        Box::new(move |cc| {
            solidcraft_engine::crash::phase("app setup");
            solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
            let mut app_ui = SolveApp::new(Session::default(), services(shell_bridge.clone_ref(), Arc::clone(&shell_state)));
            app_ui.apply_off_thread = Some(solidcraft_ui_egui::dialogs::APPLY_AT_ONCE);
            // The viewport draws through an egui_wgpu paint callback, so the app needs the render
            // state: without this the UI works but the 3D scene (grid, origin planes, bodies) is
            // never painted and nothing can be selected.
            if let Some(rs) = &cc.wgpu_render_state {
                log::info!("graphics: {:?}, MSAA {}x", rs.adapter.get_info().backend, rs.msaa_samples);
                app_ui.set_wgpu(rs, DEPTH_BITS, STENCIL_BITS, rs.msaa_samples);
                solidcraft_ui_egui::gpu::install_picture_renderer(rs);
            } else {
                log::error!("no wgpu render state: the 3D viewport cannot be painted");
            }
            // Optional loopback control server, so tests and agents can click by name instead of
            // guessing screen coordinates (`adb forward tcp:9977 tcp:9977`, JSON lines).
            if let Some(port) = std::env::var("SOLIDCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse::<u16>().ok()) {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app_ui = app_ui.with_control(rx);
                log::info!("control server listening on 127.0.0.1:{port}");
            }
            Ok(Box::new(Shell {
                solve: app_ui,
                app: shell_app,
                bridge: shell_bridge,
                state: shell_state,
                ime,
                modifiers: Modifiers::default(),
                keyboard_wanted: false,
            }))
        }),
    );
    if let Err(error) = result {
        log::error!("SolidCraft could not start: {error}");
    }
    // eframe returning means the window is gone: on Android the activity would stay alive with no
    // surface (a black screen), so log it and finish the activity instead.
    log::info!("eframe event loop returned; finishing the activity");
    finish_activity(&app_finish);
}
