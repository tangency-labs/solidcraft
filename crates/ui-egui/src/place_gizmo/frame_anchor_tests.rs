//! Frame Member's hot points: the section at the first line's start with a dot per anchor; a
//! click sets the anchor, the marker's drag sets `locate` or snaps to a dot; off, or with
//! unfinished values, nothing is drawn.

use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::frame::{Anchor, SectionView};

use super::*;
use crate::dialogs_frame::{ANCHORS, Member};
use crate::scenario::Harness;

/// A post: a 3D sketch line from the origin straight up, seen from the top, close enough that a
/// 40 mm tube's dots stand well apart, with Frame Member open on it (the default rect tube).
fn post() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Post path"})).unwrap();
    h.app.run("sketch.options", json!({"sketch_3d": true})).unwrap();
    h.app.run("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.step(&json!({"call": "ui.camera", "params": {"target": [0, 0, 0], "distance": 250}})).unwrap();
    h.step(&json!({"start": "frame.member"})).unwrap();
    let sketch = h.app.session.doc.find_feature("Post path").unwrap().id;
    let id = h.app.session.doc.sketch(sketch).unwrap().wires[0].id.clone();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::ExternalSketchCurve { sketch, id }];
    h.settle();
    h
}

fn member(h: &mut Harness) -> &mut Member {
    match &mut h.app.dialog.as_mut().expect("a dialog is open").kind {
        Kind::Frame(m) => m,
        k => panic!("a Frame Member dialog, not {k:?}"),
    }
}

fn locate(h: &Harness) -> Option<Value> {
    match &h.app.dialog.as_ref().expect("a dialog is open").kind {
        Kind::Frame(m) => m.locate.map(|at| json!(at)),
        k => panic!("a Frame Member dialog, not {k:?}"),
    }
}

fn index(name: &str) -> usize {
    ANCHORS.iter().position(|(n, _)| *n == name).unwrap()
}

/// The section the open dialog puts on its line, as the engine lays it out.
fn view(h: &Harness) -> SectionView {
    let d = h.app.dialog.as_ref().expect("a dialog is open");
    let Kind::Frame(m) = &d.kind else { panic!("a Frame Member dialog") };
    let p = crate::dialogs_frame::shown_params(&h.app, m, &d.inputs, &d.extra).unwrap();
    solidcraft_engine::cmd::frame::member_section_view(&h.app.session, &p).unwrap()
}

fn point(v: &SectionView, a: Anchor) -> Vec2 {
    v.anchors().into_iter().find(|(b, _)| *b == a).unwrap().1
}

fn world(p: Vec3) -> Value {
    json!({"world": [p.x, p.y, p.z]})
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1.0
}

fn drag(h: &mut Harness, to: Value) {
    h.step(&json!({"drag": [{"handle": "frame_anchor:position"}, to], "steps": 12})).unwrap();
    h.settle();
}

#[test]
fn the_dialog_order_of_anchors_is_the_engines() {
    let names: Vec<String> = Anchor::ALL.iter().map(|a| serde_json::to_value(a).unwrap().as_str().unwrap().to_string()).collect();
    let dialog: Vec<&str> = ANCHORS.iter().map(|(n, _)| *n).collect();
    assert_eq!(dialog, names);
}

/// On by default: the section's hot points and the line's marker are published; anchors on one
/// point share a dot (a tube's origin, centre and centroid), and the marker sits on the anchor in
/// use.
#[test]
fn the_hot_points_and_the_marker_are_published_on_the_first_line() {
    let mut h = post();
    for name in ["origin", "top_left", "top", "top_right", "left", "right", "bottom_left", "bottom", "bottom_right", "position"] {
        assert!(h.at(&json!({"handle": format!("frame_anchor:{name}")})).is_ok(), "frame_anchor:{name} is published");
    }
    for name in ["center", "centroid"] {
        assert!(h.at(&json!({"handle": format!("frame_anchor:{name}")})).is_err(), "{name} shares the origin's dot");
    }
    let origin = h.at(&json!({"handle": "frame_anchor:origin"})).unwrap();
    let marker = h.at(&json!({"handle": "frame_anchor:position"})).unwrap();
    assert!(near(origin, marker), "the marker sits on the origin, the default anchor: {origin:?} {marker:?}");
    let (tl, br) = (h.at(&json!({"handle": "frame_anchor:top_left"})).unwrap(), h.at(&json!({"handle": "frame_anchor:bottom_right"})).unwrap());
    assert!((tl[0] - br[0]).hypot(tl[1] - br[1]) > 60.0, "the dots stand apart: {tl:?} {br:?}");
}

