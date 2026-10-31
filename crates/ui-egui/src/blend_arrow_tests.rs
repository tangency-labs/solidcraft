//! Fillet and chamfer drag arrows on outside and inside edges (#1384): the arrow points where
//! the value grows (into the body on an outside edge, into the open corner on an inside one),
//! and dragging out along it increases the value.

use crate::canvas::{anchor, blend_grow_dir};
use crate::dialogs::Kind;
use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

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
    let st = h.app.session.world_state();
    let m = st.body("Body1").unwrap().mesh();
    let i = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 1e-9)).expect("an edge there");
    Sel::Edge { body: "Body1".into(), index: i, point: p }
}

fn grow(h: &Harness, s: &Sel) -> Vec3 {
    let Sel::Edge { body, index, point } = s else { panic!("an edge") };
    blend_grow_dir(&h.app.session.world_state().body(body).unwrap().mesh(), *index, *point).unwrap()
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

const INSIDE: Vec3 = Vec3::new(20.0, 15.0, 20.0);
const OUTSIDE: Vec3 = Vec3::new(40.0, 15.0, 20.0);
const STEP_TOP: Vec3 = Vec3::new(20.0, 15.0, 30.0);

/// The direction function on its own: the corner's open side on the inside edge, into the
/// material on the outside ones, each edge of a mixed chain on its own.
#[test]
fn grow_direction_follows_each_edges_convexity() {
    let h = l_step();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let open = Vec3::new(s, 0.0, s);
    assert!(close(grow(&h, &edge(&h, INSIDE)), open), "inside: {:?}", grow(&h, &edge(&h, INSIDE)));
    assert!(close(grow(&h, &edge(&h, OUTSIDE)), Vec3::new(-s, 0.0, -s)), "outside: {:?}", grow(&h, &edge(&h, OUTSIDE)));
    // Same face normals as the inside edge, but outside: the opposite way.
    assert!(close(grow(&h, &edge(&h, STEP_TOP)), -open), "step top: {:?}", grow(&h, &edge(&h, STEP_TOP)));
}

/// A plain box's outside edge keeps its arrow into the body.
#[test]
fn box_edge_arrow_still_points_into_the_body() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    let e = edge(&h, Vec3::new(20.0, 0.0, 20.0));
    for id in ["solid.fillet", "solid.chamfer"] {
        h.app.session.selection = vec![e.clone()];
        h.app.start(id);
        let dir = anchor(&h.app, h.app.dialog.as_ref().unwrap()).unwrap().1.unwrap();
        assert!(dir.y > 0.5 && dir.z < -0.5, "{id}: {dir:?}");
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

#[test]
fn dialog_tab_blends_from_floating_values_start_at_edges_and_wrap() {
    for command in ["solid.fillet", "solid.chamfer"] {
        for placement in [crate::ui_kit::DialogButtons::Bottom, crate::ui_kit::DialogButtons::Top] {
            let mut h = l_step();
            h.app.preferences.dialog_buttons = placement;
            let selected = edge(&h, OUTSIDE);
            h.app.session.selection = vec![selected];
            h.app.start(command);
            h.frames(8);
            let canvas = egui::Id::new("sc_canvas_value");
            assert_eq!(h.ctx.memory(|m| m.focused()), Some(canvas), "the blend starts in the floating value");
            h.step(&json!({"key":"Tab"})).unwrap();
            let response = h.ctx.read_response(h.ctx.memory(|m| m.focused()).unwrap()).unwrap();
            assert!(
                response.rect.contains(crate::scenario::handle_at("dialog:Edges").unwrap()),
                "the next Tab must reach Edges, skipping Close: {placement:?}"
            );
            crate::test_support::dialog_tab_cycle(&mut h);
        }
    }
}

#[test]
fn segmented_enter_applies_fillet_without_changing_continuity() {
    let mut h = l_step();
    h.app.session.selection = vec![edge(&h, OUTSIDE)];
    h.app.start("solid.fillet");
    h.frames(8);
    let mut reached = false;
    for _ in 0..20 {
        h.step(&json!({"key":"Tab"})).unwrap();
        let at = crate::scenario::handle_at("dialog:Continuity:G2").unwrap();
        if h.ctx.memory(|m| m.focused()).and_then(|id| h.ctx.read_response(id)).is_some_and(|r| r.rect.contains(at)) {
            reached = true;
            break;
        }
    }
    assert!(reached, "G2 must be reachable with Tab");
    h.step(&json!({"key":"Enter"})).unwrap();
    assert!(h.app.dialog.is_none(), "Enter on G2 must apply the existing G1 fillet instead of changing continuity");
}
