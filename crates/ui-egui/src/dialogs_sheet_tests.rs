use crate::test_support::run_all;
use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, for_feature as edit_dialog};

/// A 100 x 60 base flange on XY (2.5 mm steel).
fn plate(mode: crate::shell::LayoutMode) -> SolveApp {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY", "name": "Base"})).unwrap();
    s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    app.ui.layout = mode;
    app
}

/// The picks decide the flange: a profile makes the base, edges then edge flanges with the
/// dialog's height, angle and bend position; Edit Feature brings the edges back.
#[test]
fn flange_follows_the_picks() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
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
}

/// A click on the plate's bottom edge picks the edge, not the base flange's sketch line lying on
/// it: the flange is an edge flange with a bend, not a flat contour wall (#6804).
#[test]
fn a_click_on_the_bottom_edge_makes_an_edge_flange() {
    let mut h = crate::scenario::Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "front"}})).unwrap();
    h.settle();
    h.app.start("sheet.flange");
    h.frames(3);
    // The bottom edge lies on the sketch plane, under the sketch's line from (0, 0) to (100, 0).
    let at = h.at(&json!({"world": [50, 0, 0]})).unwrap();
    h.step(&json!({"click": at})).unwrap();
    let items = h.app.dialog.as_ref().unwrap().inputs[0].items.clone();
    assert!(matches!(items.as_slice(), [Sel::Edge { point, .. }] if point.z.abs() < 1e-9 && point.y.abs() < 1e-9), "{items:?}");
    let c = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["type"], "edge", "{:?}", c[0].1);
}

/// Hem takes edges and an optional gap; rules make and pick a rule with its expressions.
#[test]
fn hem_and_rules() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
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
}

#[test]
fn flat_exports_use_the_save_picker() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        use std::sync::{Arc, Mutex};
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let picks = Arc::new(Mutex::new(Vec::new()));
        let captured = picks.clone();
        app.services.pick_save = Some(Box::new(move |name, extensions| {
            captured.lock().unwrap().push((name.to_string(), extensions.iter().map(|x| x.to_string()).collect::<Vec<_>>()));
            Box::pin(std::future::ready(None))
        }));
        assert_eq!(export_flat(&mut app, "Base", "csv"), "Choose an export file…");
        crate::file_picker::tick(&mut app, &crate::fonts::test_context());
        assert_eq!(export_flat(&mut app, "Base", "dxf"), "Choose an export file…");
        crate::file_picker::tick(&mut app, &crate::fonts::test_context());
        let picks = picks.lock().unwrap();
        assert!(picks[0].0.ends_with("-Base.csv"));
        assert_eq!(picks[0].1, vec!["csv"]);
        assert!(picks[1].0.ends_with("-Base.dxf"));
        assert_eq!(picks[1].1, vec!["dxf"]);
    }
}

#[test]
fn shallow_pocket_overlay_labels_depth_without_drawing_a_through_void() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
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
        let ctx = crate::fonts::test_context();
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("flat-pocket-test")));
            assert!(overlay(&mut app, &painter, Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0)), &dialog));
        });
        let texts: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.text()).collect();
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
}

/// The Flat Pattern panel shows each side of the flat size, and the thickness, whole: on one row
/// ("1250.00 mm × 1000.00 mm × 2.50 mm") the panel cut it off after the first side.
#[test]
fn flat_pattern_shows_each_side_of_the_flat_size_whole() {
    for layout in ["docked", "floating"] {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.resize", json!({"width": 1600, "height": 1200}));
        h.call("ui.set", json!({"layout": layout}));
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [1250, 1000]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        h.app.start("sheet.flat_pattern");
        h.frames(4);
        let output = h.ctx.run_ui(egui::RawInput { screen_rect: Some(h.ctx.content_rect()), focused: true, ..Default::default() }, |ui| {
            h.app.logic(ui.ctx());
            h.app.ui(ui);
        });
        let texts: Vec<_> = crate::test_support::text_shapes(&output).collect();
        for (label, value) in [("Flat size X", "1250.00 mm"), ("Flat size Y", "1000.00 mm"), ("Thickness", "2.50 mm")] {
            assert!(texts.iter().any(|t| t.galley.text() == label), "{layout}: no {label} row");
            let shown = texts.iter().find(|t| t.galley.job.text == value).unwrap_or_else(|| panic!("{layout}: {label} {value} not painted"));
            assert!(!shown.galley.elided, "{layout}: {label} is cut off");
        }
        assert!(!texts.iter().any(|t| t.galley.text().contains('×')), "{layout}: the sides are no longer run together");
    }
}

#[test]
fn flat_pattern_islands_fit_before_the_actual_command_panel() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        app.run("sketch.create", json!({"plane": "XY", "name": "Retain"})).unwrap();
        for x in [20, 60] {
            app.run("sketch.rectangle.two_point", json!({"p0": [x, 20], "p1": [x + 10, 30]})).unwrap();
        }
        app.run("sketch.finish", json!({})).unwrap();
        app.run(
            "solid.extrude",
            json!({"sketch": "Retain", "profiles": "all", "through_all": true, "direction": "symmetric", "operation": "intersect"}),
        )
        .unwrap();
        let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
        assert_eq!(flat["outline"].as_array().unwrap().len(), 2);
        app.start("sheet.flat_pattern");
        let mut dialog = app.dialog.clone().unwrap();
        let Kind::Sheet(Sm::FlatPattern { data, .. }) = &mut dialog.kind else { panic!("flat dialog") };
        *data = Some(Ok(flat));
        let viewport = Rect::from_min_max(pos2(250.0, crate::toolbar::APP_BAR_HEIGHT + crate::toolbar::RIBBON_HEIGHT), pos2(1400.0, 832.0));
        let browser = Rect::from_min_max(viewport.min, viewport.min + vec2(250.0, 450.0));
        let history = Rect::from_min_max(pos2(viewport.left(), viewport.bottom() - 40.0), viewport.max);
        if mode == crate::shell::LayoutMode::Floating {
            app.shell.content = Some(Rect::from_min_max(pos2(browser.right(), viewport.top()), pos2(viewport.right(), history.top())));
        }
        let ctx = crate::fonts::test_context();
        let id = egui::Id::new(("sc_dialog", "FLAT PATTERN"));
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output =
                ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), ..Default::default() }, |ui| {
                    crate::ui_kit::Panel::new("FLAT PATTERN", id, pos2(1400.0, 294.0), viewport, crate::frame::Side::Right, egui::Frame::NONE)
                        .width(360.0)
                        .window(&ctx)
                        .show(ui.ctx(), |ui| {
                            ui.label("Flat size: 50 × 10 × 2.5 mm");
                        });
                    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("island-fit")));
                    assert!(overlay(
                        &mut app,
                        &painter,
                        Rect::from_min_max(pos2(250.0, crate::toolbar::APP_BAR_HEIGHT + crate::toolbar::RIBBON_HEIGHT), pos2(1400.0, 832.0)),
                        &dialog
                    ));
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
        if mode == crate::shell::LayoutMode::Floating {
            assert!(
                outlines.iter().flat_map(|p| &p.points).all(|p| !browser.contains(*p) && !history.contains(*p)),
                "flat pattern fits clear of the shell cards"
            );
        }
    }
}

#[test]
fn coincident_top_and_bottom_pocket_depths_share_a_readable_annotation() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
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
        let ctx = crate::fonts::test_context();
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("coincident-pocket-test")));
            assert!(overlay(&mut app, &painter, Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0)), &dialog));
        });
        let labels: Vec<_> =
            crate::test_support::text_shapes(&output).map(|text| text.galley.text()).filter(|text| text.contains("pocket:")).collect();
        assert_eq!(labels.len(), 1, "coincident depths must not overwrite one another");
        assert!(labels[0].contains("Top pocket: 1.00 mm deep"));
        assert!(labels[0].contains("Bottom pocket: 0.50 mm deep"));
        assert!(labels[0].contains('\n'));
        assert_eq!(app.session.doc.to_json(), doc);
    }
}

#[test]
fn pocket_export_button_uses_the_picker_and_preserves_depth_layers() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"layout":mode}));
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
            Box::pin(std::future::ready(Some(chosen.clone())))
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
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn bend_table_picker_writes_the_current_body() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        app.run("sheet.flange", json!({"edges": [[50, 0, 2.5]], "height": 20})).unwrap();
        let body = app.session.model.state().sheets[0].body.clone();
        let path = std::env::temp_dir().join(format!("solidcraft-bend-table-{}.csv", std::process::id()));
        let output = path.to_string_lossy().into_owned();
        let chosen = output.clone();
        app.services.pick_save = Some(Box::new(move |_, extensions| {
            assert_eq!(extensions, &["csv"]);
            Box::pin(std::future::ready(Some(chosen.clone())))
        }));
        let undo = app.session.undo.len();
        assert_eq!(export_flat(&mut app, &body, "csv"), "Choose an export file…");
        crate::file_picker::tick(&mut app, &crate::fonts::test_context());
        assert_eq!(app.status.as_ref().map(|s| &s.0), Some(&format!("Exported {output}")));
        let contents = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(contents.lines().count(), 2);
        assert!(contents.contains(",90.000000000,up,2.500000000,"), "{contents}");
        assert_eq!(app.session.undo.len(), undo);
    }
}

