use crate::test_support::run_all;
use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, for_feature as edit_dialog};

/// A 100 x 60 base flange on XY (2.5 mm steel).
fn plate(mode: crate::ui_kit::LayoutMode) -> SolveApp {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("flat-pocket-test")));
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
        if mode == crate::ui_kit::LayoutMode::Floating {
            app.shell.content = Some(Rect::from_min_max(pos2(browser.right(), viewport.top()), pos2(viewport.right(), history.top())));
        }
        let ctx = crate::fonts::test_context();
        let id = egui::Id::unique(("sc_dialog", "FLAT PATTERN"));
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
                    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("island-fit")));
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
        if mode == crate::ui_kit::LayoutMode::Floating {
            assert!(
                outlines.iter().flat_map(|p| &p.points).all(|p| !browser.contains(*p) && !history.contains(*p)),
                "flat pattern fits clear of the shell cards"
            );
        }
    }
}

#[test]
fn coincident_top_and_bottom_pocket_depths_share_a_readable_annotation() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("coincident-pocket-test")));
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
            state.sheets.last().map(|s| s.base.outer.segs.iter().filter(|g| matches!(g, solidcore_record::Seg2::Arc { .. })).count())
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
                .map(|s| s.base.outer.segs.iter().filter(|g| matches!(g, solidcore_record::Seg2::Arc { .. })).count())
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
fn pocket_depth_labels_follow_inches_without_changing_flat_data() {
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
        let mut app = plate(mode);
        app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
        app.run("sheet.flange", json!({"edges": [[0, 30, 2.5]], "height": 30})).unwrap();
        // A path across the wall's end edge: 30 mm out at 45°, then 30 mm on along the wall.
        let a = 30.0 / 2f64.sqrt();
        app.run("sketch.create", json!({"plane": {"origin": [0, 0, 15], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "name": "Path"})).unwrap();
        app.run("sketch.line", json!({"points": [[0, 0], [a, -a], [a + 30.0, -a]], "ids": ["p1", "p2"]})).unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        let path = crate::open_meshing::session_world(&app.session).sketches.last().map(|ss| ss.feature);
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

fn flat_app(preset: &str, mode: crate::ui_kit::LayoutMode) -> SolveApp {
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
fn same_view(a: &solidcraft_viewport::Camera, b: &solidcraft_viewport::Camera) -> bool {
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
                let wheel = Event::MouseWheel {
                    source: egui::MouseWheelSource::Unknown,
                    unit: egui::MouseWheelUnit::Line,
                    delta: vec2(0.0, 3.0),
                    modifiers: none,
                    phase: egui::TouchPhase::Move,
                };
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
            let wheel = Event::MouseWheel {
                source: egui::MouseWheelSource::Unknown,
                unit: egui::MouseWheelUnit::Line,
                delta: vec2(0.0, 3.0),
                modifiers: none,
                phase: egui::TouchPhase::Move,
            };
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
    let mut app = plate(crate::ui_kit::LayoutMode::Floating);
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
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("perforation-test")));
        assert!(overlay(app, &painter, rect, dialog));
    });
    output.shapes.into_iter().filter_map(|s| if let egui::Shape::Circle(c) = s.shape { Some(c) } else { None }).collect()
}

#[test]
fn flat_perforations_match_export_centres_and_cutout_style() {
    let (mut app, dialog) = perforated_flat();
    let state = crate::open_meshing::session_world(&app.session);
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
    let state = crate::open_meshing::session_world(&app.session);
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
        let mut app = plate(crate::ui_kit::LayoutMode::Floating);
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
        let m = crate::open_meshing::session_world(&app.session).body(&body).unwrap().mesh();
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
    for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
        let mut app = plate(crate::ui_kit::LayoutMode::Docked);
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
    let mut app = plate(crate::ui_kit::LayoutMode::Docked);
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
    let mut h = crate::scenario::Harness::new();
    h.app = flat_app("fusion", crate::ui_kit::LayoutMode::Floating);
    h.app.home.open = false;
    h.app.preferences.navigation_bar = serde_json::from_value(json!({
        "slots":["orbit","spin","look_at","projection","visual_style","pan","zoom","fit","home","display"]
    }))
    .unwrap();
    h.frames(3);
    assert_eq!(crate::navigation_bar_view::canvas_caps(&h.app), crate::navigation::NavCaps { orbit: false, pick: false });
    for slot in ["orbit", "spin", "look_at", "projection", "visual_style"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_none(), "{slot}");
    }
    for slot in ["pan", "zoom", "fit", "home", "display"] {
        assert!(crate::scenario::handle_at(&format!("nav:slot:{slot}")).is_some(), "{slot}");
    }
}

