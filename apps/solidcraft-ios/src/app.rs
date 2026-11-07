use std::path::PathBuf;
use std::time::Duration;

use solidcraft_engine::Session;
use solidcraft_ui_egui::{SolveApp, fonts, gpu, prefs, recovery_ui, theme};
use winit::platform::ios::WindowExtIOS;

struct App {
    solve: SolveApp,
    preferences: PathBuf,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.solve.logic(ctx);
        // iOS may suspend/terminate without on_exit; don't leave autosave waiting for input.
        ctx.request_repaint_after(Duration::from_secs(2));
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.solve.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // The same safe-area layout as the macOS host, excluding notch and home indicator.
        let content = ui.ctx().content_rect();
        ui.painter().rect_filled(ui.max_rect(), 0.0, theme::Tokens::get().app_bar);
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| self.solve.ui(ui));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let settings = self.solve.prefs();
        storage.set_string(prefs::STORAGE_KEY, settings.clone());
        // eframe flushes its storage on a worker. iOS may freeze that worker as
        // soon as this callback returns, so keep an atomic, synchronous copy.
        if let Err(error) = solidcraft_engine::io::write_atomic(&self.preferences, settings.as_bytes(), true) {
            eprintln!("SolidCraft iOS preferences: {error}");
        }
        // Save is also called on iOS suspension, whereas application exit is not guaranteed.
        if let Err(error) = recovery_ui::flush(&mut self.solve) {
            eprintln!("SolidCraft iOS autosave: {error}");
        }
    }

    fn on_exit(&mut self) {
        solidcraft_ui_egui::save_job::tick(&mut self.solve, true);
        recovery_ui::close(&mut self.solve);
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("iOS did not provide a sandbox HOME")?;
    let documents = home.join("Documents");
    let support = home.join("Library/Application Support/SolidCraft");
    std::fs::create_dir_all(&documents)?;
    std::fs::create_dir_all(&support)?;
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = wgpu::Backends::METAL;
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_title("SolidCraft").with_fullscreen(true),
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 1,
        wgpu_options,
        persistence_path: Some(support.join("app.ron")),
        persist_window: false,
        run_and_return: false,
        ..Default::default()
    };
    eframe::run_native(
        "SolidCraft",
        options,
        Box::new(move |cc| {
            if let Some(window) = cc.winit_window() {
                window.recognize_pinch_gesture(true);
                window.recognize_pan_gesture(true, 2, 2);
            }
            fonts::setup_bundled_fonts(&cc.egui_ctx);
            let mut solve = SolveApp::new(Session::default(), crate::native::services(&documents, &cc.egui_ctx));
            solve.apply_off_thread = Some(solidcraft_ui_egui::dialogs::APPLY_AT_ONCE);
            let preferences = support.join("preferences.json");
            let settings = std::fs::read_to_string(&preferences).ok().or_else(|| cc.storage.and_then(|s| prefs::stored(|key| s.get_string(key))));
            if let Some(settings) = settings {
                solve.load_prefs(&settings);
            }
            // Exactly the desktop's offscreen CAD scene / single-sample egui composition.
            if let Some(rs) = &cc.wgpu_render_state {
                let samples = solve.set_wgpu_split(rs, 24, 8, 1);
                solve.msaa_supported = vec![samples];
                gpu::install_picture_renderer(rs);
            }
            recovery_ui::start(&mut solve, &support.join("recovery"), Duration::from_secs(15));
            Ok(Box::new(App { solve, preferences }))
        }),
    )?;
    Ok(())
}
