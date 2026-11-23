//! Dragging a part that joints place moves it as far as its joints allow: the point grabbed goes
//! toward the pointer in the plane through it that faces the view. Each frame solves the reach on
//! the drag's own copy of the design and only shows it (`Session::drag_preview`); the release
//! applies the last frame as one `joint.reach`, the drag's one undo step, and Escape leaves the
//! design as it was (#6325).
//!
//! With contact sets on, a drag stops where two parts of a set touch (#14682). Only `joint.reach`
//! itself checks that, so such a drag runs the command on the design at each frame, as every drag
//! did before: its first frame that moves adds the undo step, the later frames keep that one, and
//! Escape ends it where it is, as letting go does. So does a drag begun while moves are shown but not captured (a free
//! move, an exploded view), whose shown frames would start from where the design has the parts,
//! one in a design with more joints than the release's `from` takes, and one with the assembly's
//! Move handles open or a component active in its place.
//!
//! A part that can only turn about a point (a ball joint to a part fixed in place) turns like a
//! trackball instead (#13262): each frame asks for the smallest turn about the pivot that moves
//! the grabbed point along with the pointer, the part's axis turned with it, measured on a sphere
//! at least the part's size. The point alone leaves the part free to spin about the line through
//! the pivot and the point, and a grab near the pivot would turn it far for a small move.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use egui::Pos2;
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_engine::DragPreview;
use solidcraft_engine::doc::joints::JointKind;
use solidcraft_engine::doc::{Document, Mat, ModelState, apply_point, apply_vector, joints, mat_inverse};
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
    /// The pivot it turns about, when it only turns about one (and the preference is on).
    pivot: Option<Pivot>,
    /// Where the pointer was in the plane last frame.
    pointer: Vec3,
    /// How its frames reach the design.
    frames: Frames,
}

/// How a drag's frames reach the design.
#[derive(Clone, Debug)]
enum Frames {
    /// Each frame is solved on the drag's own copy of the design and shown; the release applies
    /// the last one (#6325).
    Shown(Box<Shown>),
    /// Each frame runs `joint.reach` on the design, as before #6325 ([`on_the_design`]).
    /// `pushed`: the drag's undo step is on top of the history (it went in with the first frame
    /// that moved).
    Applied { pushed: bool },
}

/// A drag whose frames are shown before the release applies the last.
#[derive(Clone, Debug)]
struct Shown {
    /// The design the drag began on. A change to it from elsewhere (undo pressed mid-drag) ends
    /// the drag without applying it.
    design: Arc<Document>,
    /// Public provenance of the solve baseline, frozen for this gesture.
    document_id: String,
    revision: u64,
    /// The world shown when the drag began and the moves not captured then: the viewport draws
    /// the drag's frames from it, the moving parts moved on the GPU
    /// (`viewport_joint_motions.rs`).
    world: Arc<ModelState>,
    pending: BTreeMap<u64, Mat>,
    /// The design with the joint values and placements the last frame reached: where the next
    /// frame starts from, and what the view shows.
    pose: Document,
    /// The last frame's start (every joint's values) and what it reached for. The release asks
    /// for the pose that frame showed instead (`end`).
    last: Option<(BTreeMap<u64, Vec<f64>>, Aim)>,
    /// What one frame's reach hands to the next (`joints::reach_frame`).
    carry: joints::FrameCarry,
    /// Keep membership through a failed frame and a return to the starting pose.
    reported: BTreeSet<u64>,
    observed: Option<crate::preview::JointPose>,
    error: Option<String>,
}

