use serde_json::json;
use solvecraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, for_feature as edit_dialog};

/// A 100 x 60 base flange on XY (2.5 mm steel).
fn plate() -> SolveApp {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY", "name": "Base"})).unwrap();
    s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    SolveApp::new(s, Services::default())
}

fn run_all(app: &mut SolveApp, c: Vec<(String, Value)>) {
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
}

/// The picks decide the flange: a profile makes the base, edges then edge flanges with the
/// dialog's height, angle and bend position; Edit Feature brings the edges back.
#[test]
fn flange_follows_the_picks() {
    let mut app = plate();
    let sk = app.session.doc.features[0].id;
    app.session.selection = vec![Sel::Profile { sketch: sk, index: 0 }];
    app.start("sheet.flange");
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["type"], "base");
    assert_eq!(c[0].1["profiles"], json!([0]));
    run_all(&mut app, c);
    assert_eq!(app.session.model.state().sheets.len(), 1);
    // Four edge flanges at once, 20 high, outside.
    let edges: Vec<Sel> = [[50.0, 0.0, 2.5], [50.0, 60.0, 2.5], [0.0, 30.0, 2.5], [100.0, 30.0, 2.5]]
        .iter()
        .filter_map(|p| edge_sel(&app.session, Vec3::new(p[0], p[1], p[2])))
        .collect();
    assert_eq!(edges.len(), 4);
    app.session.selection = edges;
    app.start("sheet.flange");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Sheet(Sm::Flange { height, position, .. }) = &mut d.kind {
        *height = "20 mm".into();
        *position = 1;
    }
    assert_eq!(d.primary().map(|p| p.0), Some("Height"), "the arrow drives the height");
    assert!(arrow(&app, &d).is_some_and(|n| n.dist(Vec3::Z) < 1e-9), "edge flanges rise out of the sheet");
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["type"], "edge");
    assert_eq!(c[0].1["edges"].as_array().map(Vec::len), Some(4));
    assert_eq!(c[0].1["position"], "outside");
    run_all(&mut app, c);
    let f = app.session.doc.features.last().unwrap().clone();
    let e = edit_dialog(&app, f.id, None).unwrap();
    assert_eq!(e.inputs[0].items.len(), 4);
    assert!(matches!(&e.kind, Kind::Sheet(Sm::Flange { ty: 1, position: 1, .. })));
    // Rebuilt in place.
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(c[0].1["params"]["height"], "20 mm");
    // Flat pattern: four bends; Unfold previews.
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    assert_eq!(flat["bends"].as_array().map(Vec::len), Some(4), "{flat}");
    app.dialog = None;
    app.start("sheet.unfold");
    assert_eq!(apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap()[0].0, "sheet.unfold");
}

/// Hem takes edges and an optional gap; rules make and pick a rule with its expressions.
#[test]
fn hem_and_rules() {
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    app.session.selection = edge_sel(&app.session, Vec3::new(50.0, 0.0, 2.5)).into_iter().collect();
    app.start("sheet.hem");
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["length"], "5 mm");
    assert!(c[0].1.get("gap").is_none(), "the rule's gap");
    run_all(&mut app, c);
    app.dialog = None;
    app.start("sheet.manage_rules");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Sheet(Sm::Rules { pick, name, values, loaded, .. }) = &mut d.kind {
        *pick = 1;
        *loaded = Some(1);
        *name = "Alu".into();
        *values = vec![
            "1.5 mm".into(),
            "0.4".into(),
            "Thickness".into(),
            "Thickness".into(),
            "Thickness".into(),
            "Thickness".into(),
            "square".into(),
            "Thickness".into(),
            "Thickness".into(),
        ];
    }
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["name"], "Alu");
    assert_eq!(c[0].1["thickness"], "1.5 mm");
    run_all(&mut app, c);
    assert!(app.session.doc.sheet.rules.iter().any(|r| r.name == "Alu"));
}