/// A click on a hot point makes it the anchor and drops a located point; the section moves so
/// that point sits on the line, and the picked line stays picked.
#[test]
fn clicking_a_hot_point_sets_the_anchor_and_drops_locate() {
    let mut h = post();
    if let Some(Kind::Frame(m)) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        m.locate = Some([5.0, 5.0]);
    }
    h.settle();
    h.step(&json!({"click": {"handle": "frame_anchor:top_left"}})).unwrap();
    h.settle();
    assert_eq!(member(&mut h).anchor, index("top_left"));
    assert_eq!(locate(&h), None, "the located point is dropped");
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 1, "the click picked nothing in the view");
    let (tl, marker) = (h.at(&json!({"handle": "frame_anchor:top_left"})).unwrap(), h.at(&json!({"handle": "frame_anchor:position"})).unwrap());
    assert!(near(tl, marker), "the top left corner is on the line now: {tl:?} {marker:?}");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["anchor"], json!("top_left"), "{}", c[0].1);
    assert!(c[0].1.get("locate").is_none(), "{}", c[0].1);
}

/// Dragging the marker to a free point of the section puts that point on the line (`locate`, as
/// the engine reads it); dragging it onto a hot point takes that anchor instead.
#[test]
fn dragging_the_marker_locates_a_free_point_or_snaps_to_a_hot_point() {
    let mut h = post();
    let v = view(&h);
    // Clear of every dot: 12 mm or more from each on a 40 mm tube.
    let p = Vec2::new(v.at.x + 10.0, v.at.y + 7.0);
    drag(&mut h, world(v.world(p)));
    let [x, y] = v.locate_of(p);
    let got = locate(&h).expect("the drag located a point");
    let got: Vec<f64> = got.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
    assert!((got[0] - x).abs() < 0.1 + 1e-9 && (got[1] - y).abs() < 0.1 + 1e-9, "locate {got:?}, the point is {x}, {y}");
    let marker = h.at(&json!({"handle": "frame_anchor:position"})).unwrap();
    assert!(h.at(&json!({"handle": "frame_anchor:origin"})).is_ok_and(|o| !near(o, marker)), "the section moved off the origin");

    // Onto a dot: its anchor, and no located point.
    let v = view(&h);
    drag(&mut h, world(v.world(point(&v, Anchor::BottomRight))));
    assert_eq!(member(&mut h).anchor, index("bottom_right"));
    assert_eq!(locate(&h), None, "a snapped drag leaves no located point");
    let (br, marker) = (h.at(&json!({"handle": "frame_anchor:bottom_right"})).unwrap(), h.at(&json!({"handle": "frame_anchor:position"})).unwrap());
    assert!(near(br, marker), "the bottom right corner is on the line: {br:?} {marker:?}");
}

