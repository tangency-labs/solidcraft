//! SolidCraft for Android: a thin eframe shell over the shared UI, entered directly from
//! Android's `NativeActivity` (the APK holds this cdylib and no Java or Kotlin).
//!
//! `cargo xtask android` checks it and its whole dependency graph for `aarch64-linux-android`
//! (arm64-v8a); cargo-apk reads `[package.metadata.android]` in Cargo.toml to package the APK.
//!
//! The shell runs the full [`solidcraft_ui_egui::SolveApp`] — start page, sketches, features,
//! timeline — with the desktop's settings, autosave and recovery folders, all under the app's
//! private storage (`android_main` points `HOME` and the XDG folders there), wgpu
//! over Vulkan, egui-winit's touch and pinch input, and the system Back button as Escape.
//! Save As writes a new file into the app's designs folder (there is no system file picker in
//! this shell yet: Storage Access Framework wiring, bundled CJK fallback fonts and on-device
//! screenshots are follow-ups); Open cancels until that picker lands.
//!
//! The crate denies `unsafe` (the workspace forbids it) so that `android::android_main` alone
//! can allow it: the Activity's glue loads the exported symbol, which has no safe API.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "android")]
mod android {
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
    /// A file name from the dialog field stays this short (the field is user input).
    const MAX_NAME: usize = 180;
    /// Save As looks for a free name up to this many times before it gives up (cancel).
    const NAME_TRIES: usize = 100;

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
        let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
        let result = eframe::run_native("SolidCraft", options, Box::new(setup));
        if let Err(e) = result {
            log::error!("SolidCraft failed to start: {e}");
        }
    }

    fn setup(cc: &eframe::CreationContext<'_>) -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
        solidcraft_engine::crash::phase("app setup");
        solidcraft_ui_egui::fonts::setup_fonts(&cc.egui_ctx);
        let mut solve = SolveApp::new(Session::default(), services());
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
        Ok(Box::new(Shell { solve, prefs_path, saved_prefs: loaded, prefs_at: 0.0 }))
    }

    /// The host services. This shell has no system file picker: Save As writes a *new* file
    /// into the app's designs folder (a name that is taken gets a numbered one, so nothing is
    /// ever overwritten by this path) and Open is a plain cancel — `file_picker` treats a
    /// missing service as one — until Storage Access Framework wiring lands.
    fn services() -> Services {
        Services {
            pick_save: Some(Box::new(|name: &str, _exts: &[&str]| {
                let path = solidcraft_engine::recovery::data_dir().and_then(|dir| free_path(&dir.join("designs"), name));
                Box::pin(std::future::ready(path.map(|p| p.to_string_lossy().into_owned())))
            })),
            ..Default::default()
        }
    }

    /// The dialog's `name`, kept to one file name of a sane length (the field is user input:
    /// no directories, no `..`, no endless names).
    fn safe_name(name: &str) -> String {
        let base = std::path::Path::new(name.trim()).file_name().and_then(|n| n.to_str()).unwrap_or("");
        if base.is_empty() {
            return "design.solidcraft".into();
        }
        base.chars().take(MAX_NAME).collect()
    }

    /// A free path for Save As: `designs/<name>`, else `designs/<stem> (2)<ext>` and so on.
    /// Existing files are never replaced from here — the picker has no confirm dialog.
    fn free_path(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        let _ = std::fs::create_dir_all(dir);
        let base = safe_name(name);
        let (stem, ext) = match base.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), Some(e.to_string())),
            _ => (base.clone(), None),
        };
        for n in 1..=NAME_TRIES {
            let file = if n == 1 {
                dir.join(&base)
            } else {
                match &ext {
                    Some(e) => dir.join(format!("{stem} ({n}).{e}")),
                    None => dir.join(format!("{stem} ({n})")),
                }
            };
            // An unreadable entry counts as taken: never replace what cannot be listed.
            if matches!(std::fs::exists(&file), Ok(false)) {
                return Some(file);
            }
        }
        None
    }

    struct Shell {
        solve: SolveApp,
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
