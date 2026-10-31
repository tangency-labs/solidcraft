//! The hole markers (#6818): one per placed hole, each the shared marker with its press area and
//! its grab offset; a click on one takes the hole out; the depth arrow stands clear of them.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use crate::scenario::{Harness, handle_at};

/// A 40 × 30 × 20 block and a second one beside it, seen from the top, with holes moving (and
/// not snapping, so a place is exactly where it is let go).
fn blocks() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    crate::prefs::set(&mut h.app, &json!({"hole_moves": true})).unwrap();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 20, "corner": [50, 0, 0], "body_name": "Other"})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

/// Where a world point is on screen.
fn screen(h: &mut Harness, p: Vec3) -> [f64; 2] {
    h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap()
}

/// The face places the open dialog holds.
fn places(h: &Harness) -> Vec<Vec3> {
    let d = h.app.dialog.as_ref().expect("a dialog is open");
    d.inputs[0]
        .items
        .iter()
        .map(|x| match x {
            Sel::Face { point, .. } => *point,
            other => panic!("a face place, not {other:?}"),
        })
        .collect()
}

#[test]
fn a_press_off_the_centre_keeps_its_gap_and_the_hole_lands_short_of_the_pointer() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let start = Vec3::new(10.0, 10.0, 20.0);
    let at = screen(&mut h, start);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let c = handle_at("hole:0").expect("the placed hole has a marker");
    // Grabbed 6 pt right and 4 pt below the centre (7.2 pt off it, inside the marker's reach), and
    // let go with the pointer the same gap from `to`: the hole lands on `to`, not under the
    // pointer (about 0.9 mm off here) nor where the drag only started, a few points on.
    let to = Vec3::new(30.0, 20.0, 20.0);
    let ts = screen(&mut h, to);
    h.step(&json!({"drag": [[f64::from(c.x) + 6.0, f64::from(c.y) + 4.0], [ts[0] + 6.0, ts[1] + 4.0]], "steps": 12})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [p] if p.dist(to) < 0.3), "kept the gap: {p:?}");
}

#[test]
fn each_placed_hole_has_its_own_marker_and_a_drag_moves_only_its_hole() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let first = Vec3::new(10.0, 10.0, 20.0);
    let second = Vec3::new(60.0, 15.0, 20.0);
    let at = screen(&mut h, first);
    h.step(&json!({"click": at})).unwrap();
    let at = screen(&mut h, second);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert_eq!(places(&h).len(), 2, "{:?}", places(&h));
    let (Some(a), Some(b)) = (handle_at("hole:0"), handle_at("hole:1")) else { panic!("two markers") };
    assert!(a.distance(b) > 50.0, "one marker per hole: {a:?} {b:?}");
    assert!(handle_at("hole:2").is_none());
    // The second hole dragged across its block: the first stays.
    let to = Vec3::new(55.0, 25.0, 20.0);
    let ts = screen(&mut h, to);
    h.step(&json!({"drag": [{"handle": "hole:1"}, ts], "steps": 12})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [a, b] if a.dist(first) < 1e-4 && b.dist(to) < 0.3), "{p:?}");
}

#[test]
fn a_click_within_the_markers_reach_takes_the_hole_out() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    // A hole narrower than the click's 14 pt: a pick there that missed the marker would move the
    // hole (a pick within the hole's radius takes it out too), so only the marker takes it out.
    if let Some(crate::dialogs::Kind::Hole { diameter, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *diameter = "0.5 mm".into();
    }
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let c = handle_at("hole:0").expect("a marker");
    // 14 pt from the centre: on the marker's arm, past the old handle's 9 pt.
    h.step(&json!({"click": [f64::from(c.x) + 14.0, f64::from(c.y)]})).unwrap();
    h.frames(2);
    assert!(places(&h).is_empty(), "{:?}", places(&h));
    assert!(handle_at("hole:0").is_none(), "no hole, no marker");
}

#[test]
fn the_depth_arrow_stands_clear_of_the_marker_only_while_holes_move() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let d = h.app.dialog.clone().expect("a dialog");
    assert!(crate::place_gizmo::arrow_clearance(&h.app, &d) > 0.0, "the arrow starts past the marker");
    crate::prefs::set(&mut h.app, &json!({"hole_moves": false})).unwrap();
    assert_eq!(crate::place_gizmo::arrow_clearance(&h.app, &d), 0.0, "no marker, no clearance");
}

