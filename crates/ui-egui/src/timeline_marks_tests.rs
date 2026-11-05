//! A failed feature is marked in the bottom timeline and the Feature History by a shape, not only
//! its colour (#972): the error mark where a warning's mark goes (now an icon image, so these
//! tests look for the 10-point image at the mark's place).

use egui::{Pos2, Rect, Shape, vec2};
use serde_json::json;

use crate::SolveApp;

use crate::test_support::each_shape as each;

/// The sample with Fillet1 failing (the plate made narrower than its fillets).
fn failing_sample() -> SolveApp {
    let mut s = solidcraft_engine::Session::default();
    s.run_script(&solidcraft_engine::sample::script()).unwrap();
    s.execute("parameters.change", &json!({"name": "width", "expression": "5 mm"})).unwrap();
    SolveApp::new(s, crate::Services::default())
}

/// Each feature's name and whether it failed.
fn features(app: &SolveApp) -> Vec<(String, bool)> {
    app.session.doc.features.iter().map(|f| (f.name.clone(), app.session.model.result(f.id).is_some_and(|r| r.error.is_some()))).collect()
}

/// The centres of the 10 to 12-point icon images one frame of `draw` paints: the failed-feature mark
/// (the sample has no warnings, so every such image is an error mark).
fn error_discs(app: &mut SolveApp, draw: fn(&mut SolveApp, &mut egui::Ui)) -> Vec<Pos2> {
    crate::scenario::clear_handles();
    let ctx = crate::fonts::test_context();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0));
    let out = crate::test_support::frame(&ctx, egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| draw(app, ui));
    let mut found = Vec::new();
    for clipped in &out.shapes {
        each(&clipped.shape, &mut |s| {
            if let Shape::Mesh(m) = s {
                let b = m.calc_bounds();
                if matches!(m.texture_id, egui::TextureId::Managed(_)) && (b.width() - 11.0).abs() < 1.5 && (b.height() - 11.0).abs() < 1.5 {
                    found.push(b.center());
                }
            }
        });
    }
    found
}

/// The features that have a disc where their mark goes (`at`: the mark's centre from the item's).
fn marked(app: &mut SolveApp, draw: fn(&mut SolveApp, &mut egui::Ui), at: impl Fn(Pos2, Pos2) -> bool) -> Vec<(String, bool)> {
    let discs = error_discs(app, draw);
    let handles = crate::scenario::handles("timeline:");
    features(app)
        .into_iter()
        .map(|(name, _)| {
            let item = handles.iter().find(|(h, _)| *h == format!("timeline:{name}")).map(|(_, p)| *p);
            let item = item.unwrap_or_else(|| panic!("{name} is not drawn"));
            let has = discs.iter().any(|d| at(item, *d));
            (name, has)
        })
        .collect()
}

#[test]
fn a_failed_feature_is_marked_in_the_bottom_timeline() {
    let mut app = failing_sample();
    let want = features(&app);
    assert!(want.iter().any(|(n, failed)| *failed && n == "Fillet1"), "the sample's Fillet1 fails: {want:?}");
    assert!(want.iter().filter(|(_, failed)| !failed).count() >= 3, "and others don't");
    // The mark sits in the item's top-right corner: 10 points square, inside the 28-point item.
    let got = marked(&mut app, crate::timeline::timeline, |item, d| (d.x - (item.x + 9.0)).abs() < 1.5 && (d.y - (item.y - 9.0)).abs() < 1.5);
    assert_eq!(got, want, "an error mark on each failed feature and on no other");
}

#[test]
fn a_failed_feature_is_marked_in_the_feature_history() {
    let mut app = failing_sample();
    let want = features(&app);
    // The mark sits at the row's right end, on its centre line.
    let got = marked(&mut app, crate::timeline_left::left_timeline, |item, d| (d.y - item.y).abs() < 1.5 && d.x > item.x);
    assert_eq!(got, want, "an error mark on each failed feature and on no other");
}

