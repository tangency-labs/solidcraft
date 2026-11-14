//! Dragging a part that joints place moves it as far as its joints allow: each frame runs
//! `joint.reach`, taking the point grabbed toward the pointer in the plane through it that faces
//! the view. The whole drag is one undo step.
//!
//! A part that can only turn about a point (a ball joint to a part fixed in place) turns like a
//! trackball instead (#13262): each frame asks for the smallest turn about the pivot that moves
//! the grabbed point along with the pointer, the part's axis turned with it, measured on a sphere
//! at least the part's size. The point alone leaves the part free to spin about the line through
//! the pivot and the point, and a grab near the pivot would turn it far for a small move.

use egui::Pos2;
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::doc::joints::JointKind;
use solidcraft_engine::doc::{apply_point, apply_vector, joints, mat_inverse};
use solidcraft_engine::geom::Aabb3;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::viewport::Proj;

/// A drag under way.
#[derive(Clone, Debug)]
pub struct Drag {
    occurrence: u64,
    /// The point grabbed, in the occurrence's own coordinates.
    local: Vec3,
    /// The plane the pointer moves the point in: through where it was grabbed, facing the view.
    origin: Vec3,
    normal: Vec3,
    /// The drag's undo step is on top of the history (it went in with the first frame that moved).
    pushed: bool,
    /// The pivot it turns about, when it only turns about one (and the preference is on).
    pivot: Option<Pivot>,
    /// Where the pointer was in the plane last frame.
    pointer: Vec3,
}

/// The point a part turns about, in the world, and the least radius a pointer's move turns it
/// on: the distance from the pivot to the far side of the part's box.
#[derive(Clone, Copy, Debug)]
struct Pivot {
    at: Vec3,
    radius: f64,
}

/// The pivot of a part held by a ball joint to a part fixed in place: nothing else can move that
/// point of it, so it only turns about it.
fn pivot(app: &SolveApp, occurrence: u64, fixed: &[u64]) -> Option<Pivot> {
    let s = &app.session;
    let st = s.model.state();
    let own = s.doc.assembly.joints.iter().filter(|j| !j.suppressed && j.kind == JointKind::Ball).find_map(|j| {
        let (own, other) = if j.a.occurrence == occurrence { (&j.a, &j.b) } else { (&j.b, &j.a) };
        (own.occurrence == occurrence && fixed.contains(&other.occurrence)).then_some(own)
    })?;
    let at = apply_point(&s.doc.origin_world(&st, own).ok()?, Vec3::ZERO);
    let world = joints::occurrence_world(&s.doc, occurrence);
    let comp = s.doc.occurrences.iter().find(|o| o.id == occurrence).map(|o| o.component)?;
    let bounds = st.bodies.iter().filter(|b| s.doc.model_body_component(b) == comp).fold(Aabb3::EMPTY, |bb, b| bb.union(&b.mesh().bounds()));
    let (lo, hi) = (bounds.min, bounds.max);
    let radius = [lo.x, hi.x]
        .into_iter()
        .flat_map(|x| [lo.y, hi.y].into_iter().flat_map(move |y| [lo.z, hi.z].into_iter().map(move |z| Vec3::new(x, y, z))))
        .map(|c| apply_point(&world, c).dist(at))
        .filter(|d| d.is_finite())
        .fold(0.0, f64::max);
    at.is_finite().then_some(Pivot { at, radius })
}

/// `v` turned by `angle` (radians) about the unit axis `k`.
fn turned(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    v * cos + k.cross(v) * sin + k * (k.dot(v) * (1.0 - cos))
}

/// Begin a drag on the part under `p` when joints place it and leave it free to move (it is
/// neither grounded nor fixed to what is). True when one began.
pub fn start(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    let Some((body, at)) = crate::occurrence_drag::grabbed_face(app, proj, p) else { return false };
    let s = &app.session;
    let Some(occurrence) = crate::dialogs_assembly::occurrence_of_body(s, &body).filter(|o| *o != 0) else { return false };
    if !s.doc.assembly.joints.iter().any(|j| !j.suppressed && (j.a.occurrence == occurrence || j.b.occurrence == occurrence)) {
        return false;
    }
    let sol = joints::solve(&s.doc, &s.model.state());
    if !sol.world.contains_key(&occurrence) || sol.dof.get(&occurrence).is_none_or(|d| *d == 0) {
        return false;
    }
    let Some(local) = mat_inverse(&joints::occurrence_world(&s.doc, occurrence)).map(|inv| apply_point(&inv, at)) else { return false };
    let normal = proj.ray(p).1;
    let pivot = if app.preferences.joint_drag_trackball { pivot(app, occurrence, &sol.fixed) } else { None };
    app.viewport.joint_drag = Some(Drag { occurrence, local, origin: at, normal, pushed: false, pivot, pointer: at });
    true
}