/// A picked edge split into segments: the dialog sends equal stretches with the rows' angle,
/// height and turns, and the part builds with two bends.
#[test]
fn an_edge_flange_can_be_split_into_segments() {
    let mut app = plate(crate::ui_kit::LayoutMode::Floating);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let edge = edge_sel(&app.session, Vec3::new(50.0, 0.0, 2.5)).unwrap();
    app.session.selection = vec![edge];
    app.start("sheet.flange");
    let mut d = app.dialog.clone().unwrap();
    let p = edge_point_list(&d.inputs)[0];
    if let Kind::Sheet(Sm::Flange { height, angle, segments, .. }) = &mut d.kind {
        *height = "20 mm".into();
        *angle = "90 deg".into();
        let slot = segments_slot(segments, p);
        let list = &mut segments[slot].1;
        add_segment(list);
        list[0].height = "20 mm".into();
        add_segment(list);
        list[1].height = "10 mm".into();
    }
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(
        c[0].1["edges"][0]["segments"],
        json!([{"from": 0.0, "to": 0.5, "height": "20 mm"}, {"from": 0.5, "to": 1.0, "height": "10 mm"}]),
        "{:?}",
        c[0].1
    );
    run_all(&mut app, c);
    let bends = app.session.execute("sheet.flat_pattern", &json!({})).unwrap()["bends"].clone();
    assert_eq!(bends.as_array().map(Vec::len), Some(2), "{bends}");
    assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
}

/// Edit Feature on a flange whose edges carry their own segments comes back as it was: the dialog
/// holds every edge's list with its own fractions, angle, height and side, and OK emits the same
/// `sheet.flange` parameters, byte for byte (an explicit `flip: false` under a flipped feature
/// stays explicit, and neither edge takes the other's list).
#[test]
fn editing_a_flange_keeps_every_edges_segments() {
    let mut app = plate(crate::ui_kit::LayoutMode::Floating);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let params = json!({
        "type": "edge",
        "edges": [
            {"point": [50.0, 0.0, 2.5], "segments": [
                {"from": 0.0, "to": 0.3, "angle": "90 deg", "height": "20 mm"},
                {"from": 0.3, "to": 1.0, "angle": "45 deg", "height": "15 mm", "flip": false}
            ]},
            {"point": [50.0, 60.0, 2.5], "segments": [
                {"from": 0.0, "to": 0.6, "height": "25 mm", "flip": true},
                {"from": 0.6, "to": 1.0, "height": "10 mm"}
            ]}
        ],
        "height": "20 mm",
        "angle": "90 deg",
        "position": "inside",
        "flip": true
    });
    app.run("sheet.flange", params.clone()).unwrap();
    assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
    let f = app.session.doc.features.last().unwrap().clone();
    let e = edit_dialog(&app, f.id, None).unwrap();
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(
        serde_json::to_string(&c[0].1["params"]).unwrap(),
        serde_json::to_string(&params).unwrap(),
        "the edit sends the design back as it was: {}",
        c[0].1["params"]
    );
}

/// A two-stretch edge in the Flange dialog, restored from a design with a stretch boundary that is
/// not the middle.
fn two_stretch_flange(h: &mut crate::scenario::Harness) -> u64 {
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.app
        .run(
            "sheet.flange",
            json!({
                "type": "edge",
                "edges": [{"point": [50.0, 0.0, 2.5], "segments": [
                    {"from": 0.0, "to": 0.3, "angle": "90 deg", "height": "20 mm"},
                    {"from": 0.3, "to": 1.0, "angle": "45 deg", "height": "15 mm"}
                ]}],
                "height": "20 mm", "angle": "90 deg", "position": "inside", "flip": false
            }),
        )
        .unwrap();
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    h.app.session.doc.features.last().unwrap().id
}

/// The dialog's restored stretches, `from`…`to` as typed, per picked edge.
fn restored_stretches(h: &crate::scenario::Harness) -> Vec<Vec<(String, String)>> {
    match &h.app.dialog.as_ref().expect("a dialog").kind {
        Kind::Sheet(Sm::Flange { segments, .. }) => {
            segments.iter().map(|(_, list)| list.iter().map(|r| (r.from.clone(), r.to.clone())).collect()).collect()
        }
        other => panic!("not a flange dialog: {other:?}"),
    }
}

