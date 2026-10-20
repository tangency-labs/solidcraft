//! SolidCraft for Android: a thin eframe shell over the shared UI, entered directly from
//! Android's `NativeActivity`. The APK holds this cdylib plus a small Java shim (`java/`,
//! about the Activity that catches the document pickers' answers — see [`saf`]).
//!
//! `cargo xtask android` checks it and its whole dependency graph for `aarch64-linux-android`
//! (arm64-v8a); cargo-apk2 reads `[package.metadata.android]` in Cargo.toml to package the APK.
//!
//! The shell runs the full [`solidcraft_ui_egui::SolveApp`] — start page, sketches, features,
//! timeline — with the desktop's settings, autosave and recovery folders, all under the app's
//! private storage (`android_main` points `HOME` and the XDG folders there), wgpu
//! over Vulkan, egui-winit's touch and pinch input, and the system Back button as Escape.
//! Files meet the system through [`saf`]: Open and Save As launch the Android document
//! pickers, exports go through them too, and the shell copies the engine's bytes to the
//! picked `content://` documents (bundled CJK fallback fonts and on-device screenshots are
//! still follow-ups).
//!
//! The crate denies `unsafe` (the workspace forbids it) so that the few entry points that
//! must reach Java — `android::android_main` (loaded by name) and the SAF JNI calls in
//! [`saf`] — allow it individually, each with its safety argument.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// The system file pickers and the copy-back of picked documents (`#[cfg(test)]` keeps the
/// pure part testable on the host; the JNI half compiles only on the device).
#[cfg(any(test, target_os = "android"))]
mod saf;

#[cfg(target_os = "android")]
mod android {
    use crate::saf;
    use solidcraft_engine::Session;
    use solidcraft_ui_egui::{Services, SolveApp};
    use winit::platform::android::activity::AndroidApp;

    /// The depth-stencil buffer the viewport asks for: the desktop app's, without its optional
    /// float depth (that one needs an adapter survey this shell does not do yet).
    const DEPTH_BITS: u8 = 24;
    const STENCIL_BITS: u8 = 8;
    /// The settings are checked this often (seconds) and written when they changed; also on
    /// exit. `eframe` never calls `App::save` here — it has no `Storage` on Android.
    const PREFS_INTERVAL_SECS: f64 = 5.0;

    /// What `android-activity` (through winit) calls on the Activity's main-loop thread. The
    /// NativeActivity glue looks this symbol up in the cdylib the Activity loads, so it must be
    /// exported under a fixed name — no safe API offers that, hence the `unsafe_code` allowance
    /// (it also points the environment at the app's private storage, before anything reads it).
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub fn android_main(app: AndroidApp) {
        // NativeActivity starts with `HOME=/`, which would scatter settings, the recovery
        // folder and crash reports over the read-only root: Android gives an app exactly one
        // private folder, and everything the engine resolves from the environment finds it
        // through this.
        let data = app.internal_data_path().or_else(|| app.external_data_path());
        match &data {
            Some(dir) =>
            // SAFETY: `android_main` runs before eframe, wgpu or any of our threads exist; the
            // only running code is the Activity glue, which does not read the environment
            // (edition 2024 made `env::set_var` unsafe for exactly this kind of startup call).
            unsafe {
                std::env::set_var("HOME", dir);
                std::env::set_var("XDG_DATA_HOME", dir.join(".local/share"));
                std::env::set_var("XDG_CONFIG_HOME", dir.join(".config"));
                std::env::set_var("XDG_CACHE_HOME", dir.join(".cache"));
            },
            None => log::error!("Android: no data path; settings, recovery and crash reports will not persist"),
        }
        // eframe sizes the egui render pass's depth-stencil attachment from these two, and the
        // viewport pipelines are built for exactly this format: with the 0/0 defaults wgpu rejects
        // every draw ("renderpass uses a texture with format None but the pipeline with 'sc_overlay'
        // uses ... Depth24PlusStencil8").
        // `setup` runs later on the render thread but the picker services need the Activity
        // (to launch SAF screens on the Java main thread) and the URI targets.
        let shell_app = app.clone();
        let targets = std::rc::Rc::new(saf::Targets::default());
        let shell_targets = targets.clone();
        let options = eframe::NativeOptions { depth_buffer: DEPTH_BITS, stencil_buffer: STENCIL_BITS, android_app: Some(app), ..Default::default() };
        let result = eframe::run_native("SolidCraft", options, Box::new(move |cc| setup(cc, shell_app, shell_targets)));
        if let Err(e) = result {
            log::error!("SolidCraft failed to start: {e}");
        }
    }

