//! The window on our own runner (`solidcraft_window_gpu::runner`) instead of eframe's, when asked
//! for with `SOLIDCRAFT_WINDOW_RUNNER=own`. eframe stays the default.
//!
//! The app is the same `eframe::App`: this adapter runs it on our runner's hooks, so pop-out
//! windows present without waiting for vsync and the blur draws through our own painter, with
//! egui-wgpu as it ships. What eframe does around the app, it does too: the settings file
//! (`app.ron`, written the same way), egui's memory in it, saving every `auto_save_interval` and
//! at exit, and the control channel's logic passes (#743). Not yet: the window's size and place
//! are not restored from the last run, and `Frame::storage_mut` is `None` during a pass (the app
//! saves through `App::save`).

use std::collections::HashMap;
use std::path::PathBuf;

use solidcraft_window_gpu::runner::{self, Created, Frame, RunOptions};

/// The variable that chooses the runner (after the `SOLIDCRAFT_` prefix): `own` runs ours.
pub const ENV: &str = "WINDOW_RUNNER";

/// `SOLIDCRAFT_WINDOW_RUNNER=own` asks for our own runner.
pub fn chosen() -> bool {
    solidcraft_engine::geom::env::var(ENV).is_some_and(|v| v.trim().eq_ignore_ascii_case("own"))
}

/// The settings file eframe keeps (`NativeOptions::persistence_path`): a RON map of strings.
struct Settings {
    path: Option<PathBuf>,
    kv: HashMap<String, String>,
}

impl Settings {
    fn open(path: Option<PathBuf>) -> Self {
        let kv = path
            .as_deref()
            .and_then(|p| eframe::read_settings(p).ok())
            .and_then(|text| ron::from_str::<HashMap<String, String>>(&text).ok())
            .unwrap_or_default();
        Self { path, kv }
    }
}

impl eframe::Storage for Settings {
    fn get_string(&self, key: &str) -> Option<String> {
        self.kv.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.kv.insert(key.to_owned(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.kv.remove(key);
    }
    fn flush(&mut self) {
        if let Some(path) = &self.path {
            eframe::save_to_disk(path, &self.kv);
        }
    }
}

/// egui's memory in the settings file, under eframe's key.
const EGUI_MEMORY: &str = "egui";

/// The app on our runner.
struct OnRunner<'a> {
    app: Box<dyn eframe::App + 'a>,
    frame: eframe::Frame,
    settings: Settings,
    ctx: egui::Context,
}

impl OnRunner<'_> {
    fn sync(&mut self, frame: &Frame<'_>) {
        if self.frame.wgpu_render_state.is_none() {
            self.frame.wgpu_render_state = frame.render_state().cloned();
        }
    }
}

impl runner::App for OnRunner<'_> {
    fn logic(&mut self, ctx: &egui::Context, frame: &Frame<'_>) {
        self.sync(frame);
        self.app.logic(ctx, &mut self.frame);
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &Frame<'_>) {
        self.sync(frame);
        self.app.ui(ui, &mut self.frame);
    }
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        self.app.clear_color(visuals)
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
    }
    fn repaint_on_window_event(&mut self, event: &winit::event::WindowEvent) -> Option<bool> {
        self.app.repaint_on_window_event(event)
    }
    fn repaint_on_device_event(&mut self, event: &winit::event::DeviceEvent) -> Option<bool> {
        self.app.repaint_on_device_event(event)
    }
    fn save(&mut self) {
        if self.app.persist_egui_memory() {
            let memory = self.ctx.memory(|m| ron::to_string(m).ok());
            if let Some(memory) = memory {
                eframe::Storage::set_string(&mut self.settings, EGUI_MEMORY, memory);
            }
        }
        self.app.save(&mut self.settings);
        eframe::Storage::flush(&mut self.settings);
    }
    fn auto_save_interval(&self) -> std::time::Duration {
        self.app.auto_save_interval()
    }
    fn on_exit(&mut self) {
        self.app.on_exit();
    }
}

/// Run the window on our own runner, as `eframe::run_native` would with `options` and `creator`:
/// the same viewport, device setup, settings file and app. `x11` starts winit on X11 (Linux).
pub fn run(options: eframe::NativeOptions, creator: eframe::AppCreator<'_>, x11: bool, wake: &crate::control_server::Wake) -> eframe::Result {
    solidcraft_engine::note!("SolidCraft: the window runs on its own runner (SOLIDCRAFT_{ENV}=own)");
    let renderer = eframe::egui_wgpu::RendererOptions {
        msaa_samples: u32::from(options.multisampling.max(1)),
        depth_stencil_format: eframe::egui_wgpu::depth_format_from_bits(options.depth_buffer, options.stencil_buffer),
        dithering: options.dithering,
        ..Default::default()
    };
    let transparent = options.viewport.transparent.unwrap_or(false);
    let mut event_loop_builder: Option<Box<dyn FnOnce(&mut winit::event_loop::EventLoopBuilder<runner::UserEvent>)>> = None;
    #[cfg(all(unix, not(target_os = "macos")))]
    if x11 {
        event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = x11;
    let settings = Settings::open(options.persistence_path.clone());
    let run_options = RunOptions { viewport: options.viewport, wgpu: options.wgpu_options, renderer, transparent, event_loop_builder };
    let failed: std::rc::Rc<std::cell::RefCell<Option<String>>> = std::rc::Rc::default();
    let failed_in_create = failed.clone();
    let wake = wake.clone();
    let ran = runner::run(run_options, move |created: Created| {
        let Created { ctx, render_state, waker } = created;
        wake.set(move || waker.run_logic());
        if let Some(memory) = settings.kv.get(EGUI_MEMORY).and_then(|m| ron::from_str::<egui::Memory>(m).ok()) {
            ctx.memory_mut(|m| *m = memory);
        }
        let mut cc = eframe::CreationContext::_new_kittest(ctx.clone());
        cc.storage = Some(&settings);
        cc.wgpu_render_state = render_state.clone();
        let made = creator(&cc);
        drop(cc);
        let mut frame = eframe::Frame::_new_kittest();
        frame.wgpu_render_state = render_state;
        match made {
            Ok(app) => Box::new(OnRunner { app, frame, settings, ctx }),
            Err(e) => {
                *failed_in_create.borrow_mut() = Some(e.to_string());
                Box::new(Closing)
            }
        }
    });
    if let Some(e) = failed.borrow_mut().take() {
        return Err(eframe::Error::AppCreation(e.into()));
    }
    ran.map_err(|e| eframe::Error::AppCreation(e.to_string().into()))
}

/// What runs when the app could not be made: it closes the window at once.
struct Closing;

impl runner::App for Closing {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &Frame<'_>) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    /// The settings file reads back what it wrote, in eframe's format.
    #[test]
    fn the_settings_file_keeps_what_was_saved() {
        let dir = std::env::temp_dir().join(format!("solidcraft-own-runner-{}", std::process::id()));
        let path = dir.join("app.ron");
        let mut settings = super::Settings::open(Some(path.clone()));
        eframe::Storage::set_string(&mut settings, "key", "value".into());
        eframe::Storage::flush(&mut settings);
        let read = super::Settings::open(Some(path));
        assert_eq!(eframe::Storage::get_string(&read, "key").as_deref(), Some("value"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
