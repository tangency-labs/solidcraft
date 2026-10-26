//! Hole placement on the canvas. A Single hole picked on or near a sketch point on the face (a
//! circle's centre) goes to that point, as a Multiple pick of the same point does (#657), and
//! follows it when the sketch changes (#2110); a pick
//! away from sketch points, or near one off the face under the cursor, stays where it was clicked.
//! Only points the sketch shows are snapped to (not lettering's outline points), and Select Other
//! looks the face up once for every point near the cursor.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::{Aabb3, Vec2, Vec3};
use solidcraft_engine::sketch::{CurveKind, LinkKind, SPoint};

use crate::dialogs::Kind;
use crate::scenario::Harness;
use crate::viewport::Hit;

fn personal_preset_harness() -> Harness {
    let mut h = Harness::new();
    crate::fonts::setup_bundled_fonts(&h.ctx);
    h.call("ui.resize", json!({"width":1400,"height":1100}));
    h.app.run("solid.box", json!({"name":"PresetFixture","length":40,"width":30,"height":20})).unwrap();
    h.app.session.selection = vec![Sel::Face { body: "PresetFixture".into(), index: 5, point: Vec3::new(10.0, 10.0, 20.0) }];
    h.app.start("solid.hole");
    h.frames(4);
    h.step(&json!({"click":{"handle":"dialog:section:hole_presets.title"}})).unwrap();
    h.frames(2);
    h
}

fn preset_name(h: &mut Harness, name: &str) {
    h.step(&json!({"click":{"handle":"dialog:hole_presets.name"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":name})).unwrap();
}

fn confirm_preset_name(h: &mut Harness) {
    h.step(&json!({"click":{"handle":"dialog:ok"}})).unwrap();
}

fn save_preset(h: &mut Harness, name: &str) {
    h.step(&json!({"widget":"Save new"})).unwrap();
    preset_name(h, name);
    confirm_preset_name(h);
}

fn choose_preset(h: &mut Harness, name: &str) {
    h.step(&json!({"click":{"handle":"dialog:hole_presets.preset"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:hole_presets.preset:{name}")}})).unwrap();
}

#[test]
fn hole_presets_retain_independent_bottom_choice_for_counterbore_and_countersink() {
    for (style, kind) in [("counterbore", 1), ("countersink", 2)] {
        for angled in [false, true] {
            let mut h = personal_preset_harness();
            let row = json!({"name":"Bottom choice","style":style,"extent":"distance",
                "diameter":"4 mm","depth":"12 mm","cb_diameter":"8 mm","cb_depth":"3 mm",
                "cs_diameter":"8 mm","cs_angle":"90 deg","tip_angle":"129 deg",
                "tapped":false,"thread":"M5","angled_bottom":angled});
            h.app.preferences.hole_presets = serde_json::from_value(json!([row])).unwrap();
            assert_eq!(
                serde_json::to_value(&h.app.preferences.hole_presets).unwrap()[0]["angled_bottom"],
                angled,
                "recess bottom choice must survive preference serialization independently of style"
            );
            if let Kind::Hole { opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
                opts.angled = !angled;
            }
            h.frames(2);
            choose_preset(&mut h, "Bottom choice");
            assert!(
                matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole { kind:k,opts,.. } if *k==kind && opts.angled==angled && opts.tip_angle=="129 deg")
            );
            save_preset(&mut h, "Bottom copy");
            assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap()[1]["angled_bottom"], angled);
            assert_eq!(h.app.session.doc.features.len(), 1);
        }
    }
}

#[test]
fn hole_presets_legacy_drilled_and_recess_records_keep_their_original_bottom() {
    for (style, kind, angled) in [("drilled", 0, true), ("counterbore", 1, false), ("countersink", 2, false)] {
        let mut h = personal_preset_harness();
        let row = json!({"name":"Legacy","style":style,"extent":"distance",
            "diameter":"4 mm","depth":"12 mm","cb_diameter":"8 mm","cb_depth":"3 mm",
            "cs_diameter":"8 mm","cs_angle":"90 deg","tip_angle":"129 deg","tapped":false,"thread":"M5"});
        h.app.preferences.hole_presets = serde_json::from_value(json!([row])).unwrap();
        h.frames(2);
        choose_preset(&mut h, "Legacy");
        assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole { kind:k,opts,.. } if *k==kind && opts.angled==angled));
    }
}

#[test]
fn hole_presets_through_bottom_keeps_inactive_empty_angle_text() {
    for style in [0, 1, 2] {
        let mut h = personal_preset_harness();
        if let Kind::Hole { kind, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *kind = style;
            opts.all = true;
            opts.angled = true;
            opts.tip_angle.clear();
        }
        h.frames(2);
        save_preset(&mut h, "Through");
        let rows = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
        assert_eq!(rows.as_array().unwrap().len(), 1, "inactive bottom angle must not block a through preset, style={style}");
        assert_eq!(rows[0]["tip_angle"], "");
        assert_eq!(rows[0]["angled_bottom"], true);
    }
}

#[test]
fn hole_presets_automatically_recall_only_changed_selection_without_persistent_name_or_recall_button() {
    let mut h = personal_preset_harness();
    h.step(&json!({"expect":{"hidden_handles":["dialog:hole_presets.name"],"not_shown":"Recall"}})).unwrap();
    if let Kind::Hole { diameter, depth, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *diameter = "4.2 mm".into();
        *depth = "0.5 in".into();
        opts.tapped = true;
        opts.thread = "M5".into();
    }
    save_preset(&mut h, "Workshop");
    if let Kind::Hole { diameter, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *diameter = "8 mm".into();
    }
    save_preset(&mut h, "Other");
    let document = serde_json::to_value(&*h.app.session.doc).unwrap();
    let inputs = format!("{:?}", h.app.dialog.as_ref().unwrap().inputs);
    choose_preset(&mut h, "Workshop");
    assert!(
        matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {diameter,depth,opts,..} if diameter=="4.2 mm" && depth=="0.5 in" && opts.tapped && opts.thread=="M5")
    );
    if let Kind::Hole { diameter, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *diameter = "9 mm".into();
    }
    h.frames(8);
    assert!(
        matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {diameter,..} if diameter=="9 mm"),
        "an unchanged selection must not overwrite manual edits every frame"
    );
    choose_preset(&mut h, "Select a preset");
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {diameter,..} if diameter=="9 mm"));
    choose_preset(&mut h, "Workshop");
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {diameter,..} if diameter=="4.2 mm"));
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), document);
    assert_eq!(format!("{:?}", h.app.dialog.as_ref().unwrap().inputs), inputs);
    h.step(&json!({"expect":{"hidden_handles":["dialog:hole_presets.name"],"not_shown":"Recall"}})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_none());
    h.app.start("solid.hole");
    h.frames(4);
    if let Kind::Hole { diameter, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *diameter = "9 mm".into();
    }
    h.frames(2);
    choose_preset(&mut h, "Workshop");
    assert!(
        matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {diameter,..} if diameter=="4.2 mm"),
        "the same preset can be selected in a fresh Hole dialog"
    );
    h.step(&json!({"widget":"Save new"})).unwrap();
    // Closing the tool programmatically must not leave a prompt in its next instance.
    h.app.dialog = None;
    h.frames(2);
    h.app.start("solid.hole");
    h.frames(4);
    h.step(&json!({"expect":{"hidden_handles":["dialog:hole_presets.name"]}})).unwrap();
}

