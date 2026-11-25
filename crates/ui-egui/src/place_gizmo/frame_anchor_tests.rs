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
    let mut h = post_path();
    h.step(&json!({"start": "frame.member"})).unwrap();
    let sketch = h.app.session.doc.find_feature("Post path").unwrap().id;
    let id = h.app.session.doc.sketch(sketch).unwrap().wires[0].id.clone();
    h.app.dialog.as_mut().unwrap().inputs[0].items = vec![Sel::ExternalSketchCurve { sketch, id }];
    h.settle();
    h
}

/// The post's line alone, seen from the top, no dialog open.
fn post_path() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("sketch.create", json!({"plane": "XY", "name": "Post path"})).unwrap();
    h.app.run("sketch.options", json!({"sketch_3d": true})).unwrap();
    h.app.run("sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]})).unwrap();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.settle();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.step(&json!({"call": "ui.camera", "params": {"target": [0, 0, 0], "distance": 250}})).unwrap();
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
    solidcraft_engine::cmd::member_section_view(&h.app.session, &p).unwrap()
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

/// A length the dialog shows, in mm.
fn mm(h: &Harness, text: &str) -> f64 {
    crate::length_input::eval(&h.app.session.doc, text, solidcraft_engine::doc::expr::Kind::Length).unwrap()
}

fn offset(h: &mut Harness) -> [String; 2] {
    member(h).offset.clone()
}

/// The screen point `by` (mm, along the section's own axes) from the screen point `from`; the
/// post's section lies square to the top view, so the screen is a scaled copy of its plane.
fn moved(h: &mut Harness, v: &SectionView, from: [f64; 2], by: Vec2) -> [f64; 2] {
    let a = h.at(&world(v.world(v.at))).unwrap();
    let b = h.at(&world(v.world(Vec2::new(v.at.x + by.x, v.at.y + by.y)))).unwrap();
    [from[0] + b[0] - a[0], from[1] + b[1] - a[1]]
}

/// The section point drawn at screen point `s` (the inverse of the top view's scaled copy).
fn section_at(h: &mut Harness, v: &SectionView, s: [f64; 2]) -> Vec2 {
    let c = h.at(&world(v.world(v.at))).unwrap();
    let x = h.at(&world(v.world(Vec2::new(v.at.x + 1.0, v.at.y)))).unwrap();
    let y = h.at(&world(v.world(Vec2::new(v.at.x, v.at.y + 1.0)))).unwrap();
    let (ax, ay, bx, by) = (x[0] - c[0], x[1] - c[1], y[0] - c[0], y[1] - c[1]);
    let (dx, dy) = (s[0] - c[0], s[1] - c[1]);
    let det = ax * by - ay * bx;
    Vec2::new(v.at.x + (dx * by - dy * bx) / det, v.at.y + (ax * dy - ay * dx) / det)
}

fn pos(p: [f64; 2]) -> egui::Pos2 {
    egui::pos2(p[0] as f32, p[1] as f32)
}

/// The primary button pressed at `at` (the pointer moved there first).
fn press(h: &mut Harness, at: [f64; 2]) {
    h.frame_with(vec![egui::Event::PointerMoved(pos(at))]);
    h.frame_with(vec![egui::Event::PointerButton {
        pos: pos(at),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    }]);
}

fn move_to(h: &mut Harness, at: [f64; 2]) {
    h.frame_with(vec![egui::Event::PointerMoved(pos(at))]);
    h.frames(2);
}

fn release(h: &mut Harness, at: [f64; 2]) {
    h.frame_with(vec![egui::Event::PointerButton {
        pos: pos(at),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    h.settle();
}

fn lines(h: &Harness) -> usize {
    h.app.dialog.as_ref().expect("a dialog is open").inputs[0].items.len()
}

fn drag_handle(h: &mut Harness, name: &str, to: [f64; 2], alt: bool) {
    h.step(&json!({"drag": [{"handle": name}, to], "steps": 12, "alt": alt})).unwrap();
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
    for name in [
        "origin",
        "top_left",
        "top",
        "top_right",
        "left",
        "right",
        "bottom_left",
        "bottom",
        "bottom_right",
        "position",
        "arrow_a",
        "arrow_b",
        "rotate",
    ] {
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

/// The A and B arrows move the section along its own axes: each sets its own offset, rounded to
/// 1 mm (0.1 mm with Alt), and leaves the other as it was; the member moves with it.
#[test]
fn the_a_and_b_arrows_move_the_section_along_its_axes() {
    let mut h = post();
    member(&mut h).offset = [String::new(), "5 mm".into()];
    h.settle();
    let v = view(&h);
    let from = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(20.3, 0.0));
    drag_handle(&mut h, "frame_anchor:arrow_a", to, false);
    assert_eq!(offset(&mut h), ["20 mm".to_string(), "5 mm".to_string()], "A rounded to 1 mm, B kept");
    let v = view(&h);
    let o = point(&v, Anchor::Origin);
    assert!(
        (o.x - v.at.x - 20.0).abs() < 1e-9 && (o.y - v.at.y - 5.0).abs() < 1e-9,
        "the tube's centre is 20 mm along A, 5 mm along B: {o:?} {:?}",
        v.at
    );

    let from = h.at(&json!({"handle": "frame_anchor:arrow_b"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(0.0, -12.4));
    drag_handle(&mut h, "frame_anchor:arrow_b", to, false);
    assert_eq!(offset(&mut h), ["20 mm".to_string(), "-7 mm".to_string()], "B rounded to 1 mm, A kept");
    let v = view(&h);
    let o = point(&v, Anchor::Origin);
    assert!((o.x - v.at.x - 20.0).abs() < 1e-9 && (o.y - v.at.y + 7.0).abs() < 1e-9, "{o:?} {:?}", v.at);

    // With Alt, 0.1 mm steps.
    let from = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(4.34, 0.0));
    drag_handle(&mut h, "frame_anchor:arrow_a", to, true);
    let a = offset(&mut h);
    let a = mm(&h, &a[0]);
    assert!((a - 24.3).abs() < 1e-9, "A in 0.1 mm steps: {a}");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert!(c[0].1.get("locate").is_some(), "{}", c[0].1);
}

/// The arc turns the section about the point on the line in 15° steps, writing the dialog's
/// Angle; the anchored point stays on the line.
#[test]
fn the_arc_turns_the_section_about_the_point_on_the_line_in_15_degree_steps() {
    let mut h = post();
    member(&mut h).anchor = index("bottom_left");
    h.settle();
    let before = view(&h);
    let grip = h.at(&json!({"handle": "frame_anchor:rotate"})).unwrap();
    let g = section_at(&mut h, &before, grip);
    let (r, a) = ((g.y - before.at.y).hypot(g.x - before.at.x), (g.y - before.at.y).atan2(g.x - before.at.x));
    // The grip carried 88° on about the point on the line.
    let turned = Vec2::new(before.at.x + r * (a + 88f64.to_radians()).cos(), before.at.y + r * (a + 88f64.to_radians()).sin());
    let to = h.at(&world(before.world(turned))).unwrap();
    drag_handle(&mut h, "frame_anchor:rotate", to, false);
    assert_eq!(member(&mut h).angle, "90 deg", "88° snapped to 15° steps");
    let after = view(&h);
    let gap = |p: Vec3, q: Vec3| (p - q).len();
    assert!(gap(after.start, before.start) < 1e-9, "the same line");
    assert!(gap(after.world(point(&after, Anchor::BottomLeft)), before.start) < 1e-9, "the bottom left corner stays on the line");
    assert!(after.x.dot(before.y) > 1.0 - 1e-9, "the section turned a quarter, counter-clockwise: {:?} {:?}", after.x, before.y);
    assert!(gap(after.world(point(&after, Anchor::TopRight)), before.world(point(&before, Anchor::TopRight))) > 10.0, "the far corner moved");
}

/// Offset A and B are the user's: a later change of the profile's size, the Anchor, Mirror or
/// Angle keeps them and moves the member to keep them (S10).
#[test]
fn a_typed_offset_is_kept_when_the_size_anchor_mirror_or_angle_changes() {
    let mut h = post();
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), "50 mm".into()];
    h.settle();
    let m = member(&mut h);
    m.dims[m.kind][1] = "60 mm".into();
    h.settle();
    assert_eq!(offset(&mut h), [String::new(), "50 mm".to_string()], "the height changed, the offset stays");
    let v = view(&h);
    assert!((point(&v, Anchor::Top).y - point(&v, Anchor::Bottom).y - 60.0).abs() < 1e-9, "the tube is 60 mm high now");
    assert!((point(&v, Anchor::Bottom).y - v.at.y - 50.0).abs() < 1e-9, "the tube's bottom stays 50 mm from the line");

    member(&mut h).anchor = index("top");
    h.settle();
    assert_eq!(offset(&mut h), [String::new(), "50 mm".to_string()], "the anchor changed, the offset stays");
    let v = view(&h);
    assert!((point(&v, Anchor::Top).y - v.at.y - 50.0).abs() < 1e-9, "now the top is 50 mm from the line");

    member(&mut h).anchor = index("left");
    member(&mut h).offset = ["10 mm".into(), "50 mm".into()];
    h.settle();
    member(&mut h).mirror = true;
    h.settle();
    assert_eq!(offset(&mut h), ["10 mm".to_string(), "50 mm".to_string()], "mirrored, the offset stays");
    let v = view(&h);
    let left = point(&v, Anchor::Left);
    assert!((left.x - v.at.x - 10.0).abs() < 1e-9 && (left.y - v.at.y - 50.0).abs() < 1e-9, "{left:?} {:?}", v.at);

    member(&mut h).angle = "30 deg".into();
    h.settle();
    assert_eq!(offset(&mut h), ["10 mm".to_string(), "50 mm".to_string()], "turned, the offset stays");
    let v = view(&h);
    let left = point(&v, Anchor::Left);
    assert!((left.x - v.at.x - 10.0).abs() < 1e-9 && (left.y - v.at.y - 50.0).abs() < 1e-9, "{left:?} {:?}", v.at);
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["locate"], json!(v.locate_of(v.at)), "{}", c[0].1);
}

/// Typed offsets that cannot be applied (the point on the line beyond the largest profile) are
/// refused at OK, naming the fields, never run with the `locate` they had before.
#[test]
fn ok_is_refused_when_the_typed_offsets_cannot_be_applied() {
    let mut h = post();
    member(&mut h).anchor = index("left");
    member(&mut h).offset = ["9975 mm".into(), String::new()];
    h.settle();
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["locate"], json!([-9995.0, 0.0]), "{}", c[0].1);
    // Wider, the left side is 10 mm further out: 10005 mm, past the largest profile.
    let m = member(&mut h);
    m.dims[m.kind][0] = "60 mm".into();
    h.settle();
    assert_eq!(offset(&mut h)[0], "9975 mm", "the offset stays as typed");
    let err = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(err.contains("Offset A") && err.contains("Offset B"), "{err}");
    assert!(h.at(&json!({"handle": "frame_anchor:position"})).is_ok(), "the section stays in view");
    // Typed straight past the limit, refused too.
    let m = member(&mut h);
    m.dims[m.kind][0] = "40 mm".into();
    m.offset = [format!("{} mm", solidcraft_engine::doc::frame::MAX_PROFILE_SIZE), String::new()];
    h.settle();
    let err = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(err.contains("Offset A"), "{err}");
}

/// A half-typed offset (not a length yet) leaves the section and its hot points where they were.
#[test]
fn a_half_typed_offset_keeps_the_hot_points() {
    let mut h = post();
    member(&mut h).anchor = index("bottom");
    member(&mut h).offset = [String::new(), "50 mm".into()];
    h.settle();
    let before = h.at(&json!({"handle": "frame_anchor:bottom"})).unwrap();
    member(&mut h).offset[1] = "5 +".into();
    h.settle();
    for name in ["bottom", "position", "arrow_a", "arrow_b", "rotate"] {
        assert!(h.at(&json!({"handle": format!("frame_anchor:{name}")})).is_ok(), "frame_anchor:{name} stays published");
    }
    let after = h.at(&json!({"handle": "frame_anchor:bottom"})).unwrap();
    assert!(near(before, after), "the last valid section stays: {before:?} {after:?}");
    let err = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap_err();
    assert!(err.contains("Offset A"), "{err}");
}

/// An arrow's label is its letter alone while its offset is 0 or empty, and the letter with the
/// offset (design unit, 0.1 mm shown) once it is not, live while the arrow drags.
#[test]
fn an_arrow_shows_its_offset_once_it_is_not_zero() {
    let mut h = post();
    assert!(h.widget("A").is_some() && h.widget("B").is_some(), "both empty: the letters alone");
    member(&mut h).offset = ["5 mm".into(), "0 mm".into()];
    h.settle();
    assert!(h.widget("A 5.0 mm").is_some(), "A not 0: its value");
    assert!(h.widget("B").is_some() && h.widget("B 0.0 mm").is_none(), "B is 0: the letter alone");
    let v = view(&h);
    let from = h.at(&json!({"handle": "frame_anchor:arrow_b"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(0.0, 12.4));
    press(&mut h, from);
    move_to(&mut h, [from[0] + (to[0] - from[0]) / 2.0, from[1] + (to[1] - from[1]) / 2.0]);
    move_to(&mut h, to);
    assert!(h.widget("B 12.0 mm").is_some(), "live while it drags");
    release(&mut h, to);
    assert!(h.widget("B 12.0 mm").is_some(), "and after");
}

/// A press that misses an arrow but lands next to it (or on the section) belongs to the hot
/// points: dragging from there starts no box selection and the dialog keeps its Lines. Away from
/// the section a drag still boxes.
#[test]
fn a_press_next_to_an_arrow_keeps_the_lines() {
    let mut h = post();
    let grip = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let right = h.at(&json!({"handle": "frame_anchor:right"})).unwrap();
    let len = (grip[0] - right[0]).hypot(grip[1] - right[1]);
    let (dx, dy) = ((grip[0] - right[0]) / len, (grip[1] - right[1]) / len);
    // Beside the arrow, across it: off the arrow, inside the margin round it.
    let beside = [grip[0] - dy * 20.0, grip[1] + dx * 20.0];
    let away = [beside[0] + dx * 80.0 - dy * 60.0, beside[1] + dy * 80.0 + dx * 60.0];
    let before = offset(&mut h);
    press(&mut h, beside);
    move_to(&mut h, away);
    assert!(h.app.viewport.boxsel.is_none(), "no box selection from beside the arrow");
    release(&mut h, away);
    assert_eq!(lines(&h), 1, "the Lines are kept");
    assert_eq!(offset(&mut h), before, "and nothing moved");
    assert!(h.at(&json!({"handle": "frame_anchor:position"})).is_ok(), "the section is still drawn");

    // On the section's outline, between the dots: the same.
    let (tl, tr) = (h.at(&json!({"handle": "frame_anchor:top_left"})).unwrap(), h.at(&json!({"handle": "frame_anchor:top"})).unwrap());
    let on_edge = [(tl[0] + tr[0]) / 2.0, (tl[1] + tr[1]) / 2.0];
    press(&mut h, on_edge);
    move_to(&mut h, [on_edge[0] - 90.0, on_edge[1] - 90.0]);
    assert!(h.app.viewport.boxsel.is_none(), "no box selection from the section");
    release(&mut h, [on_edge[0] - 90.0, on_edge[1] - 90.0]);
    assert_eq!(lines(&h), 1, "the Lines are kept");

    // Far from the section the view boxes as before.
    let far = [grip[0] + dx * 250.0 - dy * 150.0, grip[1] + dy * 250.0 + dx * 150.0];
    press(&mut h, far);
    move_to(&mut h, [far[0] + 40.0, far[1] + 40.0]);
    assert!(h.app.viewport.boxsel.is_some(), "a box selection away from the section");
    release(&mut h, [far[0] + 40.0, far[1] + 40.0]);
}

/// A press on an arrow or the arc that does not move a step changes nothing: an off-step typed
/// value stays, and a step is added to it.
#[test]
fn a_press_without_a_step_keeps_typed_values() {
    let mut h = post();
    member(&mut h).angle = "7 deg".into();
    member(&mut h).offset = ["20.3 mm".into(), String::new()];
    h.settle();
    for handle in ["frame_anchor:arrow_a", "frame_anchor:rotate"] {
        let at = h.at(&json!({"handle": handle})).unwrap();
        press(&mut h, at);
        // Held still past the click time, and nudged less than a step: a drag that moves nothing.
        h.frames(60);
        move_to(&mut h, [at[0] + 2.0, at[1] + 1.0]);
        release(&mut h, [at[0] + 2.0, at[1] + 1.0]);
        assert_eq!(member(&mut h).angle, "7 deg", "{handle}: the Angle stays");
        assert_eq!(offset(&mut h), ["20.3 mm".to_string(), String::new()], "{handle}: the offsets stay");
    }
    // One step along A: added to 20.3.
    let v = view(&h);
    let from = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(1.2, 0.0));
    drag_handle(&mut h, "frame_anchor:arrow_a", to, false);
    assert_eq!(offset(&mut h), ["21.3 mm".to_string(), String::new()]);
}

/// An arrow writes its own offset only: the other one's text, an expression here, stays as typed.
#[test]
fn an_arrow_leaves_the_other_offsets_text_alone() {
    let mut h = post();
    member(&mut h).anchor = index("top_right");
    let m = member(&mut h);
    m.dims[m.kind][0] = "40.3 mm".into();
    m.dims[m.kind][1] = "33.7 mm".into();
    m.offset = [String::new(), "10 mm + 0.37 mm".into()];
    h.settle();
    let v = view(&h);
    let from = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(17.0, 0.0));
    h.step(&json!({"drag": [from, to], "steps": 30})).unwrap();
    h.settle();
    assert_eq!(offset(&mut h), ["17 mm".to_string(), "10 mm + 0.37 mm".to_string()]);
    let v = view(&h);
    let tr = point(&v, Anchor::TopRight);
    assert!((tr.x - v.at.x - 17.0).abs() < 1e-9 && (tr.y - v.at.y - 10.37).abs() < 1e-9, "{tr:?} {:?}", v.at);
}

/// Edit Feature on a member placed by its anchor alone shows Offset A and B empty, and OK with
/// nothing changed adds no `locate`.
#[test]
fn an_edit_of_an_unlocated_member_shows_empty_offsets() {
    let mut h = post_path();
    let tube = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3});
    h.app.run("frame.member", json!({"sketch": "Post path", "profile": tube, "anchor": "bottom"})).unwrap();
    h.settle();
    let f = h.app.session.doc.features.last().unwrap().id;
    h.app.edit_feature(f);
    h.settle();
    assert_eq!(member(&mut h).anchor, index("bottom"));
    assert_eq!(offset(&mut h), [String::new(), String::new()], "no located point: no offsets");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let p = if c[0].0 == "timeline.redefine" { &c[0].1["params"] } else { &c[0].1 };
    assert!(p.get("locate").is_none(), "{}", c[0].1);
}