/// The marker drags past the section's outline too: a 40 mm tube put 50 mm off its line, the
/// line 50 mm below the tube's bottom (L8 "a 50mm de la linea").
#[test]
fn dragging_the_marker_past_the_section_puts_the_member_off_the_line() {
    let mut h = post();
    let v = view(&h);
    let bottom = point(&v, Anchor::Bottom);
    let p = Vec2::new(bottom.x, bottom.y - 50.0);
    drag(&mut h, world(v.world(p)));
    let [x, y] = v.locate_of(p);
    let got = locate(&h).expect("the drag located a point");
    let got: Vec<f64> = got.as_array().unwrap().iter().map(|n| n.as_f64().unwrap()).collect();
    assert!((got[0] - x).abs() < 0.1 + 1e-9 && (got[1] - y).abs() < 0.1 + 1e-9, "locate {got:?}, the point is {x}, {y}");
    let v = view(&h);
    assert!((point(&v, Anchor::Bottom).y - v.at.y - 50.0).abs() < 0.1 + 1e-9, "the tube's bottom is 50 mm from the line");
}

/// Offset A and B move the section from its anchor by a typed length: Bottom Middle and Offset B
/// 50 mm put the tube's bottom 50 mm above the line. A drag rewrites them, a click on a hot point
/// empties them, both empty is no located point, and a value that is not a length is refused.
#[test]
fn offset_fields_move_the_member_from_its_anchor() {
    let mut h = post();
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), "50 mm".into()];
    h.settle();
    let v = view(&h);
    assert!((point(&v, Anchor::Bottom).y - v.at.y - 50.0).abs() < 1e-9, "the tube's bottom is 50 mm from the line");
    assert!((point(&v, Anchor::Bottom).x - v.at.x).abs() < 1e-9, "and not moved sideways");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["locate"], json!([0.0, -70.0]), "{}", c[0].1);

    // A drag rewrites the offset: the line 10 mm left of the bottom middle moves the tube 10 mm right.
    let p = Vec2::new(point(&v, Anchor::Bottom).x - 10.0, point(&v, Anchor::Bottom).y - 30.0);
    drag(&mut h, world(v.world(p)));
    let off = member(&mut h).offset.clone();
    let mm = |t: &str| crate::length_input::eval(&h.app.session.doc, t, solidcraft_engine::doc::expr::Kind::Length).unwrap();
    assert!((mm(&off[0]) - 10.0).abs() < 0.1 + 1e-9 && (mm(&off[1]) - 30.0).abs() < 0.1 + 1e-9, "{off:?}");

    // A hot point empties them; both empty is no located point.
    h.step(&json!({"click": {"handle": "frame_anchor:top_left"}})).unwrap();
    h.settle();
    assert_eq!(member(&mut h).offset, [String::new(), String::new()]);
    assert_eq!(locate(&h), None);

    // Not a length: refused, naming the fields, and the member is not moved.
    member(&mut h).offset = ["abc".into(), String::new()];
    h.settle();
    let err = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(err.contains("Offset A"), "{err}");
    // The rows are labelled A and B, not the world's X and Y.
    assert!(h.at(&json!({"handle": "dialog:ui.frame_offset_a"})).is_ok() && h.at(&json!({"handle": "dialog:ui.frame_offset_b"})).is_ok());
    assert_eq!(solidcraft_ui_text::tr_label("ui.frame_offset_a"), "Offset A");
    assert_eq!(locate(&h), None);
}

/// An offset typed before the dialog can lay out a section (no line picked yet) is kept and
/// applied once a line is picked, not wiped.
#[test]
fn an_offset_typed_before_a_line_is_picked_applies_once_one_is() {
    let mut h = post();
    let line = std::mem::take(&mut h.app.dialog.as_mut().unwrap().inputs[0].items);
    h.settle();
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), "50 mm".into()];
    h.settle();
    h.app.dialog.as_mut().unwrap().inputs[0].items = line;
    h.settle();
    assert_eq!(member(&mut h).offset, [String::new(), "50 mm".to_string()], "the typed offset stays");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["locate"], json!([0.0, -70.0]), "{}", c[0].1);
}

