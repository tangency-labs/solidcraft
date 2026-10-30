//! World-axis component dimensions use the same endpoints as the displayed measurement.
use crate::dialogs::Kind;
use crate::scenario::Harness;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

fn result(h: &Harness) -> &Value {
    match &h.app.dialog.as_ref().unwrap().kind {
        Kind::Measure { result: Some(r), .. } => r,
        _ => panic!("expected a measurement result"),
    }
}

fn vertex_measurement(a: [f64; 3], b: [f64; 3]) -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.set_origin_shown(false);
    h.app.ui.show_browser = false;
    h.app.run("solid.box", json!({"length":3,"width":4,"height":12,"corner":[-3,0,-12],"body_name":"Box"})).unwrap();
    h.app.session.selection = [a, b].into_iter().map(|p| Sel::Vertex { body: "Box".into(), point: p.into() }).collect();
    h.app.start("inspect.measure");
    h.call("ui.view", json!({"view":"fit"}));
    h
}

/// Inspect the actual painted text, including the viewport labels (which are not widgets).
fn painted_text(h: &mut Harness) -> Vec<String> {
    // Continue the harness clock (a hair on) so egui's input history never sees time go backwards.
    let time = h.ctx.input(|i| i.time) + 1e-4;
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        time: Some(time),
        ..Default::default()
    };
    let mut output = h.ctx.run_ui(raw, |ui| h.app.ui(ui));
    output.textures_delta.clear();
    output.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t.galley.text().to_string()) } else { None }).collect()
}

fn component_rects() -> Vec<egui::Rect> {
    ["measure:label", "measure:label:x", "measure:label:y", "measure:label:z"].into_iter().map(|key| crate::scenario::rect_of(key).unwrap()).collect()
}

#[test]
fn measure_dialog_shows_the_obtuse_corner_of_selected_lines() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.line", json!({"points": [[40, 0], [0, 0], [-24, 32]]})).unwrap();
    h.app.session.selection = ["l1", "l2"].into_iter().map(|id| Sel::SketchCurve { id: id.into(), sketch: None }).collect();
    h.app.start("inspect.measure");
    for layout in ["floating", "compact"] {
        h.call("ui.set", json!({"layout": layout}));
        let texts = painted_text(&mut h);
        assert!(texts.contains(&"126.870°".into()), "{layout}: {texts:?}");
        assert!(!texts.contains(&"53.130°".into()), "the supplementary angle must not replace the corner: {texts:?}");
    }
}

#[test]
fn measurement_axis_distances_follow_the_upstream_centre_min_and_max_choices() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.set_origin_shown(false);
    h.app.ui.show_browser = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    let mut items = Vec::new();
    for (center, radius) in [([0, 0], 3), ([30, 40], 5)] {
        let made = h.app.run("sketch.circle.center", json!({"center":center,"radius":radius})).unwrap();
        items.push(Sel::SketchCurve { id: made["curves"][0].as_str().unwrap().into(), sketch: None });
    }
    h.app.session.selection = items;
    h.app.start("inspect.measure");
    h.call("ui.view", json!({"view":"fit"}));
    let measured = result(&h).clone();
    assert!((measured["center_distance"].as_f64().unwrap() - 50.).abs() < 1e-8);
    assert!(measured["distance_mm"].as_f64().unwrap() < 50. && measured["max"].as_f64().unwrap() > 50.);

    for (label, distance, from, to) in [
        ("Center to center", "center_distance", "center_from", "center_to"),
        ("Min", "distance_mm", "from", "to"),
        ("Max", "max", "max_from", "max_to"),
    ] {
        let [x, y] = h.widget(label).unwrap_or_else(|| panic!("missing upstream distance choice: {label}"));
        h.call("ui.click", json!({"x":x,"y":y}));
        let texts = painted_text(&mut h);
        for (axis, index) in [("X", 0), ("Y", 1), ("Z", 2)] {
            let delta = measured[to][index].as_f64().unwrap() - measured[from][index].as_f64().unwrap();
            let expected = format!("{axis}: {}", crate::prefs::show_mm(&h.app, delta.abs(), 1));
            assert!(texts.contains(&expected), "{label} must use its own endpoints: {texts:?}");
        }
        assert!(texts.contains(&crate::prefs::show_mm(&h.app, measured[distance].as_f64().unwrap(), 1)));
        assert_eq!(*result(&h), measured, "changing the displayed distance must retain upstream measurement results");
    }
}