/// A drag that goes a step out and comes back to its press puts back the value as typed at the
/// press, for an arrow ("20.3 mm") and for the arc ("7 deg"), not the last step it passed.
#[test]
fn a_drag_out_a_step_and_back_restores_the_press_value() {
    let mut h = post();
    member(&mut h).angle = "7 deg".into();
    member(&mut h).offset = ["20.3 mm".into(), String::new()];
    h.settle();
    let v = view(&h);
    let from = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let out = moved(&mut h, &v, from, Vec2::new(2.0, 0.0));
    press(&mut h, from);
    move_to(&mut h, [(from[0] + out[0]) / 2.0, (from[1] + out[1]) / 2.0]);
    move_to(&mut h, out);
    assert_eq!(offset(&mut h)[0], "22.3 mm", "two steps out");
    move_to(&mut h, from);
    assert_eq!(offset(&mut h)[0], "20.3 mm", "back at the press: as typed");
    assert!(h.widget("A 20.3 mm").is_some(), "the label says so");
    release(&mut h, from);
    assert_eq!(offset(&mut h), ["20.3 mm".to_string(), String::new()]);
    let v = view(&h);
    let o = point(&v, Anchor::Origin);
    assert!((o.x - v.at.x - 20.3).abs() < 1e-9, "the member is where the label said: {o:?} {:?}", v.at);

    let before = view(&h);
    let grip = h.at(&json!({"handle": "frame_anchor:rotate"})).unwrap();
    let g = section_at(&mut h, &before, grip);
    let (r, a) = ((g.y - before.at.y).hypot(g.x - before.at.x), (g.y - before.at.y).atan2(g.x - before.at.x));
    let turned = |deg: f64| Vec2::new(before.at.x + r * (a + deg.to_radians()).cos(), before.at.y + r * (a + deg.to_radians()).sin());
    let (half, out) = (h.at(&world(before.world(turned(9.0)))).unwrap(), h.at(&world(before.world(turned(18.0)))).unwrap());
    press(&mut h, grip);
    move_to(&mut h, half);
    move_to(&mut h, out);
    assert_eq!(member(&mut h).angle, "22 deg", "a step out");
    move_to(&mut h, grip);
    assert_eq!(member(&mut h).angle, "7 deg", "back at the press: as typed");
    release(&mut h, grip);
    assert_eq!(member(&mut h).angle, "7 deg");
}

