//! SolidCraft for Android: a thin eframe shell over the shared UI, entered directly from
//! Android's `NativeActivity` (the APK holds this cdylib and no Java or Kotlin).
//!
//! `cargo xtask android` checks it and its whole dependency graph for `aarch64-linux-android`
//! (arm64-v8a); cargo-apk reads `[package.metadata.android]` in Cargo.toml to package the APK.
//! This first change is the platform's feasibility gate — geom through ui-egui must build for
//! Android and stay building; the real [`solidcraft_ui_egui::SolveApp`] wiring, touch input and
//! Android storage arrive with the app shell.
//!
//! The crate denies `unsafe` (the workspace forbids it) so that `android::android_main` alone
//! can allow it: the Activity's glue loads the exported symbol, which has no safe API.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "android")]
mod android {
    /// What `android-activity` (through winit) calls on the Activity's main-loop thread. The
    /// NativeActivity glue looks this symbol up in the cdylib the Activity loads, so it must be
    /// exported under a fixed name — no safe API offers that, hence the `unsafe_code` allowance.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
        let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
        let result = eframe::run_native("SolidCraft", options, Box::new(|_cc| Ok(Box::new(Shell))));
        if let Err(e) = result {
            log::error!("SolidCraft failed to start: {e}");
        }
    }

    /// The skeleton window until the app shell lands.
    struct Shell;

    impl eframe::App for Shell {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            ui.heading("SolidCraft");
        }
    }
}