#[test]
fn flat_exports_use_the_save_picker() {
    use std::sync::{Arc, Mutex};
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let picks = Arc::new(Mutex::new(Vec::new()));
    let captured = picks.clone();
    app.services.pick_save = Some(Box::new(move |name, extensions| {
        captured.lock().unwrap().push((name.to_string(), extensions.iter().map(|x| x.to_string()).collect::<Vec<_>>()));
        None
    }));
    assert_eq!(export_flat(&mut app, "Base", "csv"), "Export cancelled");
    assert_eq!(export_flat(&mut app, "Base", "dxf"), "Export cancelled");
    let picks = picks.lock().unwrap();
    assert!(picks[0].0.ends_with("-Base.csv"));
    assert_eq!(picks[0].1, vec!["csv"]);
    assert!(picks[1].0.ends_with("-Base.dxf"));
    assert_eq!(picks[1].1, vec!["dxf"]);
}

#[test]
fn shallow_pocket_overlay_labels_depth_without_drawing_a_through_void() {
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "Pocket"})).unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0": [20, 20], "p1": [30, 30]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.run("solid.extrude", json!({"sketch": "Pocket", "distance": 1, "operation": "cut"})).unwrap();
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    assert_eq!(flat["cutouts"], json!([]));
    assert_eq!(flat["pockets"][0]["depth_mm"], 1.0);
    app.start("sheet.flat_pattern");
    let mut dialog = app.dialog.clone().unwrap();
    let Kind::Sheet(Sm::FlatPattern { data, .. }) = &mut dialog.kind else { panic!("flat dialog") };
    *data = Some(Ok(flat));
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("flat-pocket-test")));
        assert!(overlay(&app, &painter, Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0)), &dialog));
    });
    output.textures_delta.clear();
    let texts: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"Bottom pocket: 1.00 mm deep"));
    assert!(texts.contains(&"Pocket boundary (material remains)"));
    let paths: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Path(path) => Some(path),
            _ => None,
        })
        .collect();
    assert!(!paths.is_empty(), "the retained sheet outline is drawn");
    assert!(paths.iter().all(|p| p.fill == Color32::TRANSPARENT), "pockets must not be rendered as filled voids");
    let lines = output.shapes.iter().filter(|s| matches!(s.shape, egui::Shape::LineSegment { .. })).count();
    assert!(lines > 6, "pocket contours and legend use dashed lines, not through-cut strokes");
}

#[test]
fn flat_pattern_islands_fit_before_the_actual_command_panel() {
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "Retain"})).unwrap();
    for x in [20, 60] {
        app.run("sketch.rectangle.two_point", json!({"p0": [x, 20], "p1": [x + 10, 30]})).unwrap();
    }
    app.run("sketch.finish", json!({})).unwrap();
    app.run("solid.extrude", json!({"sketch": "Retain", "profiles": "all", "through_all": true, "direction": "symmetric", "operation": "intersect"}))
        .unwrap();
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    assert_eq!(flat["outline"].as_array().unwrap().len(), 2);
    app.start("sheet.flat_pattern");
    let mut dialog = app.dialog.clone().unwrap();
    let Kind::Sheet(Sm::FlatPattern { data, .. }) = &mut dialog.kind else { panic!("flat dialog") };
    *data = Some(Ok(flat));
    let ctx = egui::Context::default();
    let id = egui::Id::new(("sc_dialog", "FLAT PATTERN"));
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
            crate::frame::docked("FLAT PATTERN", id, pos2(1400.0, 294.0), 540.0, crate::frame::Width::Normal).min_width(360.0).show(ui.ctx(), |ui| {
                ui.label("Flat size: 50 × 10 × 2.5 mm");
            });
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("island-fit")));
            assert!(overlay(&app, &painter, Rect::from_min_max(pos2(250.0, 126.0), pos2(1400.0, 832.0)), &dialog));
        });
        output.textures_delta.clear();
    }
    let panel = ctx.memory(|m| m.area_rect(id)).unwrap();
    let outlines: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Path(p) if p.closed && p.fill == Color32::TRANSPARENT => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(outlines.len(), 2, "both retained islands are drawn");
    assert!(
        outlines.iter().flat_map(|p| &p.points).all(|p| p.x <= panel.left() - 19.0),
        "both complete islands must remain exposed before the command panel: {panel:?}"
    );
}

