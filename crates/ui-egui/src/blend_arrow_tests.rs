//! Fillet and chamfer drag arrows on outside and inside edges (#1384): the arrow points where
//! the value grows (into the body on an outside edge, into the open corner on an inside one),
//! and dragging out along it increases the value.

use crate::canvas::{anchor, blend_grow_dir};
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

/// The texts painted in the main value box in the view, left to right.
fn value_box_texts(h: &Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, area: egui::Rect, out: &mut Vec<(f32, String)>) {
        match shape {
            egui::Shape::Text(t) if area.contains(t.pos) && !t.galley.job.text.trim().is_empty() => {
                out.push((t.pos.x, t.galley.job.text.trim().to_string()));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, area, out)),
            _ => {}
        }
    }
    let area = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_canvas_area"))).expect("the value box in the view");
    let mut texts = Vec::new();
    h.shapes.iter().for_each(|s| walk(&s.shape, area, &mut texts));
    texts.sort_by(|a, b| a.0.total_cmp(&b.0));
    texts.into_iter().map(|(_, text)| text).collect()
}

/// The arrow that goes with the distance being edited is the active one and the other is dimmed;
/// with neither edited, both are as before. Which face each arrow runs along (Flip swaps them)
/// is `chamfer_face_dirs`' business, so the pairing of distance and arrow never changes.
#[test]
fn two_distances_chamfer_highlights_the_arrow_of_the_distance_being_edited() {
    use crate::canvas::{ArrowLook::*, chamfer_arrow_looks};
    assert_eq!(chamfer_arrow_looks(None), [Plain, Plain]);
    assert_eq!(chamfer_arrow_looks(Some(0)), [Active, Dimmed]);
    assert_eq!(chamfer_arrow_looks(Some(1)), [Dimmed, Active]);
    assert_eq!(chamfer_arrow_looks(Some(5)), [Plain, Plain]);
}