#[test]
fn hole_presets_name_prompt_keeps_keyboard_when_canvas_requests_value_focus() {
    let mut h = personal_preset_harness();
    let document = h.app.session.doc.to_json();
    let fields = format!("{:?}", h.app.dialog.as_ref().unwrap().kind);
    h.step(&json!({"widget":"Save new"})).unwrap();
    preset_name(&mut h, "FocusTarget");
    h.app.dialog.as_mut().unwrap().restore_value_focus = true;
    h.frames(2);
    h.step(&json!({"text":"-kept"})).unwrap();
    h.step(&json!({"key":"Enter"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap()[0]["name"], "FocusTarget-kept");
    assert_eq!(h.app.session.doc.to_json(), document);
    assert_eq!(format!("{:?}", h.app.dialog.as_ref().unwrap().kind), fields);
}

#[test]
fn hole_presets_name_prompt_cancel_duplicate_invalid_and_prefilled_rename_leave_document_untouched() {
    let mut h = personal_preset_harness();
    save_preset(&mut h, "Workshop");
    let bank = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    let document = serde_json::to_value(&*h.app.session.doc).unwrap();
    let form = format!("{:?}", h.app.dialog);
    for action in ["Save new", "Rename"] {
        h.step(&json!({"widget":action})).unwrap();
        preset_name(&mut h, "Discard this");
        h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
        assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), bank);
        assert_eq!(format!("{:?}", h.app.dialog), form);
    }
    h.step(&json!({"widget":"Save new"})).unwrap();
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(h.app.dialog.is_some(), "Escape closes only the name prompt");
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), bank);
    h.step(&json!({"widget":"Save new"})).unwrap();
    confirm_preset_name(&mut h);
    h.step(&json!({"expect":{"shown":"Enter a short name without line breaks or control characters."}})).unwrap();
    preset_name(&mut h, "workshop");
    confirm_preset_name(&mut h);
    h.step(&json!({"expect":{"shown":"A preset with this name already exists. Use a different name."}})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), bank);
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    h.step(&json!({"widget":"Rename"})).unwrap();
    confirm_preset_name(&mut h); // The current name was prefilled; renaming to itself is valid.
    h.step(&json!({"expect":{"hidden_handles":["dialog:hole_presets.name"]}})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), bank);
    h.step(&json!({"widget":"Rename"})).unwrap();
    preset_name(&mut h, "Renamed");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_some(), "Enter confirms the name, never applies the Hole");
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap()[0]["name"], "Renamed");
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), document);
    assert_eq!(format!("{:?}", h.app.dialog), form);
}

#[test]
fn hole_presets_save_explicit_source_units_including_mm_without_document_edits() {
    for unit in ["in", "mm"] {
        let mut h = personal_preset_harness();
        h.app.session.doc_mut().units = unit.into();
        if let Kind::Hole { diameter, depth, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *diameter = "0.25".into();
            *depth = "2 * 3".into();
        }
        save_preset(&mut h, "Portable");
        let presets = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
        assert_eq!(presets[0]["diameter"], format!("0.25 {unit}"));
        assert_eq!(presets[0]["depth"], format!("(2 * 3) * 1 {unit}"));
        assert_eq!(h.app.session.doc.features.len(), 1);
        assert!(h.app.dialog.is_some());
    }
}

#[test]
fn hole_presets_save_rename_delete_and_enter_only_change_the_form_or_profile() {
    let mut h = personal_preset_harness();
    save_preset(&mut h, "Workshop");
    let saved = h.app.prefs();
    h.step(&json!({"widget":"Rename"})).unwrap();
    preset_name(&mut h, "Renamed");
    confirm_preset_name(&mut h);
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap()[0]["name"], "Renamed");
    h.step(&json!({"widget":"Delete"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap().as_array().unwrap().len(), 1);
    h.step(&json!({"widget":"Confirm delete"})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), json!([]));
    assert_eq!(h.app.session.doc.features.len(), 1);
    let mut other = crate::SolveApp::new(Default::default(), Default::default());
    other.load_prefs(&saved);
    assert_eq!(serde_json::to_value(&other.preferences.hole_presets).unwrap()[0]["name"], "Workshop");
}

#[test]
fn hole_presets_inactive_incomplete_dimensions_do_not_block_valid_simple_all_preset() {
    let mut h = personal_preset_harness();
    if let Kind::Hole { diameter, depth, cb_depth, cs_diameter, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *diameter = "4 mm".into();
        *depth = "unfinished(".into();
        *cb_depth = "missing_parameter".into();
        *cs_diameter = "3 deg".into();
        opts.all = true;
    }
    save_preset(&mut h, "Simple through");
    let saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    assert_eq!(saved[0]["name"], "Simple through");
    assert_eq!(saved[0]["extent"], "all");
    assert_eq!(saved[0]["depth"], "unfinished(");
    assert_eq!(saved[0]["cb_depth"], "missing_parameter");
    assert_eq!(saved[0]["cs_diameter"], "3 deg");
    assert_eq!(h.app.session.doc.features.len(), 1);
    assert!(h.app.dialog.is_some());
}

#[test]
fn hole_presets_unknown_options_refuse_automatic_recall_without_changing_any_hole_fields() {
    let mut h = personal_preset_harness();
    save_preset(&mut h, "Future");
    let mut saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    saved[0]["thread_options"]["starts"] = json!(2);
    h.app.preferences.hole_presets = serde_json::from_value(saved).unwrap();
    choose_preset(&mut h, "Select a preset");
    if let Kind::Hole { diameter, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *diameter = "9 mm".into();
    }
    let before = serde_json::to_value(&*h.app.session.doc).unwrap();
    let form = format!("{:?}", h.app.dialog);
    choose_preset(&mut h, "Future");
    assert_eq!(format!("{:?}", h.app.dialog), form, "unsupported options must not partially change the draft");
    assert_eq!(serde_json::to_value(&*h.app.session.doc).unwrap(), before);
    h.step(&json!({"expect":{"shown":"This preset uses thread options that this version cannot recall."}})).unwrap();
    h.step(&json!({"widget":"Rename"})).unwrap();
    preset_name(&mut h, "Future renamed");
    confirm_preset_name(&mut h);
    let retained = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    assert_eq!(retained[0]["name"], "Future renamed");
    assert_eq!(retained[0]["thread_options"]["starts"], 2);
}

