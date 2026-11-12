//! Fillet and chamfer drag handles on outside and inside edges (#1384): the handle stands away
//! from the material on an edge of either kind, where it can be seen and taken hold of; dragging
//! it out increases the value, and dragging it back through its own base turns the blend into the
//! other kind. It stops where the edges around it leave room.

use crate::canvas::{anchor, blend_cap, blend_grow_dir, blend_room};
use crate::dialogs::Kind;
use crate::scenario::Harness;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

/// A 40 x 30 x 20 block with a 20 x 30 x 10 step on its left half: the edge along y at x 20,
/// z 20 is inside (concave); the block's right top edge (x 40, z 20) and the step's top right
/// edge (x 20, z 30) are outside (convex).
fn l_step() -> Harness {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    h.app.run("solid.box", json!({"length": 20, "width": 30, "height": 10, "corner": [0, 0, 20], "operation": "join"})).unwrap();
    h.frames(2);
    h
}

fn edge(h: &Harness, p: Vec3) -> Sel {
    let st = h.app.session.world_state().unwrap();
    let m = st.body("Body1").unwrap().mesh();
    let i = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 1e-9)).expect("an edge there");
    Sel::Edge { body: "Body1".into(), index: i, point: p }
}

fn grow(h: &Harness, s: &Sel) -> Vec3 {
    let Sel::Edge { body, index, point } = s else { panic!("an edge") };
    blend_grow_dir(&h.app.session.world_state().unwrap().body(body).unwrap().mesh(), *index, *point).unwrap()
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

const INSIDE: Vec3 = Vec3::new(20.0, 15.0, 20.0);
const OUTSIDE: Vec3 = Vec3::new(40.0, 15.0, 20.0);
const STEP_TOP: Vec3 = Vec3::new(20.0, 15.0, 30.0);

/// The direction function on its own: away from the material on an edge of either kind, which
/// on the inside edge is the corner's open side and on an outside one is out of the body. It is
/// the bisector of the two faces every time, so a mixed chain gets one rule, not two.
#[test]
fn the_grow_direction_leads_away_from_the_material_on_every_edge() {
    let h = l_step();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let open = Vec3::new(s, 0.0, s);
    assert!(close(grow(&h, &edge(&h, INSIDE)), open), "inside: {:?}", grow(&h, &edge(&h, INSIDE)));
    assert!(close(grow(&h, &edge(&h, OUTSIDE)), Vec3::new(s, 0.0, s)), "outside: {:?}", grow(&h, &edge(&h, OUTSIDE)));
    // The same face normals as the inside edge, and now the same answer: both lead out of the
    // material, which is where the handle can be seen and taken hold of.
    assert!(close(grow(&h, &edge(&h, STEP_TOP)), open), "step top: {:?}", grow(&h, &edge(&h, STEP_TOP)));
}

/// A plain box's outside edge: the handle stands out of the body, clear of the part, not down
/// into the material where the model hides it.
#[test]
fn a_box_edges_handle_stands_out_of_the_body() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    let e = edge(&h, Vec3::new(20.0, 0.0, 20.0));
    for id in ["solid.fillet", "solid.chamfer"] {
        h.app.session.selection = vec![e.clone()];
        h.app.start(id);
        let dir = anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().1.unwrap();
        assert!(dir.y < -0.5 && dir.z > 0.5, "{id}: {dir:?}");
        h.app.dialog = None;
    }
}

/// The value of the open fillet or chamfer window.
fn value(h: &Harness) -> f64 {
    let Some(Kind::Fillet { radius, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("a fillet window") };
    h.app.session.doc.eval(radius, solidcraft_engine::doc::expr::Kind::Length).unwrap()
}

/// On an inside edge (and on an outside one, a mixed chain's first edge either way), the arrow
/// points along the edge's own grow direction and dragging outward along it increases the value.
#[test]
fn dragging_out_along_the_arrow_grows_fillet_and_chamfer_on_both_kinds_of_edge() {
    for first in [INSIDE, OUTSIDE] {
        for id in ["solid.fillet", "solid.chamfer"] {
            let mut h = l_step();
            h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
            h.step(&json!({"call": "ui.view", "params": {"view": "fit"}})).unwrap();
            let chain = if first == INSIDE { vec![edge(&h, INSIDE), edge(&h, OUTSIDE)] } else { vec![edge(&h, OUTSIDE), edge(&h, INSIDE)] };
            h.app.session.selection = chain;
            h.app.start(id);
            h.frames(3);
            let shown = h.app.dialog.as_ref().unwrap().inputs[0].items[0].clone();
            assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items.len(), 2);
            let want = grow(&h, &shown);
            assert!(close(want, grow(&h, &edge(&h, first))) || matches!(&shown, Sel::Edge { point, .. } if *point != first));
            let (at, dir) = anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
            assert!(close(dir.unwrap(), want), "{id} at {first:?}: {dir:?} vs {want:?}");
            let before = value(&h);
            let to = at + want * 6.0;
            h.step(&json!({"drag": [{"handle": "arrow"}, {"world": [to.x, to.y, to.z]}], "ctrl": true})).unwrap();
            let after = value(&h);
            assert!(after > before && (after - 6.0).abs() < 0.3, "{id} at {first:?}: {before} -> {after}");
        }
    }
}

