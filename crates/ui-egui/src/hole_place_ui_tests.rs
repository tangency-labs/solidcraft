//! Placing a Single hole (#2171, #1047). With Snap to face and edge centres on (Preferences, off by
//! default), a pick near the middle of a face or on a round edge snaps there, and Alt held places
//! it where it was clicked. With Move holes by clicking or dragging on (also off by default), the
//! handle drags a placed hole (also one being edited) and a second pick on its face moves it.
//! Both off, picks behave as before. Either way the dialog shows the hole's sizes beside a
//! section of it.

use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;
use solidcraft_engine::geom::Vec3;

use crate::dialogs::Kind;
use crate::scenario::{Harness, handle_at, rect_of};
use crate::viewport::Hit;

/// A 40 × 30 × 20 block seen from the top, with a 10 mm hole through it at (10, 10), and hole
/// snaps and moves turned on as the Preferences dialog (or `ui.prefs`) turns them on.
fn block() -> Harness {
    let mut h = block_as_installed();
    crate::prefs::set(&mut h.app, &json!({"hole_snaps": true, "hole_moves": true})).unwrap();
    assert!(h.app.preferences.hole_snaps && h.app.preferences.hole_moves);
    h
}

/// The block with the preferences a new installation has.
fn block_as_installed() -> Harness {
    let mut h = Harness::new();
    h.app.set_origin_shown(false);
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.hole", json!({"position": [10, 10, 20], "diameter": 10, "name": "Bore"})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "top"}})).unwrap();
    h.settle();
    h
}

/// Where a world point is on screen.
fn screen(h: &mut Harness, p: Vec3) -> [f64; 2] {
    h.at(&json!({"world": [p.x, p.y, p.z]})).unwrap()
}

/// `p` on screen, moved by `dx`, `dy` points.
fn off(h: &mut Harness, p: Vec3, dx: f64, dy: f64) -> [f64; 2] {
    let c = screen(h, p);
    [c[0] + dx, c[1] + dy]
}

/// The face places the open dialog holds.
fn places(h: &Harness) -> Vec<Vec3> {
    h.app.dialog.as_ref().unwrap().inputs[0]
        .items
        .iter()
        .map(|x| match x {
            Sel::Face { point, .. } => *point,
            other => panic!("a face place, not {other:?}"),
        })
        .collect()
}

/// The last feature's id and position, when it is a hole.
fn last_hole(h: &Harness) -> (u64, Vec3) {
    let f = h.app.session.doc.features.last().unwrap();
    match &f.kind {
        FeatureKind::Hole { position, .. } => (f.id, position.0),
        other => panic!("the last feature is not a hole: {}", other.type_name()),
    }
}

fn solid_at(h: &Harness, p: Vec3) -> bool {
    h.app.session.model.state().bodies.iter().any(|b| b.body.contains(p))
}

const MIDDLE: Vec3 = Vec3::new(20.0, 15.0, 20.0);
const BORE: Vec3 = Vec3::new(10.0, 10.0, 20.0);

#[test]
fn a_pick_near_the_middle_of_a_face_goes_to_the_middle() {
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = off(&mut h, MIDDLE, 5.0, -4.0);
    h.step(&json!({"move": at})).unwrap();
    match &h.app.viewport.hover {
        Some(Hit::Face { point, .. }) => assert!(point.dist(MIDDLE) < 1e-9, "the face lights up at its middle: {point:?}"),
        other => panic!("the face lights up, not {other:?}"),
    }
    h.step(&json!({"click": at})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(MIDDLE) < 1e-9), "{:?}", places(&h));
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (_, position) = last_hole(&h);
    assert!(position.dist(MIDDLE) < 1e-9, "{position:?}");
    assert!(!solid_at(&h, MIDDLE - Vec3::new(0.0, 0.0, 5.0)), "drilled at the middle");
}

#[test]
fn a_pick_away_from_snaps_or_with_alt_held_stays_where_it_was_clicked() {
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    // Well away from the middle and from the bore's rim.
    let away = Vec3::new(30.0, 8.0, 20.0);
    let at = screen(&mut h, away);
    h.step(&json!({"click": at})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(away) < 1e-3 && p.dist(MIDDLE) > 1.0), "{:?}", places(&h));
    h.step(&json!({"key": "Escape"})).unwrap();
    // Alt held: no snap, even beside the middle.
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = off(&mut h, MIDDLE, 5.0, -4.0);
    h.step(&json!({"move": at, "alt": true})).unwrap();
    h.step(&json!({"click": at, "alt": true})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(MIDDLE) > 0.1 && (p.z - 20.0).abs() < 1e-9), "{:?}", places(&h));
}