#[test]
fn coincident_top_and_bottom_pocket_depths_share_a_readable_annotation() {
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch":"Base"})).unwrap();
    app.run("sketch.create", json!({"plane":"XY","name":"Retain"})).unwrap();
    app.run("sketch.rectangle.two_point", json!({"p0":[20,20],"p1":[30,30]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.run("solid.extrude", json!({"sketch":"Retain","distance":1,"start_offset":0.5,"operation":"intersect"})).unwrap();
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    assert_eq!(flat["cutouts"], json!([]));
    assert_eq!(flat["pockets"].as_array().unwrap().len(), 2);
    let doc = app.session.doc.to_json();
    app.start("sheet.flat_pattern");
    let mut dialog = app.dialog.clone().unwrap();
    let Kind::Sheet(Sm::FlatPattern { data, .. }) = &mut dialog.kind else { panic!("flat dialog") };
    *data = Some(Ok(flat));
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("coincident-pocket-test")));
        assert!(overlay(&app, &painter, Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0)), &dialog));
    });
    output.textures_delta.clear();
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().contains("pocket:") => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 1, "coincident depths must not overwrite one another");
    assert!(labels[0].contains("Top pocket: 1.00 mm deep"));
    assert!(labels[0].contains("Bottom pocket: 0.50 mm deep"));
    assert!(labels[0].contains('\n'));
    assert_eq!(app.session.doc.to_json(), doc);
}

#[test]
fn pocket_export_button_uses_the_picker_and_preserves_depth_layers() {
    let mut h = crate::scenario::Harness::new();
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY","name":"Base"})),
        ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[100,60]})),
        ("sketch.finish", json!({})),
        ("sheet.flange", json!({"sketch":"Base"})),
        ("sketch.create", json!({"plane":"XY","name":"Pocket"})),
        ("sketch.rectangle.two_point", json!({"p0":[20,20],"p1":[30,30]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"sketch":"Pocket","distance":1,"operation":"cut"})),
    ] {
        h.app.run(command, params).unwrap();
    }
    let path = h.dir.join("picked-pocket.dxf");
    let chosen = path.to_string_lossy().into_owned();
    h.app.services.pick_save = Some(Box::new(move |name, extensions| {
        assert!(name.ends_with("-Body1.dxf"));
        assert_eq!(extensions, &["dxf"]);
        Some(chosen.clone())
    }));
    h.app.start("sheet.flat_pattern");
    h.settle();
    let undo = h.app.session.undo.len();
    h.step(&json!({"widget":"Export DXF…"})).unwrap();
    let exported = std::fs::read_to_string(&path).expect("actual Export DXF button writes the picker destination");
    assert!(exported.contains("POCKET_BOTTOM_Z0.000000_TO_1.000000_MM"));
    assert!(exported.contains("OUTLINE"));
    assert_eq!(h.app.session.undo.len(), undo, "export does not alter the part");
    assert!(h.app.dialog.is_some(), "export keeps the flat viewer open");
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn bend_table_picker_writes_the_current_body() {
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    app.run("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20})).unwrap();
    let body = app.session.model.state().sheets[0].body.clone();
    let path = std::env::temp_dir().join(format!("solvecraft-bend-table-{}.csv", std::process::id()));
    let output = path.to_string_lossy().into_owned();
    let chosen = output.clone();
    app.services.pick_save = Some(Box::new(move |_, extensions| {
        assert_eq!(extensions, &["csv"]);
        Some(chosen.clone())
    }));
    let undo = app.session.undo.len();
    assert_eq!(export_flat(&mut app, &body, "csv"), format!("Saved {output}"));
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(contents.lines().count(), 2);
    assert!(contents.contains(",90.000000000,up,2.500000000,"), "{contents}");
    assert_eq!(app.session.undo.len(), undo);
}

#[test]
fn pocket_depth_labels_follow_inches_without_changing_flat_data() {
    let mut app = plate();
    app.run("document.units", json!({"units":"in"})).unwrap();
    assert_eq!(super::pocket_label(&app, &json!({"side":"top","depth_mm":25.4*0.1234})).as_deref(), Some("Top pocket: 0.1234 in deep"));
    assert!(super::pocket_label(&app, &json!({"side":"top","depth_mm":-1})).is_none());
}
