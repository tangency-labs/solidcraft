//! The UI kit's component gallery as a window (`cargo run -p ui-kit-gallery`) or in the browser
//! (`trunk build --release` here; the docs site serves it at docs.solidcraft.io/kit/).

/// The gallery as an eframe app: the whole window is the gallery's own layout.
struct App(ui_kit_gallery::Gallery);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.show(ui);
    }
}

fn app(cc: &eframe::CreationContext<'_>) -> App {
    App(ui_kit_gallery::Gallery::new(&cc.egui_ctx))
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    ui_kit_gallery::install_tables();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SolidCraft UI kit")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native("SolidCraft UI kit", options, Box::new(|cc| Ok(Box::new(app(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    ui_kit_gallery::install_tables();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let Some(canvas) = document.get_element_by_id("kit_canvas").and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"kit_canvas\">");
            return;
        };
        let mut options = eframe::WebOptions::default();
        // `?webgl` draws with WebGL2 where WebGPU misbehaves, as the web app allows.
        let query = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
        if query.contains("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let result = eframe::WebRunner::new().start(canvas, options, Box::new(|cc| Ok(Box::new(app(cc))))).await;
        if let Some(loading) = document.get_element_by_id("kit_loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => {
                    loading.set_inner_html(&format!("<p>The gallery failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>"))
                }
            }
        }
    });
}