/// Chamfer Type "Two Distances" (#2711): Distance 1 runs along the face that faces up most (the
/// other one with Flip) and Distance 2 along the remaining face, so the window shows an arrow
/// for each, from the edge into its face.
#[test]
fn two_distances_chamfer_has_an_arrow_along_each_face() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    // The top front edge: between the top face (up) and the front face (facing -y).
    let top_front = Vec3::new(20.0, 0.0, 20.0);
    let (along_top, along_front) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, -1.0));
    for (flip, expected) in [(false, (along_top, along_front)), (true, (along_front, along_top))] {
        h.app.session.selection = vec![edge(&h, top_front)];
        h.app.start("solid.chamfer");
        // Equal distances keep the arrow along the bisector, and have no second one.
        let d = h.app.dialog.as_ref().unwrap();
        assert!(crate::canvas::chamfer_face_dirs(&h.app, d, top_front).is_none());
        let Some(Kind::Fillet { ctype, flip: f, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("a chamfer window") };
        (*ctype, *f) = (1, flip);
        let d = h.app.dialog.as_ref().unwrap();
        let (t1, t2) = crate::canvas::chamfer_face_dirs(&h.app, d, top_front).expect("both faces");
        assert!(close(t1, expected.0) && close(t2, expected.1), "flip {flip}: {t1:?} {t2:?}");
        // Distance 1's arrow is the first one.
        let dir = anchor(&h.app, d).unwrap().1.unwrap();
        assert!(close(dir, expected.0), "flip {flip}: arrow {dir:?}");
        h.app.dialog = None;
    }
}

fn room(h: &Harness, s: &Sel) -> f64 {
    let Sel::Edge { body, index, .. } = s else { panic!("an edge") };
    let st = h.app.session.world_state().unwrap();
    blend_room(&st.body(body).unwrap().mesh(), *index).expect("the edge has room")
}

/// A fillet or chamfer has the room its neighbouring edges leave it: across each of the two
/// faces at the edge to whatever bounds them, the smaller of the two. The step's short side is
/// 10 mm, so every edge along it stops there however much room the face above or beside it has.
#[test]
fn a_blend_has_the_room_its_neighbouring_edges_leave() {
    let h = l_step();
    // 20 mm across the block's top face to the step, 20 mm down its right face to the bottom.
    assert!((room(&h, &edge(&h, OUTSIDE)) - 20.0).abs() < 1e-6, "outside: {}", room(&h, &edge(&h, OUTSIDE)));
    // 20 mm across the step's top, but only the step's 10 mm down its side.
    assert!((room(&h, &edge(&h, STEP_TOP)) - 10.0).abs() < 1e-6, "step top: {}", room(&h, &edge(&h, STEP_TOP)));
    // The inside corner is held to the same 10 mm side.
    assert!((room(&h, &edge(&h, INSIDE)) - 10.0).abs() < 1e-6, "inside: {}", room(&h, &edge(&h, INSIDE)));
}

/// A round over several edges stops at the tightest of them, and nothing else is capped: the
/// cap is what the handle and the window's arrow are held to, and only a blend has one.
#[test]
fn a_round_over_several_edges_is_capped_by_the_tightest() {
    let mut h = l_step();
    // The block's own top edge has 20 mm, the step's 10; together the round stops at 10.
    h.app.session.selection = vec![edge(&h, OUTSIDE), edge(&h, STEP_TOP)];
    h.app.start("solid.fillet");
    h.frames(2);
    let d = h.app.dialog.clone().unwrap_or_else(|| panic!("the fillet opens"));
    let cap = blend_cap(&h.app, &d).unwrap_or_else(|| panic!("both edges have room"));
    assert!((cap - 10.0).abs() < 1e-6, "the tightest edge sets it: {cap}");

    // One edge alone is its own room.
    h.app.session.selection = vec![edge(&h, OUTSIDE)];
    h.app.start("solid.fillet");
    h.frames(2);
    let d = h.app.dialog.clone().unwrap_or_else(|| panic!("the fillet opens"));
    assert!((blend_cap(&h.app, &d).unwrap_or_default() - 20.0).abs() < 1e-6);

    // An extrude is not a blend and has no room to run out of.
    h.app.session.selection = vec![Sel::Face { body: "Body1".into(), index: face_index(&h), point: Vec3::new(30.0, 15.0, 20.0) }];
    h.app.start("solid.press_pull");
    h.frames(2);
    let d = h.app.dialog.clone().unwrap_or_else(|| panic!("the push opens"));
    assert_eq!(blend_cap(&h.app, &d), None, "only a blend is capped");
}