/// Deleting a stretch in the Flange dialog keeps the edge a partition the command accepts: the
/// stretch after the removed one reaches back to the edge's start, and the ends stay at 0 and 1. The
/// emitted `sheet.flange` covers the whole edge with the surviving stretch's own values, and OK
/// applies it (#5927, #5928).
#[test]
fn deleting_a_segment_in_the_flange_dialog_keeps_the_partition_whole() {
    let mut h = crate::scenario::Harness::new();
    let id = two_stretch_flange(&mut h);
    let count = h.app.session.doc.features.len();
    h.app.edit_feature(id);
    h.frames(6);
    assert_eq!(
        restored_stretches(&h),
        vec![vec![("0".to_string(), "0.3".to_string()), ("0.3".to_string(), "1".to_string())]],
        "the design's own fractions come back"
    );
    // The first stretch's bin takes it out; the one after it reaches back to the edge's start.
    h.step(&json!({"click": {"handle": "dialog:button:Segment 1:Delete"}})).unwrap();
    h.frames(3);
    assert_eq!(restored_stretches(&h), vec![vec![("0".to_string(), "1".to_string())]], "the partition stays whole");
    // The command the dialog would run is a valid partition of the whole edge, with the surviving
    // stretch's angle and height.
    let d = h.app.dialog.clone().expect("the dialog is open");
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(
        c[0].1["params"]["edges"][0]["segments"],
        json!([{"from": 0.0, "to": 1.0, "angle": "45 deg", "height": "15 mm"}]),
        "{}",
        c[0].1["params"]
    );
    // OK submits it: the feature is redefined in place and builds one bend over the whole edge.
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK applies the repaired list");
    assert_eq!(h.app.session.doc.features.len(), count, "redefined in place");
    let f = h.app.session.doc.features.last().unwrap().clone();
    let FeatureKind::SheetFlange { edges, .. } = &f.kind else { panic!("not a flange feature") };
    assert_eq!(edges.len(), 1);
    let segs = &edges[0].segments;
    assert_eq!(segs.len(), 1, "the surviving stretch is the whole edge: {segs:?}");
    assert_eq!((segs[0].from, segs[0].to), (0.0, 1.0));
    assert_eq!(segs[0].angle.as_ref().map(|a| a.as_str()), Some("45 deg"));
    assert_eq!(segs[0].height.as_ref().map(|a| a.as_str()), Some("15 mm"));
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    let bends = h.app.session.execute("sheet.flat_pattern", &json!({})).unwrap()["bends"].clone();
    assert_eq!(bends.as_array().map(Vec::len), Some(1), "one bend over the whole edge: {bends}");
}

/// Deleting the last stretch reaches the one before it to the edge's end, and deleting the only
/// stretch left leaves the whole edge (a plain edge point): deleting never leaves a list the
/// command refuses.
#[test]
fn deleting_the_last_or_only_segment_leaves_the_whole_edge() {
    let mut h = crate::scenario::Harness::new();
    let id = two_stretch_flange(&mut h);
    let count = h.app.session.doc.features.len();
    // The last stretch: the one before it reaches to the edge's end with its own values.
    h.app.edit_feature(id);
    h.frames(6);
    h.step(&json!({"click": {"handle": "dialog:button:Segment 2:Delete"}})).unwrap();
    h.frames(3);
    assert_eq!(restored_stretches(&h), vec![vec![("0".to_string(), "1".to_string())]], "the partition stays whole");
    let d = h.app.dialog.clone().expect("the dialog is open");
    assert_eq!(
        apply_commands(&h.app, &d).unwrap()[0].1["params"]["edges"][0]["segments"],
        json!([{"from": 0.0, "to": 1.0, "angle": "90 deg", "height": "20 mm"}]),
        "the stretch before the removed one takes its place"
    );
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK applies the repaired list");
    assert_eq!(h.app.session.doc.features.len(), count, "redefined in place");
    // The only stretch left: removing it leaves the whole edge, a plain point with no segments.
    h.app.edit_feature(id);
    h.frames(6);
    assert_eq!(restored_stretches(&h), vec![vec![("0".to_string(), "1".to_string())]], "one stretch comes back");
    h.step(&json!({"click": {"handle": "dialog:button:Segment 1:Delete"}})).unwrap();
    h.frames(3);
    assert_eq!(restored_stretches(&h), vec![Vec::new()], "no stretches: the whole edge");
    let d = h.app.dialog.clone().expect("the dialog is open");
    let params = apply_commands(&h.app, &d).unwrap()[0].1["params"].clone();
    assert_eq!(params["edges"][0], json!([50.0, 0.0, 2.5]), "the whole edge, as an edge flange was before: {params}");
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK applies the whole edge");
    assert_eq!(h.app.session.doc.features.len(), count, "redefined in place");
    let f = h.app.session.doc.features.last().unwrap().clone();
    let FeatureKind::SheetFlange { edges, .. } = &f.kind else { panic!("not a flange feature") };
    assert_eq!(edges.len(), 1);
    assert!(edges[0].segments.is_empty(), "no segments: the whole edge");
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    let bends = h.app.session.execute("sheet.flat_pattern", &json!({})).unwrap()["bends"].clone();
    assert_eq!(bends.as_array().map(Vec::len), Some(1), "one bend over the whole edge: {bends}");
}

/// A four-stretch edge in the Flange dialog, restored from a design whose boundaries are not equal
/// (0, 0.1, 0.4, 0.8, 1), each stretch with its own angle and height.
fn four_stretch_flange(h: &mut crate::scenario::Harness) -> u64 {
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    h.app
        .run(
            "sheet.flange",
            json!({
                "type": "edge",
                "edges": [{"point": [50.0, 0.0, 2.5], "segments": [
                    {"from": 0.0, "to": 0.1, "angle": "90 deg", "height": "20 mm"},
                    {"from": 0.1, "to": 0.4, "angle": "45 deg", "height": "15 mm"},
                    {"from": 0.4, "to": 0.8, "angle": "90 deg", "height": "20 mm"},
                    {"from": 0.8, "to": 1.0, "angle": "45 deg", "height": "15 mm"}
                ]}],
                "height": "20 mm", "angle": "90 deg", "position": "inside", "flip": false
            }),
        )
        .unwrap();
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    h.app.session.doc.features.last().unwrap().id
}