/// The rule manager's shape selector: picked in the dialog, the shape reaches the rule and the part
/// made with it, and the dialog shows it the next time.
#[test]
fn rules_dialog_picks_the_corner_relief_shape() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app
            .run(
                "sheet.manage_rules",
                json!({"name": "AL2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "gap": "0.5 mm", "active": true}),
            )
            .unwrap();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [120, 90]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        h.app.run("sheet.flange", json!({"edges": [[60, 0, 2], [120, 45, 2], [60, 90, 2], [0, 45, 2]], "height": 20, "angle": "72 deg"})).unwrap();
        // The rounds in the base of the tray, and the shape the open dialog shows.
        let rounds = |app: &SolveApp| {
            let state = app.session.model.state();
            state.sheets.last().map(|s| s.base.outer.segs.iter().filter(|g| matches!(g, solidcraft_engine::geom::Seg2::Arc { .. })).count())
        };
        let shown = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Kind::Sheet(Sm::Rules { relief, .. })) => Some(*relief),
            _ => None,
        };
        assert_eq!(rounds(&h.app), Some(0));
        h.step(&json!({"start": "sheet.manage_rules"})).unwrap();
        h.frames(3);
        assert_eq!(shown(&h.app), Some(0), "a rule starts with the notch");
        h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape:Round"}})).unwrap();
        assert_eq!(shown(&h.app), Some(1));
        h.step(&json!({"widget": "OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert_eq!(h.app.session.doc.sheet_rule(None).corner_relief_shape, "round");
        assert_eq!(rounds(&h.app), Some(4), "one round at each corner of the tray");
        h.step(&json!({"start": "sheet.manage_rules"})).unwrap();
        h.frames(3);
        assert_eq!(shown(&h.app), Some(1), "the dialog shows the rule's shape");
        // The tear: no relief in the base, both ends of all four bends torn.
        h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape:Tear"}})).unwrap();
        assert_eq!(shown(&h.app), Some(3));
        h.step(&json!({"widget": "OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert_eq!(h.app.session.doc.sheet_rule(None).corner_relief_shape, "tear");
        let torn = h.app.session.model.state().sheets.last().map(|s| s.flanges.iter().filter(|f| f.tear == (true, true)).count());
        assert_eq!((rounds(&h.app), torn), (Some(0), Some(4)));
    }
}

/// The rule manager's angle display: picked in the dialog, it reaches the rule and the bend notes
/// of the part made with it, and the dialog shows it the next time.
#[test]
fn rules_dialog_sets_how_bend_angles_read() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("sheet.manage_rules", json!({"name": "AL2", "thickness": "2 mm", "bend_radius": "1 mm", "active": true})).unwrap();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        h.app.run("sheet.flange", json!({"edges": [[50, 0, 2]], "height": 20, "angle": "58 deg"})).unwrap();
        let shown = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Kind::Sheet(Sm::Rules { angles, .. })) => Some(*angles),
            _ => None,
        };
        let note = |app: &mut SolveApp| {
            let dxf = app.session.execute("sheet.export_dxf", &json!({"bend_notes": true})).unwrap();
            dxf["dxf"].as_str().unwrap_or_default().lines().map(str::trim).find(|l| l.starts_with("B1 ")).map(str::to_string)
        };
        assert_eq!(note(&mut h.app).as_deref(), Some("B1 UP 58%%d R1"));
        h.step(&json!({"start": "sheet.manage_rules"})).unwrap();
        h.frames(3);
        assert_eq!(shown(&h.app), Some(0), "a rule starts with bend angles");
        h.step(&json!({"click": {"handle": "dialog:Angle Display"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Angle Display:Included"}})).unwrap();
        assert_eq!(shown(&h.app), Some(1));
        h.step(&json!({"widget": "OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert_eq!(h.app.session.doc.sheet_rule(None).angle_display, "included");
        assert_eq!(note(&mut h.app).as_deref(), Some("B1 UP 122%%d R1"));
        h.step(&json!({"start": "sheet.manage_rules"})).unwrap();
        h.frames(3);
        assert_eq!(shown(&h.app), Some(1), "the dialog shows the rule's display");
        // Back to bend angles; the rule's other values are as they were.
        h.step(&json!({"click": {"handle": "dialog:Angle Display"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Angle Display:Bend"}})).unwrap();
        h.step(&json!({"widget": "OK"})).unwrap();
        let rule = h.app.session.doc.sheet_rule(None);
        assert_eq!((rule.angle_display.as_str(), rule.thickness.as_str()), ("bend", "2 mm"));
        assert_eq!(note(&mut h.app).as_deref(), Some("B1 UP 58%%d R1"));
    }
}

/// The rule manager's rectangular relief, on a base whose corners are not square: the shape and
/// the size the dialog shows reach the part, the bends and the flat pattern.
#[test]
fn the_rule_manager_relieves_a_corner_that_is_not_square() {
    let mut h = crate::scenario::Harness::new();
    h.app
        .run(
            "sheet.manage_rules",
            json!({"name": "AL2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "gap": "0.5 mm", "corner_relief": "8 mm", "active": true}),
        )
        .unwrap();
    // A rhombus whose corners are 60° and 120°, with a flange on each of its four edges.
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[0, 0], [120, 0], [165, 77.9422863406], [45, 77.9422863406]], "closed": true})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.app
        .run(
            "sheet.flange",
            json!({"edges": [[60, 0, 2], [142.5, 38.9711431703, 2], [105, 77.9422863406, 2], [22.5, 38.9711431703, 2]], "height": 38.6, "angle": "90 deg"}),
        )
        .unwrap();
    // The base of the notch has its four corners, and the bends run the whole length of the edges.
    let base = |app: &SolveApp| {
        let state = app.session.model.state();
        state.sheets.last().map(|s| (s.base.outer.segs.len(), s.flanges.iter().map(|f| s.zone_span(f).1 - s.zone_span(f).0).collect::<Vec<_>>()))
    };
    let shown = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Sheet(Sm::Rules { relief, .. })) => Some(*relief),
        _ => None,
    };
    let (segs, notch_bends) = base(&h.app).expect("the sheet");
    assert_eq!(segs, 4, "the notch cuts nothing out of the base");
    h.step(&json!({"start": "sheet.manage_rules"})).unwrap();
    h.frames(3);
    assert_eq!(shown(&h.app), Some(0), "a rule starts with the notch");
    // The rectangular relief: the shape the dialog offers, and the size the rule carries.
    h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape:Rectangular"}})).unwrap();
    assert_eq!(shown(&h.app), Some(2));
    h.frames(3);
    assert!(h.app.preview.error.is_none(), "the preview: {:?}", h.app.preview.error);
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    assert_eq!(h.app.session.doc.sheet_rule(None).corner_relief_shape, "rectangular");
    assert_eq!(h.app.session.doc.sheet_rule(None).corner_relief, "8 mm");
    // A kite at each of the four corners: two more edges than the notch's four.
    let (segs, relieved) = base(&h.app).expect("the sheet");
    assert_eq!(segs, 12, "a kite at each corner of the base");
    for (was, now) in notch_bends.iter().zip(&relieved) {
        assert!((was - now - 8.0).abs() < 1e-6, "{notch_bends:?} and {relieved:?}: the bends are 8 mm shorter");
    }
    // The flat pattern is one loop, and the part folds.
    let flat = h.app.session.execute("sheet.flat_pattern", &json!({})).expect("the flat pattern");
    assert_eq!(flat["outline"].as_array().map(Vec::len), Some(1), "{flat}");
    assert_eq!(flat["cutouts"], json!([]), "{flat}");
    assert!(h.app.preview.error.is_none(), "{:?}", h.app.preview.error);
}

/// The Flange dialog's relief rows: Rule (the default) and an empty size leave the relief to the rule,
/// a shape and a size reach the feature, Edit Feature shows them again, and Rule takes them back.
#[test]
fn flange_dialog_picks_a_corner_relief_of_its_own() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app
            .run(
                "sheet.manage_rules",
                json!({"name": "AL2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "gap": "0.5 mm", "active": true}),
            )
            .unwrap();
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [120, 90]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let edges: Vec<Sel> =
            [[60.0, 0.0, 2.0], [0.0, 45.0, 2.0]].iter().filter_map(|p| edge_sel(&h.app.session, Vec3::new(p[0], p[1], p[2]))).collect();
        assert_eq!(edges.len(), 2);
        h.app.session.selection = edges;
        h.app.start("sheet.flange");
        h.frames(3);
        let shown = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Kind::Sheet(Sm::Flange { relief, relief_size, .. })) => Some((*relief, relief_size.clone())),
            _ => None,
        };
        let relief = |app: &SolveApp| match &app.session.doc.features.last().map(|f| &f.kind) {
            Some(FeatureKind::SheetFlange { corner_relief_shape, corner_relief, .. }) => Some((corner_relief_shape.clone(), corner_relief.clone())),
            _ => None,
        };
        // Untouched, the dialog asks for nothing: the rule's relief.
        assert_eq!(shown(&h.app), Some((0, String::new())));
        let d = h.app.dialog.clone().unwrap();
        let c = apply_commands(&h.app, &d).unwrap();
        assert!(c[0].1.get("corner_relief_shape").is_none() && c[0].1.get("corner_relief").is_none(), "{}", c[0].1);
        // The optional overrides live in the collapsed Advanced group.
        h.step(&json!({"click": {"handle": "dialog:section:Advanced"}})).unwrap();
        // Round from the selector, 6 mm in the size row.
        h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Corner Relief Shape:Round"}})).unwrap();
        assert_eq!(shown(&h.app), Some((2, String::new())));
        if let Some(Kind::Sheet(Sm::Flange { relief_size, .. })) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            *relief_size = "6 mm".into();
        }
        let d = h.app.dialog.clone().unwrap();
        let c = apply_commands(&h.app, &d).unwrap();
        assert_eq!((&c[0].1["corner_relief_shape"], &c[0].1["corner_relief"]), (&json!("round"), &json!("6 mm")));
        h.step(&json!({"widget": "OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        assert_eq!(relief(&h.app), Some((Some("round".into()), Some("6 mm".into()))));
        assert!(h.app.session.model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", h.app.session.model.results);
        let arcs = |app: &SolveApp| {
            app.session
                .model
                .state()
                .sheets
                .last()
                .map(|s| s.base.outer.segs.iter().filter(|g| matches!(g, solidcraft_engine::geom::Seg2::Arc { .. })).count())
        };
        assert_eq!(arcs(&h.app), Some(1), "the round is cut at the one corner the two flanges make");
        // Edit Feature shows both again; Rule and an empty size take the feature back to the rule's relief.
        let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
        let mut e = edit_dialog(&h.app, id, None).unwrap();
        assert!(matches!(&e.kind, Kind::Sheet(Sm::Flange { relief: 2, relief_size, .. }) if relief_size == "6 mm"));
        let c = apply_commands(&h.app, &e).unwrap();
        assert_eq!((&c[0].1["params"]["corner_relief_shape"], &c[0].1["params"]["corner_relief"]), (&json!("round"), &json!("6 mm")));
        if let Kind::Sheet(Sm::Flange { relief, relief_size, .. }) = &mut e.kind {
            *relief = 0;
            relief_size.clear();
        }
        let commands = apply_commands(&h.app, &e).unwrap();
        run_all(&mut h.app, commands);
        assert_eq!(relief(&h.app), Some((None, None)));
        assert_eq!(arcs(&h.app), Some(0));
    }
}

#[test]
fn flange_dialog_picks_corner_overlap_and_clears_it_on_edit() {
    let mut h = crate::scenario::Harness::new();
    h.app
        .run(
            "sheet.manage_rules",
            json!({"name": "AL2", "thickness": "2 mm", "k_factor": 0.5, "bend_radius": "1 mm", "gap": "0.5 mm", "active": true}),
        )
        .unwrap();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [120, 90]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.app.session.selection =
        [Vec3::new(60.0, 0.0, 2.0), Vec3::new(0.0, 45.0, 2.0)].into_iter().filter_map(|point| edge_sel(&h.app.session, point)).collect();
    h.app.start("sheet.flange");
    h.frames(3);
    let shown = |app: &SolveApp| match app.dialog.as_ref().map(|dialog| &dialog.kind) {
        Some(Kind::Sheet(Sm::Flange { corners, .. })) => corners.clone(),
        _ => Vec::new(),
    };
    assert_eq!(shown(&h.app).len(), 1);
    assert_eq!(shown(&h.app)[0].kind, 0);
    h.step(&json!({"click": {"handle": "dialog:Customize each corner"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Corner Front / Left"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Corner Front / Left:Partial Overlap"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Covering Front / Left"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Covering Front / Left:Left"}})).unwrap();
    h.frames(3);
    let choices = shown(&h.app);
    assert_eq!(choices[0].kind, 1);
    assert!(choices[0].cover_second);
    assert_eq!(choices[0].overlap, "1 mm");
    assert!(h.app.preview.error.is_none(), "{:?}", h.app.preview.error);
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let saved = &commands[0].1["corners"][0];
    assert_eq!(saved["corner_type"], "partial");
    assert_eq!(saved["overlap"], "1 mm");
    assert_eq!(saved["covering"], pt(choices[0].second));
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    let id = h.app.session.doc.features.last().unwrap().id;
    let mut edited = edit_dialog(&h.app, id, None).unwrap();
    assert!(
        matches!(&edited.kind, Kind::Sheet(Sm::Flange { corners, .. }) if corners.len() == 1 && corners[0].kind == 1 && corners[0].first.dist(choices[0].second) < 1e-6)
    );
    if let Kind::Sheet(Sm::Flange { corners, .. }) = &mut edited.kind {
        corners[0].kind = 2;
    }
    let commands = apply_commands(&h.app, &edited).unwrap();
    assert_eq!(commands[0].1["params"]["corners"][0]["corner_type"], "full");
    assert!(commands[0].1["params"]["corners"][0].get("overlap").is_none());
    run_all(&mut h.app, commands);
    assert!(h.app.session.model.results.iter().all(|result| result.error.is_none()));
    if std::env::var_os("SOLIDCRAFT_SCENARIO_SHOTS").is_some() {
        h.step(&json!({"shot": "corner-overlap-folded"})).unwrap();
        h.app.run("sheet.unfold", json!({})).unwrap();
        h.step(&json!({"shot": "corner-overlap-flat"})).unwrap();
        h.app.run("sheet.refold", json!({})).unwrap();
    }
    if let Kind::Sheet(Sm::Flange { corners, .. }) = &mut edited.kind {
        corners[0].kind = 0;
    }
    let commands = apply_commands(&h.app, &edited).unwrap();
    assert_eq!(commands[0].1["params"]["corners"], json!([]));
    run_all(&mut h.app, commands);
    assert!(
        matches!(&h.app.session.doc.features.iter().find(|feature| feature.id == id).unwrap().kind, FeatureKind::SheetFlange { corners, .. } if corners.is_empty())
    );
}

#[test]
fn flange_dialog_does_not_guess_wall_names_for_a_rotated_profile() {
    let mut app = SolveApp::new(Session::default(), Services::default());
    app.run("sketch.create", json!({"plane":"XY","name":"Base"})).unwrap();
    let outline = [[0.0, 0.0], [60.0, 30.0], [40.0, 70.0], [-20.0, 40.0], [0.0, 0.0]];
    for edge in outline.windows(2) {
        app.run("sketch.line", json!({"points":edge})).unwrap();
    }
    app.run("sketch.finish", json!({})).unwrap();
    app.run("sheet.flange", json!({"sketch":"Base"})).unwrap();
    let points: Vec<_> = outline.windows(2).map(|edge| Vec3::new((edge[0][0] + edge[1][0]) * 0.5, (edge[0][1] + edge[1][1]) * 0.5, 2.5)).collect();
    assert_eq!(app.session.world_state().sheets[0].flange_corner_pairs(&points).len(), 4);
    let walls = rectangle_walls(&app, &points);
    assert_eq!(walls, vec![None; 4]);
    assert_eq!(wall_name(&walls, 0), "Wall 1");
}

#[test]
fn flange_dialog_common_corners_keep_custom_values_and_edge_identity() {
    let mut h = crate::scenario::Harness::new();
    h.app = plate(crate::shell::LayoutMode::Floating);
    h.app.run("sheet.flange", json!({"sketch":"Base"})).unwrap();
    let points = [Vec3::new(50.0, 0.0, 2.5), Vec3::new(50.0, 60.0, 2.5), Vec3::new(0.0, 30.0, 2.5), Vec3::new(100.0, 30.0, 2.5)];
    h.app.session.selection = points.iter().filter_map(|point| edge_sel(&h.app.session, *point)).collect();
    h.app.start("sheet.flange");
    h.frames(3);
    assert_eq!(rectangle_walls(&h.app, &points), [Some(0), Some(1), Some(2), Some(3)]);
    let picture = crate::scenario::handle_at("dialog:All Corners:Butt").unwrap();
    let customize = crate::scenario::handle_at("dialog:Customize each corner").unwrap();
    assert!(customize.y > picture.y + 38.0, "the next row must clear the full picture button");
    h.step(&json!({"click":{"handle":"dialog:All Corners:Full Overlap"}})).unwrap();
    h.frames(3);
    let settings = |app: &SolveApp| {
        let dialog = app.dialog.as_ref().unwrap();
        let points = edge_point_list(&dialog.inputs);
        let walls = rectangle_walls(app, &points);
        let Kind::Sheet(Sm::Flange { corners, .. }) = &dialog.kind else { panic!("not a flange") };
        corners.iter().map(|corner| (corner.kind, covering_group(corner, &points, &walls))).collect::<Vec<_>>()
    };
    assert_eq!(settings(&h.app), vec![(2, Some(0)); 4]);
    h.step(&json!({"click":{"handle":"dialog:Covering"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Covering:Left and right walls"}})).unwrap();
    h.frames(3);
    assert_eq!(settings(&h.app), vec![(2, Some(1)); 4]);
    h.call("ui.camera", json!({"yaw":1.3,"pitch":0.8}));
    h.frames(3);
    assert_eq!(rectangle_walls(&h.app, &points), [Some(0), Some(1), Some(2), Some(3)], "wall names belong to the sheet, not the camera");
    for _ in 0..600 {
        h.frames(2);
        if h.app.preview.active && !h.app.preview.busy {
            break;
        }
        std::thread::yield_now();
    }
    assert!(h.app.preview.active && !h.app.preview.busy, "the preview finishes before testing hover");
    let selection = serde_json::to_value(&h.app.session.selection).unwrap();
    let preview_key = h.app.preview.key;
    h.step(&json!({"move":{"handle":"dialog:Covering"}})).unwrap();
    h.frames(3);
    let hovered = h.ctx.data(|data| data.get_temp::<Vec<Vec3>>(egui::Id::new("sheet_corner_hover"))).unwrap();
    assert_eq!(hovered.len(), 2);
    assert!(hovered.iter().all(|point| point.x == 0.0 || point.x == 100.0));
    assert_eq!(h.app.preview.key, preview_key, "hover must reuse the computed preview");
    assert_eq!(serde_json::to_value(&h.app.session.selection).unwrap(), selection);
    assert_eq!(h.app.preview.sheet_walls.len(), 4);
    h.step(&json!({"move":[350,200]})).unwrap();
    h.frames(3);
    assert!(h.ctx.data(|data| data.get_temp::<Vec<Vec3>>(egui::Id::new("sheet_corner_hover"))).unwrap().is_empty());
    h.app.dialog.as_mut().unwrap().inputs[0].items.reverse();
    h.frames(3);
    assert_eq!(settings(&h.app), vec![(2, Some(1)); 4]);
    let removed = h.app.dialog.as_mut().unwrap().inputs[0].items.pop().unwrap();
    h.frames(3);
    h.app.dialog.as_mut().unwrap().inputs[0].items.push(removed);
    h.frames(3);
    assert_eq!(settings(&h.app), vec![(2, Some(1)); 4], "new corners inherit the common choice");
    h.call("ui.resize", json!({"width":1200,"height":1800}));
    h.step(&json!({"click":{"handle":"dialog:Customize each corner"}})).unwrap();
    h.frames(12);
    for _ in 0..600 {
        h.frames(2);
        if h.app.preview.active && !h.app.preview.busy {
            break;
        }
        std::thread::yield_now();
    }
    assert!(h.app.preview.active && !h.app.preview.busy);
    let preview_key = h.app.preview.key;
    for (label, expected) in
        [("Front / Left", [5.0, 5.0]), ("Front / Right", [95.0, 5.0]), ("Rear / Left", [5.0, 55.0]), ("Rear / Right", [95.0, 55.0])]
    {
        let (first, second) = label.split_once(" / ").unwrap();
        let at = crate::scenario::handle_at(&format!("dialog:Corner {label}"))
            .or_else(|| crate::scenario::handle_at(&format!("dialog:Corner {second} / {first}")))
            .unwrap();
        h.step(&json!({"move":[at.x, at.y]})).unwrap();
        h.frames(3);
        let corner = h.ctx.data(|data| data.get_temp::<Option<[Vec3; 2]>>(egui::Id::new("sheet_corner_marker"))).flatten().unwrap();
        let marker = corner_marker(&h.app.preview.sheet_walls, corner).unwrap();
        assert!((marker.x - expected[0]).abs() < 1e-6 && (marker.y - expected[1]).abs() < 1e-6, "{label}: {marker:?}");
    }
    h.step(&json!({"move":{"handle":"dialog:Covering Rear / Left"}})).unwrap();
    h.frames(3);
    assert_eq!(h.ctx.data(|data| data.get_temp::<Vec<Vec3>>(egui::Id::new("sheet_corner_hover"))).unwrap().len(), 1);
    assert!(h.ctx.data(|data| data.get_temp::<Option<[Vec3; 2]>>(egui::Id::new("sheet_corner_marker"))).flatten().is_some());
    h.step(&json!({"move":[350,200]})).unwrap();
    h.frames(3);
    assert!(h.ctx.data(|data| data.get_temp::<Option<[Vec3; 2]>>(egui::Id::new("sheet_corner_marker"))).flatten().is_none());
    assert_eq!(h.app.preview.key, preview_key, "corner feedback must not reevaluate the model");
    assert_eq!(serde_json::to_value(&h.app.session.selection).unwrap(), selection, "corner feedback must not change selected edges");
    h.step(&json!({"click":{"handle":"dialog:Corner Rear / Left"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Corner Rear / Left:Butt"}})).unwrap();
    h.frames(3);
    let before = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(before[0].1["corners"].as_array().unwrap().len(), 3);
    h.frames(5);
    assert_eq!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap(), before, "drawing a mixed dialog must not reset it");
    h.step(&json!({"click":{"handle":"dialog:Customize each corner"}})).unwrap();
    h.call("ui.resize", json!({"width":800,"height":600}));
    h.frames(3);
    if let Some(value_box) = h.ctx.memory(|memory| memory.area_rect(egui::Id::new("sc_canvas_area")))
        && h.ctx.memory(|memory| memory.areas().visible_last_frame(&egui::LayerId::new(egui::Order::Foreground, egui::Id::new("sc_canvas_area"))))
    {
        let panel = h.app.viewport.dialog_rect.unwrap();
        assert!(!value_box.intersects(panel), "the floating height must not cover dialog fields: {value_box:?}, {panel:?}");
    }
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "the footer remains reachable in a small window");
    let id = h.app.session.doc.features.last().unwrap().id;
    let edited = edit_dialog(&h.app, id, None).unwrap();
    assert_eq!(apply_commands(&h.app, &edited).unwrap()[0].1["params"]["corners"], before[0].1["corners"]);
    assert!(h.app.session.model.results.iter().all(|result| result.error.is_none()));
    h.call("ui.resize", json!({"width":1200,"height":1100}));
    let reopened = h.call("ui.editFeature", json!({"feature":id}));
    assert_eq!(reopened["ok"], true, "{reopened}");
    h.frames(3);
    assert_eq!(
        apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["params"]["corners"],
        before[0].1["corners"],
        "an omitted saved Butt corner must stay Butt when rows are rebuilt"
    );
    h.step(&json!({"click":{"handle":"dialog:All Corners:Partial Overlap"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:Overlap"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"1.2 mm"})).unwrap();
    h.frames(3);
    let partial = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(partial[0].1["params"]["corners"].as_array().unwrap().len(), 4);
    assert!(partial[0].1["params"]["corners"].as_array().unwrap().iter().all(|corner| corner["overlap"] == "1.2 mm"));
    h.step(&json!({"click":{"handle":"dialog:All Corners:Butt"}})).unwrap();
    assert_eq!(apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap()[0].1["params"]["corners"], json!([]));
}

#[test]
fn flange_corner_gap_fields_keep_expressions_overrides_and_rule_defaults() {
    let mut harness = crate::scenario::Harness::new();
    harness.app = plate(crate::shell::LayoutMode::Floating);
    harness.call("ui.resize", json!({"width":1200,"height":1100}));
    harness.app.run("sheet.flange", json!({"sketch":"Base"})).unwrap();
    harness.app.run("parameters.change", json!({"name":"corner_clearance", "expression":"0.4 mm"})).unwrap();
    let points = [Vec3::new(50.0, 0.0, 2.5), Vec3::new(50.0, 60.0, 2.5), Vec3::new(0.0, 30.0, 2.5), Vec3::new(100.0, 30.0, 2.5)];
    harness.app.session.selection = points.iter().filter_map(|point| edge_sel(&harness.app.session, *point)).collect();
    harness.app.start("sheet.flange");
    harness.frames(3);
    assert_eq!(apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap()[0].1["corners"], json!([]));
    harness.step(&json!({"click":{"handle":"dialog:Corner Gap"}})).unwrap();
    harness.step(&json!({"key":"A","cmd":true})).unwrap();
    harness.step(&json!({"text":"corner_clearance + 0.1 mm"})).unwrap();
    harness.frames(3);
    let commands = apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap();
    let corners = commands[0].1["corners"].as_array().unwrap();
    assert_eq!(corners.len(), 4);
    assert!(corners.iter().all(|corner| corner["gap"] == "corner_clearance + 0.1 mm" && corner["corner_type"] == "butt"));
    let removed = harness.app.dialog.as_mut().unwrap().inputs[0].items.pop().unwrap();
    harness.frames(3);
    harness.app.dialog.as_mut().unwrap().inputs[0].items.push(removed);
    harness.frames(3);
    let reselected = apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(reselected[0].1["edges"], commands[0].1["edges"]);
    let reselected_corners = reselected[0].1["corners"].as_array().unwrap();
    assert_eq!(reselected_corners.len(), corners.len());
    assert!(corners.iter().all(|corner| reselected_corners.contains(corner)));
    harness.step(&json!({"click":{"handle":"dialog:Customize each corner"}})).unwrap();
    harness.frames(3);
    harness.step(&json!({"click":{"handle":"dialog:Corner Gap Front / Left"}})).unwrap();
    harness.step(&json!({"key":"A","cmd":true})).unwrap();
    harness.step(&json!({"text":"0.8 mm"})).unwrap();
    harness.frames(3);
    let mixed = apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap()[0].1["corners"].clone();
    assert_eq!(mixed.as_array().unwrap().iter().filter(|corner| corner["gap"] == "0.8 mm").count(), 1);
    harness.frames(5);
    assert_eq!(apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap()[0].1["corners"], mixed);
    harness.step(&json!({"widget":"OK"})).unwrap();
    assert!(harness.app.dialog.is_none());
    let feature = harness.app.session.doc.features.last().unwrap().id;
    let edited = edit_dialog(&harness.app, feature, None).unwrap();
    assert_eq!(apply_commands(&harness.app, &edited).unwrap()[0].1["params"]["corners"], mixed);
    assert_eq!(harness.call("ui.editFeature", json!({"feature":feature}))["ok"], true);
    harness.frames(3);
    harness.step(&json!({"click":{"handle":"dialog:Corner Gap"}})).unwrap();
    harness.step(&json!({"key":"A","cmd":true})).unwrap();
    harness.step(&json!({"text":"corner_clearance"})).unwrap();
    harness.frames(3);
    harness.step(&json!({"key":"A","cmd":true})).unwrap();
    harness.step(&json!({"key":"Backspace"})).unwrap();
    harness.frames(3);
    assert_eq!(apply_commands(&harness.app, harness.app.dialog.as_ref().unwrap()).unwrap()[0].1["params"]["corners"], json!([]));
    harness.step(&json!({"widget":"OK"})).unwrap();
    assert!(matches!(&harness.app.session.doc.features.last().unwrap().kind, FeatureKind::SheetFlange { corners, .. } if corners.is_empty()));
}

#[test]
fn standalone_corner_join_dialog_selects_previews_applies_and_reopens() {
    let mut h = crate::scenario::Harness::new();
    h.app
        .run("sheet.manage_rules", json!({"name":"AL2", "thickness":"2 mm", "k_factor":0.5, "bend_radius":"1 mm", "gap":"0.5 mm", "active":true}))
        .unwrap();
    h.app.run("sketch.create", json!({"plane":"XY", "name":"Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0], "p1":[120,90]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch":"Base"})).unwrap();
    h.app.run("sheet.flange", json!({"edges":[[60,0,2]], "height":38.6})).unwrap();
    h.app.run("sheet.flange", json!({"edges":[[120,45,2]], "height":38.6})).unwrap();
    h.app.run("parameters.change", json!({"name":"corner_clearance", "expression":"0.4 mm"})).unwrap();
    let state = h.app.session.model.state();
    let tags: Vec<String> = state.sheets[0].flanges.iter().map(|flange| flange.tag.clone()).collect();
    h.app.start("sheet.corner_join");
    let mut dialog = h.app.dialog.clone().unwrap();
    assert!(apply_commands(&h.app, &dialog).is_err());
    assert_eq!(dialog.inputs.iter().map(|input| input.label).collect::<Vec<_>>(), ["Flange 1", "Flange 2"]);
    for (input, tag) in dialog.inputs.iter_mut().zip(&tags) {
        input.items = corner_flange_sel(&h.app, "Body1", tag).into_iter().collect();
        assert_eq!(input.items.len(), 1);
    }
    h.app.dialog = Some(dialog);
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:Corner Type"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Corner Type:Partial Overlap"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:Covering"}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Covering:Flange 2"}})).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"dialog:Corner Gap"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"corner_clearance"})).unwrap();
    h.frames(3);
    assert!(h.app.preview.error.is_none(), "{:?}", h.app.preview.error);
    let commands = apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(commands[0].0, "sheet.corner_join");
    assert_eq!(commands[0].1["covering"], tags[1]);
    assert_eq!(commands[0].1["covered"], tags[0]);
    assert_eq!(commands[0].1["overlap"], "1 mm");
    assert_eq!(commands[0].1["gap"], "corner_clearance");
    h.step(&json!({"widget":"OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    let id = h.app.session.doc.features.last().unwrap().id;
    let mut edited = edit_dialog(&h.app, id, None).unwrap();
    assert!(
        matches!(&edited.kind, Kind::Sheet(Sm::CornerJoin { kind:1, cover_second:0, overlap, gap }) if overlap == "1 mm" && gap == "corner_clearance")
    );
    assert!(edited.inputs.iter().all(|input| input.items.len() == 1));
    let commands = apply_commands(&h.app, &edited).unwrap();
    assert_eq!(commands[0].1["params"]["covering"], tags[1]);
    assert_eq!(commands[0].1["params"]["covered"], tags[0]);
    if let Kind::Sheet(Sm::CornerJoin { kind, .. }) = &mut edited.kind {
        *kind = 2;
    }
    let commands = apply_commands(&h.app, &edited).unwrap();
    assert_eq!(commands[0].1["params"]["corner_type"], "full");
    assert!(commands[0].1["params"].get("overlap").is_none());
    run_all(&mut h.app, commands);
    assert!(h.app.session.model.results.iter().all(|result| result.error.is_none()));
    if let Kind::Sheet(Sm::CornerJoin { kind, .. }) = &mut edited.kind {
        *kind = 0;
    }
    let commands = apply_commands(&h.app, &edited).unwrap();
    run_all(&mut h.app, commands);
    edited.inputs[1].items.clear();
    assert!(apply_commands(&h.app, &edited).is_err());
}

#[test]
fn pocket_depth_labels_follow_inches_without_changing_flat_data() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("document.units", json!({"units":"in"})).unwrap();
        assert_eq!(super::pocket_label(&app, &json!({"side":"top","depth_mm":25.4*0.1234})).as_deref(), Some("Top pocket: 0.1234 in deep"));
        assert!(super::pocket_label(&app, &json!({"side":"top","depth_mm":-1})).is_none());
    }
}

/// Edges with an angle and/or a height of their own: the texts of the dialog's rows per edge
/// become `{point, angle?, height?}` edges while the others stay plain points; Edit Feature
/// brings them back and rebuilds the same flanges.
#[test]
fn edge_flanges_can_have_an_angle_and_a_height_of_their_own() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let edges: Vec<Sel> = [[50.0, 0.0, 2.5], [50.0, 60.0, 2.5], [0.0, 30.0, 2.5], [100.0, 30.0, 2.5]]
            .iter()
            .filter_map(|p| edge_sel(&app.session, Vec3::new(p[0], p[1], p[2])))
            .collect();
        assert_eq!(edges.len(), 4);
        let point = |i: usize| if let Sel::Edge { point, .. } = &edges[i] { *point } else { Vec3::ZERO };
        app.session.selection = edges.clone();
        app.start("sheet.flange");
        let mut d = app.dialog.clone().unwrap();
        if let Kind::Sheet(Sm::Flange { height, angle, own, .. }) = &mut d.kind {
            *height = "20 mm".into();
            *angle = "75 deg".into();
            // The second edge: its own angle; the fourth: both; the third: texts left blank.
            own.push((point(1), "100 deg".into(), String::new()));
            own.push((point(3), "90 deg".into(), "15 mm".into()));
            own.push((point(2), "  ".into(), String::new()));
        }
        let c = apply_commands(&app, &d).unwrap();
        assert_eq!(
            c[0].1["edges"],
            json!([pt(point(0)), {"point": pt(point(1)), "angle": "100 deg"}, pt(point(2)), {"point": pt(point(3)), "angle": "90 deg", "height": "15 mm"}]),
            "no text or a blank one: a plain point"
        );
        run_all(&mut app, c);
        let bends = app.session.execute("sheet.flat_pattern", &json!({})).unwrap()["bends"].clone();
        let angles: Vec<f64> = bends.as_array().unwrap().iter().map(|b| b["angle_deg"].as_f64().unwrap().round()).collect();
        assert_eq!(angles, [75.0, 100.0, 75.0, 90.0], "{bends}");
        assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
        // Edit Feature: the edges and their own values come back, and OK rebuilds the same part.
        let f = app.session.doc.features.last().unwrap().clone();
        let mut e = edit_dialog(&app, f.id, None).unwrap();
        assert_eq!(e.inputs[0].items.len(), 4);
        let Kind::Sheet(Sm::Flange { own, .. }) = &e.kind else { panic!("not a flange dialog") };
        assert_eq!(own.len(), 2, "{own:?}");
        assert!(own.iter().any(|o| o.0.dist(point(1)) < 1e-9 && o.1 == "100 deg" && o.2.is_empty()), "{own:?}");
        assert!(own.iter().any(|o| o.0.dist(point(3)) < 1e-9 && o.1 == "90 deg" && o.2 == "15 mm"), "{own:?}");
        let volume = |app: &mut SolveApp| app.session.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        let before = volume(&mut app);
        let c = apply_commands(&app, &e).unwrap();
        assert_eq!(c[0].0, "timeline.redefine");
        assert_eq!(c[0].1["params"]["edges"][1], json!({"point": pt(point(1)), "angle": "100 deg"}));
        assert_eq!(c[0].1["params"]["edges"][3]["height"], "15 mm");
        run_all(&mut app, c);
        let after = volume(&mut app);
        assert!((after - before).abs() < 1e-6 * before, "{after} vs {before}");
        // Taking edges out of the selection leaves each value with its edge.
        e.inputs[0].items.remove(2);
        e.inputs[0].items.remove(0);
        let c = apply_commands(&app, &e).unwrap();
        assert_eq!(
            c[0].1["params"]["edges"],
            json!([{"point": pt(point(1)), "angle": "100 deg"}, {"point": pt(point(3)), "angle": "90 deg", "height": "15 mm"}])
        );
    }
}

/// The Flange dialog's Further Bends make the command's `then` (#4094): a flap with two bends is
/// one flange command, one body and one undo step, and a bend with no height is refused.
#[test]
fn further_bends_in_the_flange_dialog_make_one_chained_flap() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        app.run("sheet.flange", json!({"edges": [[0, 30, 2.5]], "height": 30})).unwrap();
        let features = app.session.doc.features.len();
        let end = edge_sel(&app.session, Vec3::new(1.25, 0.0, 15.0)).expect("the wall's end edge");
        app.session.selection = vec![end];
        app.start("sheet.flange");
        let mut d = app.dialog.clone().unwrap();
        let Kind::Sheet(Sm::Flange { height, angle, then, .. }) = &mut d.kind else { panic!("not a flange dialog") };
        *height = "30 mm".into();
        *angle = "45 deg".into();
        // A bend added with the button starts at 45° and 20 mm; give the first a height of its own.
        then.push(("45 deg".into(), String::new(), false));
        assert!(apply_commands(&app, &d).is_err(), "a bend needs a height");
        let Kind::Sheet(Sm::Flange { then, .. }) = &mut d.kind else { panic!("not a flange dialog") };
        then[0].1 = "50 mm".into();
        let c = apply_commands(&app, &d).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].1["then"], json!([{"height": "50 mm", "angle": "45 deg"}]));
        // A bend that turns the other way says so, against the flange's own side.
        let Kind::Sheet(Sm::Flange { then, .. }) = &mut d.kind else { panic!("not a flange dialog") };
        then.push(("45 deg".into(), "30 mm".into(), true));
        assert_eq!(
            apply_commands(&app, &d).unwrap()[0].1["then"],
            json!([{"height": "50 mm", "angle": "45 deg"}, {"height": "30 mm", "angle": "45 deg", "flip": true}])
        );
        let Kind::Sheet(Sm::Flange { then, .. }) = &mut d.kind else { panic!("not a flange dialog") };
        then.pop();
        run_all(&mut app, c);
        assert_eq!(app.session.model.state().bodies.len(), 1, "the flap is part of the sheet");
        assert_eq!(app.session.doc.features.len(), features + 2, "one flange per bend");
        assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
        // A dialog with no further bends sends no `then`.
        app.session.selection = vec![edge_sel(&app.session, Vec3::new(50.0, 0.0, 2.5)).expect("an edge")];
        app.dialog = None;
        app.start("sheet.flange");
        let d = app.dialog.clone().unwrap();
        assert!(apply_commands(&app, &d).unwrap()[0].1.get("then").is_none());
    }
}

/// The Add bend button and a further bend's bin work in the real dialog.
#[test]
fn add_bend_and_remove_bend_buttons_work() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.app.run("sheet.flange", json!({"edges": [[0, 30, 2.5]], "height": 30})).unwrap();
    h.app.session.selection = vec![edge_sel(&h.app.session, Vec3::new(1.25, 0.0, 15.0)).expect("the wall's end edge")];
    h.step(&json!({"start": "sheet.flange"})).unwrap();
    h.frames(6);
    let bends = |h: &crate::scenario::Harness| match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Sheet(Sm::Flange { then, .. }) => then.clone(),
        _ => panic!("not a flange dialog"),
    };
    assert!(bends(&h).is_empty(), "no further bends to start with");
    for want in 1..=2 {
        h.step(&json!({"widget": "Add bend"})).unwrap();
        h.frames(3);
        assert_eq!(bends(&h).len(), want);
    }
    assert_eq!(bends(&h)[0], ("45 deg".to_string(), "20 mm".to_string(), false));
    h.step(&json!({"click": {"handle": "dialog:button:Bend 2:Delete"}})).unwrap();
    h.frames(3);
    assert_eq!(bends(&h).len(), 1, "the bin takes one bend out");
}

/// The Flange dialog's Height From row: Outer Sharp (the default) asks for nothing, Inner Sharp
/// reaches the feature, and Edit Feature shows it again.
#[test]
fn flange_dialog_measures_the_height_from_the_chosen_datum() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.set", json!({"layout":mode}));
        h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let edges: Vec<Sel> = edge_sel(&h.app.session, Vec3::new(50.0, 0.0, 2.5)).into_iter().collect();
        assert_eq!(edges.len(), 1);
        h.app.session.selection = edges;
        h.app.start("sheet.flange");
        h.frames(3);
        let shown = |app: &SolveApp| match app.dialog.as_ref().map(|d| &d.kind) {
            Some(Kind::Sheet(Sm::Flange { height_from, .. })) => Some(*height_from),
            _ => None,
        };
        assert_eq!(shown(&h.app), Some(0));
        let d = h.app.dialog.clone().unwrap();
        let c = apply_commands(&h.app, &d).unwrap();
        assert!(c[0].1.get("height_from").is_none(), "{}", c[0].1);
        h.step(&json!({"click": {"handle": "dialog:Height From"}})).unwrap();
        h.step(&json!({"click": {"handle": "dialog:Height From:Inner Sharp"}})).unwrap();
        assert_eq!(shown(&h.app), Some(1));
        let d = h.app.dialog.clone().unwrap();
        let c = apply_commands(&h.app, &d).unwrap();
        assert_eq!(c[0].1["height_from"], json!("inner"), "{}", c[0].1);
        h.step(&json!({"widget": "OK"})).unwrap();
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
        let saved = |app: &SolveApp| match app.session.doc.features.last().map(|f| &f.kind) {
            Some(FeatureKind::SheetFlange { height_from, .. }) => height_from.clone(),
            _ => None,
        };
        assert_eq!(saved(&h.app).as_deref(), Some("inner"));
        // Edit Feature shows Inner Sharp; Tangent then reaches the feature.
        let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
        let mut e = edit_dialog(&h.app, id, None).unwrap();
        assert!(matches!(&e.kind, Kind::Sheet(Sm::Flange { height_from: 1, .. })));
        let c = apply_commands(&h.app, &e).unwrap();
        assert_eq!(c[0].1["params"]["height_from"], json!("inner"));
        if let Kind::Sheet(Sm::Flange { height_from, .. }) = &mut e.kind {
            *height_from = 2;
        }
        let commands = apply_commands(&h.app, &e).unwrap();
        run_all(&mut h.app, commands);
        assert_eq!(saved(&h.app).as_deref(), Some("tangent"));
    }
}

/// The Flange dialog's Bend Position offers material outside and tangent, and Edit Feature shows them again.
#[test]
fn flange_dialog_offers_material_outside_and_tangent_bend_positions() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.app.session.selection = edge_sel(&h.app.session, Vec3::new(50.0, 0.0, 2.5)).into_iter().collect();
    h.app.start("sheet.flange");
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Bend Position"}})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Bend Position:Material Outside"}})).unwrap();
    let d = h.app.dialog.clone().unwrap();
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(c[0].1["position"], json!("material_outside"), "{}", c[0].1);
    h.step(&json!({"widget": "OK"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    let id = h.app.session.doc.features.last().map(|f| f.id).unwrap();
    let mut e = edit_dialog(&h.app, id, None).unwrap();
    assert!(matches!(&e.kind, Kind::Sheet(Sm::Flange { position: 3, .. })));
    if let Kind::Sheet(Sm::Flange { position, .. }) = &mut e.kind {
        *position = 4;
    }
    let commands = apply_commands(&h.app, &e).unwrap();
    assert_eq!(commands[0].1["params"]["position"], json!("tangent"));
    run_all(&mut h.app, commands);
    let saved = match h.app.session.doc.features.last().map(|f| &f.kind) {
        Some(FeatureKind::SheetFlange { position, .. }) => position.clone(),
        _ => String::new(),
    };
    assert_eq!(saved, "tangent");
}

/// Convert takes a fixed face and, for a solid or shelled body, its bend edges; Edit Feature
/// brings both back.
#[test]
fn convert_takes_the_fixed_face_and_the_bend_edges() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 100, "width": 60, "height": 30, "body_name": "Tray"})).unwrap();
    s.execute("solid.shell", &json!({"faces": [[50, 30, 30]], "thickness": 2})).unwrap();
    let mut app = SolveApp::new(s, Services::default());
    app.start("sheet.convert");
    let mut d = app.dialog.clone().unwrap();
    assert_eq!(d.inputs.iter().map(|i| i.label).collect::<Vec<_>>(), ["Fixed Face", "Bend Edges"]);
    assert!(apply_commands(&app, &d).is_err(), "nothing to convert without a face");
    // The face alone is the plate conversion: no `bends`.
    d.inputs[0].items = face_sel(&app.session, Vec3::new(50.0, 30.0, 0.0)).into_iter().collect();
    assert!(apply_commands(&app, &d).unwrap()[0].1.get("bends").is_none());
    let edges: Vec<Sel> = [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]]
        .iter()
        .filter_map(|p| edge_sel(&app.session, Vec3::new(p[0], p[1], p[2])))
        .collect();
    d.inputs[1].items = edges;
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].0, "sheet.convert");
    assert_eq!(c[0].1["bends"], json!([[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]]));
    run_all(&mut app, c);
    let bends = |app: &mut SolveApp| app.session.execute("sheet.flat_pattern", &json!({})).unwrap()["bends"].as_array().map(Vec::len);
    assert_eq!(bends(&mut app), Some(4));
    // Edit Feature: the face and the four edges are back in their rows, and OK rebuilds the sheet.
    let f = app.session.doc.features.last().unwrap().clone();
    let e = edit_dialog(&app, f.id, None).unwrap();
    assert_eq!((e.inputs[0].items.len(), e.inputs[1].items.len()), (1, 4));
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(c[0].1["params"]["bends"].as_array().map(Vec::len), Some(4));
    run_all(&mut app, c);
    assert_eq!(bends(&mut app), Some(4));
    assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
}

