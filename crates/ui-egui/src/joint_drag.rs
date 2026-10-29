//! Dragging a part that joints place moves it as far as its joints allow: each frame runs
//! `joint.reach`, taking the point grabbed toward the pointer in the plane through it that faces
//! the view. The whole drag is one undo step.

use egui::Pos2;
use serde_json::json;
use solidcraft_engine::doc::{apply_point, joints, mat_inverse};
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

/// A drag under way.
#[derive(Clone, Debug)]
pub struct Drag {
    occurrence: u64,
    /// The point grabbed, in the occurrence's own coordinates.
    local: Vec3,
    /// The plane the pointer moves the point in: through where it was grabbed, facing the view.
    origin: Vec3,
    normal: Vec3,
    /// The undo history's mark when the drag began: its frames fold into one step.
    mark: solidcraft_engine::Mark,
}

/// Begin a drag on the part under `p` when joints place it and leave it free to move (it is
/// neither grounded nor fixed to what is). True when one began.
pub fn start(app: &mut SolveApp, proj: &Proj, p: Pos2) -> bool {
    // The face under the pointer, past the part's own edges and vertices (not past sketches or
    // construction geometry in front of it).
    let hits = crate::viewport::pick(app, proj, p);
    let Some((body, at)) =
        hits.into_iter().take_while(|h| matches!(h, Hit::Face { .. } | Hit::Edge { .. } | Hit::Vertex { .. })).find_map(|h| match h {
            Hit::Face { body, point, .. } => Some((body, point)),
            _ => None,
        })
    else {
        return false;
    };
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
    app.viewport.joint_drag = Some(Drag { occurrence, local, origin: at, normal, mark: app.session.history.mark() });
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
    let at = apply_point(&joints::occurrence_world(&app.session.doc, d.occurrence), d.local);
    let params =
        json!({"occurrence": d.occurrence, "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z], "quiet": true, "continuous": true});
    // One undo step for the whole drag (a full history loses one old step for it, not one a frame).
    if app.run("joint.reach", params).is_ok() {
        app.session.history.fold_since(d.mark);
    }
}

pub fn end(app: &mut SolveApp) {
    app.viewport.joint_drag = None;
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use solidcraft_engine::doc::{apply_point, joints};
    use solidcraft_engine::geom::Vec3;

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
        let undo = h.app.session.history.len();
        press_and_move(&mut h, a, b);
        assert!(h.app.viewport.joint_drag.is_some() && h.app.viewport.boxsel.is_none(), "the drag moves the arm, not a selection box");
        release(&mut h, b);
        assert!(h.app.viewport.joint_drag.is_none());
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "{:?}", arm_at(&h, end));
        assert_eq!(h.app.session.history.len(), undo + 1, "one undo step for the drag");
        assert!(!h.app.session.log.iter().any(|l| l.contains("Reach")), "frames are not echoed: {:?}", h.app.session.log);
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9, "{:?}", arm_at(&h, end));
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

    /// With the undo history full, a drag still adds one step: it drops the oldest once, not one
    /// per frame.
    #[test]
    fn a_drag_on_a_full_undo_history_drops_one_step() {
        let (mut h, _, a, b) = hinged_arm();
        let step = h.app.session.history.last().cloned().expect("an undo step");
        let existing = h.app.session.history.steps().to_vec();
        h.app.session.history.clear();
        for i in 0..200 - existing.len() {
            h.app.session.history.push(solidcraft_engine::Snapshot { label: format!("step {i}"), ..step.clone() });
        }
        for step in existing {
            h.app.session.history.push(step);
        }
        let second = h.app.session.history.steps().get(1).map(|s| s.label.clone());
        press_and_move(&mut h, a, b);
        release(&mut h, b);
        assert_eq!(h.app.session.history.len(), 200);
        assert_eq!(h.app.session.history.steps().first().map(|s| s.label.clone()), second, "only the oldest step went");
        assert_eq!(h.app.session.history.last().map(|s| s.label.as_str()), Some("Reach Point"), "the drag's step is on top");
    }
}