#[test]
fn a_pick_on_a_round_edge_goes_to_its_centre() {
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    // On the bore's rim, and a few points outside it on the top face.
    for (dx, label) in [(0.0, "on the rim"), (3.0, "beside the rim")] {
        let rim = off(&mut h, BORE + Vec3::new(5.0, 0.0, 0.0), dx, 0.0);
        let c = screen(&mut h, BORE);
        assert!((rim[0] - c[0]).abs() > 2.0 * f64::from(crate::hole_place::POINT_REACH), "the rim is out of the centre's reach");
        h.step(&json!({"click": rim})).unwrap();
        assert!(matches!(places(&h).as_slice(), [p] if p.dist(BORE) < 1e-6), "{label}: {:?}", places(&h));
        h.step(&json!({"key": "Escape"})).unwrap();
        h.step(&json!({"start": "solid.hole"})).unwrap();
    }
    // A counterbore round the bore: drilled on its axis.
    let rim = screen(&mut h, BORE + Vec3::new(5.0, 0.0, 0.0));
    h.step(&json!({"click": rim})).unwrap();
    if let Some(Kind::Hole { kind, cb_diameter, cb_depth, diameter, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *kind = 1;
        *diameter = "12 mm".into();
        *cb_diameter = "16 mm".into();
        *cb_depth = "3 mm".into();
    }
    // Changing the type adds fields; draw them before sending Enter to the focused editor.
    h.frames(3);
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (id, position) = last_hole(&h);
    assert!(position.dist(BORE) < 1e-6, "{position:?}");
    assert!(h.app.session.model.result(id).is_some_and(|r| r.error.is_none()), "the counterbore cuts");
    assert!(!solid_at(&h, BORE + Vec3::new(7.5, 0.0, -1.5)) && solid_at(&h, BORE + Vec3::new(8.5, 0.0, -1.5)), "8 mm round the axis");
    assert!(!solid_at(&h, BORE + Vec3::new(5.5, 0.0, -8.0)) && solid_at(&h, BORE + Vec3::new(6.5, 0.0, -8.0)), "then 6 mm");
}

#[test]
fn the_handle_drags_a_placed_hole_and_snaps() {
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let start = Vec3::new(30.0, 8.0, 20.0);
    let at = screen(&mut h, start);
    h.step(&json!({"click": at})).unwrap();
    h.frames(2);
    assert!(handle_at("hole:0").is_some(), "the placed hole has a handle");
    // Dragged to beside the middle: it snaps there.
    let to = off(&mut h, MIDDLE, 4.0, 3.0);
    h.step(&json!({"drag": [{"handle": "hole:0"}, to]})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(MIDDLE) < 1e-9), "{:?}", places(&h));
    // Then onto a free spot: it goes where it is let go.
    let free = Vec3::new(32.0, 24.0, 20.0);
    let to = screen(&mut h, free);
    h.step(&json!({"drag": [{"handle": "hole:0"}, to]})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(free) < 0.2), "{:?}", places(&h));
    // A drag is not a pick: still one hole, and nothing was box-selected.
    assert_eq!(places(&h).len(), 1);
    h.step(&json!({"key": "Enter"})).unwrap();
    let (_, position) = last_hole(&h);
    assert!(position.dist(free) < 0.2, "{position:?}");
}

/// What the last frame painted in the handles' white core round `c`: the dot's radius, each
/// line's ends and each filled shape's farthest point, as distances from `c`, and how many
/// circles of any colour are centred there (the dot and its casing, nothing else).
struct Painted {
    dots: Vec<f32>,
    lines: Vec<(f32, f32)>,
    tips: Vec<f32>,
    circles: usize,
}

