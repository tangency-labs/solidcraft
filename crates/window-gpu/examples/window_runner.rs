//! Opens one window on the crate's own runner and painter (no eframe):
//! `cargo run -p solidcraft-window-gpu --example window_runner`.

use solidcraft_window_gpu::egui_wgpu;
use solidcraft_window_gpu::runner::{App, Frame, RunOptions, run};

struct Demo {
    clicks: u32,
}

impl App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &Frame<'_>) {
        // The runner's `ui` is the whole window already.
        let adapter = frame.render_state().map(|rs| rs.adapter.get_info());
        ui.label(format!("Adapter: {}", adapter.map_or_else(|| "none yet".to_owned(), |a| format!("{:?} {}", a.backend, a.name))));
        if ui.button(format!("Clicked {} times", self.clicks)).clicked() {
            self.clicks += 1;
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = RunOptions {
        viewport: egui::ViewportBuilder::default().with_title("window runner").with_inner_size([480.0, 320.0]),
        wgpu: egui_wgpu::WgpuConfiguration::default(),
        renderer: egui_wgpu::RendererOptions::default(),
        transparent: false,
        event_loop_builder: None,
    };
    run(options, |_| Box::new(Demo { clicks: 0 }))?;
    Ok(())
}