/// The stretches the dialog holds for the one edge, as `from`…`to` typed, from a design with the
/// boundaries 0, 0.1, 0.4, 0.8, 1: deleting a stretch whose values are intact bridges the gap from
/// the neighbours, so only the removed stretch's boundaries change and OK submits a partition the
/// command accepts (#5927, #5928).
#[test]
fn deleting_a_segment_with_its_values_intact_keeps_every_other_boundary() {
    let mut h = crate::scenario::Harness::new();
    let id = four_stretch_flange(&mut h);
    let count = h.app.session.doc.features.len();
    h.app.edit_feature(id);
    h.frames(6);
    assert_eq!(
        restored_stretches(&h),
        vec![vec![
            ("0".to_string(), "0.1".to_string()),
            ("0.1".to_string(), "0.4".to_string()),
            ("0.4".to_string(), "0.8".to_string()),
            ("0.8".to_string(), "1".to_string())
        ]],
        "the design's own fractions come back"
    );
    // The second stretch's bin takes it out: the first reaches to where the third starts.
    h.step(&json!({"click": {"handle": "dialog:button:Segment 2:Delete"}})).unwrap();
    h.frames(3);
    assert_eq!(
        restored_stretches(&h),
        vec![vec![("0".to_string(), "0.4".to_string()), ("0.4".to_string(), "0.8".to_string()), ("0.8".to_string(), "1".to_string())]],
        "the boundaries 0.4, 0.8 and 1 are where they were"
    );
    let d = h.app.dialog.clone().expect("the dialog is open");
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(
        c[0].1["params"]["edges"][0]["segments"],
        json!([
            {"from": 0.0, "to": 0.4, "angle": "90 deg", "height": "20 mm"},
            {"from": 0.4, "to": 0.8, "angle": "90 deg", "height": "20 mm"},
            {"from": 0.8, "to": 1.0, "angle": "45 deg", "height": "15 mm"}
        ]),
        "{}",
        c[0].1["params"]
    );
    // OK submits it: the feature is redefined in place and builds.
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK applies the repaired list");
    assert_eq!(h.app.session.doc.features.len(), count, "redefined in place");
    let f = h.app.session.doc.features.last().unwrap().clone();
    let FeatureKind::SheetFlange { edges, .. } = &f.kind else { panic!("not a flange feature") };
    let segs: Vec<(f64, f64)> = edges[0].segments.iter().map(|s| (s.from, s.to)).collect();
    assert_eq!(segs, vec![(0.0, 0.4), (0.4, 0.8), (0.8, 1.0)], "a partition of the edge");
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    assert_eq!(
        h.app.session.execute("sheet.flat_pattern", &json!({})).unwrap()["bends"].as_array().map(Vec::len),
        Some(2),
        "the two bends that are left"
    );
}

/// The same delete with the removed stretch's `to` cleared, as a user leaves it mid-edit: the repair
/// reads the neighbours, not the removed row, so the boundaries the user did not touch stay put
/// (the untouched 0.8 does not become 2/3) and OK submits a valid partition (#5927, #5928).
#[test]
fn deleting_a_mid_edit_segment_keeps_the_boundaries_it_did_not_touch() {
    let mut h = crate::scenario::Harness::new();
    let id = four_stretch_flange(&mut h);
    let count = h.app.session.doc.features.len();
    h.app.edit_feature(id);
    h.frames(6);
    // The user clears the second stretch's `to` in its field.
    h.step(&json!({"widget": "Segment 2 to"})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"key": "Delete"})).unwrap();
    h.frames(3);
    assert_eq!(restored_stretches(&h)[0][1], ("0.1".to_string(), String::new()), "the removed stretch's `to` is empty, as while editing");
    // Delete that stretch: the gap is bridged from the neighbours.
    h.step(&json!({"click": {"handle": "dialog:button:Segment 2:Delete"}})).unwrap();
    h.frames(3);
    assert_eq!(
        restored_stretches(&h),
        vec![vec![("0".to_string(), "0.4".to_string()), ("0.4".to_string(), "0.8".to_string()), ("0.8".to_string(), "1".to_string())]],
        "the untouched boundaries 0.8 and 1 did not move"
    );
    let d = h.app.dialog.clone().expect("the dialog is open");
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(
        c[0].1["params"]["edges"][0]["segments"],
        json!([
            {"from": 0.0, "to": 0.4, "angle": "90 deg", "height": "20 mm"},
            {"from": 0.4, "to": 0.8, "angle": "90 deg", "height": "20 mm"},
            {"from": 0.8, "to": 1.0, "angle": "45 deg", "height": "15 mm"}
        ]),
        "{}",
        c[0].1["params"]
    );
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK applies the repaired list");
    assert_eq!(h.app.session.doc.features.len(), count, "redefined in place");
    let f = h.app.session.doc.features.last().unwrap().clone();
    let FeatureKind::SheetFlange { edges, .. } = &f.kind else { panic!("not a flange feature") };
    let segs: Vec<(f64, f64)> = edges[0].segments.iter().map(|s| (s.from, s.to)).collect();
    assert_eq!(segs, vec![(0.0, 0.4), (0.4, 0.8), (0.8, 1.0)], "a partition of the edge");
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
}