fn painted(h: &Harness, c: egui::Pos2) -> Painted {
    use egui::epaint::{ColorMode, Shape};
    let core = crate::theme::Tokens::get().icons.highlight;
    let mut p = Painted { dots: Vec::new(), lines: Vec::new(), tips: Vec::new(), circles: 0 };
    for clipped in &h.shapes {
        crate::test_support::each_shape(&clipped.shape, &mut |s| match s {
            Shape::Circle(o) if o.center.distance(c) < 0.5 => {
                p.circles += 1;
                if o.fill == core {
                    p.dots.push(o.radius);
                }
            }
            Shape::Path(path) if path.points.iter().all(|q| q.distance(c) < 30.0) => {
                let far = path.points.iter().map(|q| q.distance(c)).fold(0.0, f32::max);
                if path.closed && path.fill == core {
                    p.tips.push(far);
                } else if !path.closed
                    && matches!(path.stroke.color, ColorMode::Solid(col) if col == core)
                    && let [a, b] = path.points.as_slice()
                {
                    p.lines.push((a.distance(c), b.distance(c)));
                }
            }
            _ => {}
        });
    }
    p
}

#[test]
fn a_placed_hole_s_handle_is_the_shared_move_marker() {
    // The move marker Rest, Boss and Snap Fit use (#6818): a white dot with four dashed arms at
    // rest, arrowheads when pointed at, solid shafts while it drags; no ring round the centre.
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = screen(&mut h, Vec3::new(30.0, 8.0, 20.0));
    h.step(&json!({"click": at})).unwrap();
    let away = off(&mut h, Vec3::new(30.0, 8.0, 20.0), 60.0, 40.0);
    h.step(&json!({"move": away})).unwrap();
    h.frames(30);
    let c = handle_at("hole:0").expect("the placed hole has a handle");
    let close = |got: &[f32], want: f32| got.len() == 4 && got.iter().all(|d| (d - want).abs() < 0.3);
    let rest = painted(&h, c);
    assert_eq!(rest.dots, vec![3.9], "one white dot");
    assert_eq!(rest.circles, 2, "the dot and its casing, no ring");
    let inner: Vec<f32> = rest.lines.iter().map(|l| l.1).collect();
    assert!(rest.lines.iter().all(|l| (l.0 - 6.0).abs() < 0.3) && close(&inner, 11.5), "four inner dashes: {:?}", rest.lines);
    assert!(close(&rest.tips, 19.3), "four outer dashes: {:?}", rest.tips);
    // Pointed at: the outer dashes grow into arrowheads.
    h.step(&json!({"move": [c.x, c.y]})).unwrap();
    h.frames(30);
    let hot = painted(&h, c);
    assert!(close(&hot.tips, 22.5), "four arrowheads: {:?}", hot.tips);
    assert_eq!(hot.circles, 2, "still no ring");
    // Dragged: the inner dashes run on into the heads.
    h.frame_with(crate::test_support::pointer(c, true));
    for k in 1..=30 {
        h.frame_with(vec![egui::Event::PointerMoved(c + egui::vec2(k as f32, 0.0))]);
    }
    let c = handle_at("hole:0").expect("the dragged hole keeps its handle");
    let drag = painted(&h, c);
    let shafts: Vec<f32> = drag.lines.iter().map(|l| l.1).collect();
    assert!(close(&shafts, 13.8), "four solid shafts: {:?}", drag.lines);
    assert!(close(&drag.tips, 22.5), "{:?}", drag.tips);
    h.frame_with(crate::test_support::pointer(c, false));
}

#[test]
fn a_second_pick_on_the_face_moves_the_hole_and_a_pick_on_it_takes_it_out() {
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let first = Vec3::new(30.0, 8.0, 20.0);
    let at = screen(&mut h, first);
    h.step(&json!({"click": at})).unwrap();
    let second = Vec3::new(32.0, 24.0, 20.0);
    let at2 = screen(&mut h, second);
    h.step(&json!({"click": at2})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(second) < 0.2), "moved, not added: {:?}", places(&h));
    // On the hole itself (inside its 5 mm): taken out.
    h.step(&json!({"click": at2})).unwrap();
    assert!(places(&h).is_empty(), "{:?}", places(&h));
}

