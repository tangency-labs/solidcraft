use serde_json::json;
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::{PickMode, Sel};

use crate::scenario::Harness;
use crate::viewport::Hit;

/// A 40 × 30 × 20 box `Base` with a 10 mm cube joined on top: its top face is the cube's feature's,
/// its bottom face the box's. Returns the two features' ids.
fn stacked(h: &mut Harness) -> (u64, u64) {
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "Base"})).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 20], "operation": "join"})).unwrap();
    let ids: Vec<u64> = h.app.session.doc.features.iter().map(|f| f.id).collect();
    (ids[0], ids[1])
}

/// The face of `body` whose triangles lie highest (`top`) or lowest, as a hit.
fn face(h: &Harness, body: &str, top: bool) -> Hit {
    let st = h.app.session.world_state();
    let m = st.body(body).unwrap().mesh();
    let mut faces: std::collections::BTreeMap<u32, (Vec3, f64)> = Default::default();
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let [a, b, c] = m.tri(t).unwrap();
        let e = faces.entry(*f).or_insert((Vec3::ZERO, 0.0));
        e.0 += (a + b + c) / 3.0;
        e.1 += 1.0;
    }
    let (index, point) = faces
        .into_iter()
        .map(|(f, (sum, n))| (f as usize, sum / n))
        .max_by(|a, b| if top { a.1.z.total_cmp(&b.1.z) } else { b.1.z.total_cmp(&a.1.z) })
        .unwrap();
    Hit::Face { body: body.into(), index, point }
}

fn picks(h: &Harness, hit: &Hit) -> Option<Sel> {
    crate::viewport::candidate(&h.app, std::slice::from_ref(hit)).map(|(_, sel)| sel)
}

fn set_mode(h: &mut Harness, mode: PickMode) {
    h.call("engine.execute", json!({"command": "select.mode", "params": {"mode": mode}}));
    h.frames(1);
    assert_eq!(h.app.ui.pick_mode, mode);
}

/// Each mode turns a click on a face into its own kind of object (#4255); without components a
/// body is its own part and component, and the sketch mode picks no faces at all.
#[test]
fn each_mode_picks_its_object_from_a_face() {
    let mut h = Harness::new();
    let (block, cube) = stacked(&mut h);
    let (top, bottom) = (face(&h, "Base", true), face(&h, "Base", false));
    let Hit::Face { index, .. } = top else { panic!("a face") };
    set_mode(&mut h, PickMode::Faces);
    assert!(matches!(picks(&h, &top), Some(Sel::Face { index: i, .. }) if i == index));
    let body = Some(Sel::Body { name: "Base".into() });
    set_mode(&mut h, PickMode::Bodies);
    assert_eq!(picks(&h, &top), body);
    set_mode(&mut h, PickMode::Features);
    assert_eq!(picks(&h, &top), Some(Sel::Feature { id: cube }));
    assert_eq!(picks(&h, &bottom), Some(Sel::Feature { id: block }));
    for mode in [PickMode::Parts, PickMode::Components] {
        set_mode(&mut h, mode);
        assert_eq!(picks(&h, &top), body, "{mode:?}");
    }
    set_mode(&mut h, PickMode::Sketch);
    assert_eq!(picks(&h, &top), None);
}

/// Parts picks the innermost placement holding the body, Components the top-level one; a body of
/// the top-level design is still its own.
#[test]
fn parts_and_components_pick_the_placements_holding_the_body() {
    let mut h = Harness::new();
    h.app.run("component.create", json!({"name": "Frame"})).unwrap();
    h.app.run("component.create", json!({"name": "Bracket", "parent": "Frame"})).unwrap();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 5, "body_name": "Plate"})).unwrap();
    h.app.run("component.activate", json!({"component": 0})).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "body_name": "Loose"})).unwrap();
    let occurrences = &h.app.session.doc.occurrences;
    let frame = occurrences.iter().find(|o| o.parent == 0).unwrap().id;
    let bracket = occurrences.iter().find(|o| o.parent != 0).unwrap().id;
    let plate = h.app.session.occurrence_bodies(&[frame, bracket], false).unwrap().remove(0);
    let (placed, loose) = (face(&h, &plate, true), face(&h, "Loose", true));
    set_mode(&mut h, PickMode::Parts);
    assert_eq!(picks(&h, &placed), Some(Sel::Occurrence { path: vec![frame, bracket] }));
    assert_eq!(picks(&h, &loose), Some(Sel::Body { name: "Loose".into() }));
    set_mode(&mut h, PickMode::Components);
    assert_eq!(picks(&h, &placed), Some(Sel::Occurrence { path: vec![frame] }));
    assert_eq!(picks(&h, &loose), Some(Sel::Body { name: "Loose".into() }));
}