/// A 40 × 30 × 20 block, seen from the top, with a sketch on its top face: a circle of radius 4
/// about (12, 9). Returns the circle centre in the world.
fn block_with_circle() -> (Harness, Vec3) {
    let (h, centre) = block_and_circle_on(json!({"plane": {"face": [20, 15, 20]}}), [12.0, 9.0]);
    assert!((centre.z - 20.0).abs() < 1e-9, "the circle lies on the top face: {centre:?}");
    (h, centre)
}

/// The block, seen from the top, and a circle of radius 4 about `at` (sketch coordinates) in the
/// sketch `sketch` (`sketch.create`'s parameters) makes. Returns the circle centre in the world.
fn block_and_circle_on(sketch: Value, at: [f64; 2]) -> (Harness, Vec3) {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("sketch.create", sketch).unwrap();
    h.app.run("sketch.circle.center", json!({"center": at, "radius": 4})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    let st = h.app.session.world_state().unwrap();
    let ss = st.sketches.first().unwrap();
    let centre = ss
        .sketch
        .curves
        .iter()
        .find_map(|c| match c.kind {
            CurveKind::Circle { c, .. } => Some(ss.plane.to_world(ss.sketch.points[c].pos)),
            _ => None,
        })
        .unwrap();
    (h, centre)
}

/// Where a world point is on screen.
fn screen(h: &mut Harness, p: Vec3) -> [f64; 2] {
    h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap()
}

/// Five pixels off where `p` is on screen: within reach of a sketch point pick, not on `p`.
fn near(h: &mut Harness, p: Vec3) -> [f64; 2] {
    let c = screen(h, p);
    [c[0] + 4.0, c[1] + 3.0]
}

/// Everything under a screen point, as a click there sees it.
fn hits_at(h: &Harness, at: [f64; 2]) -> Vec<Hit> {
    let proj = crate::viewport::projection(&h.app, h.app.viewport.rect.unwrap());
    crate::viewport::pick(&h.app, &proj, egui::pos2(at[0] as f32, at[1] as f32))
}

/// The face the cursor is over, where the ray meets it, after hovering `at`.
fn hovered_face_point(h: &mut Harness, at: [f64; 2]) -> Vec3 {
    h.step(&json!({ "move": at })).unwrap();
    match &h.app.viewport.hover {
        Some(Hit::Face { point, .. }) => *point,
        other => panic!("the face lights up, not {other:?}"),
    }
}

/// The last feature's hole placement: its id, its position and whether it is at sketch points.
fn hole(h: &Harness) -> (u64, Vec3, bool) {
    let f = h.app.session.doc.features.last().unwrap();
    match &f.kind {
        FeatureKind::Hole { position, points, .. } => (f.id, position.0, points.is_some()),
        other => panic!("the last feature is not a hole: {}", other.type_name()),
    }
}

/// The face places the open dialog's first input holds.
fn face_places(h: &Harness) -> Vec<(String, Vec3)> {
    h.app.dialog.as_ref().unwrap().inputs[0]
        .items
        .iter()
        .map(|x| match x {
            Sel::Face { body, point, .. } => (body.clone(), *point),
            other => panic!("a face place, not {other:?}"),
        })
        .collect()
}

/// Whether the model holds material at `p`.
fn solid_at(h: &Harness, p: Vec3) -> bool {
    h.app.session.model.state().bodies.iter().any(|b| b.body.contains(p))
}

#[test]
fn a_single_hole_picked_near_a_circle_centre_goes_to_the_centre() {
    let (mut h, centre) = block_with_circle();
    let click = near(&mut h, centre);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let d = h.app.dialog.as_ref().unwrap();
    assert!(matches!(&d.kind, Kind::Hole { opts, .. } if !opts.multiple), "a new hole starts with Single placement");
    h.step(&json!({"move": click})).unwrap();
    assert!(
        matches!(&h.app.viewport.hover, Some(Hit::SketchPoint { .. })),
        "the centre lights up as what the pick snaps to: {:?}",
        h.app.viewport.hover
    );
    h.step(&json!({"click": click})).unwrap();
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(_, p)] if p.dist(centre) < 1e-9), "the face is picked at the centre {centre:?}, not {places:?}");
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (id, position, at_points) = hole(&h);
    assert!(at_points, "a Single hole snapped to the centre keeps the point");
    assert!(position.dist(centre) < 1e-9, "the hole is at the circle centre {centre:?}, not at {position:?}");
    assert!(h.app.session.model.result(id).is_some_and(|r| r.error.is_none() && r.warning.is_none()));
    // The bore (5 mm across, 10 mm deep) is round about the centre: empty on its axis and up to
    // 2.4 mm from it, solid 2.6 mm from it in every direction.
    let bore = |h: &Harness| {
        for k in 0..8 {
            let a = std::f64::consts::FRAC_PI_4 * f64::from(k);
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            let at = |r: f64| centre + dir * r - Vec3::new(0.0, 0.0, 5.0);
            assert!(!solid_at(h, at(0.0)) && !solid_at(h, at(2.4)), "the bore is empty {a} rad from +x");
            assert!(solid_at(h, at(2.6)), "the wall is solid {a} rad from +x");
        }
    };
    bore(&h);
    // Edit Feature shows the Single placement on the top face at the centre, and OK keeps it.
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": id}})).unwrap();
    h.frames(4);
    let d = h.app.dialog.as_ref().expect("the hole dialog");
    assert!(matches!(&d.kind, Kind::Hole { opts, .. } if !opts.multiple), "{:?}", d.kind);
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(body, p)] if body == "Body1" && p.dist(centre) < 1e-6), "{places:?}");
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (again, position, at_points) = hole(&h);
    assert_eq!(again, id, "the same feature");
    assert!(at_points && position.dist(centre) < 1e-9, "{position:?}");
    bore(&h);
}

