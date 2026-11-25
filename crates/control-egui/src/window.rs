//! The window's own control requests over `egui::Context`, which any app answers the same way.

use serde_json::Value;

/// The window is minimised or fully covered (another macOS Space), so it presents no frames
/// (#1677). Unknown counts as shown: the screenshot deadline still ends a wait for a frame that
/// never comes.
pub fn hidden(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().visible()) == Some(false)
}

/// `ui.resize {width, height}`: the window's inner size in points, kept within the app's `min`
/// (its window's minimum size) and 8192×8192.
pub fn resize(ctx: &egui::Context, params: &Value, min: [f32; 2]) -> Result<Value, String> {
    let f = |k: &str| params.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let (Some(w), Some(h)) = (f("width"), f("height")) else { return Err("missing width/height".into()) };
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2((w as f32).max(min[0]).min(8192.0), (h as f32).max(min[1]).min(8192.0))));
    Ok(Value::Null)
}