/// `then` with a split edge is refused, not dropped: the message names the edge and says where the
/// chain goes instead, and the guide carries the same words (#5927, #5928).
#[test]
fn then_with_segments_is_refused_with_a_message() {
    let mut app = plate(crate::ui_kit::LayoutMode::Floating);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let edge = edge_sel(&app.session, Vec3::new(50.0, 0.0, 2.5)).unwrap();
    app.session.selection = vec![edge];
    app.start("sheet.flange");
    let mut d = app.dialog.clone().unwrap();
    let p = edge_point_list(&d.inputs)[0];
    if let Kind::Sheet(Sm::Flange { height, angle, segments, then, .. }) = &mut d.kind {
        *height = "20 mm".into();
        *angle = "90 deg".into();
        let slot = segments_slot(segments, p);
        add_segment(&mut segments[slot].1);
        add_segment(&mut segments[slot].1);
        then.push(("45 deg".into(), "20 mm".into(), false));
    }
    let error = apply_commands(&app, &d).unwrap_err();
    assert_eq!(
        error, "edge 1 is split into segments: a split edge takes no `then`; bend each segment's far edge in its own feature",
        "the refusal names the edge"
    );
    // The guide says the same, so an agent meets the words here and there.
    assert!(
        include_str!("../../../skills/solidcraft-sheetmetal/reference.md")
            .contains("a split edge takes no `then`; bend each segment's far edge in its own feature"),
        "the guide carries the refusal"
    );
    // Without segments the chain still goes.
    let mut d = d.clone();
    if let Kind::Sheet(Sm::Flange { segments, .. }) = &mut d.kind {
        segments.clear();
    }
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["then"], json!([{"height": "20 mm", "angle": "45 deg"}]));
}

/// A flange on the picked `edges`, each split into the same two stretches with heights and angles
/// of its own: the dialog's per-edge rows then carry the same labels on every edge.
fn multi_edge_flange(h: &mut crate::scenario::Harness, edges: &[[f64; 3]], heights: &[(&str, &str)], angles: &[(&str, &str)]) -> u64 {
    // A window tall enough for every edge's rows: each picked edge adds six rows, and a dialog
    // taller than the screen has rows below it.
    h.call("ui.resize", json!({"width": 1600, "height": 2600}));
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let list: Vec<serde_json::Value> = edges
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({"point": p, "segments": [
                {"from": 0.0, "to": 0.5, "angle": angles[i].0, "height": heights[i].0},
                {"from": 0.5, "to": 1.0, "angle": angles[i].1, "height": heights[i].1}
            ]})
        })
        .collect();
    h.app
        .run("sheet.flange", json!({"type": "edge", "edges": list, "height": "20 mm", "angle": "90 deg", "position": "inside", "flip": false}))
        .unwrap();
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    h.app.session.doc.features.last().unwrap().id
}

/// The dialog's stretches, per picked edge, as `(from, to, angle, height)` typed.
fn segment_rows(h: &crate::scenario::Harness) -> Vec<Vec<(String, String, String, String)>> {
    match &h.app.dialog.as_ref().expect("a dialog").kind {
        Kind::Sheet(Sm::Flange { segments, .. }) => segments
            .iter()
            .map(|(_, list)| list.iter().map(|r| (r.from.clone(), r.to.clone(), r.angle.clone(), r.height.clone())).collect())
            .collect(),
        other => panic!("not a flange dialog: {other:?}"),
    }
}

/// The number a `height`/`angle` field holds, for comparing a drag's effect.
fn field_number(text: &str) -> f64 {
    let number: String = text.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    number.parse().unwrap_or_else(|e| panic!("`{text}` is not a number: {e}"))
}

/// Drag the label of the `k`th row named `text` sideways by `dx` points. Every picked edge's rows
/// carry the same labels, and the label rects are published in draw order, one per edge, so the
/// `k`th is the `k`th edge's row.
fn drag_label(h: &mut crate::scenario::Harness, text: &str, k: usize, dx: f32) {
    let rects: Vec<egui::Rect> = crate::scenario::rects(&format!("dialog:label:{text}")).into_iter().map(|(_, r)| r).collect();
    assert!(rects.len() > k, "`{text}`: {} label rects drawn, want one per edge: {rects:?}", rects.len());
    eprintln!("DEBUG `{text}`[{k}] rects={rects:?} window={:?} content={:?}", crate::scenario::rect_of("dialog:window"), h.ctx.content_rect());
    let c = rects[k].center();
    h.step(&json!({"drag": [[c.x, c.y], [c.x + dx, c.y]], "steps": 8})).unwrap();
    h.frames(3);
}