#[test]
fn a_single_hole_at_a_circle_centre_follows_the_circle() {
    let (mut h, centre) = block_with_circle();
    let click = near(&mut h, centre);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    h.step(&json!({"click": click})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (id, _, _) = hole(&h);
    let sketch = h.app.session.world_state().unwrap().sketches.first().unwrap().feature;
    let circle = h
        .app
        .session
        .world_state()
        .unwrap()
        .sketches
        .first()
        .unwrap()
        .sketch
        .curves
        .iter()
        .find(|c| matches!(c.kind, CurveKind::Circle { .. }))
        .unwrap()
        .id
        .clone();
    // The circle's centre moves 8 mm along the sketch's first axis: the hole goes with it.
    h.app.run("sketch.edit", json!({ "sketch": sketch })).unwrap();
    h.app.run("sketch.move_point", json!({"point": format!("{circle}.center"), "to": [20, 9]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    let st = h.app.session.world_state().unwrap();
    let ss = st.sketches.first().unwrap();
    let moved = ss
        .sketch
        .curves
        .iter()
        .find_map(|c| match c.kind {
            CurveKind::Circle { c, .. } => Some(ss.plane.to_world(ss.sketch.points[c].pos)),
            _ => None,
        })
        .unwrap();
    assert!((moved.dist(centre) - 8.0).abs() < 1e-6, "the centre moved 8 mm: {centre:?} -> {moved:?}");
    assert!(h.app.session.model.result(id).is_some_and(|r| r.error.is_none() && r.warning.is_none()));
    let down = Vec3::new(0.0, 0.0, 5.0);
    assert!(!solid_at(&h, moved - down), "the hole is at the moved centre {moved:?}");
    assert!(solid_at(&h, centre - down), "the old centre {centre:?} is solid again");
    // Edit Feature shows it as Single, picked where the centre is now, and OK keeps it there.
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": id}})).unwrap();
    h.frames(4);
    let d = h.app.dialog.as_ref().expect("the hole dialog");
    assert!(matches!(&d.kind, Kind::Hole { opts, .. } if !opts.multiple), "{:?}", d.kind);
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(_, p)] if p.dist(moved) < 1e-6), "{places:?}");
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (again, _, at_points) = hole(&h);
    assert!(again == id && at_points, "the same feature, still at the point");
    assert!(!solid_at(&h, moved - down));
}

#[test]
fn a_single_hole_picked_away_from_sketch_points_stays_where_it_was_clicked() {
    let (mut h, centre) = block_with_circle();
    // Inside the circle, well away from its centre (and from every other sketch point).
    let away = centre + Vec3::new(3.0, 2.0, 0.0);
    let click = screen(&mut h, away);
    let c = screen(&mut h, centre);
    assert!(c.iter().zip(click).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt() > 16.0, "twice a point pick's reach from the centre");
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let raw = hovered_face_point(&mut h, click);
    h.step(&json!({"click": click})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none());
    let (_, position, at_points) = hole(&h);
    assert!(!at_points);
    assert!(position.dist(raw) < 1e-9 && position.dist(away) < 1e-3, "the hole stays where it was clicked {raw:?}, not at {position:?}");
}

#[test]
fn a_single_hole_picked_near_a_sketch_point_off_the_face_under_it_stays_where_it_was_clicked() {
    // The circle 10 mm above the top face (its centre lies on no face), then on the bottom face
    // (hidden behind the top face): either way its centre is under the pick on screen, but not on
    // the face picked.
    // (The bottom face's sketch looks up through the block: its y runs along the world's −y.)
    let cases = [(json!({"plane": "XY", "offset": 30}), [12.0, 9.0], 30.0), (json!({"plane": {"face": [20, 15, 0]}}), [12.0, -9.0], 0.0)];
    for (sketch, at, z) in cases {
        let (mut h, centre) = block_and_circle_on(sketch, at);
        assert!(centre.dist(Vec3::new(12.0, 9.0, z)) < 1e-9, "{centre:?}");
        let click = near(&mut h, centre);
        h.step(&json!({"start": "solid.hole"})).unwrap();
        let hits = hits_at(&h, click);
        assert!(matches!(hits.first(), Some(Hit::SketchPoint { .. })), "the click reaches the centre: {hits:?}");
        let raw = hovered_face_point(&mut h, click);
        h.step(&json!({"click": click})).unwrap();
        h.step(&json!({"key": "Enter"})).unwrap();
        assert!(h.app.dialog.is_none());
        let (_, position, at_points) = hole(&h);
        assert!(!at_points);
        assert!(position.dist(raw) < 1e-9, "the hole stays where it was clicked {raw:?}, not at {position:?}");
        assert!((position.z - 20.0).abs() < 1e-9 && position.dist(Vec3::new(centre.x, centre.y, 20.0)) > 0.1, "{position:?}");
    }
}

#[test]
fn select_other_offers_the_centre_and_the_face_as_picked() {
    let (mut h, centre) = block_with_circle();
    let click = near(&mut h, centre);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    // Where the click's ray meets the top face.
    let ray = hits_at(&h, click);
    let Some(Hit::Face { point: raw, .. }) = ray.iter().find(|x| matches!(x, Hit::Face { .. })) else { panic!("{ray:?}") };
    let raw = *raw;
    assert!(raw.dist(centre) > 0.1);
    for (row, want) in [(0, centre), (1, raw)] {
        let list = h.call("ui.selectOther", json!({"x": click[0], "y": click[1]}))["result"].clone();
        let labels: Vec<&str> = list["rows"].as_array().into_iter().flatten().filter_map(|r| r["label"].as_str()).collect();
        assert!(labels.len() == 2 && labels[0].contains("centre") && labels[1].starts_with("Face"), "the snapped centre, then the face: {labels:?}");
        h.call("ui.selectOther", json!({ "pick": row }));
        let places = face_places(&h);
        assert!(matches!(places.as_slice(), [(_, p)] if p.dist(want) < 1e-9), "row {row} places at {want:?}: {places:?}");
        // Start over for the other row (a second pick on the same face would take it out).
        h.step(&json!({"key": "Escape"})).unwrap();
        h.step(&json!({"start": "solid.hole"})).unwrap();
    }
}

#[test]
fn select_other_keeps_the_face_as_picked_beside_many_snapped_points() {
    // More points under the cursor than Select Other lists of one kind: the snapped places are
    // capped as points, and the face as picked keeps its row.
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("sketch.create", json!({"plane": {"face": [20, 15, 20]}})).unwrap();
    let many = crate::select_other::MAX_PER_KIND + 4;
    for i in 0..many {
        let a = i as f64 * 0.3;
        h.app.run("sketch.point", json!({"point": [12.0 + 0.1 * a.cos(), 9.0 + 0.1 * a.sin()]})).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    let click = near(&mut h, Vec3::new(12.0, 9.0, 20.0));
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let list = h.call("ui.selectOther", json!({"x": click[0], "y": click[1]}))["result"].clone();
    let rows = list["rows"].as_array().cloned().unwrap_or_default();
    let count = |prefix: &str| rows.iter().filter(|r| r["label"].as_str().is_some_and(|l| l.starts_with(prefix))).count();
    assert_eq!((count("Point"), count("Face")), (crate::select_other::MAX_PER_KIND, 1), "{list}");
    assert_eq!(list["total"], json!(many + 1), "{list}");
}

/// The block, seen from the top, with an "L" 10 mm high written on its top face from (5, 5), and
/// with a circle of radius 0.5 about `circle` (millimetres right of the foot's lower right corner)
/// when given. Returns the corner, then the circle centre, in the world.
fn block_with_lettering(circle: Option<f64>) -> (Harness, Vec3, Option<Vec3>) {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("sketch.create", json!({"plane": {"face": [20, 15, 20]}})).unwrap();
    h.app.run("sketch.text", json!({"text": "L", "at": [5, 5], "height": 10})).unwrap();
    let outline = |h: &Harness| {
        let st = h.app.session.world_state().unwrap();
        let ss = st.sketches.first().unwrap();
        let sk = &ss.sketch;
        let text = |p: &&SPoint| p.link.as_deref().and_then(|l| sk.link(l)).is_some_and(|l| l.kind == LinkKind::Text);
        sk.points.iter().filter(text).map(|p| p.pos).collect::<Vec<Vec2>>()
    };
    let corner = outline(&h).into_iter().max_by(|a, b| (a.x - a.y).total_cmp(&(b.x - b.y))).unwrap();
    let centre = circle.map(|dx| Vec2::new(corner.x + dx, corner.y));
    if let Some(c) = centre {
        h.app.run("sketch.circle.center", json!({"center": [c.x, c.y], "radius": 0.5})).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    let st = h.app.session.world_state().unwrap();
    let plane = st.sketches.first().unwrap().plane;
    assert_eq!(outline(&h).len(), 6, "the L's outline has six corners");
    (h, plane.to_world(corner), centre.map(|c| plane.to_world(c)))
}

/// Where the ray through a screen point meets the nearest face.
fn face_point_at(h: &Harness, at: [f64; 2]) -> Vec3 {
    let ray = hits_at(h, at);
    match ray.iter().find(|x| matches!(x, Hit::Face { .. })) {
        Some(Hit::Face { point, .. }) => *point,
        _ => panic!("no face under {at:?}: {ray:?}"),
    }
}

#[test]
fn a_single_hole_picked_beside_lettering_stays_where_it_was_clicked() {
    // Text outlines' points are not drawn and not picked on their own (#718 review): a click 5 px
    // off a letter's corner keeps the face point it was clicked at.
    let (mut h, corner, _) = block_with_lettering(None);
    assert!((corner.z - 20.0).abs() < 1e-9, "the lettering lies on the top face: {corner:?}");
    let click = near(&mut h, corner);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let raw = face_point_at(&h, click);
    h.step(&json!({"move": click})).unwrap();
    assert!(matches!(&h.app.viewport.hover, Some(Hit::Face { .. })), "the face lights up, not a letter's corner: {:?}", h.app.viewport.hover);
    h.step(&json!({"click": click})).unwrap();
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(_, p)] if p.dist(raw) < 1e-9), "the face is picked where clicked {raw:?}, not {places:?}");
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "OK closed the dialog: {:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (_, position, at_points) = hole(&h);
    assert!(!at_points && position.dist(raw) < 1e-9 && position.dist(corner) > 0.5, "{position:?}");
}

