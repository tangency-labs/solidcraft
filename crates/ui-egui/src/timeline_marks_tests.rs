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
    let got = marked(&mut app, crate::timeline::panel_inline, |item, d| (d.x - (item.x + 9.0)).abs() < 1.5 && (d.y - (item.y - 9.0)).abs() < 1.5);
    assert_eq!(got, want, "an error mark on each failed feature and on no other");
}

#[test]
fn a_failed_feature_is_marked_in_the_feature_history() {
    let mut app = failing_sample();
    let want = features(&app);
    // The mark sits at the row's right end, on its centre line.
    let got = marked(&mut app, crate::timeline_left::panel_contents, |item, d| (d.y - item.y).abs() < 1.5 && d.x > item.x);
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
    let t = solidcraft_theme::Tokens::get();
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

/// With the History docked in the browser, a sketch's row in its History folder takes the state's
/// colour and names the state in its tooltip, as the sketch's Sketches folder row does (#22185).
#[test]
fn a_sketchs_row_in_the_browsers_history_folder_takes_its_states_colour() {
    let mut h = sketch_states_design();
    h.app.run("view.history_layout", json!({"dock": "browser"})).unwrap();
    assert!(h.app.preferences.history_in_browser);
    h.app.tree.expanded.insert("c0/features".into());
    let t = solidcraft_theme::Tokens::get();
    let want = [("Guide", t.sketch), ("Fixed", t.sketch_fixed), ("Reach", t.error)];
    let row = |h: &crate::scenario::Harness, name: &str| format!("row:browser-feature:c0:{}", h.app.session.doc.find_feature(name).unwrap().id);
    let (shapes, textures) = fresh_frame(&mut h);
    for (name, colour) in want {
        let at = crate::scenario::handle_at(&row(&h, name)).unwrap_or_else(|| panic!("{name}: no History folder row"));
        assert_eq!(icon_colours(&shapes, &textures, at, 15.5..=16.5), vec![colour.to_array()], "{name}: History folder");
    }
    let fixed = row(&h, "Fixed");
    h.step(&json!({"move": {"handle": fixed}})).unwrap();
    h.frames(60);
    let texts = painted_texts(&mut h);
    assert!(texts.iter().any(|t| t.contains("Fully constrained")), "no `Fully constrained` in {texts:?}");
    // Preference off: the rows keep the sketch artwork, all alike.
    h.app.preferences.sketch_state_icons = false;
    let (shapes, textures) = fresh_frame(&mut h);
    let icons: Vec<_> =
        want.iter().map(|(name, _)| icon_colours(&shapes, &textures, crate::scenario::handle_at(&row(&h, name)).unwrap(), 15.5..=16.5)).collect();
    assert!(icons.iter().all(|colours| colours.len() > 1), "the sketch artwork, not one ink: {icons:?}");
    assert!(icons.windows(2).all(|w| w[0] == w[1]), "every sketch's icon alike");
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

/// How the text holding `text` is painted this frame: its colour, whether it is cut short (an
/// ellipsis in place of the rest) and its number of lines. `None` when nothing holds it.
fn painted_as(h: &mut crate::scenario::Harness, text: &str) -> Option<(egui::Color32, bool, usize)> {
    h.frame();
    let mut found = None;
    for clipped in &h.shapes {
        each(&clipped.shape, &mut |s| {
            if let Shape::Text(t) = s
                && t.galley.text().contains(text)
            {
                let section = t.galley.job.sections.first().map(|s| s.format.color).filter(|c| *c != egui::Color32::PLACEHOLDER);
                let colour = t.override_text_color.or(section).unwrap_or(t.fallback_color);
                let cut = t.galley.rows.iter().any(|row| row.glyphs.iter().any(|g| g.chr == '…'));
                found = Some((colour, cut, t.galley.rows.len()));
            }
        });
    }
    found
}

/// A failed feature's tooltip has its whole error, in the error colour, on lines that wrap,
/// wherever its mark shows (#8834): the left History cut it off after the summary line, and the
/// bottom History and the browser painted it like any other line, so it read like a warning.
#[test]
fn a_failed_features_tooltip_shows_its_whole_error_in_the_error_colour_wherever_its_mark_shows() {
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.session.run_script(&solidcraft_engine::sample::script()).unwrap();
    h.app.run("parameters.change", json!({"name": "width", "expression": "5 mm"})).unwrap();
    h.settle();
    let fillet = h.app.session.doc.find_feature("Fillet1").unwrap().id;
    let tip = crate::timeline::feature_tip(&h.app, fillet).unwrap();
    let error = tip.error.clone().unwrap();
    // Copy Error Message still copies it as plain text.
    assert_eq!(crate::timeline::feature_message(&h.app, fillet).unwrap(), format!("{}\n{error}", tip.head));
    let red = solidcraft_theme::Tokens::get().error;
    let hover = |h: &mut crate::scenario::Harness, handle: &str| {
        h.step(&json!({"move": {"handle": handle}})).unwrap();
        h.frames(60);
        painted_as(h, &error).unwrap_or_else(|| panic!("{handle}: `{error}` not painted"))
    };
    h.app.tree.expanded.insert("c0/features".into());
    for (place, handle) in [("bottom History", "timeline:Fillet1"), ("browser row", "row:Fillet1")] {
        let (colour, cut, _) = hover(&mut h, handle);
        assert_eq!((colour, cut), (red, false), "{place}");
    }
    // Folded, the browser's folder says the same of the feature it holds.
    h.app.tree.expanded.remove("c0/features");
    h.frame();
    let folder =
        ["row:Features", "row:History"].into_iter().find(|handle| crate::scenario::handle_at(handle).is_some()).expect("the features folder row");
    let (colour, cut, _) = hover(&mut h, folder);
    assert_eq!((colour, cut), (red, false), "folded browser folder");
    h.app.preferences.timeline_left = true;
    for width in [56.0, 260.0] {
        h.app.preferences.timeline_left_width = width;
        let (colour, cut, _) = hover(&mut h, "timeline:Fillet1");
        assert_eq!((colour, cut), (red, false), "left History {width} wide");
    }
}

/// #17697: SMLib's check of an inserted STEP part runs on after the design shows. When it ends
/// with a part to repair, the import's History chip warns and the status line says so, once.
#[test]
fn a_part_the_check_fails_after_the_import_shows_raises_the_chip_and_the_status_once() {
    let dir = std::env::temp_dir().join(format!("solidcraft-17697-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("block.step");
    std::fs::write(&path, include_str!("../../../tests/step-regressions/block_edge_off_its_face.step")).unwrap();
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.insert_path_with(&path.to_string_lossy(), &Default::default(), false).unwrap();
    let id = app.session.doc.features.iter().find(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Import { .. })).expect("the import").id;
    let report = app.session.model.result(id).and_then(|r| r.import_report.clone()).expect("the import's check");
    report.settled().expect("the check ends");
    app.status = None;
    let ctx = crate::fonts::test_context();
    crate::timeline::tick_import_checks(&mut app, &ctx);
    let result = app.session.model.result(id).unwrap();
    assert!(crate::timeline::has_warning(result), "the chip warns");
    assert!(matches!(crate::timeline::feature_status(&app, id), Some(crate::ui_kit::Status::Warning)));
    let summary = crate::timeline::import_summary(result.import_report.as_deref().unwrap());
    let (text, _, error) = app.status.clone().expect("the status line says so");
    assert_eq!((text, error), (crate::i18n::tr_diagnostic(&summary), false));
    let details = crate::timeline::feature_message(&app, id).unwrap();
    assert!(details.contains("solid 'Bloco': needs repair before it can be modelled: SMLib's check fails it: "), "{details}");
    app.status = None;
    crate::timeline::tick_import_checks(&mut app, &ctx);
    assert!(app.status.is_none(), "once");
    let _ = std::fs::remove_dir_all(&dir);
}

/// #20633: an import's check of its parts runs for minutes after the design shows, and the window
/// is asked for no frames while it does. Its end asks for exactly one frame, the one whose tick
/// settles the result; nothing is asked for after that.
#[test]
fn a_running_imports_check_asks_for_no_frames_and_its_end_wakes_the_window_once() {
    let dir = std::env::temp_dir().join(format!("solidcraft-20633-ui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("block.step");
    std::fs::write(&path, include_str!("../../../tests/step-regressions/block_edge_off_its_face.step")).unwrap();
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.insert_path_with(&path.to_string_lossy(), &Default::default(), false).unwrap();
    let id = app.session.doc.features.iter().find(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Import { .. })).expect("the import").id;
    // The check of a file this small ends at once: hold one running to ask of the tick what it
    // asks of the window while a large one runs.
    let end = app.session.model.running_import_check_for_tests(id);
    assert!(app.session.model.checking_imports(), "a check is in flight");
    let ctx = crate::fonts::test_context();
    fn frame(app: &mut SolveApp, ctx: &egui::Context, time: f64) -> Option<std::time::Duration> {
        let raw = egui::RawInput { time: Some(time), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            crate::timeline::tick_import_checks(app, ui.ctx());
        });
        out.textures_delta.clear();
        out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay)
    }
    frame(&mut app, &ctx, 0.0);
    frame(&mut app, &ctx, 0.01);
    let running = frame(&mut app, &ctx, 0.02);
    assert!(running.is_some_and(|d| d.as_secs_f64() > 1.0), "a running check asked for a frame after {running:?}: it must ask for none");
    end(vec![]);
    let woke = frame(&mut app, &ctx, 0.04);
    assert!(woke.is_some_and(|d| d.is_zero()), "the end of the check asks for the next frame at once, got {woke:?}");
    assert!(!app.session.model.checking_imports(), "the frame it asked for settles the result");
    let after = frame(&mut app, &ctx, 0.06);
    assert!(after.is_some_and(|d| d.as_secs_f64() > 1.0), "and asks for no more after that, got {after:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
