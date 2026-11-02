//! A drawing's exports don't depend on the window (#4894 W23-3; Cam, 2 Nov): the sheet is built in
//! millimetres at a fixed dpi, so the window's scale and size never reach the file.

use serde_json::json;

use crate::scenario::Harness;

/// The drawing milestone's plate, 100 x 80 x 20 mm, and its front view at 1:2 on the drawing's A3
/// sheet, open in a headless window.
fn plate_drawing() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 100, "width": 80, "height": 20, "body_name": "Plate"})).unwrap();
    h.app.run("drawing.create", json!({})).unwrap();
    let view = json!({"sheet": "Sheet 1", "view": "front", "scale": "1:2", "position_mm": [150, 150]});
    h.app.run("drawing.view.base", view).unwrap();
    h.frames(3);
    h
}

/// The sheet as SVG and as a 150 dpi PNG, each written to a file by `drawing.export` sent over the
/// control channel (which carries the window's context, as an agent's export does), read back:
/// `tag` names the files.
fn exported(h: &mut Harness, tag: &str) -> [Vec<u8>; 2] {
    ["svg", "png"].map(|format| {
        let path = h.dir.join(format!("{tag}.{format}"));
        let dpi = (format == "png").then_some(150);
        let mut p = json!({"sheet": "Sheet 1", "format": format, "path": path.to_string_lossy()});
        if let Some(dpi) = dpi {
            p["dpi"] = json!(dpi);
        }
        let reply = h.call("engine.execute", json!({"command": "drawing.export", "params": p}));
        assert_eq!(reply["ok"], true, "{reply}");
        std::fs::read(&path).unwrap()
    })
}

/// Cam's two screens: the same session exports on a 4K screen (2 pixels a point, the window filling
/// it), is dragged to a 1080p screen (1 pixel a point, a smaller window) and draws a few frames,
/// then exports again; and at the harness's own scale and size. The window's context then has 2
/// and 1 pixels a point. The SVG and the PNG are the same bytes every time.
#[test]
fn a_drawing_exports_the_same_on_a_4k_and_a_1080p_screen() {
    let mut h = plate_drawing();
    let here = exported(&mut h, "here");
    // The 4K screen at 200 %: 3840 x 2160 pixels are 1920 x 1080 points.
    h.screen_scale = Some(2.0);
    h.call("ui.resize", json!({"width": 1920, "height": 1080}));
    h.frames(3);
    assert_eq!(h.ctx.pixels_per_point(), 2.0);
    let on_4k = exported(&mut h, "4k");
    // Dragged to the 1080p screen at 100 %, in a window smaller than the screen.
    h.screen_scale = Some(1.0);
    h.call("ui.resize", json!({"width": 1280, "height": 720}));
    h.frames(3);
    assert_eq!(h.ctx.pixels_per_point(), 1.0);
    let on_1080p = exported(&mut h, "1080p");
    for (screen, files) in [("4K", &on_4k), ("1080p", &on_1080p)] {
        assert!(files[0] == here[0], "the SVG on the {screen} screen differs");
        assert!(files[1] == here[1], "the PNG on the {screen} screen differs");
    }
    assert!(here[0].starts_with(b"<?xml") && here[1].starts_with(b"\x89PNG"));
}