/// The value box in the view shows and edits the distance being edited, and names it, with the
/// distances unequal and Flip either way.
#[test]
fn two_distances_chamfer_box_shows_the_distance_being_edited() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
    for flip in [false, true] {
        h.app.session.selection = vec![edge(&h, Vec3::new(20.0, 0.0, 20.0))];
        h.app.start("solid.chamfer");
        let Some(Kind::Fillet { ctype, radius, distance2, flip: f, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) else {
            panic!("a chamfer window")
        };
        (*ctype, *radius, *distance2, *f) = (1, "3 mm".into(), "7 mm".into(), flip);
        h.frames(3);
        let first = value_box_texts(&h);
        assert!(first.iter().any(|t| t == "Distance 1") && first.iter().any(|t| t == "3 mm"), "flip {flip}: {first:?}");
        // The keyboard goes to Distance 2.
        crate::canvas::set_chamfer_editing(h.app.dialog.as_mut().unwrap(), 1);
        h.frames(3);
        let second = value_box_texts(&h);
        assert!(second.iter().any(|t| t == "Distance 2") && second.iter().any(|t| t == "7 mm"), "flip {flip}: {second:?}");
        assert!(!second.iter().any(|t| t == "3 mm"), "flip {flip}: {second:?}");
        // The box's text is Distance 2's, so what it holds when edited goes to Distance 2.
        let Some(Kind::Fillet { radius, distance2, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("a chamfer window") };
        assert_eq!((radius.as_str(), distance2.as_str()), ("3 mm", "7 mm"), "flip {flip}");
        h.app.dialog = None;
    }
}

/// Open the real blend form and move its window just to the right of a drawn arrow.
/// This is where clearing the window can move the value field back across that arrow.
fn blend_beside_its_arrow(command: &str, two_distances: bool, flip: bool, secondary: bool) -> (Harness, Vec3) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    h.call("ui.prefs", json!({"language":"en","look_at_anim":false}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.view", json!({"view":"fit"}));
    h.frames(4);
    let selected = edge(&h, Vec3::new(20.0, 0.0, 20.0));
    h.app.session.selection = vec![selected];
    h.step(&json!({"start":command})).unwrap();
    h.frames(4);
    if two_distances {
        h.step(&json!({"click":{"handle":"dialog:Chamfer Type"}})).unwrap();
        h.step(&json!({"click":{"handle":"dialog:Chamfer Type:Two Distances"}})).unwrap();
        h.frames(4);
        if flip {
            let target = crate::scenario::handle_at("dialog:Flip").expect("the actual Flip checkbox control");
            let window = h.app.viewport.dialog_rect.expect("the actual Chamfer window");
            assert!(window.contains(target), "the Flip checkbox must be inside its command window: {window:?}, {target:?}");
            h.step(&json!({"click":{"handle":"dialog:Flip"}})).unwrap();
            h.frames(4);
        }
        let actual = h.app.dialog.as_ref().and_then(|d| match &d.kind {
            Kind::Fillet { ctype, flip, .. } => Some((*ctype, *flip)),
            _ => None,
        });
        assert_eq!(actual, Some((1, flip)), "actual Chamfer Type/Flip controls, secondary={secondary}, requested Flip={flip}");
    }
    // Render the fields again after an orbit and a pan, rather than testing fixed placement.
    h.app.cam = h.app.cam.looking_from(Vec3::new(1.0, -1.0, 0.8));
    h.app.cam.target += Vec3::new(3.0, -2.0, 0.0);
    h.frames(4);
    let handle = if secondary { "arrow2" } else { "arrow" };
    let tip = crate::scenario::handle_at(handle).expect("the actual drawn blend arrow");
    let window = h.app.viewport.dialog_rect.expect("the actual command window");
    let title = crate::scenario::handle_at("dialog:title").expect("the actual window title");
    let delta = egui::vec2(tip.x + 30.0 - window.left(), tip.y - 80.0 - window.top());
    h.call("ui.drag", json!({"x0":title.x,"y0":title.y,"x1":title.x+delta.x,"y1":title.y+delta.y,"steps":8}));
    // Pointer input drains one event a frame; wait for the entire title drag and layout.
    h.frames(24);
    let tip = crate::scenario::handle_at(handle).expect("the arrow after moving the window");
    let window = h.app.viewport.dialog_rect.unwrap();
    assert!(
        (window.left() - tip.x - 30.0).abs() < 3.0 && window.y_range().contains(tip.y),
        "fixture must put the window beside the arrow: {window:?}, {tip:?}"
    );
    assert!(!window.contains(tip), "the command window itself must leave the arrow exposed");
    let dialog = h.app.dialog.as_ref().unwrap();
    let (base, primary) = anchor(&h.app, dialog).unwrap();
    let direction = if secondary {
        crate::canvas::chamfer_face_dirs(&h.app, dialog, base).expect("both actual chamfer face directions").1
    } else {
        primary.expect("the actual primary arrow direction")
    };
    (h, base + direction * 6.0)
}

fn blend_distance(h: &Harness, secondary: bool) -> f64 {
    let Some(Kind::Fillet { radius, distance2, .. }) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("the blend form") };
    h.app.session.doc.eval(if secondary { distance2 } else { radius }, solidcraft_engine::doc::expr::Kind::Length).unwrap()
}

fn check_blend_value_field_and_drag(command: &str, two_distances: bool, flip: bool, secondary: bool) {
    let (mut h, to) = blend_beside_its_arrow(command, two_distances, flip, secondary);
    let design = h.app.session.doc.to_json();
    let undo = h.app.session.undo.len();
    // This is the real drawn egui Area, not a guessed native control handle.
    let field = h.ctx.memory(|memory| memory.area_rect(egui::Id::unique("sc_canvas_area"))).expect("the actual value field area");
    let handles = if two_distances { vec!["arrow", "arrow2"] } else { vec!["arrow"] };
    let grabs: Vec<_> = handles
        .iter()
        .map(|name| {
            let tip = crate::scenario::handle_at(name).expect("the actual arrow handle");
            // drag_arrow_to accepts a press in this 20x20-point region around its published tip.
            (*name, egui::Rect::from_center_size(tip, egui::vec2(20.0, 20.0)))
        })
        .collect();
    let before = blend_distance(&h, secondary);
    let other_before = blend_distance(&h, !secondary);
    // Attempt the real drag even when the field covers it, retaining its observed outcome.
    let handle = if secondary { "arrow2" } else { "arrow" };
    h.step(&json!({"drag":[{"handle":handle},{"world":[to.x,to.y,to.z]}],"ctrl":true})).unwrap();
    h.frames(4);
    let after = blend_distance(&h, secondary);
    for (name, grab) in &grabs {
        assert!(
            !field.intersects(*grab),
            "{command}, two={two_distances}, flip={flip}, dragged={handle}: field {field:?} covers {name} grab {grab:?}; actual distance {before} -> {after}"
        );
    }
    assert!(
        after > before && (after - 6.0).abs() < 0.3,
        "{command}, two={two_distances}, flip={flip}, {handle}: real drag should set6mm, {before} -> {after}"
    );
    if two_distances {
        assert!((blend_distance(&h, !secondary) - other_before).abs() < 1e-9, "dragging one chamfer arrow must keep the other distance");
    }
    assert_eq!(h.app.session.doc.to_json(), design, "editing a preview must not commit the feature");
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn floating_fillet_and_chamfer_fields_leave_their_arrow_draggable_beside_the_window() {
    for command in ["solid.fillet", "solid.chamfer"] {
        check_blend_value_field_and_drag(command, false, false, false);
    }
}

#[test]
fn two_distance_chamfer_field_leaves_both_arrows_draggable_beside_the_window() {
    for flip in [false, true] {
        for secondary in [false, true] {
            check_blend_value_field_and_drag("solid.chamfer", true, flip, secondary);
        }
    }
}

/// A value field stays under the pointer while the opt-in wheel stepping changes its
/// distance, even when a diagonal arrow grows into the field's old rectangle.
#[test]
fn a_hovered_blend_value_field_keeps_wheel_steps_from_reaching_the_camera() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.call("ui.resize", json!({"width":1600,"height":1200}));
    h.call("ui.prefs", json!({"language":"en","look_at_anim":false,"step_values":true}));
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.call("ui.view", json!({"view":"iso"}));
    h.call("ui.camera", json!({"target":[20,0,20],"distance":40}));
    h.app.session.selection = vec![edge(&h, Vec3::new(20.0, 0.0, 20.0))];
    h.step(&json!({"start":"solid.fillet"})).unwrap();
    h.frames(4);
    let area = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_canvas_area"))).expect("the floating field");
    let field = h.ctx.read_response(egui::Id::unique("sc_canvas_value")).expect("the Radius field").rect;
    h.step(&json!({"click":[field.center().x,field.center().y]})).unwrap();
    let before = value(&h);
    let camera = h.app.cam;
    let mut stepped_tips = Vec::new();
    for _ in 0..4 {
        h.call("ui.scroll", json!({"x":field.center().x,"y":field.center().y,"delta":40.0}));
        h.frames(2);
        stepped_tips.push(crate::scenario::handle_at("arrow").expect("the actual arrow after this wheel notch"));
    }
    assert!((value(&h) - before - 4.0).abs() < 1e-9, "all four notches must step the Radius field");
    assert_eq!(h.app.cam, camera, "no wheel notch may escape to camera zoom");
    assert!(
        stepped_tips.iter().any(|tip| area.intersects(egui::Rect::from_center_size(*tip, egui::vec2(20.0, 20.0)))),
        "the diagonal arrow must cross the held field's original area during wheel stepping: {area:?}, {stepped_tips:?}"
    );
    let kept = h.ctx.memory(|m| m.area_rect(egui::Id::unique("sc_canvas_area"))).unwrap();
    assert!(kept.contains(field.center()), "the field stays under its wheel-stepping pointer");
}