#[test]
fn a_single_hole_picked_beside_lettering_snaps_to_a_shown_point_in_reach() {
    // A circle centre 1.2 mm (8 px) right of a letter's corner. The click is nearer the corner
    // (2.4 px) but within reach of the centre (5.6 px): the centre is the point the pick snaps to.
    let (mut h, corner, centre) = block_with_lettering(Some(1.2));
    let centre = centre.unwrap();
    let (c, p) = (screen(&mut h, corner), screen(&mut h, centre));
    let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
    assert!(d > 6.0 && d < 10.0, "the centre is {d} px from the corner");
    let click = [c[0] + (p[0] - c[0]) * 0.3, c[1] + (p[1] - c[1]) * 0.3];
    h.step(&json!({"start": "solid.hole"})).unwrap();
    h.step(&json!({"click": click})).unwrap();
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(_, p)] if p.dist(centre) < 1e-9), "the face is picked at the centre {centre:?}, not {places:?}");
}

#[test]
fn a_single_hole_snaps_only_to_points_its_sketch_shows() {
    // A face sketch projects the face's edges, so the face's corners are points of the sketch: a
    // click 5 px inside a corner snaps to it while the sketch shows projected geometry, and stays
    // where it was clicked while the sketch hides it (Show Projected off).
    for shown in [true, false] {
        let mut h = Harness::new();
        h.app.set_origin_shown(false);
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.run("sketch.create", json!({"plane": {"face": [20, 15, 20]}})).unwrap();
        h.app.run("sketch.options", json!({"show_projected": shown})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
        h.settle();
        let corner = Vec3::new(40.0, 30.0, 20.0);
        let c = screen(&mut h, corner);
        let click = [c[0] - 4.0, c[1] + 3.0];
        h.step(&json!({"start": "solid.hole"})).unwrap();
        let raw = face_point_at(&h, click);
        assert!(raw.dist(corner) > 0.5 && (raw.z - 20.0).abs() < 1e-9, "{raw:?}");
        h.step(&json!({"click": click})).unwrap();
        let want = if shown { corner } else { raw };
        let places = face_places(&h);
        assert!(matches!(places.as_slice(), [(_, p)] if p.dist(want) < 1e-9), "projected shown: {shown}; at {want:?}, not {places:?}");
    }
    // A hidden sketch offers no points: its circle's centre is not snapped to.
    let (mut h, centre) = block_with_circle();
    let name = h.app.session.doc.features.iter().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).unwrap().name.clone();
    h.app.run("browser.visibility", json!({"items": [name], "visible": false})).unwrap();
    let click = near(&mut h, centre);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let raw = face_point_at(&h, click);
    h.step(&json!({"click": click})).unwrap();
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(_, p)] if p.dist(raw) < 1e-9 && p.dist(centre) > 0.1), "{places:?}");
    // Show Points off only hides the markers of the sketch being edited (a finished sketch draws
    // none): its circle's centre is still snapped to.
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("sketch.create", json!({"plane": {"face": [20, 15, 20]}})).unwrap();
    h.app.run("sketch.options", json!({"show_points": false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center": [12, 9], "radius": 4})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    let centre = Vec3::new(12.0, 9.0, 20.0);
    let click = near(&mut h, centre);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    h.step(&json!({"click": click})).unwrap();
    let places = face_places(&h);
    assert!(matches!(places.as_slice(), [(_, p)] if p.dist(centre) < 1e-9), "Show Points off: at the centre, not {places:?}");
}

/// Three Select Other lists' worth of sketch points under the cursor, with a Single hole started.
struct Cluster {
    h: Harness,
    /// How many points.
    many: usize,
    /// 5 px off them.
    click: [f64; 2],
    /// Triangles of the body under the click, of the face under it, and of that face's triangles
    /// the ones whose bounding box comes within a micrometre of the points' bounding box.
    body_tris: usize,
    face_tris: usize,
    near_tris: usize,
}

impl Cluster {
    /// Select Other at the click: the list, then (planar face lookups, triangles the snap looked
    /// at) while making it.
    fn select_other(&mut self) -> (Value, (usize, usize)) {
        crate::dialogs::take_snap_work();
        let list = self.h.call("ui.selectOther", json!({"x": self.click[0], "y": self.click[1]}))["result"].clone();
        (list, crate::dialogs::take_snap_work())
    }
}

/// How many rows of a Select Other list are labelled `prefix`….
fn rows_named(list: &Value, prefix: &str) -> usize {
    list["rows"].as_array().into_iter().flatten().filter(|r| r["label"].as_str().is_some_and(|l| l.starts_with(prefix))).count()
}

/// A cylinder of radius 15 and height 20 standing on the origin, seen from `view`, with the
/// points within 0.1 mm of `at` in the sketch `sketch.create` makes with `sketch`.
fn cylinder_with_cluster(sketch: Value, at: [f64; 2], view: &str) -> Cluster {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.cylinder", json!({"radius": 15, "height": 20})).unwrap();
    h.app.run("sketch.create", sketch).unwrap();
    let many = 3 * crate::select_other::MAX_PER_KIND;
    let local: Vec<Vec2> = (0..many).map(|i| i as f64 * 0.3).map(|a| Vec2::new(at[0] + 0.1 * a.cos(), at[1] + 0.1 * a.sin())).collect();
    for p in &local {
        h.app.run("sketch.point", json!({"point": [p.x, p.y]})).unwrap();
    }
    h.app.run("sketch.finish", json!({})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": view}})).unwrap();
    h.settle();
    let plane = h.app.session.world_state().unwrap().sketches.first().unwrap().plane;
    let centre = plane.to_world(Vec2::new(at[0], at[1]));
    let mut points = Aabb3::EMPTY;
    local.iter().for_each(|p| points.add(plane.to_world(*p)));
    let click = near(&mut h, centre);
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let ray = hits_at(&h, click);
    let Some(Hit::Face { body, index, .. }) = ray.iter().find(|x| matches!(x, Hit::Face { .. })) else { panic!("no face under {centre:?}: {ray:?}") };
    let m = h.app.session.world_state().unwrap().body(body).unwrap().mesh();
    let face: Vec<[Vec3; 3]> = m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f as usize == *index).filter_map(|(t, _)| m.tri(t)).collect();
    let near_points = |t: &[Vec3; 3]| {
        let mut b = Aabb3::EMPTY;
        t.iter().for_each(|v| b.add(*v));
        let gap = (b.min - points.max).max(points.min - b.max);
        gap.x.max(gap.y).max(gap.z) <= 1e-3
    };
    let (body_tris, face_tris, near_tris) = (m.triangles.len(), face.len(), face.iter().filter(|t| near_points(t)).count());
    assert!(body_tris >= 2 * face_tris, "the body has other faces: {body_tris} and {face_tris} triangles");
    Cluster { h, many, click, body_tris, face_tris, near_tris }
}

#[test]
fn select_other_over_a_dense_cluster_looks_the_face_up_once() {
    // The points on the cylinder's top face, a disc of a small part of the body's triangles. Select
    // Other looks the face under the cursor up once for all of them, not once per point (#718
    // review): one planarity lookup, which tessellates the body, and one pass over the body's
    // triangles. Each point is then tested against the face's triangles near the points only,
    // not the face's or the body's; every point on the face is still counted.
    let mut c = cylinder_with_cluster(json!({"plane": {"face": [0, 0, 20]}}), [5.0, 4.0], "top");
    let (many, body, face, near) = (c.many, c.body_tris, c.face_tris, c.near_tris);
    assert!(body >= 4 * face && 4 * near <= face, "{body} triangles in the body, {face} in the face, {near} of them near the points");
    let (list, (lookups, tested)) = c.select_other();
    assert_eq!((rows_named(&list, "Point"), rows_named(&list, "Face")), (crate::select_other::MAX_PER_KIND, 1), "{list}");
    assert_eq!(list["total"], json!(many + 1), "every point on the face is counted: {list}");
    assert_eq!(lookups, 1, "the face is looked up once for {many} points");
    assert!(tested <= body + many * near, "{tested} triangles looked at for {many} points ({body} in the body, {near} near the points)");
}

#[test]
fn select_other_over_a_dense_cluster_off_the_face_tests_no_triangles() {
    // The points 10 mm above the cylinder's top face, seen from the top: under the cursor, but
    // off the face's plane. Each is turned down without a triangle test, and the face as picked
    // is all that is offered.
    let mut c = cylinder_with_cluster(json!({"plane": "XY", "offset": 30}), [5.0, 4.0], "top");
    let (many, body) = (c.many, c.body_tris);
    let (list, (lookups, tested)) = c.select_other();
    assert_eq!((rows_named(&list, "Point"), rows_named(&list, "Face")), (0, 1), "{list}");
    assert_eq!(list["total"], json!(1), "{list}");
    assert_eq!(lookups, 1, "the face is looked up once for {many} points");
    assert_eq!(tested, body, "one pass over the body's {body} triangles, and no test for the {many} points off the face");
}

#[test]
fn select_other_over_a_dense_cluster_by_a_curved_face_looks_it_up_once() {
    // The points in a sketch through the cylinder's axis, seen from the front: the face under the
    // cursor is the curved side. A hole goes on it where it is picked (sketch points snap only
    // on flat faces): it is looked up once, its triangles not at all, and only the face is offered.
    let mut c = cylinder_with_cluster(json!({"plane": "XZ"}), [5.0, -10.0], "front");
    let many = c.many;
    let (list, (lookups, tested)) = c.select_other();
    assert_eq!((rows_named(&list, "Point"), rows_named(&list, "Face")), (0, 1), "the curved face alone: {list}");
    assert_eq!(lookups, 1, "the curved face is looked up once for {many} points");
    assert_eq!(tested, 0, "no triangle of a curved face is looked at");
}

#[test]
fn select_other_snaps_no_more_points_than_the_budget_allows() {
    // With the snap's budget cut to 30 point-in-triangle tests, at most 30 of the points on the
    // face are offered (each takes one test at least), and the face as picked keeps its row.
    let mut c = cylinder_with_cluster(json!({"plane": {"face": [0, 0, 20]}}), [5.0, 4.0], "top");
    let (many, body) = (c.many, c.body_tris);
    crate::dialogs::set_snap_budget(Some(30));
    let (list, (_, tested)) = c.select_other();
    crate::dialogs::set_snap_budget(None);
    assert!(many > 30, "more points than the budget");
    assert_eq!(rows_named(&list, "Face"), 1, "{list}");
    let (points, total) = (rows_named(&list, "Point"), list["total"].as_u64().unwrap() as usize);
    assert!(points >= 1 && total > points && total <= 30 + 1, "some points within the budget, and the face: {list}");
    assert!(tested <= body + 30, "{tested} triangles looked at");
}

#[test]
fn a_face_place_snaps_to_the_points_of_shown_geometry_only() {
    // Which points a Single hole snaps to, against the rule the viewport draws them by: not a
    // text outline's points (`view::sketch_point_visible`), and no linked point while Show
    // Projected is off. An id names its first link; a point whose link is gone counts as shown.
    let (h, _, _) = block_with_lettering(None);
    let mut sk = h.app.session.world_state().unwrap().sketches.first().unwrap().sketch.clone();
    let text = sk.links.iter().find(|l| l.kind == LinkKind::Text).unwrap().clone();
    let edges = sk.links.iter().find(|l| l.kind != LinkKind::Text).unwrap().clone();
    // Later links reusing the ids, with the other kind: the first link of an id is the one.
    let (mut not_text, mut now_text) = (text.clone(), edges.clone());
    not_text.kind = edges.kind;
    now_text.kind = LinkKind::Text;
    sk.links.extend([not_text, now_text]);
    let mut lost = sk.points.last().unwrap().clone();
    lost.id = "lost".into();
    lost.link = Some("gone".into());
    sk.points.push(lost);
    let linked = |l: &str| sk.points.iter().filter(|p| p.link.as_deref() == Some(l)).count();
    assert!(linked(&text.id) == 6 && linked(&edges.id) == 4, "the L's corners and the face's");
    for hide in [false, true] {
        sk.view.hide_projected = hide;
        let takes = crate::dialogs::SnapPoints::of(&sk);
        for (i, p) in sk.points.iter().enumerate() {
            let want = solidcraft_engine::view::sketch_point_visible(&sk, i) && !(hide && p.link.is_some());
            assert_eq!(takes.takes(p), want, "point {} (link {:?}), Show Projected off: {hide}", p.id, p.link);
        }
        let taken = sk.points.iter().filter(|p| takes.takes(p)).count();
        assert_eq!(taken, if hide { 1 } else { 1 + 4 + 1 }, "the origin, and unless hidden the face's corners and the lost point");
    }
}

#[test]
fn combined_recessed_point_presets_keep_partial_depth_across_units_and_legacy_defaults() {
    for recessed in [1, 2] {
        let mut h = personal_preset_harness();
        if let Kind::Hole { diameter, kind, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            diameter.clear(); // Inactive for tapped holes; Custom supplies its drill size.
            *kind = recessed;
            opts.angled = true;
            opts.tip_angle = "118 deg".into();
            opts.tapped = true;
            opts.thread = "M8x1.2".into();
            opts.thread_full_depth = false;
            opts.thread_depth = "5".into();
        }
        save_preset(&mut h, "Combined point");
        let stored = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
        assert_eq!(stored[0]["angled_bottom"], true);
        assert_eq!(stored[0]["thread_options"]["depth"], "5 mm");
        h.app.preferences.hole_presets = serde_json::from_value(stored.clone()).unwrap();
        h.app.session.doc_mut().units = "in".into();
        choose_preset(&mut h, "Select a preset");
        if let Kind::Hole { kind, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *kind = 0;
            opts.angled = false;
            opts.thread_full_depth = true;
        }
        let before = h.app.session.doc.to_json();
        choose_preset(&mut h, "Combined point");
        assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {kind,opts,..}
            if *kind==recessed && opts.angled && opts.tip_angle=="118 deg"
            && !opts.thread_full_depth && opts.thread_depth=="5 mm" && opts.thread=="M8x1.2"
            && opts.custom_thread.as_ref().is_none_or(|draft| draft.error.is_none())));
        assert_eq!(h.app.session.doc.to_json(), before, "automatic recall only changes the form");
        assert!((h.app.session.doc.eval("5 mm", solidcraft_engine::doc::expr::Kind::Length).unwrap() - 5.0).abs() < 1e-9);
        // Old recessed presets omit the additive fields: flat and Full remain their defaults.
        choose_preset(&mut h, "Select a preset");
        let mut legacy = stored.clone();
        legacy[0].as_object_mut().unwrap().remove("angled_bottom");
        legacy[0].as_object_mut().unwrap().remove("thread_options");
        h.app.preferences.hole_presets = serde_json::from_value(legacy).unwrap();
        choose_preset(&mut h, "Combined point");
        assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole {kind,opts,..}
            if *kind==recessed && !opts.angled && opts.thread_full_depth));
        choose_preset(&mut h, "Select a preset");
        let mut future = stored;
        future[0]["thread_options"]["starts"] = json!(2);
        h.app.preferences.hole_presets = serde_json::from_value(future).unwrap();
        let draft = format!("{:?}", h.app.dialog);
        choose_preset(&mut h, "Combined point");
        assert_eq!(format!("{:?}", h.app.dialog), draft, "unknown options must refuse before changing any bottom/thread field");
    }
}