pub fn active(app: &SolveApp) -> bool {
    app.viewport.joint_drag.is_some()
}

/// Reach for the point under `p`.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(d) = app.viewport.joint_drag.clone() else { return };
    let (o, dir) = proj.ray(p);
    let facing = dir.dot(d.normal);
    if facing.abs() < 1e-9 {
        return;
    }
    let target = o + dir * ((d.origin - o).dot(d.normal) / facing);
    if !target.is_finite() {
        return;
    }
    let world = joints::occurrence_world(&app.session.doc, d.occurrence);
    let at = apply_point(&world, d.local);
    let mut params =
        json!({"occurrence": d.occurrence, "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z], "quiet": true, "continuous": true});
    if let Some(pv) = d.pivot {
        if let Some(drag) = app.viewport.joint_drag.as_mut() {
            drag.pointer = target;
        }
        // The pointer's move across the line from the pivot, as a turn on a sphere about it.
        let arm = at - pv.at;
        let Some(out) = arm.normalized() else { return };
        let moved = target - d.pointer;
        let across = moved - out * moved.dot(out);
        let angle = across.len() / arm.len().max(pv.radius);
        let Some(k) = out.cross(across).normalized().filter(|_| angle.is_finite() && angle > 0.0) else { return };
        let (to, z) = (pv.at + turned(arm, k, angle), turned(apply_vector(&world, Vec3::Z), k, angle));
        params["point"] = json!([to.x, to.y, to.z]);
        params["z"] = json!([z.x, z.y, z.z]);
    }
    // One undo step for the whole drag: the first frame's, kept on top while the others run (not
    // truncated by depth, which a full history would make lose its oldest steps frame by frame).
    let own = if d.pushed { app.session.undo.pop() } else { None };
    // A frame that changes the design adds an undo step (a full history drops its oldest for it,
    // so its length says nothing).
    let before = app.session.doc.clone();
    let pushed = app.run("joint.reach", params).is_ok() && !std::sync::Arc::ptr_eq(&before, &app.session.doc);
    match own {
        Some(step) => {
            if pushed {
                app.session.undo.pop();
            }
            app.session.undo.push(step);
        }
        None if pushed => {
            if let Some(d) = app.viewport.joint_drag.as_mut() {
                d.pushed = true;
            }
        }
        None => {}
    }
}

pub fn end(app: &mut SolveApp) {
    app.viewport.joint_drag = None;
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcore_record::Vec3;
    use solidcraft_engine::Sel;
    use solidcraft_engine::doc::{apply_point, joints};
    use solidcraft_engine::geom::prelude::*;

    use crate::scenario::Harness;
    use crate::test_support::play;

    fn run(h: &mut Harness, id: &str, p: Value) -> Value {
        h.app.run(id, p.clone()).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn screen(h: &mut Harness, p: Vec3) -> egui::Pos2 {
        let [x, y] = h.at(&json!({"world": [p.x, p.y, p.z]})).expect("on screen");
        egui::pos2(x as f32, y as f32)
    }

    /// Where the arm's point `local` (its own coordinates) is.
    fn arm_at(h: &Harness, local: Vec3) -> Vec3 {
        let s = &h.app.session;
        let c = s.doc.find_component("Arm").unwrap_or(0);
        let o = s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0);
        apply_point(&joints::occurrence_world(&s.doc, o), local)
    }

    /// An arm hinged about +Z on a grounded base (seen from the top): its far end, and the screen
    /// points of that end and of the end turned a quarter round.
    fn hinged_arm() -> (Harness, Vec3, egui::Pos2, egui::Pos2) {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (name, corner, size) in [("Base", [-5, -5, 0], [10, 10, 5]), ("Arm", [-3, -3, 5], [63, 6, 5])] {
            run(&mut h, "component.activate", json!({"component": "root"}));
            run(&mut h, "component.create", json!({"name": name}));
            run(&mut h, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
        }
        run(&mut h, "component.activate", json!({"component": "root"}));
        let base = h.app.session.doc.find_component("Base").and_then(|c| h.app.session.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        run(&mut h, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
        let at = |o: &str| json!({"occurrence": o, "point": [0, 0, 5], "z": [0, 0, 1]});
        run(&mut h, "joint.create", json!({"type": "revolute", "a": at("Base"), "b": at("Arm"), "flip": true, "name": "Hinge"}));
        h.call("ui.view", json!({"view": "top"}));
        h.app.cam.distance = 200.0;
        h.frames(3);
        let end = Vec3::new(55.0, 0.0, 10.0);
        let (a, b) = (screen(&mut h, end), screen(&mut h, Vec3::new(0.0, 55.0, 10.0)));
        (h, end, a, b)
    }

    /// Press at `a` and move to `b` in 12 frames, holding the button there.
    fn press_and_move(h: &mut Harness, a: egui::Pos2, b: egui::Pos2) {
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: a,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.app.synthetic.extend((1..=12).map(|k| egui::Event::PointerMoved(a + (b - a) * (k as f32 / 12.0))));
        play(h);
    }

    fn release(h: &mut Harness, b: egui::Pos2) {
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: b,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        play(h);
    }

    /// Dragged by its far end a quarter turn round, the arm turns there in one undo step, and undo
    /// turns it back.
    #[test]
    fn dragging_a_jointed_part_turns_it_about_its_hinge() {
        let (mut h, end, a, b) = hinged_arm();
        let undo = h.app.session.undo.len();
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the arm, not a selection box");
        release(&mut h, b);
        assert!(h.app.viewport.joint_drag.is_none());
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "{:?}", arm_at(&h, end));
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        assert!(!h.app.session.log.iter().any(|l| l.contains("Reach")), "frames are not echoed: {:?}", h.app.session.log);
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9, "{:?}", arm_at(&h, end));
    }

    /// #15648: a grounded cover over the arm's far end hides that end from above. With nothing
    /// selected, a drag there draws a selection box; with the arm selected in the Browser, the
    /// same drag turns the arm.
    #[test]
    fn a_selected_jointed_part_turns_where_a_grounded_part_is_in_front() {
        let (mut h, end, a, b) = hinged_arm();
        let cover = run(&mut h, "component.create", json!({"name": "Cover"}))["occurrence"].as_u64().expect("occurrence");
        run(&mut h, "solid.box", json!({"length": 10, "width": 10, "height": 15, "corner": [50, -5, 0]}));
        run(&mut h, "component.activate", json!({"component": "root"}));
        run(&mut h, "occurrence.ground", json!({"occurrence": cover, "grounded": true}));
        run(&mut h, "select.set", json!({"items": []}));
        h.frames(2);
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_none() && h.app.viewport.boxsel.is_some(), "with nothing selected, the cover in front takes the drag");
        release(&mut h, b);
        assert!(arm_at(&h, end).dist(end) < 1e-9, "{:?}", arm_at(&h, end));
        let arm = h.app.session.doc.find_component("Arm").and_then(|c| h.app.session.doc.occurrence_of(c)).map(|o| o.id).expect("the arm");
        run(&mut h, "select.set", json!({"items": [Sel::Occurrence { path: vec![arm] }]}));
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag turns the selected arm");
        release(&mut h, b);
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "{:?}", arm_at(&h, end));
    }

    /// #5137: while a part is dragged along its joint, nothing under the pointer highlights; hover
    /// comes back once the drag ends.
    #[test]
    fn dragging_a_jointed_part_highlights_nothing_under_the_pointer() {
        let (mut h, _, a, b) = hinged_arm();
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        play(&mut h);
        assert!(h.app.viewport.hover.is_some(), "the arm under the pointer must highlight before the drag");
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some(), "the drag must move the arm");
        assert!(h.app.viewport.hover.is_none(), "dragging a part must not highlight what is under the pointer");
        h.frames(2);
        assert!(h.app.viewport.hover.is_none(), "a drag held still must not highlight either");
        release(&mut h, b);
        h.frames(2);
        assert!(h.app.viewport.hover.is_some(), "hover must return after the drag");
    }

    /// A ball (a 20 mm cube) on a ball joint at the origin, pitch limited to ±28°, on a grounded
    /// socket, seen from the front.
    fn ball_on_socket(trackball: bool) -> Harness {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.joint_drag_trackball = trackball;
        for (name, corner, size) in [("Socket", [-5, -5, -10], [10, 10, 10]), ("Ball", [-10, -10, 0], [20, 20, 20])] {
            run(&mut h, "component.activate", json!({"component": "root"}));
            run(&mut h, "component.create", json!({"name": name}));
            run(&mut h, "solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner}));
        }
        run(&mut h, "component.activate", json!({"component": "root"}));
        let socket = h.app.session.doc.find_component("Socket").and_then(|c| h.app.session.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        run(&mut h, "occurrence.ground", json!({"occurrence": socket, "grounded": true}));
        let at = |o: &str| json!({"occurrence": o, "point": [0, 0, 0], "z": [0, 0, 1]});
        run(&mut h, "joint.create", json!({"type": "ball", "a": at("Socket"), "b": at("Ball"), "flip": true, "name": "Ball joint"}));
        run(&mut h, "joint.limits", json!({"joint": "Ball joint", "index": 1, "min": -28, "max": 28}));
        h.call("ui.view", json!({"view": "front"}));
        h.app.cam.distance = 150.0;
        h.frames(3);
        h
    }

    /// The ball's axes in the world.
    fn ball_axes(h: &Harness) -> [Vec3; 3] {
        let s = &h.app.session;
        let o = s.doc.find_component("Ball").and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        let m = joints::occurrence_world(&s.doc, o);
        [Vec3::X, Vec3::Y, Vec3::Z].map(|v| solidcraft_engine::doc::apply_vector(&m, v))
    }

    /// The largest turn of the ball in one frame (degrees), and its whole turn, dragged from the
    /// point `grab` (world) by `by` on screen in 40 frames.
    fn drag_ball(h: &mut Harness, grab: Vec3, by: egui::Vec2) -> (f64, f64) {
        let start = ball_axes(h);
        let a = screen(h, grab);
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        h.app.synthetic.push(egui::Event::PointerButton {
            pos: a,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        // A few pixels, as egui needs to start a drag.
        h.app.synthetic.extend((1..=4).map(|k| egui::Event::PointerMoved(a + by * (k as f32 / 160.0))));
        play(h);
        let turn = |p: &[Vec3; 3], q: &[Vec3; 3]| {
            let trace: f64 = p.iter().zip(q).map(|(u, v)| u.dot(*v)).sum();
            ((trace - 1.0) / 2.0).clamp(-1.0, 1.0).acos().to_degrees()
        };
        let mut worst = 0.0f64;
        for k in 2..=40 {
            let before = ball_axes(h);
            h.app.synthetic.push(egui::Event::PointerMoved(a + by * (k as f32 / 40.0)));
            play(h);
            worst = worst.max(turn(&before, &ball_axes(h)));
        }
        assert!(h.app.viewport.joint_drag.is_some(), "the drag must move the ball");
        release(h, a + by);
        (worst, turn(&start, &ball_axes(h)))
    }

    /// #13262: dragged by its side near the joint, a part on a ball joint turns smoothly, frame by
    /// frame, and still turns as far as the pointer leads it. With the preference off it is the
    /// drag from before, which twists the part far in a single frame.
    #[test]
    fn a_part_on_a_ball_joint_turns_smoothly_when_dragged_near_the_joint() {
        // Beside the origin's Z axis, which a click on it would pick.
        let grab = Vec3::new(-4.0, -10.0, 3.0);
        let mut smooth = ball_on_socket(true);
        let (worst, whole) = drag_ball(&mut smooth, grab, egui::vec2(120.0, -120.0));
        // About 1.2° a frame, up to 2.9° along the cone's edge.
        assert!(worst < 4.0, "the largest turn in a frame: {worst}°");
        assert!(whole > 20.0, "the ball turned only {whole}°");
        let mut before = ball_on_socket(false);
        let (worst, _) = drag_ball(&mut before, grab, egui::vec2(120.0, -120.0));
        // 3.3° to 7.2° a frame for the same moves: it spins about the grabbed point as well.
        assert!(worst > 6.0, "with the preference off the drag is as before: {worst}°");
    }

    /// With the undo history full, a drag still adds one step: it drops the oldest once, not one
    /// per frame.
    #[test]
    fn a_drag_on_a_full_undo_history_drops_one_step() {
        let (mut h, _, a, b) = hinged_arm();
        let step = h.app.session.undo.last().cloned().expect("an undo step");
        while h.app.session.undo.len() < 200 {
            let mut older = step.clone();
            older.label = format!("step {}", h.app.session.undo.len());
            h.app.session.undo.insert(0, older);
        }
        let second = h.app.session.undo.get(1).map(|s| s.label.clone());
        press_and_move(&mut h, a, b);
        release(&mut h, b);
        assert_eq!(h.app.session.undo.len(), 200);
        assert_eq!(h.app.session.undo.first().map(|s| s.label.clone()), second, "only the oldest step went");
        assert_eq!(h.app.session.undo.last().map(|s| s.label.as_str()), Some("Reach Point"), "the drag's step is on top");
    }
}