#[test]
fn editing_a_hole_drags_it_to_a_new_place() {
    // Moving a hole (#1047): Edit Feature, drag its handle, OK.
    let mut h = block();
    let (id, _) = last_hole(&h);
    h.step(&json!({"call": "ui.editFeature", "params": {"feature": id}})).unwrap();
    h.frames(4);
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(BORE) < 1e-6), "{:?}", places(&h));
    let to = off(&mut h, MIDDLE, -3.0, 4.0);
    h.step(&json!({"drag": [{"handle": "hole:0"}, to]})).unwrap();
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().map(|d| &d.error));
    let (again, position) = last_hole(&h);
    assert_eq!(again, id, "the same feature");
    assert!(position.dist(MIDDLE) < 1e-9, "{position:?}");
    let down = Vec3::new(0.0, 0.0, 10.0);
    assert!(solid_at(&h, BORE - down), "the old place is solid again");
    assert!(!solid_at(&h, MIDDLE - down), "the hole is at the middle now");
}

#[test]
fn snaps_and_moves_are_off_by_default_and_picks_behave_as_before() {
    let mut h = block_as_installed();
    assert!(!h.app.preferences.hole_snaps && !h.app.preferences.hole_moves, "a new installation places holes as it always did");
    h.step(&json!({"start": "solid.hole"})).unwrap();
    // Beside the middle: no snap.
    let at = off(&mut h, MIDDLE, 5.0, -4.0);
    h.step(&json!({"move": at})).unwrap();
    match &h.app.viewport.hover {
        Some(Hit::Face { point, .. }) => assert!(point.dist(MIDDLE) > 0.1, "{point:?}"),
        other => panic!("the face lights up, not {other:?}"),
    }
    h.step(&json!({"click": at})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(MIDDLE) > 0.1 && (p.z - 20.0).abs() < 1e-9), "{:?}", places(&h));
    h.frames(2);
    assert!(handle_at("hole:0").is_none(), "no drag handle");
    // A second pick on the face takes the hole out, as it always did.
    let elsewhere = screen(&mut h, Vec3::new(32.0, 24.0, 20.0));
    h.step(&json!({"click": elsewhere})).unwrap();
    assert!(places(&h).is_empty(), "{:?}", places(&h));
}

#[test]
fn moving_holes_is_its_own_setting() {
    // Moves on, snaps off: the handle drags a hole to where it is let go, and a second pick on
    // its face moves it; neither snaps.
    let mut h = block_as_installed();
    crate::prefs::set(&mut h.app, &json!({"hole_moves": true})).unwrap();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let start = screen(&mut h, Vec3::new(30.0, 8.0, 20.0));
    h.step(&json!({"click": start})).unwrap();
    h.frames(2);
    let to = off(&mut h, MIDDLE, 5.0, -4.0);
    h.step(&json!({"drag": [{"handle": "hole:0"}, to]})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(MIDDLE) > 0.1 && p.dist(MIDDLE) < 2.0), "dragged, not snapped: {:?}", places(&h));
    let elsewhere = Vec3::new(32.0, 24.0, 20.0);
    let at = screen(&mut h, elsewhere);
    h.step(&json!({"click": at})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(elsewhere) < 0.2), "moved, not taken out: {:?}", places(&h));
    // Snaps on, moves off: a pick snaps, and a second pick on the face takes the hole out.
    h.step(&json!({"key": "Escape"})).unwrap();
    crate::prefs::set(&mut h.app, &json!({"hole_snaps": true, "hole_moves": false})).unwrap();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    let at = off(&mut h, MIDDLE, 5.0, -4.0);
    h.step(&json!({"click": at})).unwrap();
    assert!(matches!(places(&h).as_slice(), [p] if p.dist(MIDDLE) < 1e-9), "{:?}", places(&h));
    h.frames(2);
    assert!(handle_at("hole:0").is_none(), "no drag handle");
    let at = screen(&mut h, elsewhere);
    h.step(&json!({"click": at})).unwrap();
    assert!(places(&h).is_empty(), "{:?}", places(&h));
}