fn preset_dirty_save_enabled(h: &Harness) -> bool {
    let node = h
        .output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .rev()
        .find(|(_, node)| node.label() == Some("Save changes to selected preset"))
        .map(|(_, node)| node)
        .expect("the small Save action must be present");
    !node.is_disabled()
}

fn dirty_depth(h: &mut Harness, text: &str) {
    if let Kind::Hole { depth, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *depth = text.into();
    }
    h.frames(3);
}

fn dirty_save(h: &mut Harness) {
    h.step(&json!({"click":{"handle":"dialog:hole_presets:overwrite"}})).unwrap();
}

#[test]
fn hole_presets_dirty_indicator_and_disabled_save_follow_changes_and_revert() {
    let mut h = personal_preset_harness();
    assert!(!preset_dirty_save_enabled(&h));
    dirty_depth(&mut h, "12 mm");
    save_preset(&mut h, "Workshop");
    let library = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    h.frames(8);
    assert!(!preset_dirty_save_enabled(&h), "unchanged frames must not dirty a saved preset");
    dirty_save(&mut h);
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), library);
    dirty_depth(&mut h, "13 mm");
    assert!(preset_dirty_save_enabled(&h));
    h.step(&json!({"expect":{"shown":"* Workshop"}})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap()[0]["name"], "Workshop");
    let field = crate::scenario::rect_of("dialog:widget:hole_presets.preset").unwrap();
    let save = crate::scenario::rect_of("dialog:hole_presets:overwrite").unwrap();
    assert!(save.left() >= field.right() && save.width() <= 32.0, "small save belongs right of the name");
    choose_preset(&mut h, "Select a preset");
    assert!(!preset_dirty_save_enabled(&h));
    choose_preset(&mut h, "Workshop");
    dirty_depth(&mut h, "13 mm");
    dirty_depth(&mut h, "12 mm");
    assert!(!preset_dirty_save_enabled(&h), "reverting the parameter clears dirty state without saving");
    h.step(&json!({"expect":{"not_shown":"* Workshop"}})).unwrap();
}