/// A dimension changed after Offset B is typed keeps the offset as typed and moves `locate` with
/// the section: the tube's bottom stays 50 mm from the line at 60 mm high as at 40 mm.
#[test]
fn a_dimension_change_keeps_the_typed_offset() {
    let mut h = post();
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), "50 mm".into()];
    h.settle();
    let m = member(&mut h);
    m.dims[m.kind][1] = "60 mm".into();
    h.settle();
    assert_eq!(member(&mut h).offset, [String::new(), "50 mm".to_string()], "the typed offset stays");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["locate"], json!([0.0, -80.0]), "{}", c[0].1);
    let v = view(&h);
    assert!((point(&v, Anchor::Bottom).y - v.at.y - 50.0).abs() < 1e-9, "the tube's bottom is 50 mm from the line");
}

/// A design parameter a dimension names, changed while the dialog is open, lays the section out
/// again: `locate` follows the new height, the typed offset as it was.
#[test]
fn a_parameter_change_lays_the_section_out_again() {
    let mut h = post();
    h.app.run("parameters.add", json!({"name": "H", "expression": "40 mm"})).unwrap();
    let m = member(&mut h);
    m.dims[m.kind][1] = "H".into();
    m.anchor = index("bottom");
    m.offset = [String::new(), "50 mm".into()];
    h.settle();
    assert_eq!(locate(&h), Some(json!([0.0, -70.0])));
    h.app.run("parameters.change", json!({"name": "H", "expression": "60 mm"})).unwrap();
    h.settle();
    assert_eq!(locate(&h), Some(json!([0.0, -80.0])), "laid out with H = 60 mm");
    assert_eq!(member(&mut h).offset, [String::new(), "50 mm".to_string()]);
}

/// An offset whose point on the line would be past the largest profile is refused by OK, naming
/// the fields, rather than run without it.
#[test]
fn an_offset_past_the_largest_profile_is_refused() {
    let mut h = post();
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), format!("{} mm", solidcraft_engine::doc::frame::MAX_PROFILE_SIZE)];
    h.settle();
    let err = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(err.contains("Offset A") && err.contains("Offset B"), "{err}");
    assert_eq!(locate(&h), None, "no located point is left from other values");
}

/// A press in the corner of a hot point's square press area is the overlay's, not Select Other's.
#[test]
fn a_press_in_a_hot_points_corner_is_owned() {
    let h = post();
    let c = DRAWN.with(|d| d.borrow().first().copied()).expect("the hot points were drawn");
    let corner = c + vec2(REACH - 0.5, REACH - 0.5);
    assert!(c.distance(corner) > REACH, "outside the circle of reach");
    assert!(contains_pointer(&h.app, corner));
}

/// Unfinished or refused values show nothing and never panic: no line picked, a dimension that
/// is not a length, zero, negative or overflowing; a section with no size or not a number.
#[test]
fn unfinished_or_hostile_values_show_nothing() {
    let mut h = post();
    let lines = std::mem::take(&mut h.app.dialog.as_mut().unwrap().inputs[0].items);
    h.settle();
    assert!(h.at(&json!({"handle": "frame_anchor:position"})).is_err(), "no line, no section");
    h.app.dialog.as_mut().unwrap().inputs[0].items = lines;
    for bad in ["abc", "0 mm", "-3 mm", "1e999 mm", ""] {
        let m = member(&mut h);
        m.dims[m.kind][0] = bad.into();
        h.settle();
        assert!(h.at(&json!({"handle": "frame_anchor:position"})).is_err(), "a width of `{bad}` shows no section");
        assert!(h.app.dialog.is_some(), "the dialog is still open");
    }
    let v = SectionView {
        start: Vec3::new(f64::NAN, 0.0, 0.0),
        end: Vec3::new(0.0, 0.0, 1.0),
        x: Vec3::new(1.0, 0.0, 0.0),
        y: Vec3::new(0.0, 1.0, 0.0),
        regions: Vec::new(),
        at: Vec2::new(0.0, 0.0),
        origin: Vec2::new(0.0, 0.0),
        mirror: false,
    };
    assert!(laid_out(v.clone()).is_none(), "a start that is not a number");
    assert!(laid_out(SectionView { start: Vec3::new(0.0, 0.0, 0.0), ..v }).is_none(), "a section with no size");
}