/// The room is measured across the faces beside the edge, which cannot see what a round must
/// also fit round: a rim whose corners are rounded takes no more than those corners, whatever
/// the faces allow. The kernel says so when it refuses one, naming the largest that fits, and
/// the handle then stops there instead of standing where nothing builds.
#[test]
fn the_kernels_own_limit_holds_the_handle_where_the_room_alone_would_not() {
    let mut h = l_step();
    h.app.session.selection = vec![edge(&h, OUTSIDE)];
    h.app.start("solid.fillet");
    h.frames(2);
    let d = h.app.dialog.clone().unwrap_or_else(|| panic!("the fillet opens"));
    assert!((blend_cap(&h.app, &d).unwrap_or_default() - 20.0).abs() < 1e-6, "the room alone");

    // What a refusal of this very setup said: 6 mm, and the handle goes no further.
    h.app.preview.fit = Some((7, 6.0));
    assert!((blend_cap(&h.app, &d).unwrap_or_default() - 6.0).abs() < 1e-6, "the kernel's word");
    // Never upward: a limit wider than the room leaves the room in charge.
    h.app.preview.fit = Some((7, 50.0));
    assert!((blend_cap(&h.app, &d).unwrap_or_default() - 20.0).abs() < 1e-6, "the tighter of the two");
    h.app.preview.fit = None;

    // And it is still only a blend that is capped at all.
    h.app.session.selection = vec![Sel::Face { body: "Body1".into(), index: face_index(&h), point: Vec3::new(30.0, 15.0, 20.0) }];
    h.app.start("solid.press_pull");
    h.frames(2);
    let d = h.app.dialog.clone().unwrap_or_else(|| panic!("the push opens"));
    h.app.preview.fit = Some((7, 6.0));
    assert_eq!(blend_cap(&h.app, &d), None, "a push has no blend to cap");
}

/// The index of the block's own top face (the one at z 20, right of the step).
fn face_index(h: &Harness) -> usize {
    let st = h.app.session.world_state().unwrap();
    let b = st.body("Body1").unwrap();
    let m = b.mesh();
    let f = m
        .tri_face
        .iter()
        .copied()
        .find(|f| {
            solidcraft_engine::geom::snap::planar_face_center(&m, *f)
                .is_some_and(|(c, n)| (c.z - 20.0).abs() < 1e-6 && n.z > 0.9 && c.x > 20.0)
        })
        .expect("the block's own top face");
    usize::try_from(f).unwrap_or_default()
}

/// Which kind of blend the open window makes is the drag's to say, not only the bare edge's:
/// carried back through the handle's own base, a fillet becomes a chamfer at the size the drag has
/// reached, and carried back again it rounds off once more. The handle turns with it each time, so
/// the same drag carries on sizing what it is now making instead of running into the near end.
#[test]
fn a_drag_back_through_the_base_turns_a_fillet_into_a_chamfer_and_back() {
    let is_chamfer = |h: &Harness| match h.app.dialog.as_ref().map(|d| &d.kind) {
        Some(Kind::Fillet { chamfer, .. }) => *chamfer,
        other => panic!("not a blend: {other:?}"),
    };
    let mut h = l_step();
    h.step(&json!({"call": "ui.view", "params": {"view": "iso"}})).unwrap();
    h.step(&json!({"call": "ui.view", "params": {"view": "fit"}})).unwrap();
    h.app.session.selection = vec![edge(&h, OUTSIDE)];
    h.app.start("solid.fillet");
    h.frames(3);
    assert!(!is_chamfer(&h), "it opens as a fillet");
    let (at, dir) = anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let dir = dir.expect("the handle has a direction");

    // Out along the handle first: an ordinary fillet of about 4 mm.
    let out = at + dir * 4.0;
    h.step(&json!({"drag": [{"handle": "arrow"}, {"world": [out.x, out.y, out.z]}], "ctrl": true})).unwrap();
    assert!(!is_chamfer(&h), "out along the handle keeps it a fillet");
    assert!((value(&h) - 4.0).abs() < 0.5, "{}", value(&h));

    // Back through the base: a chamfer, sized by how far past the base the drag went.
    let back = at - dir * 3.0;
    h.step(&json!({"drag": [{"handle": "arrow"}, {"world": [back.x, back.y, back.z]}], "ctrl": true})).unwrap();
    assert!(is_chamfer(&h), "dragged back through its base, the fillet becomes a chamfer");
    assert!(value(&h) > 0.0, "a chamfer is a size, never a direction: {}", value(&h));

    // The handle turned with it, so the same gesture back again rounds it off once more.
    let (at2, dir2) = anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let dir2 = dir2.expect("the turned handle has a direction");
    assert!(close(dir2, -dir), "the handle turned round: {dir2:?} against {dir:?}");
    let back2 = at2 - dir2 * 3.0;
    h.step(&json!({"drag": [{"handle": "arrow"}, {"world": [back2.x, back2.y, back2.z]}], "ctrl": true})).unwrap();
    assert!(!is_chamfer(&h), "and back again it is a fillet");
}