/// Three sketches, one in each state (#7245): Guide is under constrained, Fixed is fully
/// constrained, and Reach's constraints conflict once `w` moves Guide's line out of its 12 mm
/// line's reach.
fn sketch_states_design() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    let app = &mut h.app;
    app.run("parameters.add", json!({"name": "w", "expression": "10 mm"})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "Guide"})).unwrap();
    app.run("sketch.line", json!({"points": [[10, -30], [10, 30]], "ids": ["guide"], "connect": false})).unwrap();
    app.run("sketch.constraint.horizontal_vertical", json!({"line": "guide", "mode": "vertical"})).unwrap();
    app.run("sketch.dimension", json!({"entities": ["origin", "guide.start"], "type": "horizontal", "value": "w"})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "Reach"})).unwrap();
    let projected = app.run("sketch.project", json!({"refs": [{"sketch": "Guide", "curve": "guide"}]})).unwrap()["links"][0]["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let line = app.run("sketch.line", json!({"points": [[0, 0], [10, -(44.0_f64).sqrt()]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
    app.run("sketch.dimension", json!({"entities": [line], "value": 12})).unwrap();
    app.run("sketch.constraint.coincident", json!({"a": format!("{line}.end"), "b": projected})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.run("sketch.create", json!({"plane": "XY", "name": "Fixed"})).unwrap();
    let rectangle = app.run("sketch.rectangle.two_point", json!({"p0": [30, 0], "p1": [50, 20]})).unwrap();
    app.run("sketch.constraint.fix", json!({"entities": rectangle["curves"]})).unwrap();
    app.run("sketch.finish", json!({})).unwrap();
    app.run("parameters.change", json!({"name": "w", "expression": "20 mm"})).unwrap();
    h.settle();
    h
}

/// One frame of the app on a fresh context, so that every icon's texture is uploaded again: the
/// frame's shapes, and each uploaded texture's opaque colours.
fn fresh_frame(h: &mut crate::scenario::Harness) -> (Vec<egui::epaint::ClippedShape>, std::collections::HashMap<egui::TextureId, Vec<[u8; 4]>>) {
    h.ctx = crate::fonts::test_context();
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
    let mut textures = std::collections::HashMap::new();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let app = &mut h.app;
        let mut out = h.ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        for (id, delta) in out.textures_delta.set.iter().flat_map(|(id, deltas)| deltas.iter().map(move |d| (id, d))) {
            let egui::ImageData::Color(image) = &delta.image;
            let mut colours: Vec<[u8; 4]> = image.pixels.iter().filter(|p| p.a() == 255).map(|p| p.to_array()).collect();
            colours.sort_unstable();
            colours.dedup();
            textures.insert(*id, colours);
        }
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    (shapes, textures)
}

/// The opaque colours of the icon image drawn on the row at `at` (its centre's height), `size`
/// points square, nearest `at` (a History chip's own icon; a browser row's eye is 15 points).
fn icon_colours(
    shapes: &[egui::epaint::ClippedShape],
    textures: &std::collections::HashMap<egui::TextureId, Vec<[u8; 4]>>,
    at: Pos2,
    size: std::ops::RangeInclusive<f32>,
) -> Vec<[u8; 4]> {
    let mut best: Option<(f32, egui::TextureId)> = None;
    for clipped in shapes {
        each(&clipped.shape, &mut |s| {
            if let Shape::Mesh(m) = s {
                let b = m.calc_bounds();
                let distance = (b.center().x - at.x).abs();
                if (b.center().y - at.y).abs() < 1.5 && size.contains(&b.width()) && best.is_none_or(|(d, _)| distance < d) {
                    best = Some((distance, m.texture_id));
                }
            }
        });
    }
    let (_, id) = best.unwrap_or_else(|| panic!("no icon image on the row at {at:?}"));
    textures.get(&id).cloned().unwrap_or_else(|| panic!("the icon's texture {id:?} was not uploaded"))
}

#[test]
fn a_sketchs_icon_takes_its_states_colour_in_both_histories_and_the_browser() {
    let mut h = sketch_states_design();
    let st = h.app.session.model.state();
    let report = |name: &str| st.sketches.iter().find(|s| s.name == name).map(|s| s.report.clone()).unwrap();
    assert!(report("Guide").ok() && report("Guide").dof > 0);
    assert!(report("Fixed").ok() && report("Fixed").dof == 0);
    assert!(!report("Reach").ok(), "{:?}", report("Reach"));
    let states = crate::sketch_state::SketchStates::of(&h.app);
    let t = crate::theme::Tokens::get();
    let want = [("Guide", t.sketch), ("Fixed", t.sketch_fixed), ("Reach", t.error)];
    for (name, colour) in want {
        let id = h.app.session.doc.find_feature(name).unwrap().id;
        assert_eq!(states.get(id).and_then(|s| s.ink(&t)), Some(colour), "{name}");
    }
    // Bottom History (20-point icons in 28-point chips) and the browser's Sketches folder
    // (16-point icons beside 15-point eyes).
    h.app.tree.expanded.insert("c0/sketches".into());
    let (shapes, textures) = fresh_frame(&mut h);
    for (name, colour) in want {
        let chip = crate::scenario::handle_at(&format!("timeline:{name}")).unwrap();
        assert_eq!(icon_colours(&shapes, &textures, chip, 19.0..=21.0), vec![colour.to_array()], "{name}: History strip");
        let row = crate::scenario::handle_at(&format!("row:{name}")).unwrap();
        assert_eq!(icon_colours(&shapes, &textures, row, 15.5..=16.5), vec![colour.to_array()], "{name}: browser");
    }
    let extrude_free = icon_colours(&shapes, &textures, crate::scenario::handle_at("row:Origin").unwrap(), 15.5..=16.5);
    assert!(extrude_free.len() > 1, "other rows keep their artwork");
    // The left History, in either width.
    h.app.preferences.timeline_left = true;
    for names in [false, true] {
        h.app.preferences.timeline_left_width = if names { 260.0 } else { 56.0 };
        let (shapes, textures) = fresh_frame(&mut h);
        for (name, colour) in want {
            let chip = crate::scenario::handle_at(&format!("timeline:{name}")).unwrap();
            assert_eq!(icon_colours(&shapes, &textures, chip, 14.0..=20.0), vec![colour.to_array()], "{name}: left History, names {names}");
        }
    }
    // Preference off: every sketch's icon is its artwork again, as before.
    h.app.preferences.sketch_state_icons = false;
    h.app.preferences.timeline_left = false;
    assert!(crate::sketch_state::SketchStates::of(&h.app).get(h.app.session.doc.find_feature("Guide").unwrap().id).is_none());
    let (shapes, textures) = fresh_frame(&mut h);
    let (mut chips, mut rows) = (Vec::new(), Vec::new());
    for (name, _) in want {
        let chip = crate::scenario::handle_at(&format!("timeline:{name}")).unwrap();
        let row = crate::scenario::handle_at(&format!("row:{name}")).unwrap();
        chips.push(icon_colours(&shapes, &textures, chip, 19.0..=21.0));
        rows.push(icon_colours(&shapes, &textures, row, 15.5..=16.5));
    }
    for icons in [chips, rows] {
        assert!(icons.iter().all(|colours| colours.len() > 1), "the sketch artwork, not one ink: {icons:?}");
        assert!(icons.windows(2).all(|w| w[0] == w[1]), "every sketch's icon alike");
    }
}

/// Every text one frame paints.
fn painted_texts(h: &mut crate::scenario::Harness) -> Vec<String> {
    h.frame();
    let mut texts = Vec::new();
    for clipped in &h.shapes {
        each(&clipped.shape, &mut |s| {
            if let Shape::Text(t) = s {
                texts.push(t.galley.text().to_string());
            }
        });
    }
    texts
}

#[test]
fn a_sketch_rows_tooltip_names_its_state() {
    let mut h = sketch_states_design();
    h.app.tree.expanded.insert("c0/sketches".into());
    h.settle();
    for (handle, want) in [("timeline:Reach", "Constraints conflict"), ("row:Fixed", "Fully constrained")] {
        h.step(&json!({"move": {"handle": handle}})).unwrap();
        h.frames(60);
        let texts = painted_texts(&mut h);
        assert!(texts.iter().any(|t| t.contains(want)), "{handle}: no `{want}` in {texts:?}");
    }
    h.step(&json!({"move": {"handle": "timeline:Guide"}})).unwrap();
    h.frames(60);
    let texts = painted_texts(&mut h);
    assert!(texts.iter().any(|t| t.contains("Under constrained (") && t.contains("degrees of freedom)")), "{texts:?}");
    // Preference off: the tooltips are as before.
    h.app.preferences.sketch_state_icons = false;
    for (handle, gone) in [("timeline:Reach", "Constraints conflict"), ("row:Fixed", "Fully constrained")] {
        h.step(&json!({"move": {"handle": handle}})).unwrap();
        h.frames(60);
        let texts = painted_texts(&mut h);
        assert!(!texts.iter().any(|t| t.contains(gone)), "{handle}: `{gone}` in {texts:?}");
    }
}