#[test]
fn the_dialog_shows_the_sizes_beside_a_section_of_the_hole() {
    let mut h = block();
    h.step(&json!({"start": "solid.hole"})).unwrap();
    h.frames(3);
    let section = rect_of("dialog:figure_picture:hole section").expect("the section is drawn");
    let at = |name: &str| handle_at(&format!("dialog:{name}")).unwrap_or_else(|| panic!("no field {name}"));
    // A drilled hole to a depth: depth, diameter and tip angle, top to bottom, right of it.
    let (depth, dia, tip) = (at("Depth"), at("Diameter"), at("Tip Angle"));
    assert!(depth.y < dia.y && dia.y < tip.y, "{depth:?} {dia:?} {tip:?}");
    assert!([depth, dia, tip].iter().all(|p| p.x > section.right() && p.y > section.top() && p.y < section.bottom()));
    // A counterbore: its diameter and depth come first.
    if let Some(Kind::Hole { kind, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        *kind = 1;
    }
    h.frames(3);
    let (cb, cbd, depth, dia) = (at("Counterbore Ø"), at("Counterbore depth"), at("Depth"), at("Diameter"));
    assert!(cb.y < cbd.y && cbd.y < depth.y && depth.y < dia.y, "{cb:?} {cbd:?} {depth:?} {dia:?}");
    assert!(handle_at("dialog:Tip Angle").is_some(), "a counterbore retains the selected angled bottom");
    if let Some(Kind::Hole { opts, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        opts.angled = false;
    }
    h.frames(3);
    assert!(handle_at("dialog:Tip Angle").is_none(), "a flat bottom needs no tip angle");
    // Through all: no depth.
    if let Some(Kind::Hole { opts, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        opts.all = true;
    }
    h.frames(3);
    assert!(handle_at("dialog:Depth").is_none());
    // The depth field still takes typing (scenarios click it by its name).
    if let Some(Kind::Hole { opts, kind, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) {
        opts.all = false;
        *kind = 0;
    }
    h.frames(3);
    h.step(&json!({"click": {"handle": "dialog:Depth"}})).unwrap();
    h.step(&json!({"key": "A", "cmd": true})).unwrap();
    h.step(&json!({"text": "7 mm"})).unwrap();
    h.frames(2);
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole { depth, .. } if depth == "7 mm"));
}

#[test]
fn tapped_thread_depth_field_keeps_focus_and_typing_separate_from_acceptance() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    let hole = h.app.run("solid.hole", json!({"position":[20,15,20],"thread":"M6","depth":12})).unwrap()["feature"].as_u64().unwrap();
    let original = h.app.session.doc.to_json();
    h.step(&json!({"call":"ui.editFeature","params":{"feature":hole}})).unwrap();
    h.frames(3);
    // Opening Edit Feature intentionally rolls the marker back; focusing a field must
    // preserve that editing state, and Cancel must restore the original marker too.
    let editing = h.app.session.doc.to_json();
    assert!(handle_at("dialog:Thread depth").is_none(), "full tapping needs no partial-depth field");
    h.step(&json!({"click":{"handle":"dialog:Full thread depth"}})).unwrap();
    h.frames(3);
    assert!(handle_at("dialog:Thread depth").is_some(), "partial depth belongs beside the section");
    h.step(&json!({"click":{"handle":"dialog:Thread depth"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"1 cm / 2"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.session.doc.to_json(), editing, "selecting and typing a dimension must not apply the model");
    assert!(matches!(&h.app.dialog.as_ref().unwrap().kind, Kind::Hole { opts, .. } if !opts.thread_full_depth && opts.thread_depth == "1 cm / 2"));
    h.step(&json!({"key":"Escape"})).unwrap();
    h.frames(3);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.to_json(), original, "Cancel preserves the accepted design");
}

/// The upstream Preferences split must retain both Hole controls on Design.
#[test]
fn hole_design_preferences_keep_both_controls_and_cancel_after_page_split() {
    let mut h = Harness::new();
    h.call("ui.prefs", json!({"language":"en", "open":"Design"}));
    let page = rect_of("prefs:page_body").unwrap();
    for label in ["Snap to face and edge centers", "Move holes by clicking or dragging"] {
        let mut point = None;
        for _ in 0..12 {
            point = h.output.accesskit_update.as_ref().and_then(|tree| {
                tree.nodes.iter().rev().find_map(|(_, node)| {
                    if node.label() != Some(label) {
                        return None;
                    }
                    let b = node.bounds()?;
                    let p = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                    page.contains(p).then_some(p)
                })
            });
            if point.is_some() {
                break;
            }
            h.call("ui.scroll", json!({"x":page.center().x,"y":page.center().y,"delta":-100}));
            h.frames(12);
        }
        let p = point.unwrap_or_else(|| panic!("Hole preference missing from Design: {label}"));
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
    }
    assert!(h.app.preferences.hole_snaps && h.app.preferences.hole_moves);
    h.call("ui.prefs", json!({"button":"cancel"}));
    assert!(!h.app.preferences.hole_snaps && !h.app.preferences.hole_moves);
}