/// The Path Flange dialog takes the edge and the path's sketch lines: one feature, one body,
/// and Edit Feature brings the edge and the path back.
#[test]
fn path_flange_dialog_follows_the_path() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        app.run("sheet.flange", json!({"edges": [[0, 30, 2.5]], "height": 30})).unwrap();
        // A path across the wall's end edge: 30 mm out at 45°, then 30 mm on along the wall.
        let a = 30.0 / 2f64.sqrt();
        app.run("sketch.create", json!({"plane": {"origin": [0, 0, 15], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Path"})).unwrap();
        app.run("sketch.line", json!({"points": [[0, 0], [a, -a], [a + 30.0, -a]], "ids": ["p1", "p2"]})).unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        let path = app.session.world_state().sketches.last().map(|ss| ss.feature);
        // A newer sketch with the same line ids: the picks name the path's sketch (#3726).
        app.run("sketch.create", json!({"plane": {"origin": [0, 0, 60], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Other"})).unwrap();
        app.run("sketch.line", json!({"points": [[50, 50], [60, 50], [60, 60]], "ids": ["p1", "p2"]})).unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        let pick = |id: &str| Sel::SketchCurve { id: id.into(), sketch: path };
        let features = app.session.doc.features.len();
        app.start("sheet.path_flange");
        let mut d = app.dialog.clone().unwrap();
        assert!(apply_commands(&app, &d).is_err(), "no edge picked yet");
        d.inputs[0].items = vec![edge_sel(&app.session, Vec3::new(1.25, 0.0, 15.0)).expect("the wall's end edge")];
        assert!(apply_commands(&app, &d).is_err(), "no path picked yet");
        // One line picked takes the whole path, and the dialog shows the flanges it makes.
        d.inputs[1].items = vec![pick("p2")];
        path_chain(&app, &mut d.inputs);
        assert_eq!(d.inputs[1].items, vec![pick("p1"), pick("p2")], "in order along the path");
        let lengths = path_lengths(&app, &d.inputs);
        assert!(lengths.len() == 2 && lengths.iter().all(|l| (l - 30.0).abs() < 1e-6), "{lengths:?}");
        d.inputs[1].items = vec![pick("p1"), pick("p2")];
        let c = apply_commands(&app, &d).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].0, "sheet.path_flange");
        assert_eq!(c[0].1["curves"], json!(["p1", "p2"]));
        assert_eq!(c[0].1["sketch"], json!(path));
        run_all(&mut app, c);
        assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
        assert_eq!(app.session.doc.features.len(), features + 1, "one feature");
        assert_eq!(app.session.model.state().bodies.len(), 1, "the flap is part of the sheet");
        let f = app.session.doc.features.last().cloned().unwrap();
        let e = edit_dialog(&app, f.id, None).unwrap();
        assert!(matches!(&e.kind, Kind::Sheet(Sm::Path { .. })));
        assert_eq!((e.inputs[0].items.len(), e.inputs[1].items.len()), (1, 2));
        assert_eq!(e.inputs[1].items, vec![pick("p1"), pick("p2")], "the path's own sketch");
        assert!(apply_commands(&app, &e).is_ok());
    }
}

// ---- flat pattern navigation ----

fn flat_app(preset: &str, mode: crate::shell::LayoutMode) -> SolveApp {
    let mut app = plate(mode);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    app.start("sheet.flat_pattern");
    let Some(Kind::Sheet(Sm::FlatPattern { data, .. })) = app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("flat dialog") };
    *data = Some(Ok(flat));
    app.preferences.nav = preset.into();
    app.preferences.zoom_to_cursor = true;
    app
}

/// Same view in the flat plane: the Design viewport may anchor a zoom on the model's surface
/// (a little out of the plane), the flat view anchors on its plane.
fn same_view(a: &solidcraft_engine::render::Camera, b: &solidcraft_engine::render::Camera) -> bool {
    (a.target.x - b.target.x).abs() < 1e-3
        && (a.target.y - b.target.y).abs() < 1e-3
        && (a.distance - b.distance).abs() < 1e-9
        && a.yaw == b.yaw
        && a.pitch == b.pitch
}

fn frame(app: &mut SolveApp, ctx: &egui::Context, time: &mut f64, mods: egui::Modifiers, events: Vec<egui::Event>) {
    *time += 1.0 / 60.0;
    let events = std::iter::once(egui::Event::ModifiersChanged(mods)).chain(events).collect();
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        time: Some(*time),
        focused: true,
        events,
        ..Default::default()
    };
    crate::test_support::frame(ctx, raw, |ui| crate::viewport::show(app, ui));
}

/// The same pan and zoom input moves the flat pattern's view exactly as it moves the Design
/// viewport's camera, under every navigation preset, with and without reverse zoom and zoom to the
/// cursor. The flat view never orbits.
#[test]
fn flat_pattern_pans_and_zooms_like_the_design_viewport() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        use egui::{Event, Modifiers, PointerButton};
        let none = Modifiers::NONE;
        let chords = [none, Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT, Modifiers::SHIFT | Modifiers::ALT];
        for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
            for (reverse, to_cursor) in [(false, true), (true, true), (false, false)] {
                let (flat_ctx, design_ctx) = (crate::fonts::test_context(), crate::fonts::test_context());
                let mut flat = flat_app(preset, mode);
                let mut design = flat_app(preset, mode);
                design.dialog = None;
                for app in [&mut flat, &mut design] {
                    app.preferences.zoom_reverse = reverse;
                    app.preferences.zoom_to_cursor = to_cursor;
                }
                let (mut ft, mut dt) = (0.0, 0.0);
                frame(&mut flat, &flat_ctx, &mut ft, none, vec![]);
                frame(&mut flat, &flat_ctx, &mut ft, none, vec![]);
                assert!(flat.flat_view.fitted);
                // The Design viewport starts from the flat view's camera: top-down, orthographic.
                design.cam = flat.flat_view.cam;
                design.ui.perspective = false;
                let design_before = flat.cam;
                let (mut panned, mut zoomed) = (0, 0);
                let at = pos2(300.0, 250.0);
                for button in [PointerButton::Primary, PointerButton::Middle, PointerButton::Secondary] {
                    for mods in chords {
                        let mode = crate::prefs::nav_mode(&flat, button.into_nav(), mods);
                        if !matches!(mode, Some(crate::viewport::NavMode::Pan | crate::viewport::NavMode::Zoom)) {
                            continue;
                        }
                        let before = flat.flat_view.cam;
                        let mut both = |events: Vec<Event>, mods: Modifiers, flat: &mut SolveApp, design: &mut SolveApp| {
                            frame(flat, &flat_ctx, &mut ft, mods, events.clone());
                            frame(design, &design_ctx, &mut dt, mods, events);
                        };
                        both(vec![Event::PointerMoved(at)], mods, &mut flat, &mut design);
                        both(vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: mods }], mods, &mut flat, &mut design);
                        let mut p = at;
                        for _ in 0..5 {
                            p += vec2(14.0, -9.0);
                            both(vec![Event::PointerMoved(p)], mods, &mut flat, &mut design);
                        }
                        both(vec![Event::PointerButton { pos: p, button, pressed: false, modifiers: mods }], mods, &mut flat, &mut design);
                        for _ in 0..10 {
                            both(vec![], none, &mut flat, &mut design);
                        }
                        assert!(same_view(&flat.flat_view.cam, &design.cam), "{preset}: {button:?} drag with {mods:?} ({mode:?})");
                        assert_ne!(flat.flat_view.cam, before, "{preset}: {button:?} drag with {mods:?} moves the flat view");
                        if mode == Some(crate::viewport::NavMode::Pan) {
                            panned += 1
                        } else {
                            zoomed += 1
                        }
                        flat.flat_view.cam = before;
                        design.cam = before;
                    }
                }
                assert!(panned + zoomed >= 1, "{preset}: the preset has a pan or zoom drag");
                // The wheel zooms (to the cursor when asked), identically.
                let before = flat.flat_view.cam;
                let wheel =
                    Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 3.0), modifiers: none, phase: egui::TouchPhase::Move };
                for (app, ctx, t) in [(&mut flat, &flat_ctx, &mut ft), (&mut design, &design_ctx, &mut dt)] {
                    frame(app, ctx, t, none, vec![Event::PointerMoved(at)]);
                    frame(app, ctx, t, none, vec![wheel.clone()]);
                    for _ in 0..30 {
                        frame(app, ctx, t, none, vec![]);
                    }
                }
                assert_ne!(flat.flat_view.cam.distance, before.distance, "{preset}: the wheel zooms the flat view");
                assert!(same_view(&flat.flat_view.cam, &design.cam), "{preset} reverse {reverse} cursor {to_cursor}: wheel zoom");
                assert_eq!(flat.flat_view.cam.distance < before.distance, !reverse, "{preset}: reverse zoom flips the wheel");
                if to_cursor {
                    assert!(flat.flat_view.cam.target.dist(before.target) > 1e-6, "{preset}: zoom to the cursor moves the target");
                }
                assert_eq!(flat.flat_view.cam.pitch, before.pitch);
                assert_eq!(flat.flat_view.cam.yaw, before.yaw);
                assert_eq!(flat.cam, design_before, "{preset}: the Design camera is untouched");
            }
        }
    }
}