/// What a frame reaches for: where the grabbed point goes (world) and, for a part turning about
/// a pivot, the unit direction (world) its own Z axis then points.
#[derive(Clone, Copy, Debug)]
struct Aim {
    to: Vec3,
    z: Option<Vec3>,
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

/// Whether a drag's frames run on the design, as every drag's did before #6325, rather than
/// being shown and applied on release:
/// - with contact sets on, which stop a drag where parts touch: only the command checks that;
/// - while moves are shown but not captured (a free move, an exploded view): shown frames would
///   solve from the design's placements, not the ones shown;
/// - in a design with more joints than the release's `from` takes, which could not say where the
///   last frame started;
/// - with the assembly's Move handles open, or a component active in its place in the assembly:
///   the handles, and the sketches of the active component, are placed from the design.
fn on_the_design(app: &SolveApp) -> bool {
    let s = &app.session;
    s.doc.assembly.contact_enabled
        || !s.pending_moves.is_empty()
        || s.doc.assembly.joints.len() > solidcraft_engine::cmd::MAX_REACH_JOINTS
        || app.viewport.joint_handles.is_some()
        || (s.active_component != 0 && s.active_occurrence_path.is_some())
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
    let revision = app.session.design_revision();
    let s = &app.session;
    let frames = if on_the_design(app) {
        Frames::Applied { pushed: false }
    } else {
        Frames::Shown(Box::new(Shown {
            design: s.doc.clone(),
            document_id: s.document_id.clone(),
            revision,
            world: crate::open_meshing::session_world(s),
            pending: s.pending_moves.clone(),
            pose: (*s.doc).clone(),
            last: None,
            carry: joints::FrameCarry::default(),
            reported: BTreeSet::new(),
            observed: None,
            error: None,
        }))
    };
    // The face under the pointer is not lit from here on (#5137): gone now rather than when the
    // frame gets to it, so the drag's first frame is already drawn by moving the bodies.
    app.viewport.hover = None;
    app.viewport.joint_drag = Some(Drag { occurrence, local, origin: at, normal, pivot, pointer: at, frames });
    true
}

/// Whether a part is being dragged along its joints.
pub fn active(app: &SolveApp) -> bool {
    app.viewport.joint_drag.is_some()
}

/// The completed CPU pose of the active scratch drag, only for its original public baseline.
/// Applied drags already edit the design; they have no scratch pose to report.
pub(crate) fn pose_snapshot(app: &SolveApp, revision: u64) -> Option<(Option<&crate::preview::JointPose>, Option<&str>)> {
    let Frames::Shown(sh) = &app.viewport.joint_drag.as_ref()?.frames else { return None };
    if sh.document_id != app.session.document_id
        || sh.revision != revision
        || !Arc::ptr_eq(&sh.design, &app.session.doc)
        || !app.session.drag_preview.as_ref().is_some_and(|preview| Arc::ptr_eq(&preview.design, &sh.design))
    {
        return None;
    }
    Some((sh.observed.as_ref(), sh.error.as_deref()))
}

/// The design, world and uncaptured moves the drag under way began on, when its frames are shown
/// rather than run on the design.
pub(crate) fn began_on(app: &SolveApp) -> Option<(&Arc<Document>, &Arc<ModelState>, &BTreeMap<u64, Mat>)> {
    match &app.viewport.joint_drag.as_ref()?.frames {
        Frames::Shown(sh) => Some((&sh.design, &sh.world, &sh.pending)),
        Frames::Applied { .. } => None,
    }
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

/// What a frame reaches for with the pointer at `target` in the drag's plane, the part placed by
/// `world` now: the point under the pointer, or, for a part that only turns about a pivot, the
/// pointer's move across the line from the pivot as a turn on a sphere about it. `None` when the
/// pointer's move turns nothing.
fn aim(d: &Drag, world: &Mat, target: Vec3) -> Option<Aim> {
    let Some(pv) = d.pivot else { return Some(Aim { to: target, z: None }) };
    let arm = apply_point(world, d.local) - pv.at;
    let out = arm.normalized()?;
    let moved = target - d.pointer;
    let across = moved - out * moved.dot(out);
    let angle = across.len() / arm.len().max(pv.radius);
    let k = out.cross(across).normalized().filter(|_| angle.is_finite() && angle > 0.0)?;
    let z = turned(apply_vector(world, Vec3::Z), k, angle).normalized()?;
    Some(Aim { to: pv.at + turned(arm, k, angle), z: Some(z) })
}

/// `joint.reach`'s parameters for a frame of a drag: the occurrence's point now at `at` (world)
/// goes where `aim` says.
fn reach_params(occurrence: u64, at: Vec3, aim: Aim) -> Value {
    let mut params =
        json!({"occurrence": occurrence, "at": [at.x, at.y, at.z], "point": [aim.to.x, aim.to.y, aim.to.z], "quiet": true, "continuous": true});
    if let Some(z) = aim.z {
        params["z"] = json!([z.x, z.y, z.z]);
    }
    params
}

/// Reach for the point under `p`: solved on the drag's copy of the design and shown, or, with
/// contact sets on, run on the design.
pub fn to(app: &mut SolveApp, proj: &Proj, p: Pos2) {
    let Some(mut d) = app.viewport.joint_drag.take() else { return };
    // Shown frames stand for the design the drag began on: changed from elsewhere, the drag ends.
    if let Frames::Shown(sh) = &d.frames
        && !Arc::ptr_eq(&sh.design, &app.session.doc)
    {
        app.session.drag_preview = None;
        return;
    }
    let (o, dir) = proj.ray(p);
    let facing = dir.dot(d.normal);
    let target = o + dir * ((d.origin - o).dot(d.normal) / facing);
    if facing.abs() >= 1e-9 && target.is_finite() {
        let world = match &d.frames {
            Frames::Shown(sh) => joints::occurrence_world(&sh.pose, d.occurrence),
            Frames::Applied { .. } => joints::occurrence_world(&app.session.doc, d.occurrence),
        };
        let aimed = aim(&d, &world, target);
        if d.pivot.is_some() {
            d.pointer = target;
        }
        if let Some(aimed) = aimed {
            match &mut d.frames {
                Frames::Shown(sh) => show(app, d.occurrence, d.local, sh, aimed),
                Frames::Applied { pushed } => *pushed = apply(app, reach_params(d.occurrence, apply_point(&world, d.local), aimed), *pushed),
            }
        }
    }
    app.viewport.joint_drag = Some(d);
}

/// Solve one frame on the drag's copy of the design and show it.
fn show(app: &mut SolveApp, occurrence: u64, local: Vec3, sh: &mut Shown, aim: Aim) {
    let start = values(&sh.pose);
    let reach = joints::Reach { occurrence, point: local, to: aim.to, axis: aim.z.map(|z| (Vec3::Z, z)), joints: None };
    // A reach that can't move (a limit, a loop) leaves the pose where it was.
    let reached = solidcraft_engine::cmd::reach_preview(&mut sh.pose, &app.session.model.state(), &reach, &mut sh.carry);
    sh.last = Some((start, aim));
    app.session.drag_preview = Some(DragPreview { design: sh.design.clone(), moves: moves(&sh.design, &sh.pose) });
    sh.observed = None;
    sh.error = None;
    // Visibility and uncaptured moves can change the public baseline without replacing doc.
    // Never relabel a continuing drag's old solve as a pose of those newer inputs.
    if sh.document_id != app.session.document_id || sh.revision != app.session.design_revision() {
        return;
    }
    match reached {
        Ok(reached) => {
            sh.reported.extend(reached.values.keys().copied());
            let joints = solidcraft_engine::cmd::posed_joints(&sh.design, &sh.pose, sh.reported.iter().copied());
            sh.reported.extend(joints.iter().map(|joint| joint.joint));
            sh.observed = app.preview.completed_joint_pose(Some("joint.reach"), joints);
        }
        // A failed reach can still replace placements. Its old report is not that shown pose.
        Err(error) => sh.error = Some(error),
    }
}

/// Run one frame on the design: `joint.reach` with `params`. One undo step for the whole drag:
/// the first frame's, kept on top while the others run (not truncated by depth, which a full
/// history would make lose its oldest steps frame by frame). `pushed`: that step is on top
/// already; returns whether it is now.
fn apply(app: &mut SolveApp, params: Value, pushed: bool) -> bool {
    let own = if pushed { app.session.undo.pop() } else { None };
    // A frame that changes the design adds an undo step (a full history drops its oldest for it,
    // so its length says nothing).
    let before = app.session.doc.clone();
    let moved = app.run("joint.reach", params).is_ok() && !Arc::ptr_eq(&before, &app.session.doc);
    match own {
        Some(step) => {
            if moved {
                app.session.undo.pop();
            }
            app.session.undo.push(step);
            true
        }
        None => moved,
    }
}

/// Let go: one `joint.reach` puts the design where the last frame showed it. It starts `from` the
/// joint values that frame showed and reaches for where that frame had the grabbed point (and the
/// part's axis), which it is at already: nothing is solved again. Doing the last frame's reach
/// again at the command's full effort ended elsewhere wherever the pointer was out of reach: on a
/// ball joint's cone it slid on along the edge, 10 degrees past what the view showed. A drag
/// whose frames ran on the design has it there already.
pub fn end(app: &mut SolveApp) {
    let Some(d) = app.viewport.joint_drag.take() else { return };
    app.session.drag_preview = None;
    app.viewport.joint_motions = None;
    let Frames::Shown(sh) = d.frames else { return };
    let Some((_, aim)) = sh.last else { return };
    if !Arc::ptr_eq(&sh.design, &app.session.doc) {
        return;
    }
    let at = apply_point(&joints::occurrence_world(&app.session.doc, d.occurrence), d.local);
    let shown = joints::occurrence_world(&sh.pose, d.occurrence);
    let there = Aim { to: apply_point(&shown, d.local), z: aim.z.and_then(|_| apply_vector(&shown, Vec3::Z).normalized()) };
    let mut params = reach_params(d.occurrence, at, there);
    let from = solidcraft_engine::cmd::reach_from(&sh.design, &values(&sh.pose));
    if from.as_array().is_some_and(|f| !f.is_empty()) {
        params["from"] = from;
    }
    let _ = app.run("joint.reach", params);
}

/// Whether Escape cancels the drag under way: one whose frames are shown. A drag whose frames
/// run on the design ends where it is under Escape, as before (egui stops the pointer's drag,
/// which the viewport takes as the release).
pub(crate) fn cancellable(app: &SolveApp) -> bool {
    began_on(app).is_some()
}

/// Escape: a drag whose frames are shown ends, its part back where it was, and leaves no undo
/// step. True when a drag was cancelled.
pub fn cancel(app: &mut SolveApp) -> bool {
    if !cancellable(app) {
        return false;
    }
    app.viewport.joint_drag = None;
    app.session.drag_preview = None;
    app.viewport.joint_motions = None;
    true
}

/// The design leaves the view (another tab comes in): no drag goes on to the next one. A shown
/// drag is cancelled, its frame not left with the design; one whose frames ran on the design
/// has it where its last frame put it and only ends.
pub(crate) fn leave(app: &mut SolveApp) {
    if !cancel(app) {
        app.viewport.joint_drag = None;
    }
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

    /// The ball's axes in the world: where a drag under way shows it, else where the design has it.
    fn ball_axes(h: &Harness) -> [Vec3; 3] {
        let doc = shown(h).map_or(&*h.app.session.doc, |sh| &sh.pose);
        let o = doc.find_component("Ball").and_then(|c| doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
        let m = joints::occurrence_world(doc, o);
        [Vec3::X, Vec3::Y, Vec3::Z].map(|v| solidcraft_engine::doc::apply_vector(&m, v))
    }

    /// The largest turn of the ball in one frame (degrees), and its whole turn, dragged from the
    /// point `grab` (world) by `by` on screen in 40 frames. The release must leave it where the
    /// last frame showed it.
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
        let last = ball_axes(h);
        release(h, a + by);
        // The release asks for the pose the last frame showed, on the cone's edge too.
        let off = turn(&last, &ball_axes(h));
        assert!(off < 1e-3, "the release leaves the ball where the last frame showed it: {off}° off");
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

    /// The drag under way, when its frames are shown rather than run on the design.
    fn shown(h: &Harness) -> Option<&super::Shown> {
        match &h.app.viewport.joint_drag.as_ref()?.frames {
            super::Frames::Shown(sh) => Some(sh),
            super::Frames::Applied { .. } => None,
        }
    }

    /// Press and release Escape.
    fn escape(h: &mut Harness) {
        for pressed in [true, false] {
            h.app.synthetic.push(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            });
        }
        play(h);
    }

    /// A read alone, without advancing the pointer or running another UI frame.
    fn read_pose(h: &mut Harness) -> Value {
        let (req, _) = crate::control::ControlRequest::new("control.joint_pose", json!({}));
        match crate::control::handle(&mut h.app, &h.ctx, &req) {
            crate::control::Outcome::Done(reply) => {
                assert_eq!(reply["ok"], true, "{reply}");
                reply["result"].clone()
            }
            _ => panic!("a joint-pose read does not capture a screenshot"),
        }
    }

    fn observed_angle(reply: &Value, joint: u64) -> f64 {
        let row = reply["pose"]["joints"].as_array().and_then(|rows| rows.iter().find(|row| row["joint"] == joint)).expect("the posed joint");
        assert_eq!(row["values"][0]["unit"], "deg", "{reply}");
        row["values"][0]["value"].as_f64().expect("an angle")
    }

    /// #20458: a reader follows the solved, limited hinge and its linked follower while the
    /// design stays still. Returning to the starting pose still reports both joints at zero.
    #[test]
    fn a_control_reader_follows_a_drag_without_changing_the_design() {
        let (mut h, end, _, _) = hinged_arm();
        let hinge = h.app.session.doc.assembly.joints.iter().find(|joint| joint.name == "Hinge").unwrap().id;
        run(&mut h, "joint.limits", json!({"joint": hinge, "min": 0, "max": 30}));
        run(&mut h, "component.create", json!({"name": "Follower"}));
        run(&mut h, "solid.box", json!({"length": 30, "width": 6, "height": 5, "corner": [-75, -3, 5]}));
        run(&mut h, "component.activate", json!({"component": "root"}));
        let at = |occurrence: &str| json!({"occurrence": occurrence, "point": [-75, 0, 5], "z": [0, 0, 1]});
        let follower = run(
            &mut h,
            "joint.create",
            json!({"type": "revolute", "a": at("Base"), "b": at("Follower"), "flip": true, "name": "Follower hinge"}),
        )["joint"]
            .as_u64()
            .expect("the follower joint");
        run(&mut h, "joint.motion_link", json!({"a": hinge, "b": follower, "ratio": 0.5}));
        h.frames(3);
        let (a, b) = (screen(&mut h, end), screen(&mut h, Vec3::new(0.0, 55.0, 10.0)));
        let before = read_pose(&mut h);
        assert!(before["pose"].is_null());
        let (doc, undo, redo, revision) = (h.app.session.doc.clone(), h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision);
        press_and_move(&mut h, a, b);

        let reached = read_pose(&mut h);
        assert_eq!(reached["pose"]["source"], "joint.reach", "{reached}");
        assert_eq!(reached["document_id"], before["document_id"]);
        assert_eq!(reached["revision"], before["revision"]);
        assert_eq!(reached["busy"], false);
        assert!(reached["error"].is_null());
        assert!((observed_angle(&reached, hinge) - 30.0).abs() < 1e-6, "{reached}");
        assert!((observed_angle(&reached, follower) - 15.0).abs() < 1e-6, "{reached}");
        let row = reached["pose"]["joints"].as_array().unwrap().iter().find(|row| row["joint"] == hinge).unwrap();
        assert_eq!(row["name"], "Hinge");
        assert_eq!(row["values"][0]["limits"], json!([0.0, 30.0]));
        let generation = reached["pose"]["generation"].as_u64().expect("a completed generation");
        assert_eq!(read_pose(&mut h), reached, "reading alone creates no new pose");
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len(), h.app.session.revision), (undo, redo, revision));
        assert!(arm_at(&h, end).dist(end) < 1e-9, "the committed arm has not moved");

        // Pull past the lower stop so both stored values return exactly to their baseline.
        let below = screen(&mut h, Vec3::new(0.0, -55.0, 10.0));
        h.app.synthetic.push(egui::Event::PointerMoved(below));
        play(&mut h);
        let returned = read_pose(&mut h);
        assert!(observed_angle(&returned, hinge).abs() < 1e-6, "{returned}");
        assert!(observed_angle(&returned, follower).abs() < 1e-6, "{returned}");
        let returned_generation = returned["pose"]["generation"].as_u64().unwrap();
        assert!(returned_generation > generation);
        escape(&mut h);
        release(&mut h, a);
        assert!(read_pose(&mut h)["pose"].is_null());
        assert!(std::sync::Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);

        press_and_move(&mut h, a, b);
        let again = read_pose(&mut h);
        assert!(again["pose"]["generation"].as_u64().unwrap() > returned_generation);
        release(&mut h, b);
        assert!(read_pose(&mut h)["pose"].is_null());
        assert_eq!(h.app.session.undo.len(), undo + 1, "the release keeps one undo step");
        let angle = h.app.session.doc.assembly.joints.iter().find(|joint| joint.id == hinge).unwrap().values[0].to_degrees();
        assert!((angle - 30.0).abs() < 1e-6, "{angle}");
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9);
    }

    /// A visibility or uncaptured-placement edit changes the public revision without replacing
    /// the design Arc. Later drag frames must not adopt that revision for their older baseline.
    #[test]
    fn a_drag_pose_does_not_adopt_changed_visibility_or_pending_moves() {
        for pending in [false, true] {
            let (mut h, _, a, b) = hinged_arm();
            press_and_move(&mut h, a, b);
            let before = read_pose(&mut h);
            assert!(before["pose"].is_object(), "{before}");
            let design = h.app.session.doc.clone();
            if pending {
                let occurrence = design.find_component("Arm").and_then(|id| design.occurrence_of(id)).unwrap().id;
                let mut placement = joints::occurrence_world(&design, occurrence);
                placement[3][0] += 2.0;
                h.app.session.pending_moves.insert(occurrence, placement);
            } else {
                let body = h.app.session.model.state().bodies.first().unwrap().name.clone();
                h.app.session.visibility.hidden_bodies.push(body);
            }
            let changed = read_pose(&mut h);
            assert!(changed["revision"].as_u64().unwrap() > before["revision"].as_u64().unwrap());
            assert!(changed["pose"].is_null(), "{changed}");
            assert!(std::sync::Arc::ptr_eq(&design, &h.app.session.doc));
            h.app.synthetic.push(egui::Event::PointerMoved(b + egui::vec2(2.0, 0.0)));
            play(&mut h);
            assert!(super::active(&h.app), "the existing drag still has its original baseline");
            assert!(read_pose(&mut h)["pose"].is_null(), "a later frame does not relabel the old solve");
            escape(&mut h);
            release(&mut h, b);
        }
    }

    /// The reach can fail after a successful frame. The reader drops that frame's report,
    /// reports the error, and allocates the next generation only for a successful completion.
    #[test]
    fn a_failed_drag_frame_has_no_pose_and_consumes_no_generation() {
        let (mut h, _, a, b) = hinged_arm();
        let hinge = h.app.session.doc.assembly.joints.iter().find(|joint| joint.name == "Hinge").unwrap().id;
        press_and_move(&mut h, a, b);
        let before = read_pose(&mut h);
        let generation = before["pose"]["generation"].as_u64().unwrap();
        let mut drag = h.app.viewport.joint_drag.take().unwrap();
        let local = drag.local;
        let super::Frames::Shown(sh) = &mut drag.frames else { panic!("a scratch drag") };
        super::show(&mut h.app, 0, local, sh, super::Aim { to: Vec3::ZERO, z: None });
        h.app.viewport.joint_drag = Some(drag);
        let failed = read_pose(&mut h);
        assert!(failed["pose"].is_null(), "{failed}");
        assert!(failed["error"].as_str().is_some_and(|error| !error.is_empty()), "{failed}");

        let mut drag = h.app.viewport.joint_drag.take().unwrap();
        let (occurrence, local) = (drag.occurrence, drag.local);
        let super::Frames::Shown(sh) = &mut drag.frames else { panic!("a scratch drag") };
        let there = apply_point(&joints::occurrence_world(&sh.pose, occurrence), local);
        super::show(&mut h.app, occurrence, local, sh, super::Aim { to: there, z: None });
        h.app.viewport.joint_drag = Some(drag);
        let recovered = read_pose(&mut h);
        assert!(recovered["error"].is_null(), "{recovered}");
        assert_eq!(recovered["pose"]["generation"].as_u64().unwrap(), generation + 1);
        assert!((observed_angle(&recovered, hinge) - observed_angle(&before, hinge)).abs() < 1e-6);
        escape(&mut h);
        release(&mut h, b);
    }

    /// Native, dialog and connection-owned previews count completed poses in one sequence.
    #[test]
    fn a_drag_shares_completed_generations_with_the_other_joint_sources() {
        let (mut h, end, a, b) = hinged_arm();
        let hinge = h.app.session.doc.assembly.joints.iter().find(|joint| joint.name == "Hinge").unwrap().id;
        press_and_move(&mut h, a, b);
        let first = read_pose(&mut h)["pose"]["generation"].as_u64().unwrap();
        escape(&mut h);
        release(&mut h, b);

        h.call("ui.start", json!({"command": "joint.drive", "params": {"joint": hinge, "values": [20]}}));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let dialog = loop {
            let reply = read_pose(&mut h);
            if reply["pose"]["source"] == "joint.drive" && reply["busy"] == false {
                break reply["pose"]["generation"].as_u64().unwrap();
            }
            assert!(std::time::Instant::now() < deadline, "the joint dialog preview did not complete");
            std::thread::sleep(std::time::Duration::from_millis(1));
            h.frame();
        };
        assert!(dialog > first);
        h.call("ui.key", json!({"key": "Escape"}));
        assert!(read_pose(&mut h)["pose"].is_null());

        let client = std::sync::Arc::new(());
        let drive = |h: &mut Harness, params: Value| {
            let (mut request, reply) = crate::control::ControlRequest::new("control.joint_drive", params);
            request.connection = Some(std::sync::Arc::downgrade(&client));
            assert!(request.ticket.take());
            h.app.run_control(&h.ctx, request);
            let reply = reply.try_recv().expect("the synchronous control drive reply");
            assert_eq!(reply["ok"], true, "{reply}");
            reply["result"].clone()
        };
        let document_id = h.app.session.document_id.clone();
        let movement = drive(&mut h, json!({"action": "begin", "document_id": document_id, "joint": hinge}))["movement"].as_u64().unwrap();
        drive(&mut h, json!({"action": "update", "movement": movement, "value": 20}));
        let controlled = read_pose(&mut h);
        assert_eq!(controlled["pose"]["source"], "control.joint_drive");
        let control_generation = controlled["pose"]["generation"].as_u64().unwrap();
        assert!(control_generation > dialog);
        drive(&mut h, json!({"action": "cancel", "movement": movement}));
        assert!(read_pose(&mut h)["pose"].is_null());

        let (a, b) = (screen(&mut h, end), screen(&mut h, Vec3::new(0.0, 55.0, 10.0)));
        press_and_move(&mut h, a, b);
        let native = read_pose(&mut h);
        assert_eq!(native["pose"]["source"], "joint.reach");
        assert!(native["pose"]["generation"].as_u64().unwrap() > control_generation);
        escape(&mut h);
        release(&mut h, b);
    }

    /// The far end of the arm's body where the world state (the view) shows it.
    fn shown_arm_reach(h: &Harness) -> f64 {
        let s = &h.app.session;
        let c = s.doc.find_component("Arm").unwrap_or(0);
        let w = s.world_state().unwrap();
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
        let shown_y = shown_arm_reach(&h);
        assert!((shown_y - 60.0).abs() < 0.5, "the view shows the arm turned a quarter round: {shown_y}");
        let target = shown(&h).and_then(|sh| sh.last.as_ref().map(|l| l.1.to)).expect("a frame");
        release(&mut h, b);
        assert!(h.app.session.drag_preview.is_none());
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        assert!((shown_arm_reach(&h) - shown_y).abs() < 1e-6, "the design ends where the last frame showed it");
        let applied = arm_at(&h, end);
        run(&mut h, "edit.undo", json!({}));
        let at = arm_at(&h, end);
        let fresh = run(&mut h, "joint.reach", json!({"occurrence": "Arm", "at": [at.x, at.y, at.z], "point": [target.x, target.y, target.z]}));
        assert!(arm_at(&h, end).dist(applied) < 1e-6, "{:?} vs {applied:?} ({fresh})", arm_at(&h, end));
    }

    /// Drag modes which commit each frame have no scratch pose. A caller follows the committed
    /// state there, and the existing drag still keeps one Undo step.
    #[test]
    fn a_committed_drag_reports_no_scratch_joint_pose() {
        let (mut h, end, a, b) = hinged_arm();
        run(&mut h, "contact.enable_sets", json!({}));
        let before = read_pose(&mut h);
        let design = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        press_and_move(&mut h, a, b);
        let current = read_pose(&mut h);
        assert!(current["pose"].is_null(), "{current}");
        assert_eq!(current["busy"], false);
        assert!(current["revision"].as_u64().unwrap() > before["revision"].as_u64().unwrap());
        assert!(!std::sync::Arc::ptr_eq(&design, &h.app.session.doc));
        assert!(arm_at(&h, end).dist(end) > 1.0, "the design already holds the moved arm");
        assert_eq!(h.app.session.undo.len(), undo + 1);
        release(&mut h, b);
        assert!(read_pose(&mut h)["pose"].is_null());
        assert_eq!(h.app.session.undo.len(), undo + 1);
        run(&mut h, "edit.undo", json!({}));
        assert!(arm_at(&h, end).dist(end) < 1e-9);
    }

    /// #6325: Escape in a drag leaves the design as it was, with no undo step.
    #[test]
    fn escape_cancels_a_joint_drag() {
        let (mut h, end, a, b) = hinged_arm();
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        let before = shown_arm_reach(&h);
        press_and_move(&mut h, a, b);
        assert!((shown_arm_reach(&h) - before).abs() > 10.0, "the arm is shown turned while dragged");
        escape(&mut h);
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

    /// The slot of body `name` in the motion table (0: none).
    fn slot_of(h: &Harness, name: &str) -> u32 {
        h.app.viewport.palette.lock().unwrap().slots(name).map_or(0, |s| s.motion)
    }

    /// #6325: on a GPU viewport the frames of a drag move the arm on the GPU. The world shown
    /// stays the one the drag began on (nothing is placed again, and no scene is built again),
    /// the arm's motion puts it where the frame's solve has it, the view's extent follows it, and
    /// the release draws the design as it then is. A whole part selected still moves that way:
    /// its highlight is the model's own triangles drawn again, which move with it. A face lit
    /// on the moving part, and Wireframe, draw the frame from the world.
    #[test]
    fn a_drag_moves_its_part_on_the_gpu() {
        let (mut h, end, a, b) = hinged_arm();
        h.app.viewport.gpu = Some(crate::gpu::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: None,
            samples: 1,
            tess: false,
            oit: false,
            edges: false,
        });
        h.frames(2);
        let before = crate::open_meshing::shown_world(&h.app);
        let arm = before.bodies.iter().find(|b| b.mesh().bounds().max.x > 50.0).map(|b| b.name.clone()).expect("the arm's body");
        let slot = slot_of(&h, &arm);
        assert_ne!(slot, 0, "the arm's body has a slot in the motion table");
        let scene = h.app.viewport.key;
        press_and_move(&mut h, a, b);
        h.frames(1);
        let shown = crate::open_meshing::shown_world(&h.app);
        assert!(std::sync::Arc::ptr_eq(&before, &shown), "the world shown is the one the drag began on");
        assert_eq!(h.app.viewport.key, scene, "and its model scene is not built again");
        // Nothing in the frame placed the design at the drag's frame: the sketch tracking guides
        // asked for the world every frame, outside a sketch too (#6325).
        h.frames(1);
        assert!(h.app.session.cached_world_state().is_none(), "a frame drawn by moving the bodies places nothing");
        let moved_to = |h: &Harness| {
            let motions = crate::viewport::joint_motions::motions(&h.app);
            (motions.len() == 1).then(|| {
                let m = motions.of(slot);
                let p = [end.x as f32, end.y as f32, end.z as f32];
                let at = [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3]);
                Vec3::new(f64::from(at[0]), f64::from(at[1]), f64::from(at[2]))
            })
        };
        let at = moved_to(&h).expect("the arm's body alone is moved");
        assert!(at.dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "its motion puts the arm's end where the frame's solve has it: {at:?}");
        let bounds = crate::viewport::model_bounds(&h.app);
        assert!(bounds.max.y > 55.0 && bounds.max.x < 10.0, "the extent follows the arm: {bounds:?}");
        assert!(crate::viewport::joint_motions::ground(&h.app).is_some(), "and the ground under it");
        // The arm selected, as a body and as its part in the Browser: lit by its own triangles
        // drawn again, it is still moved on the GPU.
        let doc = h.app.session.doc.clone();
        let part = doc.find_component("Arm").and_then(|c| doc.occurrence_of(c)).map(|o| o.id).expect("the arm placed");
        for selected in [solidcraft_engine::Sel::Body { name: arm.clone() }, solidcraft_engine::Sel::Occurrence { path: vec![part] }] {
            h.app.session.selection = vec![selected.clone()];
            h.frames(1);
            assert!(moved_to(&h).is_some(), "{selected:?} selected: the arm is still moved on the GPU");
            assert!(std::sync::Arc::ptr_eq(&before, &crate::open_meshing::shown_world(&h.app)), "{selected:?}: nothing placed again");
        }
        // A face lit on the base, which stays, is where the world the drag began on has it: the
        // arm is still moved on the GPU, and the pick places nothing (the timeline asks at every
        // frame which feature made a picked face).
        let base = before.bodies.iter().find(|b| b.name != arm).map(|b| b.name.clone()).expect("the base's body");
        h.app.session.selection = vec![solidcraft_engine::Sel::Face { body: base, index: 0, point: end }];
        h.frames(2);
        assert!(moved_to(&h).is_some(), "a face lit on a body that stays leaves the arm moved on the GPU");
        assert!(h.app.session.cached_world_state().is_none(), "and places nothing");
        // A face lit on the moving arm is geometry of its own, built where the world has it: the
        // frame is drawn from the world.
        h.app.session.selection = vec![solidcraft_engine::Sel::Face { body: arm.clone(), index: 0, point: end }];
        h.frames(1);
        assert!(crate::viewport::joint_motions::motions(&h.app).is_empty(), "a face lit on the moving part draws the frame from the world");
        assert!(!std::sync::Arc::ptr_eq(&before, &crate::open_meshing::shown_world(&h.app)));
        h.app.session.selection.clear();
        h.frames(1);
        assert!(moved_to(&h).is_some(), "and moved on the GPU again without it");
        // Wireframe's outlines on curved surfaces are worked out from the world shown: so is the frame.
        let style = std::mem::replace(&mut h.app.ui.visual_style, 2);
        h.frames(1);
        assert!(crate::viewport::joint_motions::motions(&h.app).is_empty(), "Wireframe draws the frame from the world");
        h.app.ui.visual_style = style;
        h.frames(1);
        assert!(moved_to(&h).is_some(), "and the style it had moves the arm on the GPU again");
        release(&mut h, b);
        h.frames(1);
        assert!(
            h.app.viewport.joint_motions.is_none() && crate::viewport::joint_motions::motions(&h.app).is_empty(),
            "the release moves nothing on the GPU"
        );
    }

    /// #6325: on a device that sorts see-through surfaces on the CPU, a moving part with a face
    /// painted in an opaque colour of its own is still moved on the GPU (it is drawn as the body
    /// is); a face made see-through draws the frame from the world, sorted back to front.
    #[test]
    fn a_painted_face_moves_on_the_gpu_unless_it_is_see_through() {
        for (opacity, on_the_gpu) in [(1.0, true), (0.5, false)] {
            let (mut h, _, a, b) = hinged_arm();
            let arm = crate::open_meshing::shown_world(&h.app)
                .bodies
                .iter()
                .find(|b| b.mesh().bounds().max.x > 50.0)
                .map(|b| b.name.clone())
                .expect("the arm's body");
            run(&mut h, "appearance.assign", json!({"body": arm, "faces": [[30, 0, 10]], "color": "#cc2222", "opacity": opacity}));
            h.app.viewport.gpu = Some(crate::gpu::GpuTarget {
                format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
                depth: None,
                samples: 1,
                tess: false,
                oit: false,
                edges: false,
            });
            h.frames(2);
            press_and_move(&mut h, a, b);
            h.frames(1);
            let moved = !crate::viewport::joint_motions::motions(&h.app).is_empty();
            assert_eq!(moved, on_the_gpu, "a face at opacity {opacity}: moved on the GPU {moved}");
            release(&mut h, b);
        }
    }

    /// #6325: every part a frame moves is moved on the GPU, each by its own move, however many
    /// ways they move in: a frame that moves more occurrences moves their bodies too.
    #[test]
    fn every_part_a_frame_moves_is_moved_on_the_gpu() {
        let (mut h, _, a, b) = hinged_arm();
        let mut parts = Vec::new();
        for k in 0..6 {
            run(&mut h, "component.activate", json!({"component": "root"}));
            let o = run(&mut h, "component.create", json!({"name": format!("Bit {k}")}))["occurrence"].as_u64().expect("occurrence");
            run(&mut h, "solid.box", json!({"length": 1, "width": 1, "height": 1, "corner": [100 + 2 * k, 100, 0]}));
            parts.push(o);
        }
        // A part with no body: moved, it moves nothing on the GPU.
        run(&mut h, "component.activate", json!({"component": "root"}));
        parts.push(run(&mut h, "component.create", json!({"name": "Tag"}))["occurrence"].as_u64().expect("occurrence"));
        run(&mut h, "component.activate", json!({"component": "root"}));
        h.app.viewport.gpu = Some(crate::gpu::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: None,
            samples: 1,
            tess: false,
            oit: false,
            edges: false,
        });
        h.frames(2);
        press_and_move(&mut h, a, b);
        h.frames(1);
        assert_eq!(crate::viewport::joint_motions::motions(&h.app).len(), 1, "the arm alone moves");
        // The frame moves each bit its own way too.
        let placed: Vec<_> = h.app.session.doc.occurrences.iter().filter(|o| parts.contains(&o.id)).map(|o| (o.id, o.transform)).collect();
        assert_eq!(placed.len(), 7);
        let frame = h.app.session.drag_preview.as_mut().expect("a frame");
        for (k, (id, mut moved)) in placed.into_iter().enumerate() {
            moved[3][2] += 1.0 + k as f64;
            frame.moves.insert(id, moved);
        }
        // Followed as the view follows a frame (a whole frame would solve the drag again, and
        // its frame moves the arm alone).
        crate::viewport::joint_motions::follow(&mut h.app);
        let motions = crate::viewport::joint_motions::motions(&h.app);
        assert_eq!(motions.len(), 7, "the arm and the six bits, each moved; the part without a body moves nothing");
        assert!(crate::viewport::joint_motions::world(&h.app).is_some(), "the world shown is the one the drag began on");
        release(&mut h, b);
    }

    /// The arm of [`hinged_arm`] with a grounded blocker in its way an eighth of a turn round.
    fn blocked_arm() -> (Harness, Vec3, egui::Pos2, egui::Pos2) {
        let (mut h, end, a, b) = hinged_arm();
        let blocker = run(&mut h, "component.create", json!({"name": "Blocker"}))["occurrence"].as_u64().expect("occurrence");
        run(&mut h, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [25, 25, 5]}));
        run(&mut h, "component.activate", json!({"component": "root"}));
        run(&mut h, "occurrence.ground", json!({"occurrence": blocker, "grounded": true}));
        h.app.viewport.gpu = Some(crate::gpu::GpuTarget {
            format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm,
            depth: None,
            samples: 1,
            tess: false,
            oit: false,
            edges: false,
        });
        h.frames(2);
        (h, end, a, b)
    }

    /// A drag of the arm from `a` to `b` whose frames must run on the design, as every drag did
    /// before #6325: nothing is shown ahead of the design or moved on the GPU, each frame moves
    /// the design, the whole drag is one undo step, and Escape ends it where it is, as letting go
    /// does. Returns where the arm's end is after the release.
    fn drag_on_the_design(h: &mut Harness, end: Vec3, a: egui::Pos2, b: egui::Pos2) -> Vec3 {
        let (doc, undo) = (h.app.session.doc.clone(), h.app.session.undo.len());
        press_and_move(h, a, b);
        assert!(h.app.viewport.joint_drag.is_some(), "the drag moves the arm");
        assert!(shown(h).is_none() && h.app.session.drag_preview.is_none(), "its frames are not shown ahead of the design");
        assert!(super::began_on(&h.app).is_none() && crate::viewport::joint_motions::motions(&h.app).is_empty(), "nor moved on the GPU");
        assert!(!std::sync::Arc::ptr_eq(&doc, &h.app.session.doc), "each frame moves the design");
        let dragged = arm_at(h, end);
        assert!(dragged.dist(end) > 10.0, "the arm turned: {dragged:?}");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step while it is dragged");
        escape(h);
        assert!(h.app.viewport.joint_drag.is_none(), "Escape ends such a drag, as it did before");
        assert!(arm_at(h, end).dist(dragged) < 1e-9, "and leaves the arm where the drag has it");
        assert_eq!(h.app.session.undo.len(), undo + 1, "with its one undo step");
        release(h, b);
        assert!(arm_at(h, end).dist(dragged) < 1e-9, "the release after it changes nothing");
        assert_eq!(h.app.session.undo.len(), undo + 1, "one undo step for the drag");
        arm_at(h, end)
    }

    /// #14682 with #6325: with contact sets on, only `joint.reach` itself stops a drag where two
    /// parts touch, so each frame of such a drag still runs it on the design, and the arm stops at
    /// the blocker in its way. Without the contact set the same drag is shown in frames and takes
    /// the arm past the blocker.
    #[test]
    fn a_drag_with_contact_sets_on_runs_its_frames_on_the_design() {
        let (mut h, end, a, b) = blocked_arm();
        press_and_move(&mut h, a, b);
        assert!(shown(&h).is_some(), "without contact sets the drag's frames are shown");
        release(&mut h, b);
        assert!(arm_at(&h, end).dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "and nothing stops the arm: {:?}", arm_at(&h, end));
        run(&mut h, "edit.undo", json!({}));
        run(&mut h, "contact.create", json!({"occurrences": ["Arm", "Blocker"]}));
        run(&mut h, "contact.enable_sets", json!({"enabled": true}));
        h.frames(2);
        let stopped = drag_on_the_design(&mut h, end, a, b);
        assert!(stopped.y > 10.0 && stopped.y < 40.0, "the arm turned up to the blocker, not through it: {stopped:?}");
    }

    /// While the view shows a move not captured yet (`occurrence.move` with `capture: false`, an
    /// exploded view), shown frames would solve from where the design has the parts, not where
    /// the view shows them. A drag begun then runs its frames on the design, as it did before
    /// #6325, and the move shown stays.
    #[test]
    fn a_drag_over_a_move_not_captured_runs_its_frames_on_the_design() {
        let (mut h, end, a, b) = blocked_arm();
        let doc = &h.app.session.doc;
        let blocker = doc.find_component("Blocker").and_then(|c| doc.occurrence_of(c)).map(|o| o.id).expect("the blocker");
        run(&mut h, "occurrence.ground", json!({"occurrence": blocker, "grounded": false}));
        run(&mut h, "occurrence.move", json!({"occurrence": blocker, "translate": [0, 0, 20], "capture": false}));
        let pending = h.app.session.pending_moves.clone();
        assert!(!pending.is_empty(), "the move is shown, not captured");
        h.frames(2);
        let turned = drag_on_the_design(&mut h, end, a, b);
        assert!(turned.dist(Vec3::new(0.0, 55.0, 10.0)) < 0.5, "the arm turned a quarter round: {turned:?}");
        assert_eq!(h.app.session.pending_moves, pending, "the move shown stays");
    }

    /// A design changed for another under a drag (a tab opened or switched to) ends the drag: the
    /// frame it showed does not stay with the design it left, which is shown as it is when its tab
    /// comes back.
    #[test]
    fn another_design_shown_mid_drag_ends_the_drag_and_its_frame() {
        let (mut h, end, a, b) = hinged_arm();
        let before = shown_arm_reach(&h);
        press_and_move(&mut h, a, b);
        assert!(h.app.session.drag_preview.is_some(), "a frame is shown");
        crate::documents::new_tab(&mut h.app);
        assert!(h.app.viewport.joint_drag.is_none(), "the drag ends with its design's tab");
        crate::documents::switch(&mut h.app, 0);
        assert!(h.app.session.drag_preview.is_none(), "the design left behind keeps no frame of the drag");
        assert!((shown_arm_reach(&h) - before).abs() < 1e-9, "and is shown as it is");
        release(&mut h, b);
        assert!(arm_at(&h, end).dist(end) < 1e-9, "the release applies nothing");
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