#[test]
fn the_markers_follow_the_holes_own_setting_not_the_shared_one() {
    let mut h = blocks();
    crate::prefs::set(&mut h.app, &json!({"hole_moves": false, "placement_handles": true})).unwrap();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert_eq!(places(&h).len(), 1);
    assert!(handle_at("hole:0").is_none(), "Placement handles alone show no hole marker");
    crate::prefs::set(&mut h.app, &json!({"hole_moves": true, "placement_handles": false})).unwrap();
    h.frames(2);
    assert!(handle_at("hole:0").is_some(), "Move holes by clicking or dragging alone shows it");
}

#[test]
fn a_hole_whose_face_is_gone_keeps_its_marker_through_the_restored_direction() {
    // Edit Feature restores a hole whose face no longer resolves as a bare place with the
    // `direction` it had (dialogs.rs); the marker stands there all the same.
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert!(handle_at("hole:0").is_some());
    {
        let d = h.app.dialog.as_mut().expect("a dialog");
        d.inputs[0].items = vec![Sel::Face { body: String::new(), index: 0, point: Vec3::new(10.0, 10.0, 20.0) }];
        d.extra.insert("direction".into(), crate::place_gizmo::pt(Vec3::new(0.0, 0.0, 1.0)));
    }
    h.frames(2);
    assert!(handle_at("hole:0").is_some(), "the marker stays with the place");
    let d = h.app.dialog.as_mut().expect("a dialog");
    d.extra.remove("direction");
    h.frames(2);
    assert!(handle_at("hole:0").is_none(), "no face and no direction: nowhere to stand");
}

/// The open Hole dialog's reference distances, as typed.
fn distances(h: &Harness) -> [String; 2] {
    match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(crate::dialogs::Kind::Hole { opts, .. }) => opts.refs.distance.clone(),
        _ => panic!("the hole dialog is open"),
    }
}

/// Reference distance `k` as a length (mm); a measured one carries floating-point noise in its text.
fn distance(h: &Harness, k: usize) -> f64 {
    h.app.session.doc.eval(&distances(h)[k], solidcraft_engine::doc::expr::Kind::Length).unwrap_or(f64::NAN)
}

/// A hole at (10, 10) on the first block's top, held by the block's left edge (x = 0) at 10 mm.
fn held_by_the_left_edge() -> Harness {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Reference 1"}})).unwrap();
    let left = screen(&mut h, Vec3::new(0.0, 15.0, 20.0));
    h.step(&json!({"click": left})).unwrap();
    h.settle();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(Vec3::new(10.0, 10.0, 20.0)) < 0.3), "{:?}", places(&h));
    assert!((distance(&h, 0) - 10.0).abs() < 1e-6, "measured from the pick: {:?}", distances(&h));
    h
}

#[test]
fn one_reference_edge_holds_the_hole_to_a_line_the_marker_drags_along() {
    let mut h = held_by_the_left_edge();
    // The next reference input is the active one now; the marker still moves the hole.
    assert!(handle_at("hole:0").is_some(), "the held hole keeps its marker");
    let to = screen(&mut h, Vec3::new(25.0, 22.0, 20.0));
    h.step(&json!({"drag": [{"handle": "hole:0"}, to], "steps": 12})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [p] if (p.x - 10.0).abs() < 1e-6 && (p.y - 22.0).abs() < 0.3), "along the edge, 10 mm from it: {p:?}");
    assert!((distance(&h, 0) - 10.0).abs() < 1e-6, "the distance is kept, not measured again: {:?}", distances(&h));
}

#[test]
fn a_typed_distance_survives_a_drag_along_the_edge() {
    let mut h = held_by_the_left_edge();
    h.step(&json!({"click": {"handle": "dialog:Distance 1"}})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "12"})).unwrap();
    h.frames(3);
    assert!(matches!(places(&h).as_slice(), [p] if (p.x - 12.0).abs() < 1e-6), "typed: the hole goes to 12 mm: {:?}", places(&h));
    let to = screen(&mut h, Vec3::new(30.0, 24.0, 20.0));
    h.step(&json!({"drag": [{"handle": "hole:0"}, to], "steps": 12})).unwrap();
    h.settle();
    assert_eq!(distances(&h)[0].trim(), "12", "the typed text is left alone by the drag");
    assert!(matches!(places(&h).as_slice(), [p] if (p.x - 12.0).abs() < 1e-6 && (p.y - 24.0).abs() < 0.3), "{:?}", places(&h));
}