trait IntoNav {
    fn into_nav(self) -> crate::prefs::NavButton;
}
impl IntoNav for egui::PointerButton {
    fn into_nav(self) -> crate::prefs::NavButton {
        match self {
            egui::PointerButton::Middle => crate::prefs::NavButton::Middle,
            egui::PointerButton::Secondary => crate::prefs::NavButton::Right,
            _ => crate::prefs::NavButton::Left,
        }
    }
}

/// Input over the flat pattern, including orbit bindings, wheel and pinch, leaves the Design
/// camera and selection alone.
#[test]
fn design_camera_is_unchanged_while_the_flat_pattern_has_focus() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        use egui::{Event, Modifiers, PointerButton};
        let none = Modifiers::NONE;
        for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
            let ctx = crate::fonts::test_context();
            let mut app = flat_app(preset, mode);
            let mut t = 0.0;
            frame(&mut app, &ctx, &mut t, none, vec![]);
            let cam = app.cam;
            let at = pos2(300.0, 250.0);
            for button in [PointerButton::Primary, PointerButton::Middle, PointerButton::Secondary] {
                for mods in [none, Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT, Modifiers::SHIFT | Modifiers::ALT] {
                    frame(&mut app, &ctx, &mut t, mods, vec![Event::PointerMoved(at)]);
                    frame(&mut app, &ctx, &mut t, mods, vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: mods }]);
                    for i in 1..6 {
                        frame(&mut app, &ctx, &mut t, mods, vec![Event::PointerMoved(at + vec2(14.0, -9.0) * i as f32)]);
                    }
                    frame(&mut app, &ctx, &mut t, mods, vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: mods }]);
                }
            }
            let wheel = Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 3.0), modifiers: none, phase: egui::TouchPhase::Move };
            frame(&mut app, &ctx, &mut t, none, vec![wheel]);
            frame(&mut app, &ctx, &mut t, none, vec![Event::Zoom(1.3)]);
            for _ in 0..30 {
                frame(&mut app, &ctx, &mut t, none, vec![]);
            }
            assert_eq!(app.cam, cam, "{preset}: the Design camera stays put under the flat pattern");
            assert!(app.session.selection.is_empty(), "{preset}: nothing underneath is selected");
            assert_eq!(app.flat_view.cam.pitch, app.flat_view.cam.pitch.clamp(1.5, 2.0), "{preset}: still looking straight down");
        }
    }
}

