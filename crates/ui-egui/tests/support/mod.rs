use serde_json::json;
use solidcraft_ui_egui::scenario::Harness;

/// Identical tool-first line fixtures, retaining optional connect and scenario frame timing.
pub fn sketch_lines(points: &[[[i32; 2]; 2]], connect: Option<bool>) -> Harness {
    let mut h = Harness::new();
    let mut steps = vec![json!({"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}})];
    for points in points {
        let mut params = json!({"points":points,"infer":false});
        if let Some(connect) = connect {
            params["connect"] = json!(connect);
        }
        steps.push(json!({"call":"engine.execute","params":{"command":"sketch.line","params":params}}));
    }
    steps.push(json!({"call":"ui.view","params":{"view":"top"}}));
    h.run(&json!(steps)).unwrap();
    h
}
