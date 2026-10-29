//! Dragging a part that joints place moves it as far as its joints allow: the point grabbed goes
//! toward the pointer in the plane through it that faces the view. Each frame solves the reach on
//! the drag's own copy of the design and only shows it (`Session::drag_preview`); the release
//! applies the last frame as one `joint.reach`, the drag's one undo step, and Escape leaves the
//! design as it was (#6325).

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::Pos2;
use serde_json::json;
use solidcraft_engine::DragPreview;
use solidcraft_engine::doc::{Document, Mat, apply_point, joints, mat_inverse};
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
    /// The design the drag began on. A change to it from elsewhere (undo pressed mid-drag) ends
    /// the drag without applying it.
    design: Arc<Document>,
    /// The design with the joint values and placements the last frame reached: where the next
    /// frame starts from, and what the view shows.
    pose: Document,
    /// The last frame's start (every joint's values) and the point it reached for: the release
    /// does that same reach, so the design ends where the view showed it.
    last: Option<(BTreeMap<u64, Vec<f64>>, Vec3)>,
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
    let (design, pose) = (s.doc.clone(), (*s.doc).clone());
    app.viewport.joint_drag = Some(Drag { occurrence, local, origin: at, normal, design, pose, last: None });
    true
}

pub fn active(app: &SolveApp) -> bool {
    app.viewport.joint_drag.is_some()
}

/// Every joint's values in `doc`.
fn values(doc: &Document) -> BTreeMap<u64, Vec<f64>> {
    doc.assembly.joints.iter().map(|j| (j.id, j.values.clone())).collect()
}

/// The occurrences `pose` places otherwise than `design`, and where (in their parents).
fn moves(design: &Document, pose: &Document) -> BTreeMap<u64, Mat> {
    let mine: BTreeMap<u64, &Mat> = design.occurrences.iter().map(|o| (o.id, &o.transform)).collect();
    pose.occurrences.iter().filter(|o| mine.get(&o.id).is_some_and(|m| **m != o.transform)).map(|o| (o.id, o.transform)).collect()
}

/// Reach for the point under `p`: solved on the drag's copy of the design and shown.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(mut d) = app.viewport.joint_drag.take() else { return };
    if !Arc::ptr_eq(&d.design, &app.session.doc) {
        app.session.drag_preview = None;
        return;
    }
    let (o, dir) = proj.ray(p);
    let facing = dir.dot(d.normal);
    let target = o + dir * ((d.origin - o).dot(d.normal) / facing);
    if facing.abs() >= 1e-9 && target.is_finite() {
        let start = values(&d.pose);
        let reach = joints::Reach { occurrence: d.occurrence, point: d.local, to: target, axis: None, joints: None };
        // A reach that can't move (a limit, a loop) leaves the pose where it was.
        let _ = solidcraft_engine::cmd::reach_preview(&mut d.pose, &app.session.model.state(), &reach);
        d.last = Some((start, target));
        app.session.drag_preview = Some(DragPreview { design: d.design.clone(), moves: moves(&d.design, &d.pose) });
    }
    app.viewport.joint_drag = Some(d);
}