#[test]
fn a_pick_elsewhere_on_the_face_keeps_a_held_hole_on_its_line() {
    let mut h = held_by_the_left_edge();
    h.step(&json!({"click": {"handle": "dialog:Face"}})).unwrap();
    let at = screen(&mut h, Vec3::new(30.0, 25.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [p] if (p.x - 10.0).abs() < 1e-6 && (p.y - 25.0).abs() < 0.3), "moved along the edge only: {p:?}");
}

#[test]
fn two_reference_edges_free_the_hole_again_and_a_drag_sets_both_distances() {
    let mut h = held_by_the_left_edge();
    h.step(&json!({"click": {"handle": "dialog:Reference 2"}})).unwrap();
    let front = screen(&mut h, Vec3::new(20.0, 0.0, 20.0));
    h.step(&json!({"click": front})).unwrap();
    h.settle();
    assert!((distance(&h, 1) - 10.0).abs() < 1e-4, "{:?}", distances(&h));
    let to = screen(&mut h, Vec3::new(25.0, 22.0, 20.0));
    h.step(&json!({"drag": [{"handle": "hole:0"}, to], "steps": 12})).unwrap();
    h.settle();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(Vec3::new(25.0, 22.0, 20.0)) < 0.3), "free again: {:?}", places(&h));
    assert!((distance(&h, 0) - 25.0).abs() < 0.3 && (distance(&h, 1) - 22.0).abs() < 0.3, "both distances follow: {:?}", distances(&h));
}