#[test]
fn measurement_axis_distances_show_absolute_world_components_in_every_view() {
    for reverse in [false, true] {
        let (a, b) = ([0., 0., 0.], [-3., 4., -12.]);
        let mut h = if reverse { vertex_measurement(b, a) } else { vertex_measurement(a, b) };
        assert!((result(&h)["distance_mm"].as_f64().unwrap() - 13.).abs() < 1e-8);
        for view in ["iso", "top", "front", "right"] {
            h.call("ui.view", json!({"view":view}));
            let texts = painted_text(&mut h);
            for text in ["13.00 mm", "X: 3.00 mm", "Y: 4.00 mm", "Z: 12.00 mm"] {
                assert!(texts.iter().any(|t| t == text), "{view}, reverse={reverse}: {texts:?}");
            }
            let rects = component_rects();
            for (i, a) in rects.iter().enumerate() {
                assert!(rects.iter().skip(i + 1).all(|b| !a.intersects(*b)), "labels overlap in {view}: {rects:?}");
            }
        }
        h.call("ui.set", json!({"perspective":true}));
        h.call("ui.prefs", json!({"up_axis":"y"}));
        let texts = painted_text(&mut h);
        assert!(texts.contains(&"Z: 12.00 mm".to_string()), "view orientation does not change world-axis distances");
        h.call("ui.key", json!({"key":"Escape"}));
        assert!(crate::scenario::handles("measure:label").is_empty(), "all temporary dimensions disappear on close");
    }
}

#[test]
fn measurement_axis_distances_keep_zero_components_readable_and_follow_units() {
    let mut h = vertex_measurement([0., 0., 0.], [-3., 0., 0.]);
    h.call("ui.view", json!({"view":"top"}));
    let texts = painted_text(&mut h);
    assert!(texts.contains(&"Y: 0.00 mm".into()) && texts.contains(&"Z: 0.00 mm".into()));
    let rects = component_rects();
    for (i, a) in rects.iter().enumerate() {
        assert!(rects.iter().skip(i + 1).all(|b| !a.intersects(*b)), "zero-component labels must remain readable: {rects:?}");
    }
    h.app.run("document.units", json!({"units":"in"})).unwrap();
    h.call("ui.prefs", json!({"length_decimals":3}));
    let texts = painted_text(&mut h);
    assert!(texts.contains(&"X: 0.1181 in".into()), "inch dimensions retain a meaningful fourth digit: {texts:?}");
    assert!(texts.contains(&"Y: 0.000 in".into()) && texts.contains(&"Z: 0.000 in".into()));
}

#[test]
fn measurement_axis_distances_can_be_disabled_saved_and_restored_in_preferences() {
    assert!(serde_json::from_value::<crate::prefs::Prefs>(json!({})).unwrap().measure_axis_distances, "existing settings get the default-on option");
    let mut h = vertex_measurement([0., 0., 0.], [-3., 4., -12.]);
    let measurement = result(&h).clone();
    h.call("ui.prefs", json!({"open":"Graphics"}));
    // The kit's 24 pt rows scroll inside the bounded Preferences page.
    let page = crate::scenario::rect_of("prefs:page_body").unwrap();
    for _ in 0..8 {
        if page.contains_rect(crate::scenario::rect_of("prefs:measure_axis_distances").unwrap()) {
            break;
        }
        h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-100}));
        h.frames(12);
    }
    let checkbox_rect = crate::scenario::rect_of("prefs:measure_axis_distances").unwrap();
    assert!(page.contains_rect(checkbox_rect), "the toggle must be visible before clicking: {checkbox_rect:?} in {page:?}");
    let checkbox = checkbox_rect.center();
    h.call("ui.click", json!({"x":checkbox.x,"y":checkbox.y}));
    assert!(!h.app.preferences.measure_axis_distances);
    assert!(crate::scenario::handle_at("measure:label").is_some(), "total stays visible");
    assert!(crate::scenario::handle_at("measure:label:x").is_none());
    assert_eq!(*result(&h), measurement, "hiding the extra overlay does not change the measurement");
    h.call("ui.prefs", json!({"button":"ok"}));
    let texts = painted_text(&mut h);
    assert!(!texts.iter().any(|text| text.starts_with("X: ") || text.starts_with("Y: ") || text.starts_with("Z: ")));
    let mut restored = crate::SolveApp::new(Default::default(), Default::default());
    restored.load_prefs(&h.app.prefs());
    assert!(!restored.preferences.measure_axis_distances, "disabled preference survives restart");
    h.call("ui.prefs", json!({"open":"Graphics","measure_axis_distances":true}));
    assert!(crate::scenario::handle_at("measure:label:x").is_some());
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert!(!h.app.preferences.measure_axis_distances, "Cancel restores the saved choice");
    h.call("ui.prefs", json!({"open":"Graphics"}));
    h.call("ui.prefs", json!({"button":"defaults"}));
    assert!(h.app.preferences.measure_axis_distances);
    h.call("ui.prefs", json!({"button":"apply"}));
    h.call("ui.prefs", json!({"measure_axis_distances":false}));
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert!(h.app.preferences.measure_axis_distances, "Cancel restores the last Apply");
}