/// Fit and Home frame the flat pattern (not the model), and leave the Design camera alone.
#[test]
fn fit_and_home_frame_the_flat_pattern() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let ctx = crate::fonts::test_context();
        let mut app = flat_app("fusion", mode);
        let mut t = 0.0;
        let none = egui::Modifiers::NONE;
        frame(&mut app, &ctx, &mut t, none, vec![]);
        let fitted = app.flat_view.cam;
        // The 100 x 60 pattern sits inside the view with room for the command panel.
        let scale = 600.0 / (2.0 * fitted.half_height());
        assert!(100.0 * scale < 800.0 && 60.0 * scale < 600.0 && 100.0 * scale > 200.0, "fits: {scale}");
        let design = app.cam;
        for view in ["fit", "home"] {
            app.flat_view.cam.pan(120.0, -80.0, 600.0);
            app.flat_view.cam.zoom_at(0.3, None);
            assert_ne!(app.flat_view.cam, fitted);
            app.run(if view == "fit" { "view.fit" } else { "view.home" }, json!({})).unwrap();
            frame(&mut app, &ctx, &mut t, none, vec![]);
            assert!(
                app.flat_view.cam.target.dist(fitted.target) < 1e-9 && (app.flat_view.cam.distance - fitted.distance).abs() < 1e-9,
                "{view} refits"
            );
            assert_eq!(app.cam, design, "{view} leaves the Design camera alone");
        }
        app.flat_view.cam.pan(50.0, 50.0, 600.0);
        app.fit_view();
        frame(&mut app, &ctx, &mut t, none, vec![]);
        assert!(app.flat_view.cam.target.dist(fitted.target) < 1e-9, "fit_view refits");
        // Closing the dialog returns the viewport to the Design camera, and reopening refits.
        app.flat_view.cam.pan(50.0, 50.0, 600.0);
        app.dialog = None;
        frame(&mut app, &ctx, &mut t, none, vec![]);
        assert!(!app.flat_view.fitted);
    }
}