/// The area the hot points own goes with the section: once nothing is drawn (Lines emptied, or
/// the dialog closed and opened again with no lines) the view picks and box selects there again.
#[test]
fn the_owned_area_goes_with_the_section() {
    let mut h = post();
    let (tl, top) = (h.at(&json!({"handle": "frame_anchor:top_left"})).unwrap(), h.at(&json!({"handle": "frame_anchor:top"})).unwrap());
    let on_edge = [(tl[0] + top[0]) / 2.0, (tl[1] + top[1]) / 2.0];
    let to = [on_edge[0] - 90.0, on_edge[1] - 90.0];
    assert!(contains_pointer(&h.app, pos(on_edge)), "the section's area is owned while it is drawn");
    let line = std::mem::take(&mut h.app.dialog.as_mut().unwrap().inputs[0].items);
    h.settle();
    assert!(!contains_pointer(&h.app, pos(on_edge)), "no section: nothing owned");
    press(&mut h, on_edge);
    move_to(&mut h, to);
    assert!(h.app.viewport.boxsel.is_some(), "a box selection from where the section was");
    release(&mut h, to);

    h.app.dialog.as_mut().unwrap().inputs[0].items = line;
    h.settle();
    assert!(contains_pointer(&h.app, pos(on_edge)), "drawn again");
    crate::dialogs::cancel(&mut h.app);
    h.settle();
    h.step(&json!({"start": "frame.member"})).unwrap();
    h.settle();
    assert_eq!(lines(&h), 0, "opened again with no lines");
    assert!(!contains_pointer(&h.app, pos(on_edge)), "nothing drawn: nothing owned");
    press(&mut h, on_edge);
    move_to(&mut h, to);
    assert!(h.app.viewport.boxsel.is_some(), "a box selection from where the section was");
    release(&mut h, to);
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
/// Seen too small for dots, arrows or arc, the marker alone is drawn and no area round the
/// section is owned, so only the marker's own square takes the corner.
#[test]
fn a_press_in_a_hot_points_corner_is_owned() {
    let mut h = post();
    h.step(&json!({"call": "ui.camera", "params": {"target": [0, 0, 0], "distance": 20000}})).unwrap();
    h.settle();
    assert!(OWNED.with(|o| o.borrow().is_empty()), "a tiny section owns no area round it");
    let c = DRAWN.with(|d| d.borrow().first().copied()).expect("the marker was drawn");
    let corner = c + vec2(REACH - 0.5, REACH - 0.5);
    assert!(c.distance(corner) > REACH, "outside the circle of reach");
    assert!(contains_pointer(&h.app, corner));
}

/// Unfinished or refused values show nothing and never panic: no line picked, a dimension that
/// is not a length, zero, negative or overflowing; a section with no size or not a number.
#[test]
fn unfinished_or_hostile_values_show_nothing() {
    let mut h = post();
    let handles = ["position", "arrow_a", "arrow_b", "rotate"];
    let lines = std::mem::take(&mut h.app.dialog.as_mut().unwrap().inputs[0].items);
    h.settle();
    for name in handles {
        assert!(h.at(&json!({"handle": format!("frame_anchor:{name}")})).is_err(), "no line, no {name}");
    }
    h.app.dialog.as_mut().unwrap().inputs[0].items = lines;
    for bad in ["abc", "0 mm", "-3 mm", "1e999 mm", ""] {
        let m = member(&mut h);
        m.dims[m.kind][0] = bad.into();
        h.settle();
        for name in handles {
            assert!(h.at(&json!({"handle": format!("frame_anchor:{name}")})).is_err(), "a width of `{bad}` shows no {name}");
        }
        assert!(h.app.dialog.is_some(), "the dialog is still open");
    }
    // A hostile angle or offset: nothing drawn from it, no panic.
    let m = member(&mut h);
    m.dims[m.kind][0] = "40 mm".into();
    for (angle, off) in [("1e999 deg", ""), ("abc", ""), ("", "1e999 mm"), ("", "nan")] {
        let m = member(&mut h);
        m.angle = angle.into();
        m.offset = [off.into(), String::new()];
        h.settle();
        assert!(h.app.dialog.is_some(), "the dialog is still open");
    }
    // Too small on screen: no dots, arrows or arc.
    let m = member(&mut h);
    m.angle = String::new();
    m.offset = Default::default();
    h.step(&json!({"call": "ui.camera", "params": {"target": [0, 0, 0], "distance": 20000}})).unwrap();
    h.settle();
    for name in ["origin", "arrow_a", "arrow_b", "rotate"] {
        assert!(h.at(&json!({"handle": format!("frame_anchor:{name}")})).is_err(), "a tiny section shows no {name}");
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

/// Two posts in one 3D sketch with Frame Member open on two groups: group 1 the post away from
/// the view, group 2 (shown) the post at the origin, seen from the top as in `post`.
fn two_posts() -> Harness {
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
    h
}

/// With two groups, the A arrow moves the section of the group shown only: its Offset A and
/// `locate` change, the other group keeps no located point, and showing the other group and back
/// shows each group's own offset.
#[test]
fn the_arrows_act_on_the_group_shown() {
    let mut h = two_posts();
    let v = view(&h);
    let from = h.at(&json!({"handle": "frame_anchor:arrow_a"})).unwrap();
    let to = moved(&mut h, &v, from, Vec2::new(20.3, 0.0));
    drag_handle(&mut h, "frame_anchor:arrow_a", to, false);
    assert_eq!(offset(&mut h)[0], "20 mm", "A rounded to 1 mm on the shown group");
    let c = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let groups = c[0].1["groups"].as_array().expect("two groups").clone();
    assert!(groups[1].get("locate").is_some(), "the shown group is located: {}", c[0].1);
    assert!(groups[0].get("locate").is_none(), "the other group is not moved: {}", c[0].1);
    let v = view(&h);
    let o = point(&v, Anchor::Origin);
    assert!((o.x - v.at.x - 20.0).abs() < 1e-9, "the shown group's tube is 20 mm along A: {o:?} {:?}", v.at);

    let show = |h: &mut Harness, k: usize| {
        let d = h.app.dialog.as_mut().unwrap();
        let Kind::Frame(m) = &mut d.kind else { panic!("a Frame Member dialog") };
        m.show(k, &mut d.inputs);
        h.settle();
    };
    show(&mut h, 0);
    assert_eq!(member(&mut h).offset, [String::new(), String::new()], "group 1 has no offset");
    show(&mut h, 1);
    assert_eq!(offset(&mut h)[0], "20 mm", "group 2's offset is read back from its locate");
}