/// The view numbers the picks as the panel does (#6020): "1" beside the first pick's end of the
/// line, "2" beside the second's, in every view and whichever way round they were picked, clear
/// of the distance label.
#[test]
fn the_view_numbers_the_two_picks() {
    for reverse in [false, true] {
        let (a, b) = ([0., 0., 0.], [-3., 4., -12.]);
        let (first, second) = if reverse { (b, a) } else { (a, b) };
        let mut h = vertex_measurement(first, second);
        for view in ["iso", "top", "front", "right"] {
            h.call("ui.view", json!({"view": view}));
            let texts = painted_text(&mut h);
            assert!(texts.iter().any(|t| t == "1") && texts.iter().any(|t| t == "2"), "{view}: {texts:?}");
            let mut at = |p: [f64; 3]| {
                let r = h.call("ui.at", json!({ "world": p }))["result"].clone();
                egui::pos2(r[0].as_f64().unwrap() as f32, r[1].as_f64().unwrap() as f32)
            };
            let (pa, pb) = (at(first), at(second));
            let label = crate::scenario::rect_of("measure:label").unwrap();
            for (n, own, other) in [("1", pa, pb), ("2", pb, pa)] {
                let r = crate::scenario::rect_of(&format!("measure:item:{n}")).unwrap();
                assert!(r.center().distance(own) < r.center().distance(other), "{view}, reverse={reverse}: {n} beside its own pick");
                assert!(r.center().distance(own) < 40.0, "{view}: {n} is {} from its pick", r.center().distance(own));
                assert!(!r.intersects(label), "{view}: {n} covers the distance");
            }
        }
        h.call("ui.key", json!({"key": "Escape"}));
        assert!(crate::scenario::handles("measure:item:1").is_empty(), "the numbers go with the measure");
    }
}

#[test]
fn the_distance_row_has_a_copy_button_that_copies_the_shown_value() {
    let mut h = vertex_measurement([0., 0., 0.], [3., 4., 0.]);
    h.frames(4);
    let want = crate::prefs::show_mm(&h.app, 5.0, 1);
    assert!(crate::scenario::handle_at("measure:copy:distance").is_some(), "the Distance row publishes its copy button");
    let at = crate::scenario::handle_at("measure:copy:distance").unwrap_or_default();
    let (hover, _) = crate::control::ControlRequest::new("ui.move", json!({"x": f64::from(at.x), "y": f64::from(at.y)}));
    crate::control::handle(&mut h.app, &h.ctx, &hover);
    h.frames(2);
    let (request, _reply) = crate::control::ControlRequest::new("ui.click", json!({"x": f64::from(at.x), "y": f64::from(at.y)}));
    crate::control::handle(&mut h.app, &h.ctx, &request);
    let mut copied = Vec::new();
    for _ in 0..8 {
        h.frame();
        copied.extend(h.output.commands.iter().filter_map(|c| match c {
            egui::OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        }));
    }
    assert_eq!(copied, [want]);
}

/// STL outlines are real distances even though they are not B-rep operands.
#[test]
fn measure_imported_stl_edges_and_vertices_from_viewport_clicks() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.set_origin_shown(false);
    h.app.ui.show_browser = false;
    let path = h.dir.join("measure.stl");
    h.app.run("solid.box", json!({"length":20,"width":30,"height":10})).unwrap();
    h.app.run("file.save_mesh", json!({"path":path})).unwrap();
    h.app.run("file.new", json!({})).unwrap();
    let imported = h.app.run("file.insert_mesh", json!({"path":path})).unwrap();
    h.app.run("component.activate", json!({"component":"root"})).unwrap();
    h.app.run("occurrence.copy", json!({"component":imported["component"],"translate":[40,0,0]})).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(4);

    for offset in [0., 40.] {
        h.app.start("inspect.measure");
        h.step(&json!({"click":{"world":[offset + 20.,0,5]}})).unwrap();
        assert!(
            matches!(h.app.dialog.as_ref().unwrap().inputs[0].items.as_slice(), [Sel::Edge { .. }]),
            "must select the outline, not the whole STL face"
        );
        assert_eq!(result(&h)["items"][0]["type"], "edge");
        assert!((result(&h)["items"][0]["length_mm"].as_f64().unwrap() - 10.).abs() < 1e-6);
        let texts = painted_text(&mut h);
        assert!(texts.iter().any(|text| text.contains("10.00 mm")), "{texts:?}");
        h.step(&json!({"click":{"world":[offset,0,5]}})).unwrap();
        assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.iter().all(|s| matches!(s, Sel::Edge { .. })));
        assert!((result(&h)["distance_mm"].as_f64().unwrap() - 20.).abs() < 1e-6);
        assert!(result(&h)["angle_deg"].as_f64().unwrap().abs() < 1e-6);
        h.call("ui.key", json!({"key":"Escape"}));
        h.app.session.selection.clear();
        h.app.start("inspect.measure");
        for x in [offset, offset + 20.] {
            h.step(&json!({"click":{"world":[x,0,10]}})).unwrap();
        }
        assert!(h.app.dialog.as_ref().unwrap().inputs[0].items.iter().all(|s| matches!(s, Sel::Vertex { .. })));
        assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2);
        assert!((result(&h)["distance_mm"].as_f64().unwrap() - 20.).abs() < 1e-6);
        h.call("ui.key", json!({"key":"Escape"}));
        h.app.session.selection.clear();
    }
}