/// Two picked edges, each split into two stretches: dragging one edge's label changes that edge's
/// value alone. The labels repeat, so a row's scrub (label-drag) id must be scoped per edge; with
/// one shared form scope the rows share the id, the last edge's row takes the drag and every
/// edge's value moves with it (#5927, #5928).
#[test]
fn dragging_one_edges_segment_height_leaves_the_other_edge_alone() {
    let mut h = crate::scenario::Harness::new();
    let id = multi_edge_flange(
        &mut h,
        &[[50.0, 0.0, 2.5], [50.0, 60.0, 2.5]],
        &[("20 mm", "15 mm"), ("25 mm", "10 mm")],
        &[("90 deg", "45 deg"), ("60 deg", "30 deg")],
    );
    h.app.edit_feature(id);
    h.frames(6);
    let before = segment_rows(&h);
    assert_eq!(before.len(), 2, "the dialog holds both edges: {before:?}");
    assert_eq!(
        before[0],
        vec![("0".into(), "0.5".into(), "90 deg".into(), "20 mm".into()), ("0.5".into(), "1".into(), "45 deg".into(), "15 mm".into())],
        "{before:?}"
    );
    assert_eq!(
        before[1],
        vec![("0".into(), "0.5".into(), "60 deg".into(), "25 mm".into()), ("0.5".into(), "1".into(), "30 deg".into(), "10 mm".into())],
        "{before:?}"
    );
    // Drag edge 2's second stretch's angle label, the last edge in the form: with one shared
    // scope this drag moved edge 1's angle too, because both rows held the same id.
    drag_label(&mut h, "Segment 2 angle", 1, 40.0);
    let after_last = segment_rows(&h);
    assert_eq!(after_last[0], before[0], "edge 1 is untouched by edge 2's drag: {after_last:?}");
    assert_ne!(after_last[1][1].2, before[1][1].2, "edge 2's dragged angle moved: {after_last:?}");
    assert_eq!(after_last[1][0], before[1][0], "edge 2's other stretch stays");
    // Drag edge 1's first stretch's height label; 40 points moves a 20 mm value by 4 mm.
    drag_label(&mut h, "Segment 1 height", 0, 40.0);
    let after = segment_rows(&h);
    assert_ne!(field_number(&after[0][0].3), field_number(&before[0][0].3), "the dragged edge's height moved: {after:?}");
    assert_eq!(after[0][0].0, before[0][0].0);
    assert_eq!(after[0][0].1, before[0][0].1);
    assert_eq!(after[0][0].2, before[0][0].2, "the dragged row's angle stays");
    assert_eq!(after[0][1], before[0][1], "the dragged edge's other stretch stays");
    assert_eq!(after[1], after_last[1], "edge 2's height and angle are untouched by edge 1's drag: {after:?}");
    // The command the dialog would run carries each drag in its own edge only.
    let d = h.app.dialog.clone().expect("the dialog is open");
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    let params = &c[0].1["params"];
    assert_eq!(params["edges"][1]["point"], json!([50.0, 60.0, 2.5]), "{}", params);
    assert_eq!(params["edges"][1]["segments"][0], json!({"from": 0.0, "to": 0.5, "angle": "60 deg", "height": "25 mm"}), "{}", params);
    assert_eq!(params["edges"][1]["segments"][1], json!({"from": 0.5, "to": 1.0, "angle": after[1][1].2, "height": "10 mm"}), "{}", params);
    assert_eq!(params["edges"][0]["segments"][0]["height"], json!(after[0][0].3.clone()), "{}", params);
    assert_eq!(params["edges"][0]["segments"][0]["angle"], json!("90 deg"), "{}", params);
    assert_eq!(params["edges"][0]["segments"][1], json!({"from": 0.5, "to": 1.0, "angle": "45 deg", "height": "15 mm"}), "{}", params);
}

/// Three picked edges with the same two stretches each: dragging one field on each edge changes
/// that edge's value alone, so every edge's rows hold ids of their own (#5927, #5928).
#[test]
fn dragging_each_of_three_edges_fields_changes_only_that_edge() {
    let mut h = crate::scenario::Harness::new();
    let id = multi_edge_flange(
        &mut h,
        &[[50.0, 0.0, 2.5], [50.0, 60.0, 2.5], [0.0, 30.0, 2.5]],
        &[("20 mm", "15 mm"), ("25 mm", "10 mm"), ("30 mm", "12 mm")],
        &[("90 deg", "45 deg"), ("60 deg", "30 deg"), ("80 deg", "40 deg")],
    );
    h.app.edit_feature(id);
    h.frames(6);
    let mut before = segment_rows(&h);
    assert_eq!(before.len(), 3, "the dialog holds all three edges: {before:?}");
    for (edge, label) in [(0usize, "Segment 1 height"), (1, "Segment 2 angle"), (2, "Segment 1 angle")] {
        drag_label(&mut h, label, edge, 40.0);
        let after = segment_rows(&h);
        for other in 0..3 {
            if other == edge {
                continue;
            }
            assert_eq!(after[other], before[other], "edge {} after dragging edge {}'s `{label}`: {after:?}", other + 1, edge + 1);
        }
        let moved = match label {
            "Segment 1 height" => after[edge][0].3 != before[edge][0].3,
            "Segment 2 angle" => after[edge][1].2 != before[edge][1].2,
            _ => after[edge][0].2 != before[edge][0].2,
        };
        assert!(moved, "edge {}'s `{label}` moved: {after:?}", edge + 1);
        before = after;
    }
    let d = h.app.dialog.clone().expect("the dialog is open");
    let params = apply_commands(&h.app, &d).unwrap()[0].1["params"].clone();
    let seg = |edge: usize, m: usize| params["edges"][edge]["segments"][m].clone();
    assert_eq!(seg(0, 0)["height"], json!(before[0][0].3.clone()), "{params}");
    assert_eq!(seg(1, 1)["angle"], json!(before[1][1].2.clone()), "{params}");
    assert_eq!(seg(2, 0)["angle"], json!(before[2][0].2.clone()), "{params}");
    // Every stretch the drags did not touch is the design's.
    assert_eq!(seg(0, 1), json!({"from": 0.5, "to": 1.0, "angle": "45 deg", "height": "15 mm"}), "{params}");
    assert_eq!(seg(1, 0), json!({"from": 0.0, "to": 0.5, "angle": "60 deg", "height": "25 mm"}), "{params}");
    assert_eq!(seg(2, 1), json!({"from": 0.5, "to": 1.0, "angle": "40 deg", "height": "12 mm"}), "{params}");
    assert_eq!(seg(0, 0)["from"], json!(0.0), "{params}");
    assert_eq!(seg(0, 0)["to"], json!(0.5), "{params}");
    assert_eq!(seg(1, 1)["height"], json!("10 mm"), "{params}");
    assert_eq!(seg(2, 0)["height"], json!("30 mm"), "{params}");
}