/// The Parts mode and Render's Ctrl-drop share one walk (#6426 had its own): each copy of a
/// repeated, nested placement is its own part, and a body of the top-level design is its own.
#[test]
fn every_copy_of_a_repeated_nested_placement_is_its_own_part() {
    let mut h = Harness::new();
    let frame = h.app.run("component.create", json!({"name": "Frame"})).unwrap()["component"].as_u64().unwrap();
    h.app.run("component.create", json!({"name": "Bracket", "parent": "Frame"})).unwrap();
    h.app.run("solid.box", json!({"length": 20, "width": 20, "height": 5, "body_name": "Plate"})).unwrap();
    h.app.run("component.activate", json!({"component": "root"})).unwrap();
    h.app.run("occurrence.copy", json!({"component": "Bracket", "parent": "Frame", "translate": [30, 0, 0]})).unwrap();
    h.app.run("occurrence.copy", json!({"component": frame, "translate": [0, 50, 0]})).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-50, 0, 0], "body_name": "Loose"})).unwrap();
    crate::browser::prepare_body_paths(&mut h.app);
    let bodies: Vec<String> = h.app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let parts: Vec<Sel> = bodies.iter().map(|b| crate::selection_mode::part_of(&h.app, b)).collect();
    let paths: std::collections::BTreeSet<Vec<u64>> =
        parts.iter().filter_map(|p| if let Sel::Occurrence { path } = p { Some(path.clone()) } else { None }).collect();
    assert_eq!(paths.len(), 4, "two Brackets in each of two Frames: {paths:?}");
    assert!(paths.iter().all(|p| p.len() == 2), "{paths:?}");
    assert!(parts.contains(&Sel::Body { name: "Loose".into() }));
}

/// The selection filter narrows the mode: "Faces" off still lets a whole-object mode pick through
/// a face (as "Select bodies" did), edges and vertices belong to Faces & edges alone, and the
/// sketch kinds apply in every mode.
#[test]
fn the_filter_narrows_the_mode() {
    let mut h = Harness::new();
    stacked(&mut h);
    let top = face(&h, "Base", true);
    let edge = Hit::Edge { body: "Base".into(), index: 0, mid: Vec3::ZERO, point: Vec3::ZERO };
    let curve = Hit::SketchCurve { sketch: 1, id: "c1".into(), straight: true, point: Vec3::ZERO };
    let allowed = |h: &Harness, hit: &Hit| crate::selection_mode::allowed(&h.app, hit);
    h.app.ui.pick_off = vec!["faces".into()];
    set_mode(&mut h, PickMode::Faces);
    assert!(!allowed(&h, &top) && allowed(&h, &edge));
    for mode in [PickMode::Bodies, PickMode::Features, PickMode::Parts, PickMode::Components] {
        set_mode(&mut h, mode);
        assert!(allowed(&h, &top) && !allowed(&h, &edge), "{mode:?}");
    }
    h.app.ui.pick_off = vec!["edges".into()];
    set_mode(&mut h, PickMode::Faces);
    assert!(allowed(&h, &top) && !allowed(&h, &edge));
    set_mode(&mut h, PickMode::Sketch);
    assert!(!allowed(&h, &top) && !allowed(&h, &edge) && allowed(&h, &curve));
    h.app.ui.pick_off = vec!["sketch_curves".into()];
    assert!(!allowed(&h, &curve));
}