/// Two groups of Frame Member (`groups`): Offset A and B and the hot points act on the group shown
/// and are kept with it; the other group's placement is left alone, and showing a group again
/// shows its own offset.
#[test]
fn offsets_and_hot_points_act_on_the_group_shown() {
    // Two posts in one 3D sketch: the one in view (group 2, shown) and one away from it (group 1).
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Post path"})).unwrap();
    h.app.run("sketch.options", json!({"sketch_3d": true})).unwrap();
    h.app.run("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})).unwrap();
    h.app.run("sketch.line3d", json!({"points": [[-150, 0, 0], [-150, 0, 300]]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.step(&json!({"call": "ui.camera", "params": {"target": [0, 0, 0], "distance": 250}})).unwrap();
    h.step(&json!({"start": "frame.member"})).unwrap();
    let sketch = h.app.session.doc.find_feature("Post path").unwrap().id;
    let wires = &h.app.session.doc.sketch(sketch).unwrap().wires;
    assert_eq!(wires.len(), 2, "two posts");
    let line = |k: usize| vec![Sel::ExternalSketchCurve { sketch, id: wires[k].id.clone() }];
    let (near, far) = (line(0), line(1));
    let d = h.app.dialog.as_mut().unwrap();
    d.inputs[0].items = far;
    let Kind::Frame(m) = &mut d.kind else { panic!("a Frame Member dialog") };
    m.add_group(&mut d.inputs);
    d.inputs[0].items = near;
    h.settle();

    // Group 2, shown: Bottom Middle and Offset B 50 mm.
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), "50 mm".into()];
    h.settle();
    let v = view(&h);
    assert!((point(&v, Anchor::Bottom).y - v.at.y - 50.0).abs() < 1e-9, "the shown group's section is laid out");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let groups = c[0].1["groups"].as_array().expect("two groups").clone();
    assert_eq!(groups[1]["locate"], json!([0.0, -70.0]), "{}", c[0].1);
    assert!(groups[0].get("locate").is_none(), "the other group is not moved: {}", c[0].1);

    // Showing group 1 shows its own (empty) offset; back on group 2 the offset is read from its locate.
    let show = |h: &mut Harness, k: usize| {
        let d = h.app.dialog.as_mut().unwrap();
        let Kind::Frame(m) = &mut d.kind else { panic!("a Frame Member dialog") };
        m.show(k, &mut d.inputs);
        h.settle();
    };
    show(&mut h, 0);
    assert_eq!(member(&mut h).offset, [String::new(), String::new()]);
    assert_eq!(locate(&h), None);
    show(&mut h, 1);
    let off = member(&mut h).offset.clone();
    let mm = |t: &str| crate::length_input::eval(&h.app.session.doc, t, solidcraft_engine::doc::expr::Kind::Length).unwrap_or(0.0);
    assert!(off[0].trim().is_empty() || mm(&off[0]).abs() < 1e-9, "{off:?}");
    assert!((mm(&off[1]) - 50.0).abs() < 1e-9, "{off:?}");

    // A hot point on the shown group drops its located point only. Located 50 mm up, the section's
    // top row sits under the toolbar at this camera: the view is centred on it so the click lands.
    let v = view(&h);
    let c = v.world(point(&v, Anchor::Center));
    h.step(&json!({"call": "ui.camera", "params": {"target": [c.x, c.y, c.z], "distance": 250}})).unwrap();
    h.settle();
    h.step(&json!({"click": {"handle": "frame_anchor:top_left"}})).unwrap();
    h.settle();
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let groups = c[0].1["groups"].as_array().expect("two groups").clone();
    assert!(groups[1].get("locate").is_none(), "{}", c[0].1);
    assert_eq!(groups[1]["anchor"], json!("top_left"), "{}", c[0].1);
    assert!(groups[0].get("locate").is_none(), "{}", c[0].1);
}