/// The byte-identical per-edge round-trip after a label drag: the drag changes the dragged
/// stretch's height only, the other edge's list comes back byte for byte as the design wrote it,
/// and Edit Feature on the redefined feature sends the same bytes back (#5927, #5928).
#[test]
fn a_dragged_segment_round_trip_keeps_every_other_edge_byte_identical() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.resize", json!({"width": 1600, "height": 2000}));
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Base"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let design = json!({
        "type": "edge",
        "edges": [
            {"point": [50.0, 0.0, 2.5], "segments": [
                {"from": 0.0, "to": 0.3, "angle": "90 deg", "height": "20 mm"},
                {"from": 0.3, "to": 1.0, "angle": "45 deg", "height": "15 mm", "flip": false}
            ]},
            {"point": [50.0, 60.0, 2.5], "segments": [
                {"from": 0.0, "to": 0.6, "height": "25 mm", "flip": true},
                {"from": 0.6, "to": 1.0, "height": "10 mm"}
            ]}
        ],
        "height": "20 mm",
        "angle": "90 deg",
        "position": "inside",
        "flip": true
    });
    h.app.run("sheet.flange", design.clone()).unwrap();
    assert!(h.app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", h.app.session.model.results);
    let id = h.app.session.doc.features.last().unwrap().id;
    h.app.edit_feature(id);
    h.frames(6);
    let before = segment_rows(&h);
    assert_eq!(before.len(), 2, "{before:?}");
    // Drag edge 1's first stretch's height: 20 mm moves by 4 mm at 40 points.
    drag_label(&mut h, "Segment 1 height", 0, 40.0);
    let after = segment_rows(&h);
    assert_eq!(after[1], before[1], "edge 2's list is untouched: {after:?}");
    assert_ne!(after[0][0].3, before[0][0].3, "edge 1's dragged height moved: {after:?}");
    assert_eq!(
        (after[0][0].0.as_str(), after[0][0].1.as_str(), after[0][0].2.as_str()),
        ("0", "0.3", "90 deg"),
        "the dragged stretch keeps its bounds and angle"
    );
    let d = h.app.dialog.clone().expect("the dialog is open");
    let c = apply_commands(&h.app, &d).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    let params = c[0].1["params"].clone();
    assert_eq!(
        serde_json::to_string(&params["edges"][1]).unwrap(),
        serde_json::to_string(&design["edges"][1]).unwrap(),
        "the edge the drag did not touch goes back as the design wrote it: {params}"
    );
    let mut expected = design.clone();
    expected["edges"][0]["segments"][0]["height"] = json!(after[0][0].3.clone());
    assert_eq!(serde_json::to_string(&params).unwrap(), serde_json::to_string(&expected).unwrap(), "only the dragged height differs: {params}");
    // OK applies it; Edit Feature sends the same parameters back, byte for byte.
    h.step(&json!({"click": {"handle": "dialog:ok"}})).unwrap();
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK applies the dragged value");
    let f = h.app.session.doc.features.last().unwrap().id;
    h.app.edit_feature(f);
    h.frames(6);
    let d = h.app.dialog.clone().expect("the dialog is open again");
    let again = apply_commands(&h.app, &d).unwrap();
    assert_eq!(
        serde_json::to_string(&again[0].1["params"]).unwrap(),
        serde_json::to_string(&params).unwrap(),
        "the edit after the drag is byte-identical: {}",
        again[0].1["params"]
    );
    assert_eq!(segment_rows(&h)[1], before[1], "edge 2 still comes back as it was");
}

