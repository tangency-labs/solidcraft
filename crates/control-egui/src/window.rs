//! The window's own control requests over `egui::Context`, which any app answers the same way.

use serde_json::Value;

/// The window is minimised or fully covered (another macOS Space), so it presents no frames
/// (#1677). Unknown counts as shown: the screenshot deadline still ends a wait for a frame that
/// never comes.
pub fn hidden(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().visible()) == Some(false)
}

/// `ui.resize {width, height}`: the window's inner size in points, kept within 320×240 and
/// 8192×8192.
pub fn resize(ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let f = |k: &str| params.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let (Some(w), Some(h)) = (f("width"), f("height")) else { return Err("missing width/height".into()) };
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w.clamp(320.0, 8192.0) as f32, h.clamp(240.0, 8192.0) as f32)));
    Ok(Value::Null)
}