    fn setup(
        cc: &eframe::CreationContext<'_>,
        app: AndroidApp,
        targets: std::rc::Rc<saf::Targets>,
    ) -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
        solidcraft_engine::crash::phase("app setup");
        solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
        let mut solve = SolveApp::new(Session::default(), services(app.clone(), targets.clone()));
        // Heavy features compute off the window thread after 150 ms (the desktop's choice).
        solve.apply_off_thread = Some(solidcraft_ui_egui::dialogs::APPLY_AT_ONCE);
        let prefs_path = solidcraft_engine::recovery::data_dir().map(|dir| dir.join("prefs.json"));
        let loaded = prefs_path.as_ref().and_then(|path| std::fs::read_to_string(path).ok());
        if let Some(saved) = &loaded {
            solve.load_prefs(saved);
        }
        if let Some(rs) = &cc.wgpu_render_state {
            solve.set_wgpu(rs, DEPTH_BITS, STENCIL_BITS, rs.msaa_samples);
            solidcraft_ui_egui::gpu::install_picture_renderer(rs);
            let info = rs.adapter.get_info();
            solidcraft_engine::crash::set_environment(&format!(
                "Renderer: {:?} {} ({}), MSAA {}x",
                info.backend, info.name, info.driver, rs.msaa_samples
            ));
        }
        // Crashes leave a report in the crash folder with the design and the commands that
        // reproduce them (solidcraft_engine::crash, docs/crash-reports.md); fresh ones are
        // announced on the first frames, as on the desktop.
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
        solidcraft_ui_egui::crash_ui::announce(&mut solve, &crashed);
        // No launch file: the empty design takes the units preference of new designs (#1205)
        // and the start page offers New Design, Open, the samples and the recents.
        solidcraft_ui_egui::prefs::apply_new_design(&mut solve);
        let rev = solve.session.revision;
        solve.home.show(rev);
        // Autosave, and offer what earlier sessions left behind.
        if let Some(dir) = solidcraft_engine::recovery::default_dir() {
            let every = if solve.autosave_minutes > 0.0 { solve.autosave_minutes * 60.0 } else { f64::MAX / 4.0 };
            solidcraft_ui_egui::recovery_ui::start(&mut solve, &dir, std::time::Duration::from_secs_f64(every.min(1e9)));
        }
        solidcraft_engine::crash::phase("first frame");
        Ok(Box::new(Shell { solve, app, targets, prefs_path, saved_prefs: loaded, prefs_at: 0.0 }))
    }

    /// The host services: the system document pickers of [`saf`]. `pick_save` branches on the
    /// extensions the command offers — a design save gets the mirror-capable picker, every
    /// other export the one-shot picker; both answer with a future the shell completes once
    /// the system reports back.
    fn services(app: AndroidApp, targets: std::rc::Rc<saf::Targets>) -> Services {
        Services {
            pick_open: Some(Box::new({
                let (app, targets) = (app.clone(), targets.clone());
                move |_exts: &[&str]| Box::pin(saf::Picker::open(app.clone(), targets.clone()))
            })),
            pick_save: Some(Box::new(move |name: &str, exts: &[&str]| {
                if exts.contains(&solidcraft_engine::io::DESIGN_EXT) {
                    Box::pin(saf::Picker::design(app.clone(), targets.clone(), name))
                } else {
                    Box::pin(saf::Picker::export(app.clone(), targets.clone(), name))
                }
            })),
            ..Default::default()
        }
    }

    struct Shell {
        solve: SolveApp,
        /// The Activity handle the [`saf`] pickers launch their screens through (posted to
        /// the Java main thread — the render thread itself must not call into Java UI).
        app: AndroidApp,
        /// The picked documents this process still writes bytes to; `saf::deliver` syncs it.
        targets: std::rc::Rc<saf::Targets>,
        /// Where the settings live: `eframe` has no `Storage` on Android (`file_storage`
        /// returns `None` there), so the host keeps them in this file itself.
        prefs_path: Option<std::path::PathBuf>,
        /// The settings as they are on disk (`None` until the first successful write).
        saved_prefs: Option<String>,
        /// `solve.now` of the last settings check.
        prefs_at: f64,
    }

    impl Shell {
        /// Write the settings when they changed, at most every [`PREFS_INTERVAL_SECS`], and on
        /// exit (`force`). Through a temporary file so an interruption cannot truncate them;
        /// a failed write leaves the old file and is retried at the next check.
        fn save_prefs(&mut self, force: bool) {
            if !force && self.solve.now - self.prefs_at < PREFS_INTERVAL_SECS {
                return;
            }
            self.prefs_at = self.solve.now;
            let prefs = self.solve.prefs();
            if self.saved_prefs.as_deref() == Some(prefs.as_str()) {
                return;
            }
            let Some(path) = self.prefs_path.clone() else { return };
            let tmp = path.with_extension("json.tmp");
            let written = std::fs::write(&tmp, &prefs).and_then(|()| std::fs::rename(&tmp, &path));
            if let Err(e) = written {
                log::error!("settings: could not write {}: {e}", path.display());
                return;
            }
            self.saved_prefs = Some(prefs);
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            // The design on screen is the one whose commands a crash report replays.
            self.solve.session.crash_journal = solidcraft_engine::crash::enabled();
            self.solve.logic(ctx);
            // Export temporaries and linked mirrors reach their picked `content://` documents
            // here (the engine writes plain paths only); quiet unless a copy fails.
            saf::deliver(&self.app, &self.targets, &mut self.solve);
            if self.solve.quit_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            self.save_prefs(false);
        }
        fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
            // The system Back button reaches egui as `BrowserBack` (winit's Android keycode
            // map); SolidCraft's dialogs, tools and menus all act on Escape, so Back cancels
            // what is open exactly like Escape does — and, as on the desktop, does nothing
            // when nothing is. The system gestures leave the app; autosave covers the state.
            for event in &mut raw.events {
                let egui::Event::Key { key, .. } = event else { continue };
                if *key == egui::Key::BrowserBack {
                    *key = egui::Key::Escape;
                }
            }
            // Before the app's own hook, which adds the control channel's events.
            self.solve.raw_input_hook(raw);
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.solve.ui(ui);
        }
        fn on_exit(&mut self) {
            // The agent chat's agent and everything it started end with the app.
            solidcraft_ui_egui::agent_chat::shutdown(&mut self.solve);
            solidcraft_ui_egui::recovery_ui::close(&mut self.solve);
            solidcraft_engine::crash::end_session();
            self.save_prefs(true);
        }
    }
}