#[test]
fn hole_presets_dirty_overwrite_keeps_name_and_all_pointed_left_partial_parameters() {
    let mut h = personal_preset_harness();
    if let Kind::Hole { kind, depth, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
        *kind = 2;
        *depth = "12 mm".into();
        opts.angled = true;
        opts.tapped = true;
        opts.thread = "M8".into();
        opts.thread_full_depth = false;
        opts.thread_depth = "5 mm".into();
    }
    save_preset(&mut h, "Workshop");
    let document = h.app.session.doc.to_json();
    dirty_depth(&mut h, "13 mm");
    dirty_save(&mut h);
    let saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    assert_eq!(saved.as_array().unwrap().len(), 1);
    assert_eq!(saved[0]["name"], "Workshop");
    assert_eq!(saved[0]["depth"], "13 mm");
    assert_eq!(saved[0]["angled_bottom"], true);
    assert_eq!(saved[0]["thread_options"]["depth"], "5 mm");
    assert!(!preset_dirty_save_enabled(&h));
    h.step(&json!({"expect":{"hidden_handles":["dialog:hole_presets.name"],"not_shown":"* Workshop"}})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), document, "overwrite edits only the personal library");
}

#[test]
fn hole_presets_dirty_legacy_drilled_bottom_is_unchanged_after_recall_and_revert() {
    let mut h = personal_preset_harness();
    dirty_depth(&mut h, "12 mm");
    save_preset(&mut h, "Legacy");
    let mut saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    assert_eq!(saved[0]["style"], "drilled");
    saved[0].as_object_mut().unwrap().remove("angled_bottom");
    choose_preset(&mut h, "Select a preset");
    h.app.preferences.hole_presets = serde_json::from_value(saved).unwrap();
    choose_preset(&mut h, "Legacy");
    h.frames(8);
    assert!(!preset_dirty_save_enabled(&h), "legacy Drilled implies the same effective angled bottom");
    dirty_depth(&mut h, "13 mm");
    assert!(preset_dirty_save_enabled(&h));
    dirty_depth(&mut h, "12 mm");
    assert!(!preset_dirty_save_enabled(&h));
}