#[test]
fn a_second_hole_on_another_face_moves_freely_while_the_first_is_held_by_its_edge() {
    // The reference edges place the first hole only: a hole on the other block keeps its own
    // face and goes where it is dragged, not onto the first hole's line.
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let first = Vec3::new(10.0, 10.0, 20.0);
    let at = screen(&mut h, first);
    h.step(&json!({"click": at})).unwrap();
    let at = screen(&mut h, Vec3::new(60.0, 15.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.step(&json!({"click": {"handle": "dialog:Reference 1"}})).unwrap();
    let left = screen(&mut h, Vec3::new(0.0, 15.0, 20.0));
    h.step(&json!({"click": left})).unwrap();
    h.settle();
    assert!((distance(&h, 0) - 10.0).abs() < 1e-6, "measured for the first hole: {:?}", distances(&h));
    let to = Vec3::new(65.0, 25.0, 20.0);
    let ts = screen(&mut h, to);
    h.step(&json!({"drag": [{"handle": "hole:1"}, ts], "steps": 12})).unwrap();
    h.settle();
    let p = places(&h);
    assert!(matches!(p.as_slice(), [a, b] if a.dist(first) < 1e-4 && b.dist(to) < 0.3), "the second hole went where it was dragged: {p:?}");
    let d = h.app.dialog.as_ref().expect("a dialog is open");
    assert!(matches!(&d.inputs[0].items[1], Sel::Face { body, .. } if body == "Other"), "on its own block: {:?}", d.inputs[0].items);
    assert!((distance(&h, 0) - 10.0).abs() < 1e-6, "the first hole's distance is unchanged: {:?}", distances(&h));
}

#[test]
fn the_hole_after_one_taken_out_does_not_take_over_its_hover() {
    // A click takes out the first hole under the pointer; the second takes its index, not its
    // marker's state: its marker is not drawn hot on the click's frame (the press and hover are
    // the first one's) and does not fade from there after it.
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    let at = screen(&mut h, Vec3::new(60.0, 15.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let c = handle_at("hole:0").expect("the first hole's marker");
    h.step(&json!({"move": [f64::from(c.x), f64::from(c.y)]})).unwrap();
    h.frames(30);
    assert!(crate::scenario::count_of("hole:0:hot").is_some_and(|t| t > 0.99), "hovered: {:?}", crate::scenario::count_of("hole:0:hot"));
    // The click frame by frame: the press, then the release that takes the hole out.
    let button = |pressed| egui::Event::PointerButton { pos: c, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    h.frame_with(vec![button(true)]);
    h.frame_with(vec![button(false)]);
    let p = places(&h);
    assert!(matches!(p.as_slice(), [p] if p.dist(Vec3::new(60.0, 15.0, 20.0)) < 1e-4), "the first hole is out: {p:?}");
    assert_eq!(crate::scenario::count_of("hole:0:hot"), Some(0.0), "not given the click's hover");
    h.frames(1);
    assert_eq!(crate::scenario::count_of("hole:0:hot"), Some(0.0), "not fading from the hole taken out");
}

#[test]
fn a_held_marker_seen_end_on_leaves_the_pointer_to_the_view() {
    // Seen from the front, the left edge the hole is held by runs straight into the screen: the
    // marker is the dot alone and takes no press, so navigation is not kept from it either.
    let mut h = held_by_the_left_edge();
    h.step(&json!({"call": "ui.view", "params": {"view": "front"}})).unwrap();
    h.settle();
    let c = handle_at("hole:0").expect("the held hole is still drawn");
    assert!(!crate::place_gizmo::contains_pointer(&h.app, c), "no press area, so the view keeps the pointer");
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    let c = handle_at("hole:0").expect("the hole's marker");
    assert!(crate::place_gizmo::contains_pointer(&h.app, c), "seen from the top it drags along the edge again");
}

#[test]
fn over_a_hole_marker_the_cursor_is_move_and_select_other_yields_to_it() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(10.0, 10.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    let c = handle_at("hole:0").expect("a marker");
    let on = [f64::from(c.x) + 6.0, f64::from(c.y)];
    h.step(&json!({"move": on})).unwrap();
    h.frames(2);
    assert_eq!(h.output.cursor_icon, egui::CursorIcon::Move, "the move cursor, as over the other markers");
    // Without markers, a hold there lists what is under the pointer.
    crate::prefs::set(&mut h.app, &json!({"hole_moves": false})).unwrap();
    h.frames(2);
    h.step(&json!({"hold": on})).unwrap();
    h.frames(2);
    assert!(crate::select_other::is_open(&h.app), "no marker: Select Other opens there");
    h.step(&json!({"key": "Escape"})).unwrap();
    h.frames(2);
    assert!(!crate::select_other::is_open(&h.app));
    assert_eq!(places(&h).len(), 1, "{:?}", places(&h));
    // With the marker, the press is the marker's: no list, and its release is the marker's click.
    crate::prefs::set(&mut h.app, &json!({"hole_moves": true})).unwrap();
    h.frames(2);
    h.step(&json!({"hold": on})).unwrap();
    h.frames(2);
    assert!(!crate::select_other::is_open(&h.app), "Select Other yields to the marker");
    assert!(places(&h).is_empty(), "the marker took the press: {:?}", places(&h));
}

#[test]
fn the_dragged_footprint_is_the_holes_mouth() {
    let mut h = blocks();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let base = Vec3::new(10.0, 10.0, 20.0);
    let n = Vec3::new(0.0, 0.0, 1.0);
    let radius = |h: &Harness| {
        let d = h.app.dialog.as_ref().expect("a dialog is open");
        let pts = super::footprint(&h.app, d, base, n);
        assert!(!pts.is_empty());
        pts.iter().map(|p| p.dist(base)).fold(0.0, f64::max)
    };
    let set = |h: &mut Harness, k: usize| {
        if let Some(crate::dialogs::Kind::Hole { diameter, kind, cb_diameter, cs_diameter, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
            (*kind, *diameter, *cb_diameter, *cs_diameter) = (k, "4 mm".into(), "8 mm".into(), "12 mm".into());
        }
    };
    set(&mut h, 0);
    assert!((radius(&h) - 2.0).abs() < 1e-9, "a simple hole: the bore");
    set(&mut h, 1);
    assert!((radius(&h) - 4.0).abs() < 1e-9, "a counterbore: its diameter");
    set(&mut h, 2);
    assert!((radius(&h) - 6.0).abs() < 1e-9, "a countersink: its diameter");
}