fn perforated_flat() -> (SolveApp, Dialog) {
    let mut app = plate(crate::shell::LayoutMode::Floating);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    app.run("sheet.perforate", json!({"diameter": 3, "pitch": 6})).unwrap();
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    assert_eq!(flat["perforations"][0]["count"], 155);
    app.start("sheet.flat_pattern");
    let mut dialog = app.dialog.clone().unwrap();
    let Kind::Sheet(Sm::FlatPattern { data, .. }) = &mut dialog.kind else { panic!("flat dialog") };
    *data = Some(Ok(flat));
    (app, dialog)
}

fn flat_circles(app: &mut SolveApp, dialog: &Dialog, rect: Rect) -> Vec<egui::epaint::CircleShape> {
    let ctx = crate::fonts::test_context();
    let input = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
    let output = crate::test_support::frame(&ctx, input, |ui| {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("perforation-test")));
        assert!(overlay(app, &painter, rect, dialog));
    });
    output.shapes.into_iter().filter_map(|s| if let egui::Shape::Circle(c) = s.shape { Some(c) } else { None }).collect()
}

#[test]
fn flat_perforations_match_export_centres_and_cutout_style() {
    let (mut app, dialog) = perforated_flat();
    let state = app.session.world_state();
    let sheet = state.sheets.last().unwrap();
    let holes = sheet.perforation_holes(&sheet.perforations[0]).unwrap();
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0));
    let circles = flat_circles(&mut app, &dialog, rect);
    assert_eq!(circles.len(), holes.len());
    let cam = app.flat_view.cam;
    let scale = f64::from(rect.height()) / (2.0 * cam.half_height());
    for (circle, hole) in circles.iter().zip(holes) {
        let centre = pos2(
            (f64::from(rect.center().x) + (hole.x - cam.target.x) * scale) as f32,
            (f64::from(rect.center().y) - (hole.y - cam.target.y) * scale) as f32,
        );
        assert!(circle.center.distance(centre) < 0.001);
        assert!((circle.radius - (1.5 * scale) as f32).abs() < 0.001);
        assert_eq!(circle.stroke, Stroke::new(1.4, Tokens::get().text));
        assert_eq!(circle.fill, Color32::TRANSPARENT);
    }
    let cached = app.flat_view.perforations.as_ptr();
    assert_eq!(flat_circles(&mut app, &dialog, rect).len(), 155);
    assert_eq!(app.flat_view.perforations.as_ptr(), cached, "navigation reuses the centres");
    app.run("sheet.perforate", json!({"diameter": 2, "pitch": 10})).unwrap();
    let state = app.session.world_state();
    let sheet = state.sheets.last().unwrap();
    let expected: usize = sheet.perforations.iter().map(|p| sheet.perforation_holes(p).unwrap().len()).sum();
    assert!(expected > 155);
    assert_eq!(flat_circles(&mut app, &dialog, rect).len(), expected, "model changes refresh the centres");
}