/// `select.mode`, `ui.set` (`pickMode`, and the older `pickBodies`) and `ui.inspect` reach the one
/// setting, which is saved with the preferences; an older "select bodies" opens as Bodies.
#[test]
fn the_mode_through_commands_ui_set_and_saved_preferences() {
    let mut h = Harness::new();
    set_mode(&mut h, PickMode::Features);
    assert_eq!(h.call("ui.inspect", json!({}))["result"]["ui"]["pickMode"], "features");
    assert!(h.app.run("select.mode", json!({"mode": "edges"})).is_err());
    assert_eq!(h.app.ui.pick_mode, PickMode::Features);
    h.call("ui.set", json!({"pickMode": "parts"}));
    assert_eq!(h.app.ui.pick_mode, PickMode::Parts);
    h.call("ui.set", json!({"pickBodies": true}));
    assert_eq!(h.app.ui.pick_mode, PickMode::Bodies);
    h.call("ui.set", json!({"pickBodies": false}));
    assert_eq!(h.app.ui.pick_mode, PickMode::Faces);
    let reopened = |prefs: &str| {
        let mut app = crate::SolveApp::new(Default::default(), Default::default());
        app.load_prefs(prefs);
        app.ui.pick_mode
    };
    for mode in PickMode::ALL {
        h.app.ui.pick_mode = mode;
        assert_eq!(reopened(&h.app.prefs()), mode);
    }
    assert_eq!(reopened(r#"{"pick_bodies": true}"#), PickMode::Bodies);
    assert_eq!(reopened(r#"{"pick_bodies": false}"#), PickMode::Faces);
    assert_eq!(reopened(r#"{"pick_mode": "dim", "pick_bodies": true}"#), PickMode::Faces);
}

/// "Select bodies" (the filter's box and Preferences') is the Bodies mode: ticked exactly there;
/// ticking sets Bodies and unticking Faces & edges, from whichever mode was on.
#[test]
fn the_select_bodies_box_is_the_bodies_mode() {
    use crate::test_support::{frame, pointer};
    let ctx = crate::fonts::test_context();
    let mut app = crate::SolveApp::new(Default::default(), Default::default());
    // Where the box is (its layout doesn't depend on the mode).
    let mut rect = egui::Rect::NOTHING;
    frame(&ctx, Default::default(), |ui: &mut egui::Ui| rect = crate::ui_kit::checkbox(ui, "Select bodies", &mut false).rect);
    let at = rect.left_center() + egui::vec2(4.0, 0.0);
    let press = |app: &mut crate::SolveApp| {
        for down in [true, false] {
            let input = egui::RawInput { events: pointer(at, down), ..Default::default() };
            frame(&ctx, input, |ui: &mut egui::Ui| crate::selection_mode::bodies_checkbox(app, ui, "Select bodies"));
        }
    };
    for (from, to) in [
        (PickMode::Faces, PickMode::Bodies),
        (PickMode::Bodies, PickMode::Faces),
        (PickMode::Features, PickMode::Bodies),
        (PickMode::Components, PickMode::Bodies),
    ] {
        crate::selection_mode::set(&mut app, from);
        press(&mut app);
        assert_eq!(app.ui.pick_mode, to, "from {from:?}");
    }
    // Unticked in Features: ticking gives Bodies, then unticking Faces & edges, not Features.
    crate::selection_mode::set(&mut app, PickMode::Features);
    press(&mut app);
    press(&mut app);
    assert_eq!(app.ui.pick_mode, PickMode::Faces);
}

/// The navigation bar's selection-mode slot (#6585): a menu row runs `select.mode`, and the slot
/// shows the mode's icon and name. The menu lists the five modes that pick from the model, then
/// sketch geometry.
#[test]
fn the_bar_slot_switches_the_mode_and_shows_it() {
    let mut h = Harness::new();
    h.frames(2);
    let slot = crate::navigation_bar::find("selection_mode").unwrap();
    for mode in [PickMode::Components, PickMode::Sketch, PickMode::Faces] {
        let log = h.app.session.log.len();
        h.step(&json!({"click": {"handle": "nav:slot:selection_mode"}})).unwrap();
        let rows: Vec<f32> = PickMode::ALL
            .iter()
            .map(|m| crate::scenario::handle_at(&format!("nav:selection:{}", crate::selection_mode::id(*m))).unwrap().y)
            .collect();
        assert!(rows.is_sorted(), "the menu lists the modes in order: {rows:?}");
        h.step(&json!({"click": {"handle": format!("nav:selection:{}", crate::selection_mode::id(mode))}})).unwrap();
        assert_eq!(h.app.ui.pick_mode, mode);
        let label = solidcraft_engine::find_command("select.mode").unwrap().label;
        let echo = crate::i18n::Locale::for_language(&h.app.preferences.language).format("ui.command_done", &[("command", label)]);
        assert!(h.app.session.log.iter().skip(log).any(|line| line == &echo), "{mode:?}: select.mode was not run");
        assert_eq!(slot.current_icon(&h.app.ui), crate::navigation_bar::selection_mode_icon(mode));
        assert_eq!(crate::navigation_bar_view::slot_text(&h.app, slot), h.app.tr_label(crate::selection_mode::label(mode)));
    }
    // Render picks through its own filter (the same mode), and an open sketch its own geometry:
    // the slot leaves the bar in both.
    assert!(slot.available(crate::navigation_bar::canvas(&h.app)));
    crate::render_workspace::enter(&mut h.app, true).unwrap();
    assert!(!slot.available(crate::navigation_bar::canvas(&h.app)));
    crate::render_workspace::enter(&mut h.app, false).unwrap();
    assert!(slot.available(crate::navigation_bar::canvas(&h.app)));
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    assert!(!slot.available(crate::navigation_bar::canvas(&h.app)));
}

/// The pointer lights what a click would pick: the hovered face's feature, its body; nothing
/// whole while a dialog is open.
#[test]
fn hover_lights_what_a_click_would_pick() {
    let mut h = Harness::new();
    let (_, cube) = stacked(&mut h);
    let top = face(&h, "Base", true);
    set_mode(&mut h, PickMode::Features);
    let shown = |h: &Harness, sel: Sel| crate::viewport::displayed_selection(&h.app, &sel);
    let lit = crate::selection_mode::hovered(&h.app, Some(&top));
    assert!(!lit.is_empty() && lit.iter().all(|s| matches!(s, Sel::Face { .. })), "{lit:?}");
    assert_eq!(lit, shown(&h, Sel::Feature { id: cube }));
    set_mode(&mut h, PickMode::Bodies);
    assert_eq!(crate::selection_mode::hovered(&h.app, Some(&top)), shown(&h, Sel::Body { name: "Base".into() }));
    set_mode(&mut h, PickMode::Faces);
    assert!(crate::selection_mode::hovered(&h.app, Some(&top)).is_empty());
    set_mode(&mut h, PickMode::Bodies);
    h.step(&json!({"start": "solid.fillet"})).unwrap();
    h.frames(1);
    assert!(h.app.dialog.is_some());
    assert!(crate::selection_mode::hovered(&h.app, Some(&top)).is_empty());
}

/// A real click on the model selects the mode's object; Select Other offers a body behind a face
/// only in Faces & edges (a whole-object mode already turns the face into one).
#[test]
fn a_click_and_select_other_follow_the_mode() {
    let mut h = Harness::new();
    let (block, _) = stacked(&mut h);
    h.call("ui.view", json!({"view": "front"}));
    h.call("ui.view", json!({"view": "fit"}));
    h.frames(2);
    // Low on the front face: the block's own face, below the cube.
    let r = h.app.viewport.rect.unwrap();
    let p = egui::pos2(r.center().x, r.center().y + r.height() * 0.15);
    // Body rows: Faces & edges adds one behind the face; Bodies' own pick is one; Features has none.
    let body_rows = |h: &Harness| {
        let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
        crate::select_other::rows(&h.app, &proj, p).iter().filter(|row| matches!(row.sel, Sel::Body { .. })).count()
    };
    for (mode, want, rows) in [
        (PickMode::Faces, None, 1),
        (PickMode::Bodies, Some(Sel::Body { name: "Base".into() }), 1),
        (PickMode::Features, Some(Sel::Feature { id: block }), 0),
    ] {
        set_mode(&mut h, mode);
        h.app.session.selection.clear();
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        let picked = h.app.session.selection.first().cloned();
        match &want {
            None => assert!(matches!(picked, Some(Sel::Face { .. })), "{mode:?}: {picked:?}"),
            Some(w) => assert_eq!(picked.as_ref(), Some(w), "{mode:?}"),
        }
        assert_eq!(body_rows(&h), rows, "{mode:?}");
    }
}