#[test]
fn hole_presets_dirty_invalid_expression_refuses_overwrite_and_cancel_changes_no_data() {
    let mut h = personal_preset_harness();
    dirty_depth(&mut h, "12 mm");
    save_preset(&mut h, "Workshop");
    let saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    let doc = h.app.session.doc.to_json();
    dirty_depth(&mut h, "bad length");
    assert!(preset_dirty_save_enabled(&h));
    dirty_save(&mut h);
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), saved);
    h.step(&json!({"expect":{"shown":"Complete the hole parameters using valid expressions of reasonable length."}})).unwrap();
    assert!(preset_dirty_save_enabled(&h));
    h.step(&json!({"click":{"handle":"dialog:cancel"}})).unwrap();
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), saved);
    assert_eq!(h.app.session.doc.to_json(), doc);
}

#[test]
fn hole_presets_dirty_unknown_options_are_never_dropped_by_overwrite() {
    let mut h = personal_preset_harness();
    dirty_depth(&mut h, "12 mm");
    save_preset(&mut h, "Future");
    let mut saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
    saved[0]["thread_options"]["starts"] = json!(2);
    choose_preset(&mut h, "Select a preset");
    h.app.preferences.hole_presets = serde_json::from_value(saved.clone()).unwrap();
    choose_preset(&mut h, "Future");
    dirty_depth(&mut h, "13 mm");
    assert!(preset_dirty_save_enabled(&h));
    dirty_save(&mut h);
    assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), saved);
    assert!(preset_dirty_save_enabled(&h));
    h.step(&json!({"expect":{"shown":"This preset contains options that cannot be overwritten safely."}})).unwrap();
}

#[test]
fn hole_presets_dirty_invalid_active_scalars_refuse_overwrite() {
    for invalid in ["negative depth", "point angle", "countersink angle", "partial depth"] {
        let mut h = personal_preset_harness();
        if let Kind::Hole { kind, depth, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *kind = 2;
            *depth = "12 mm".into();
            opts.tapped = true;
            opts.thread = "M8".into();
            opts.angled = true;
            opts.thread_full_depth = false;
            opts.thread_depth = "5 mm".into();
        }
        save_preset(&mut h, "Workshop");
        let saved = serde_json::to_value(&h.app.preferences.hole_presets).unwrap();
        if let Kind::Hole { depth, cs_angle, opts, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            match invalid {
                "negative depth" => *depth = "-1 mm".into(),
                "point angle" => opts.tip_angle = "bad angle".into(),
                "countersink angle" => *cs_angle = "0 deg".into(),
                _ => opts.thread_depth = "-1 mm".into(),
            }
        }
        h.frames(3);
        assert!(preset_dirty_save_enabled(&h));
        dirty_save(&mut h);
        assert_eq!(serde_json::to_value(&h.app.preferences.hole_presets).unwrap(), saved, "{invalid}");
        assert!(preset_dirty_save_enabled(&h));
    }
}