#[test]
fn flat_perforations_cull_offscreen_and_subpixel_holes() {
    let (mut app, dialog) = perforated_flat();
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0));
    assert_eq!(flat_circles(&mut app, &dialog, rect).len(), 155);
    let fitted_distance = app.flat_view.cam.distance;
    // Keep a hole crossing the left viewport edge, even when its centre is outside.
    let scale = f64::from(rect.height()) / (2.0 * app.flat_view.cam.half_height());
    app.flat_view.cam.target.x = 40.5 + (f64::from(rect.width()) / 2.0 + 0.75 * scale) / scale;
    let partial = flat_circles(&mut app, &dialog, rect);
    assert!(!partial.is_empty() && partial.len() < 155);
    assert!(partial.iter().any(|c| c.center.x < rect.left() && c.center.x + c.radius > rect.left()));
    app.flat_view.cam.target.x = 1e6;
    assert!(flat_circles(&mut app, &dialog, rect).is_empty());
    app.flat_view.cam.target.x = 50.0;
    app.flat_view.cam.distance = fitted_distance * 1000.0;
    assert!(flat_circles(&mut app, &dialog, rect).is_empty());
}

/// The hem's handle sits on the centre line of the hem layer, at the outer face of its fold, and
/// points along the hem toward its tip (#5828): dragging it changes the Length, which is measured
/// from the fold's outer face to the tip, so the handle's start plus Length lands on the tip.
#[test]
fn hem_handle_runs_along_the_hem_to_its_tip() {
    for flipped in [false, true] {
        let mut app = plate(crate::shell::LayoutMode::Floating);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let body = app.session.model.state().sheets[0].body.clone();
        app.session.selection = vec![edge_sel(&app.session, Vec3::new(50.0, 0.0, 2.5)).unwrap()];
        app.start("sheet.hem");
        let mut d = app.dialog.clone().unwrap();
        let Kind::Sheet(Sm::Hem { flip, .. }) = &mut d.kind else { panic!("hem dialog") };
        *flip = flipped;
        let (at, dir) = crate::canvas::anchor(&app, &d).unwrap();
        let dir = dir.unwrap();
        let commands = apply_commands(&app, &d).unwrap();
        run_all(&mut app, commands);
        // The 5 mm hem layer: its tip is the far end of the added layer, along the sheet.
        let m = app.session.world_state().body(&body).unwrap().mesh();
        // The straight part of the layer, past the fold.
        let layer_z = |z: f64| if flipped { z > 2.505 } else { z < -0.01 };
        let tip_y = m.positions.iter().filter(|v| layer_z(v.z)).map(|v| v.y).fold(f64::NEG_INFINITY, f64::max);
        let layer: Vec<f64> = m.positions.iter().filter(|v| layer_z(v.z) && v.y > 0.5).map(|v| v.z).collect();
        let centre_z = (layer.iter().copied().fold(f64::INFINITY, f64::min) + layer.iter().copied().fold(f64::NEG_INFINITY, f64::max)) / 2.0;
        assert!(dir.dist(Vec3::Y) < 1e-6, "flip={flipped}: the handle points along the hem to its tip, got {dir:?}");
        assert!((at.z - centre_z).abs() < 0.05, "flip={flipped}: the handle starts on the hem's centre line: {at:?} vs z={centre_z}");
        assert!((at.y + 5.0 - tip_y).abs() < 0.05, "flip={flipped}: start plus Length reaches the tip: {at:?} vs y={tip_y}");
    }
}