/// A stretch's relief describes the junction at its own far end, so splitting the stretch in two
/// leaves the relief on the last part: the new junction in the middle takes none.
#[test]
fn a_split_stretch_keeps_its_relief_at_its_outer_end() {
    let mut list = vec![
        SegmentRow { from: "0".into(), to: "0.5".into(), equal: false, ..Default::default() },
        SegmentRow { from: "0.5".into(), to: "1".into(), equal: false, relief: Some((2, "3 mm".into(), "1 mm".into())), ..Default::default() },
    ];
    add_segment(&mut list);
    assert_eq!(list.len(), 3);
    assert_eq!(list[2].relief, Some((2, "3 mm".into(), "1 mm".into())), "the relief stays at the outer end");
    assert_eq!(list[1].relief, None, "the junction the split made takes none");
}

/// Deleting a stretch in the Flange dialog moves its relief to the row that takes its end, and the
/// first stretch's relief (no row ends where it did) goes with it.
#[test]
fn deleting_a_stretch_moves_its_relief_to_the_row_that_takes_its_end() {
    let rows = || {
        vec![
            SegmentRow { from: "0".into(), to: "0.3".into(), equal: false, ..Default::default() },
            SegmentRow { from: "0.3".into(), to: "0.8".into(), equal: false, relief: Some((3, String::new(), String::new())), ..Default::default() },
            SegmentRow { from: "0.8".into(), to: "1".into(), equal: false, ..Default::default() },
        ]
    };
    let mut list = rows();
    remove_segment(&mut list, 1);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].relief, Some((3, String::new(), String::new())), "the row that takes the removed end takes its relief");
    let mut first = rows();
    first[0].relief = Some((4, String::new(), String::new()));
    remove_segment(&mut first, 0);
    assert!(
        !first.iter().any(|r| r.relief.as_ref().is_some_and(|(shape, _, _)| *shape == 4)),
        "no row ends where the first stretch did: its relief goes"
    );
}

/// Edit Feature keeps a segment's whole relief (`shape`, `width`, `depth`) and sends it back byte
/// for byte: a field the dialog does not hold would be dropped on every edit.
#[test]
fn editing_a_flange_keeps_every_segments_relief() {
    let mut app = plate(crate::ui_kit::LayoutMode::Floating);
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    let params = json!({
        "type": "edge",
        "edges": [{"point": [50.0, 0.0, 2.5], "segments": [
            {"from": 0.0, "to": 0.4, "angle": "90 deg", "height": "20 mm",
             "relief": {"shape": "straight", "width": "3 mm", "depth": "1 mm"}},
            {"from": 0.4, "to": 0.7, "angle": "90 deg", "height": "15 mm",
             "relief": {"shape": "obround"}},
            {"from": 0.7, "to": 1.0, "angle": "45 deg", "height": "15 mm", "flip": false}
        ]}],
        "height": "20 mm",
        "angle": "90 deg",
        "position": "inside",
        "flip": false
    });
    app.run("sheet.flange", params.clone()).unwrap();
    assert!(app.session.model.results.iter().all(|r| r.error.is_none()), "{:?}", app.session.model.results);
    let f = app.session.doc.features.last().unwrap().clone();
    let e = edit_dialog(&app, f.id, None).unwrap();
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(
        serde_json::to_string(&c[0].1["params"]).unwrap(),
        serde_json::to_string(&params).unwrap(),
        "the edit sends the reliefs back as they were: {}",
        c[0].1["params"]
    );
}
/// Deleting a stretch that asks for no relief clears the relief of the row that takes its end, and
/// no relief is left on the last stretch, which ends at the edge's end where no junction is.
/// Carrying only a `Some` relief left the predecessor's stale one behind, at the edge's end as
/// likely as anywhere (FR-13).
#[test]
fn deleting_a_stretch_without_relief_clears_the_one_before_it() {
    let rows = || {
        vec![
            SegmentRow { from: "0".into(), to: "0.3".into(), equal: false, relief: Some((2, String::new(), String::new())), ..Default::default() },
            SegmentRow { from: "0.3".into(), to: "0.8".into(), equal: false, ..Default::default() },
            SegmentRow { from: "0.8".into(), to: "1".into(), equal: false, ..Default::default() },
        ]
    };
    // The removed stretch asks for no relief: the row that takes its end takes that, so the stale
    // relief it carried is cleared.
    let mut list = rows();
    remove_segment(&mut list, 1);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].relief, None, "the row that takes the removed end keeps a relief the removed row did not ask for");
    assert_eq!(list[1].relief, None, "the last stretch carries no relief");
    assert_eq!((list[0].from.as_str(), list[0].to.as_str()), ("0", "0.8"), "the gap is bridged as before");
    // Deleting the last stretch takes the relief of the stretch before it with it: that stretch now
    // ends at the edge's end, where there is no junction to relieve.
    let mut last = rows();
    last[1].relief = Some((3, "2 mm".into(), "1 mm".into()));
    remove_segment(&mut last, 2);
    assert_eq!(last.len(), 2);
    assert_eq!(last[1].relief, None, "a relief is left at the edge's end, where no junction is");
    assert_eq!(last[0].relief, Some((2, String::new(), String::new())), "the junction at 0.3 keeps its relief");
}