/// Let go: one `joint.reach` puts the design where the last frame showed it.
pub fn end(app: &mut SolveApp) {
    let Some(d) = app.viewport.joint_drag.take() else { return };
    app.session.drag_preview = None;
    let Some((start, target)) = d.last else { return };
    if !Arc::ptr_eq(&d.design, &app.session.doc) {
        return;
    }
    let at = apply_point(&joints::occurrence_world(&app.session.doc, d.occurrence), d.local);
    let mut params =
        json!({"occurrence": d.occurrence, "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z], "quiet": true, "continuous": true});
    let from = solidcraft_engine::cmd::reach_from(&d.design, &start);
    if from.as_array().is_some_and(|f| !f.is_empty()) {
        params["from"] = from;
    }
    let _ = app.run("joint.reach", params);
}

/// Escape: the part goes back where it was, and the drag leaves no undo step. True when a drag
/// was cancelled.
pub fn cancel(app: &mut SolveApp) -> bool {
    if app.viewport.joint_drag.take().is_none() {
        return false;
    }
    app.session.drag_preview = None;
    true
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

    /// The far end of the arm's body where the world state (the view) shows it.
    fn shown_arm_reach(h: &Harness) -> f64 {
        let s = &h.app.session;
        let c = s.doc.find_component("Arm").unwrap_or(0);
        let w = s.world_state();
        w.bodies.iter().filter(|b| b.component == c).map(|b| b.mesh().bounds().max.y).fold(f64::NEG_INFINITY, f64::max)
    }

    /// #6325: the frames of a drag only show the arm moving: the design, its history and its
    /// evaluation stay as they were until the release applies the last frame in one `joint.reach`,
    /// which leaves the arm where that frame showed it and where a fresh reach puts it.
    #[test]
    fn a_drag_shows_its_frames_and_applies_only_the_last() {
        let (mut h, end, a, b) = hinged_arm();
        let (doc, undo, revision) = (h.app.session.doc.clone(), h.app.session.undo.len(), h.app.session.revision);
        press_and_move(&mut h, a, b);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc), "a drag frame changes no design");
        assert_eq!((h.app.session.undo.len(), h.app.session.revision), (undo, revision), "nor the history");
        assert!(arm_at(&h, end).dist(end) < 1e-9, "the design keeps the arm where it was");
        let shown = shown_arm_reach(&h);
        assert!((shown - 60.0).abs() < 0.5, "the view shows the arm turned a quarter round: {shown}");
        let target = h.app.viewport.joint_drag.as_ref().and_then(|d| d.last.as_ref().map(|l| l.1)).expect("a frame");
        release(&mut h, b);
        assert!(h.app.session.drag_preview.is_none());
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        assert!((shown_arm_reach(&h) - shown).abs() < 1e-6, "the design ends where the last frame showed it");
        let applied = arm_at(&h, end);
        run(&mut h, "edit.undo", json!({}));
        let at = arm_at(&h, end);
        let fresh = run(&mut h, "joint.reach", json!({"occurrence": "Arm", "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z]}));
        assert!(arm_at(&h, end).dist(applied) < 1e-6, "{:?} vs {applied:?} ({fresh})", arm_at(&h, end));
    }

    /// #6325: Escape in a drag leaves the design as it was, with no undo step.
    #[test]
    fn escape_cancels_a_joint_drag() {
        let (mut h, end, a, b) = hinged_arm();
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        let before = shown_arm_reach(&h);
        press_and_move(&mut h, a, b);
        assert!((shown_arm_reach(&h) - before).abs() > 10.0, "the arm is shown turned while dragged");
        for pressed in [true, false] {
            h.app.synthetic.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            });
        }
        play(&mut h);
        assert!(h.app.viewport.joint_drag.is_none() && h.app.session.drag_preview.is_none());
        assert!((shown_arm_reach(&h) - before).abs() < 1e-9, "the view shows the arm back where it was");
        release(&mut h, b);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc), "the release after Escape changes nothing");
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(arm_at(&h, end).dist(end) < 1e-9);
    }

    /// #6325: a change to the design from elsewhere mid-drag (undo) ends the drag: its frame is no
    /// longer shown, and the release applies nothing.
    #[test]
    fn a_design_changed_mid_drag_ends_the_drag() {
        let (mut h, end, a, b) = hinged_arm();
        press_and_move(&mut h, a, b);
        run(&mut h, "edit.undo", json!({}));
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        assert!(h.app.session.drag_moves().is_empty(), "the frame is for the design before the undo");
        h.app.synthetic.push(egui::Event::PointerMoved(a));
        play(&mut h);
        assert!(h.app.viewport.joint_drag.is_none());
        release(&mut h, b);
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc) && h.app.session.undo.len() == undo, "the release applies nothing");
        assert!(arm_at(&h, end).dist(end) < 1e-9);
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