#[test]
fn inch_sheet_rule_edit_preserves_stored_lengths_and_dimensionless_k_factor() {
    let mut h = crate::scenario::Harness::new();
    h.app.run("sheet.manage_rules", json!({"name":"Test","thickness":"1.5","k_factor":"0.4","active":true})).unwrap();
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.app.start("sheet.manage_rules");
    h.frames(3);
    let mut d = h.app.dialog.clone().unwrap();
    let unchanged = apply_commands(&h.app, &d).unwrap().remove(0).1;
    assert_eq!(unchanged["thickness"], json!("1.5 mm"));
    assert_eq!(unchanged["k_factor"], json!("0.4"));
    let Kind::Sheet(Sm::Rules { values, .. }) = &mut d.kind else { panic!("no rules") };
    values[0] = "0.125".into();
    let changed = apply_commands(&h.app, &d).unwrap().remove(0).1;
    assert_eq!(changed["thickness"], json!("0.125 in"));
    assert_eq!(changed["k_factor"], unchanged["k_factor"]);
}

#[test]
fn perforate_toolbar_opens_without_running_empty_parameters() {
    for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let features = app.session.doc.features.len();
        app.start("sheet.perforate");
        let dialog = app.dialog.as_ref().expect("Perforate opens a dialog");
        assert!(dialog.previews());
        assert_eq!(app.session.doc.features.len(), features);
        assert!(!app.status.as_ref().is_some_and(|(_, _, error)| *error));
        let commands = apply_commands(&app, dialog).unwrap();
        assert_eq!(commands[0].0, "sheet.perforate");
        assert_eq!(commands[0].1["diameter"], "3 mm");
        assert_eq!(commands[0].1["pitch"], "6 mm");
        assert_eq!(commands[0].1["pattern"], "staggered");
        assert!(commands[0].1.get("margin").is_none());
        let mut dialog = dialog.clone();
        let ctx = crate::fonts::test_context();
        let output = crate::test_support::frame(&ctx, egui::RawInput::default(), |ui| {
            if let Kind::Sheet(k) = &mut dialog.kind {
                rows(&mut app, ui, k, &mut dialog.inputs);
            }
        });
        let texts: Vec<_> = crate::test_support::text_shapes(&output).map(|text| text.galley.text()).collect();
        for label in ["Diameter", "Pitch", "Margin", "Pattern", "Staggered (60°)", "3 mm", "6 mm", "from the diameter"] {
            assert!(texts.contains(&label), "missing field or default: {label}");
        }
    }
}

#[test]
fn perforate_ok_targets_the_picked_sheet_and_updates_flat_data() {
    for face in [false, true] {
        let mut app = plate(crate::shell::LayoutMode::Docked);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        let body = app.session.model.state().sheets[0].body.clone();
        app.session.selection =
            vec![if face { face_sel(&app.session, Vec3::new(50.0, 30.0, 2.5)).unwrap() } else { Sel::Body { name: body.clone() } }];
        app.start("sheet.perforate");
        let dialog = app.dialog.as_ref().expect("Perforate opens a dialog");
        let commands = apply_commands(&app, dialog).unwrap();
        assert_eq!(commands[0].1["body"], body);
        run_all(&mut app, commands);
        let flat = app.session.execute("sheet.flat_pattern", &json!({"body": body})).unwrap();
        let field = &flat["perforations"][0];
        assert_eq!(field["diameter"], 3.0);
        assert_eq!(field["pitch"], 6.0);
        assert_eq!(field["margin"], 3.0);
        assert!(field["count"].as_u64().unwrap() > 0);
    }
}

#[test]
fn perforate_edit_restores_dimensions_pattern_and_target() {
    let mut app = plate(crate::shell::LayoutMode::Docked);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let body = app.session.model.state().sheets[0].body.clone();
    app.run("sheet.perforate", json!({"body": body, "diameter": "4 mm", "pitch": "9 mm", "margin": "5 mm", "pattern": "square"})).unwrap();
    let feature = app.session.doc.features.last().unwrap();
    let dialog = edit_dialog(&app, feature.id, None).expect("Perforate can be edited");
    let commands = apply_commands(&app, &dialog).unwrap();
    assert_eq!(commands[0].0, "timeline.redefine");
    let params = &commands[0].1["params"];
    for (key, value) in [("body", body.as_str()), ("diameter", "4 mm"), ("pitch", "9 mm"), ("margin", "5 mm"), ("pattern", "square")] {
        assert_eq!(params[key], value, "{key} is restored");
    }
}

#[test]
fn flat_pattern_navigation_slots_hide_orbit_controls_and_keep_fit_home() {
    let ctx = crate::fonts::test_context();
    let mut app = flat_app("fusion", crate::shell::LayoutMode::Floating);
    app.preferences.navigation_bar = serde_json::from_value(json!({
        "slots":["orbit","spin","look_at","projection","visual_style","pan","zoom","fit","home","display"]
    }))
    .unwrap();
    let mut time = 0.0;
    crate::scenario::clear_handles();
    frame(&mut app, &ctx, &mut time, egui::Modifiers::NONE, vec![]);
    assert_eq!(crate::navigation_bar::canvas_caps(&app), crate::navigation::NavCaps { orbit: false, pick: false });
    for slot in ["orbit", "spin", "look_at", "projection", "visual_style"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_none(), "{slot}");
    }
    for slot in ["pan", "zoom", "fit", "home", "display"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_some(), "{slot}");
    }
}
