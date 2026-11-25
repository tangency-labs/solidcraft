//! `joint.reach`: inverse kinematics against closed-form answers (planar arms), the nearest pose
//! out of reach, limits, and a four-bar reached through its rocker.

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use super::tests::{cube_ring, four_bar, meet, occ, part, pin, place, run};
use crate::Session;

/// A planar arm about +Z on a grounded base: Shoulder at the origin, Upper 60 long, Elbow, Fore
/// 40 long, and with `hand` a Wrist and Hand 20 long, all along +X at 0°. Elbow starts at `elbow`°.
fn arm(hand: bool, elbow: f64) -> Session {
    let mut s = Session::default();
    part(&mut s, "Base", [-1.0, -1.0, -1.0]);
    part(&mut s, "Upper", [30.0, -1.0, -1.0]);
    part(&mut s, "Fore", [80.0, -1.0, -1.0]);
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    pin(&mut s, "revolute", "Shoulder", "Base", "Upper", Vec3::ZERO, Vec3::Z);
    pin(&mut s, "revolute", "Elbow", "Upper", "Fore", Vec3::new(60.0, 0.0, 0.0), Vec3::Z);
    if hand {
        part(&mut s, "Hand", [110.0, -1.0, -1.0]);
        pin(&mut s, "revolute", "Wrist", "Fore", "Hand", Vec3::new(100.0, 0.0, 0.0), Vec3::Z);
    }
    run(&mut s, "joint.drive", json!({"joint": "Elbow", "value": elbow}));
    s
}

/// A joint's first value as `joint.list` gives it (degrees for an angle).
fn value(s: &mut Session, joint: &str) -> f64 {
    let l = run(s, "joint.list", json!({"quiet": true}));
    l["joints"]
        .as_array()
        .and_then(|js| js.iter().find(|j| j["name"] == joint))
        .map_or(f64::NAN, |j| j["values"][0]["value"].as_f64().unwrap_or(f64::NAN))
}

/// `joint.reach` of the occurrence's point `local` (its own coordinates).
fn reach(s: &mut Session, occurrence: &str, local: Vec3, mut p: Value) -> Value {
    p["occurrence"] = json!(occurrence);
    p["at"] = pt(place(s, occurrence, local));
    run(s, "joint.reach", p)
}

fn pt(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn got(r: &Value) -> Vec3 {
    Vec3::new(r["point"][0].as_f64().unwrap_or(f64::NAN), r["point"][1].as_f64().unwrap_or(f64::NAN), r["point"][2].as_f64().unwrap_or(f64::NAN))
}

/// The two-link arm's closed-form answer for a tip at (x, y), elbow bent the positive way: the
/// shoulder and elbow angles in degrees.
fn two_link(x: f64, y: f64, l1: f64, l2: f64) -> (f64, f64) {
    let c2 = (x * x + y * y - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
    let t2 = c2.acos();
    let t1 = y.atan2(x) - (l2 * t2.sin()).atan2(l1 + l2 * t2.cos());
    (t1.to_degrees(), t2.to_degrees())
}

#[test]
fn a_two_link_arm_reaches_its_closed_form_pose() {
    let mut s = arm(false, 30.0);
    let tip = Vec3::new(100.0, 0.0, 0.0);
    let to = Vec3::new(50.0, 50.0, 0.0);
    let r = reach(&mut s, "Fore", tip, json!({"point": pt(to)}));
    assert_eq!(
        (&r["reached"], &r["moved"], &r["stopped_by_limit"], &r["stopped_by_loop"]),
        (&json!(true), &json!(true), &Value::Null, &Value::Null),
        "{r}"
    );
    assert!(r["error_mm"].as_f64().is_some_and(|e| e < 1e-6), "{r}");
    assert!(place(&s, "Fore", tip).dist(to) < 1e-6, "{:?}", place(&s, "Fore", tip));
    let (t1, t2) = two_link(50.0, 50.0, 60.0, 40.0);
    assert!((value(&mut s, "Shoulder") - t1).abs() < 1e-6 && (value(&mut s, "Elbow") - t2).abs() < 1e-6, "{r}");
    assert_eq!(r["values"].as_array().map(Vec::len), Some(2), "{r}");
    // Already there, a reach moves nothing and adds no undo step.
    let steps = s.undo.len();
    let r = reach(&mut s, "Fore", tip, json!({"point": pt(to)}));
    assert_eq!((&r["reached"], &r["moved"], s.undo.len()), (&json!(true), &json!(false), steps), "{r}");
    // One undo step puts it back.
    run(&mut s, "edit.undo", json!({}));
    assert!((value(&mut s, "Elbow") - 30.0).abs() < 1e-9 && value(&mut s, "Shoulder").abs() < 1e-9);
}

/// A three-link arm puts its tip on the point and its hand along the axis given: the wrist is
/// then the two-link answer, and the hand's turn makes up the rest.
#[test]
fn a_three_link_arm_reaches_a_point_and_an_axis() {
    let mut s = arm(true, 30.0);
    let tip = Vec3::new(120.0, 0.0, 0.0);
    // The hand's length, along its own X: at 30° in the world now (the elbow's turn).
    let along = place(&s, "Hand", tip) - place(&s, "Hand", Vec3::new(100.0, 0.0, 0.0));
    let r = reach(&mut s, "Hand", tip, json!({"point": [50, 50, 0], "z": [0, 1, 0], "axis": pt(along)}));
    assert_eq!(r["reached"], true, "{r}");
    assert!(r["error_deg"].as_f64().is_some_and(|e| e < 1e-6), "{r}");
    let (t1, t2) = two_link(50.0, 30.0, 60.0, 40.0);
    let t3 = 90.0 - t1 - t2;
    for (joint, want) in [("Shoulder", t1), ("Elbow", t2), ("Wrist", t3)] {
        assert!((value(&mut s, joint) - want).abs() < 1e-6, "{joint}: {} wants {want}: {r}", value(&mut s, joint));
    }
}

#[test]
fn out_of_reach_the_arm_points_at_the_target() {
    let tip = Vec3::new(100.0, 0.0, 0.0);
    // 150 mm away: the arm stretches toward it, 50 mm short.
    let mut s = arm(false, 30.0);
    let r = reach(&mut s, "Fore", tip, json!({"point": [-90, 120, 0]}));
    assert_eq!(
        (&r["reached"], &r["moved"], &r["stopped_by_limit"], &r["stopped_by_loop"]),
        (&json!(false), &json!(true), &Value::Null, &Value::Null),
        "{r}"
    );
    assert!(r["error_mm"].as_f64().is_some_and(|e| (e - 50.0).abs() < 1e-3), "{r}");
    assert!(got(&r).dist(Vec3::new(-60.0, 80.0, 0.0)) < 1e-2, "{r}");
    assert!(place(&s, "Fore", tip).dist(got(&r)) < 1e-9, "{r}");
    // Out of its plane: the nearest is the point under the target.
    let mut s = arm(false, 30.0);
    let r = reach(&mut s, "Fore", tip, json!({"point": [50, 50, 30]}));
    assert_eq!(r["reached"], false, "{r}");
    assert!(r["error_mm"].as_f64().is_some_and(|e| (e - 30.0).abs() < 1e-6), "{r}");
    assert!(got(&r).dist(Vec3::new(50.0, 50.0, 0.0)) < 1e-6, "{r}");
}

/// A drag's frames settle where the full reach does, without working out what stopped it.
#[test]
fn drag_frames_settle_where_the_reach_does_and_leave_out_the_report() {
    use solidcraft_doc::joints::{Reach, reach as full, reach_frame};
    let mut s = arm(false, 10.0);
    run(&mut s, "joint.limits", json!({"joint": "Elbow", "min": -30, "max": 30}));
    let st = s.model.state();
    let r = Reach {
        occurrence: occ(&s, "Fore"),
        point: Vec3::new(100.0, 0.0, 0.0),
        to: Vec3::new(50.0, 50.0, 0.0),
        axis: None,
        joints: None,
        path: Vec::new(),
    };
    let told = full(&s.doc, &st, &r).unwrap();
    assert!(!told.limits.is_empty(), "{told:?}");
    let mut carry = solidcraft_doc::joints::FrameCarry::default();
    let frame = reach_frame(&s.doc, &st, &r, &mut carry).unwrap();
    assert!(frame.limits.is_empty() && frame.stopped_by_loop.is_none(), "{frame:?}");
    let mut pose = (*s.doc).clone();
    for _ in 0..30 {
        super::reach_preview(&mut pose, &st, &r, &mut carry).unwrap();
    }
    let angle =
        |doc: &solidcraft_doc::Document, joint: &str| doc.assembly.joints.iter().find(|j| j.name == joint).map_or(0.0, |j| j.values[0]).to_degrees();
    let elbow = angle(&pose, "Elbow");
    assert!((elbow - 30.0).abs() < 1e-3, "{elbow}");
    // The whole pose, not the elbow alone: the frames end where the full reach puts the arm.
    let mut full_pose = (*s.doc).clone();
    for (id, q) in &told.values {
        if let Some(j) = full_pose.assembly.joints.iter_mut().find(|j| j.id == *id) {
            j.values.clone_from(q);
        }
    }
    let (shown, reached) = (angle(&pose, "Shoulder"), angle(&full_pose, "Shoulder"));
    assert!((shown - reached).abs() < 0.05, "the frames' shoulder {shown} against the reach's {reached}");
}

/// A lone hinge on a grounded base, its part reaching along +X, and the hinge's joint id.
fn hinge() -> Session {
    let mut s = Session::default();
    part(&mut s, "Base", [-1.0, -1.0, -1.0]);
    part(&mut s, "Upper", [30.0, -1.0, -1.0]);
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    pin(&mut s, "revolute", "Shoulder", "Base", "Upper", Vec3::ZERO, Vec3::Z);
    s
}

/// A hinge dragged by a point near its axis with the pointer three times as far out follows the
/// pointer frame by frame, and the release lands where the last frame showed it. Each frame's
/// few damped tries used to start over from the least damping, which such a pose never steps
/// with: the part stood still until it trailed the pointer by a whole capped step (5 degrees),
/// then jumped.
#[test]
fn a_hinge_follows_a_pointer_far_from_its_axis_frame_by_frame() {
    use solidcraft_doc::joints::{FrameCarry, Reach, reach as full};
    let s = hinge();
    let st = s.model.state();
    let upper = occ(&s, "Upper");
    let at = |degrees: f64| Reach {
        occurrence: upper,
        point: Vec3::new(20.0, 0.0, 0.0),
        to: Vec3::new(60.0 * degrees.to_radians().cos(), 60.0 * degrees.to_radians().sin(), 0.0),
        axis: None,
        joints: None,
        path: Vec::new(),
    };
    let angle = |doc: &solidcraft_doc::Document| doc.assembly.joints.iter().find(|j| j.name == "Shoulder").map_or(0.0, |j| j.values[0]).to_degrees();
    let mut pose = (*s.doc).clone();
    let mut carry = FrameCarry::default();
    let (mut worst, mut before) = (0.0f64, pose.clone());
    for k in 1..=45 {
        before = pose.clone();
        super::reach_preview(&mut pose, &st, &at(2.0 * f64::from(k)), &mut carry).unwrap();
        // The first frames find the damping; from then on the part is under the pointer.
        if k > 8 {
            worst = worst.max((angle(&pose) - 2.0 * f64::from(k)).abs());
        }
    }
    assert!(worst < 0.5, "the hinge trails the pointer by up to {worst} degrees");
    // The release: the full reach from where the last frame started.
    let told = full(&before, &st, &at(90.0)).unwrap();
    let released = told.values.values().next().and_then(|q| q.first()).map_or(angle(&before), |q| q.to_degrees());
    assert!((released - angle(&pose)).abs() < 0.5, "the release at {released} against the last frame at {}", angle(&pose));
}

/// Two sliders in series, the first limited to 0..300: a frame that pulls the first past its
/// limit ends with it on the limit exactly, from wherever it started, and the second slider goes
/// on following the pointer in the frames after. A value left a rounding short of its limit was
/// not held there and left no room, which cut every later step of both to nothing.
#[test]
fn a_frame_cut_at_a_limit_ends_on_it_and_the_other_values_go_on() {
    use solidcraft_doc::joints::{FrameCarry, Reach};
    let mut s = Session::default();
    part(&mut s, "Base", [-1.0, -1.0, -1.0]);
    part(&mut s, "Cross", [-1.0, -1.0, 2.0]);
    part(&mut s, "Table", [-1.0, -1.0, 5.0]);
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    pin(&mut s, "slider", "X", "Base", "Cross", Vec3::ZERO, Vec3::X);
    pin(&mut s, "slider", "Y", "Cross", "Table", Vec3::ZERO, Vec3::Y);
    run(&mut s, "joint.limits", json!({"joint": "X", "min": 0, "max": 300}));
    let st = s.model.state();
    let table = occ(&s, "Table");
    let value = |doc: &solidcraft_doc::Document, joint: &str| doc.assembly.joints.iter().find(|j| j.name == joint).map_or(f64::NAN, |j| j.values[0]);
    // Which way along X the slider's value falls toward its limit at 0: the way a reach to +10
    // could not go.
    let probe = Reach { occurrence: table, point: Vec3::ZERO, to: Vec3::new(10.0, 0.0, 0.0), axis: None, joints: None, path: Vec::new() };
    let rose = solidcraft_doc::joints::reach(&s.doc, &st, &probe).is_ok_and(|told| told.values.values().any(|q| q.first().is_some_and(|v| *v > 5.0)));
    let toward_zero = if rose { -1.0 } else { 1.0 };
    let (mut short, mut frozen) = (Vec::new(), Vec::new());
    for k in 0..200u32 {
        // X starts somewhere in its travel, each run elsewhere.
        let start = 0.37 + 1.61 * f64::from(k);
        let mut pose = (*s.doc).clone();
        if let Some(j) = pose.assembly.joints.iter_mut().find(|j| j.name == "X") {
            j.values = vec![start];
        }
        let mut carry = FrameCarry::default();
        // Pulled past X's limit at 0 while Y goes to 40, then on to 80, a dozen frames each.
        for frame in 0..24 {
            let y = if frame < 12 { 40.0 } else { 80.0 };
            let to = Vec3::new(toward_zero * (25.0 + 0.01 * f64::from(k)), y, 0.0);
            let r = Reach { occurrence: table, point: Vec3::ZERO, to, axis: None, joints: None, path: Vec::new() };
            super::reach_preview(&mut pose, &st, &r, &mut carry).unwrap();
        }
        if value(&pose, "X") != 0.0 {
            short.push((start, value(&pose, "X")));
        }
        if (value(&pose, "Y").abs() - 80.0).abs() > 1e-3 {
            frozen.push((start, value(&pose, "Y")));
        }
    }
    assert!(short.is_empty(), "X left off its limit (its start, where it ended): {short:?}");
    assert!(frozen.is_empty(), "Y stopped following the pointer (X's start, where Y ended): {frozen:?}");
}

/// `from` with contact sets on is refused, and the design is left as it was: the jump to `from`
/// would not be checked for parts passing into each other.
#[test]
fn a_reach_from_given_values_is_refused_with_contact_sets_on() {
    let mut s = hinge();
    let hinge = s.doc.assembly.joints.iter().find(|j| j.name == "Shoulder").map(|j| j.id).expect("the hinge");
    run(&mut s, "contact.enable_sets", json!({"enabled": true}));
    let doc = s.doc.clone();
    let upper = occ(&s, "Upper");
    let r = s.execute("joint.reach", &json!({"occurrence": upper, "point": [0, 60, 0], "from": [{"joint": hinge, "values": [0.5]}]}));
    let error = r.expect_err("refused").to_string();
    assert!(error.contains("contact sets"), "{error}");
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc), "the design is unchanged");
}

#[test]
fn a_limited_elbow_stops_the_reach_and_says_so() {
    let tip = Vec3::new(100.0, 0.0, 0.0);
    let mut s = arm(false, 10.0);
    run(&mut s, "joint.limits", json!({"joint": "Elbow", "min": -30, "max": 30}));
    let r = reach(&mut s, "Fore", tip, json!({"point": [50, 50, 0]}));
    // The target needs the elbow at 92°: it bends to its limit, and the arm points at the target.
    assert_eq!(r["reached"], false, "{r}");
    assert_eq!(r["stopped_by_limit"], json!([{"joint": "Elbow", "value": "angle", "limit": "max"}]), "{r}");
    assert!((value(&mut s, "Elbow") - 30.0).abs() < 1e-9, "{r}");
    let span = (60f64.powi(2) + 40f64.powi(2) + 2.0 * 60.0 * 40.0 * 30f64.to_radians().cos()).sqrt();
    let d = 50.0 * 2f64.sqrt();
    assert!(r["error_mm"].as_f64().is_some_and(|e| (e - (span - d)).abs() < 1e-4), "{r}");
    // Only the shoulder moves when only it is given: the tip swings on its circle.
    let mut s = arm(false, 30.0);
    let r = reach(&mut s, "Fore", tip, json!({"point": [0, 100, 0], "joints": ["Shoulder"]}));
    let span = (60f64.powi(2) + 40f64.powi(2) + 2.0 * 60.0 * 40.0 * 30f64.to_radians().cos()).sqrt();
    assert!(r["error_mm"].as_f64().is_some_and(|e| (e - (100.0 - span)).abs() < 1e-4), "{r}");
    assert!((value(&mut s, "Elbow") - 30.0).abs() < 1e-12, "{r}");
}

/// #19760: the folding cube toy (`cube_ring`) with one pin of hinge 8 taken out, a plate dragged
/// past where its pins' limits let it go. Each step pushed the pins at their limits and was cut
/// back to them, so the reach crept along a limit in steps that each gained a little, until its
/// work ran out: about half a second for every frame of a drag. Solved with those pins held at
/// their limits, it settles in a few steps where the limits leave it closest, and reaching again
/// for the same point gets no closer.
#[test]
fn a_reach_held_at_a_limit_settles_quickly() {
    let mut s = cube_ring(&[30, 31]);
    let plate = occ(&s, "H4");
    let at = Vec3::new(-36.672, -2.706, 14.855);
    for k in [1.0, 2.0] {
        let to = Vec3::new(at.x, at.y + 2.0 * k, at.z + 1.5 * k);
        let ask = solidcraft_doc::joints::Reach { occurrence: plate, point: at, to, axis: None, joints: None, path: Vec::new() };
        let st = s.model.state();
        let first = solidcraft_doc::joints::reach(&s.doc, &st, &ask).unwrap_or_default();
        // A tenth of the 200 000 placements one solve may spend; the crawl spent all of them.
        assert!(first.work < 20_000, "frame {k}: {} placements", first.work);
        let r = run(&mut s, "joint.reach", json!({"occurrence": "H4", "at": pt(at), "point": pt(to)}));
        let miss = r["error_mm"].as_f64().unwrap_or(f64::NAN);
        assert_eq!(r["reached"], false, "frame {k}: {r}");
        let again = run(&mut s, "joint.reach", json!({"occurrence": "H4", "at": pt(at), "point": pt(to)}));
        let closer = miss - again["error_mm"].as_f64().unwrap_or(f64::NAN);
        assert!(closer < 1e-6, "frame {k}: a second reach got {closer} mm closer: {r} then {again}");
    }
}

/// A linked elbow (elbow = −shoulder, so the forearm keeps pointing along +X) follows the
/// shoulder: the tip moves on a circle about (40, 0), and the reach turns the shoulder only.
#[test]
fn a_linked_joint_follows_the_reach() {
    let tip = Vec3::new(100.0, 0.0, 0.0);
    let mut s = arm(false, 0.0);
    run(&mut s, "joint.motion_link", json!({"a": "Shoulder", "b": "Elbow", "ratio": -1}));
    let th = 40f64.to_radians();
    let to = json!([40.0 + 60.0 * th.cos(), 60.0 * th.sin(), 0.0]);
    let r = reach(&mut s, "Fore", tip, json!({"point": to}));
    assert_eq!(r["reached"], true, "{r}");
    assert!((value(&mut s, "Shoulder") - 40.0).abs() < 1e-6 && (value(&mut s, "Elbow") + 40.0).abs() < 1e-6, "{r}");
    // Given only the linked elbow, the reach turns it through its link's source.
    run(&mut s, "edit.undo", json!({}));
    let r = reach(&mut s, "Fore", tip, json!({"point": to, "joints": ["Elbow"]}));
    assert_eq!(r["reached"], true, "{r}");
    assert!((value(&mut s, "Shoulder") - 40.0).abs() < 1e-6, "{r}");
    // The elbow held at its own limit by the link stops the reach, and is named.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "joint.limits", json!({"joint": "Elbow", "min": -30, "max": 30}));
    let r = reach(&mut s, "Fore", tip, json!({"point": to}));
    assert_eq!(r["reached"], false, "{r}");
    assert!(r["stopped_by_limit"].as_array().is_some_and(|l| l.contains(&json!({"joint": "Elbow", "value": "angle", "limit": "min"}))), "{r}");
}

/// A target straight across the pivot from the point (where the misses don't change to first
/// order) is still reached: the arm turns half round, the way its limits leave open.
#[test]
fn a_target_across_the_pivot_is_reached() {
    let tip = Vec3::new(100.0, 0.0, 0.0);
    for (limits, want) in [(None, None), (Some([-180, 0]), Some(-180.0)), (Some([0, 180]), Some(180.0))] {
        let mut s = arm(false, 0.0);
        if let Some([min, max]) = limits {
            run(&mut s, "joint.limits", json!({"joint": "Shoulder", "min": min, "max": max}));
        }
        let r = reach(&mut s, "Fore", tip, json!({"point": [-100, 0, 0], "joints": ["Shoulder"]}));
        assert_eq!(r["reached"], true, "{limits:?}: {r}");
        let got = value(&mut s, "Shoulder");
        assert!((got.abs() - 180.0).abs() < 1e-6 && want.is_none_or(|w| (got - w).abs() < 1e-6), "{limits:?}: {got}");
    }
}

/// A loop that only closes again after a full turn of a value geared up from the arm's (a joint
/// between two grounded parts, linked 36:1 to the shoulder) holds the arm: a step of a tenth of a
/// turn would close it at both ends, but not on the way.
#[test]
fn a_loop_geared_up_from_the_chain_holds_it() {
    let tip = Vec3::new(100.0, 0.0, 0.0);
    let mut s = arm(false, 0.0);
    part(&mut s, "Lock", [-1.0, 5.0, -1.0]);
    let lock = occ(&s, "Lock");
    run(&mut s, "occurrence.ground", json!({"occurrence": lock, "grounded": true}));
    pin(&mut s, "revolute", "Lock", "Base", "Lock", Vec3::ZERO, Vec3::Z);
    run(&mut s, "joint.motion_link", json!({"a": "Shoulder", "b": "Lock", "ratio": 36}));
    let r = reach(&mut s, "Fore", tip, json!({"point": [0, 1e6, 0], "joints": ["Shoulder"]}));
    assert_eq!((&r["reached"], &r["stopped_by_loop"]), (&json!(false), &json!({"joint": "Lock"})), "{r}");
    assert!(value(&mut s, "Shoulder").abs() < 1e-6, "{r}");
    // Geared up so far that even the small kick off a target straight across the pivot would turn
    // the loop a whole turn: it holds too.
    run(&mut s, "joint.delete", json!({"link": 0}));
    run(&mut s, "joint.motion_link", json!({"a": "Shoulder", "b": "Lock", "ratio": 36000}));
    let r = reach(&mut s, "Fore", tip, json!({"point": [-100, 0, 0], "joints": ["Shoulder"]}));
    assert_eq!(r["reached"], false, "{r}");
    assert!(value(&mut s, "Shoulder").abs() < 1e-6, "{r}");
}

/// A planar joint far from the point turns it and slides the long way round, and the answer
/// gives the values as joint.list does.
#[test]
fn a_planar_joint_far_from_the_point_turns_and_slides() {
    let mut s = Session::default();
    part(&mut s, "Base", [-1.0, -1.0, -1.0]);
    part(&mut s, "Puck", [99.0, -1.0, 2.0]);
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    pin(&mut s, "planar", "Table", "Base", "Puck", Vec3::ZERO, Vec3::Z);
    let r = reach(&mut s, "Puck", Vec3::new(100.0, 0.0, 0.0), json!({"point": [100, 0, 0], "z": [0, 1, 0], "axis": [1, 0, 0]}));
    assert_eq!(r["reached"], true, "{r}");
    let l = run(&mut s, "joint.list", json!({"quiet": true}));
    let listed = l["joints"].as_array().and_then(|js| js.iter().find(|j| j["name"] == "Table")).map(|j| j["values"].clone());
    assert_eq!(Some(r["values"][0]["values"].clone()), listed, "{r}");
    assert!((r["values"][0]["values"][2]["value"].as_f64().unwrap_or(f64::NAN) - 90.0).abs() < 1e-6, "{r}");
}

/// A slider on its own (a chain with no size) still travels the whole way.
#[test]
fn a_lone_slider_travels_all_the_way() {
    let mut s = Session::default();
    part(&mut s, "Base", [-1.0, -1.0, -1.0]);
    part(&mut s, "Ram", [-1.0, -1.0, 2.0]);
    let base = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    pin(&mut s, "slider", "Rail", "Base", "Ram", Vec3::ZERO, Vec3::Z);
    let r = reach(&mut s, "Ram", Vec3::ZERO, json!({"point": [0, 0, 100]}));
    assert_eq!(r["reached"], true, "{r}");
    assert!((value(&mut s, "Rail") - 100.0).abs() < 1e-6, "{r}");
    // However far.
    let r = reach(&mut s, "Ram", Vec3::ZERO, json!({"point": [0, 0, 3e7]}));
    assert_eq!(r["reached"], true, "{r}");
    // A limit on it is named whatever the chain's size.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "joint.limits", json!({"joint": "Rail", "min": -50, "max": 100}));
    let r = reach(&mut s, "Ram", Vec3::ZERO, json!({"point": [0, 0, 1e6]}));
    assert_eq!(r["stopped_by_limit"], json!([{"joint": "Rail", "value": "distance", "limit": "max"}]), "{r}");
    // And a reach of a thousandth of a millimetre still moves it.
    let r = reach(&mut s, "Ram", Vec3::ZERO, json!({"point": [0, 0, 99.999]}));
    assert_eq!((&r["reached"], &r["moved"]), (&json!(true), &json!(true)), "{r}");
}

/// A ball joint at rest tilts toward a small nudge in any direction, not only along its pitch
/// (#12873): its yaw and roll turn about the same axis at pitch 0, so a solve over those angles
/// can't tilt it the other way. It tilts the least way there, without spinning about its axis, and
/// a pitch limit still holds it, with the limit named.
#[test]
fn a_ball_joint_at_rest_tilts_every_way() {
    let tip = Vec3::new(0.0, 0.0, 21.0);
    for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (0.7, -0.7)] {
        let mut s = Session::default();
        part(&mut s, "Socket", [-1.0, -1.0, -3.0]);
        part(&mut s, "Stick", [-1.0, -1.0, 20.0]);
        let socket = occ(&s, "Socket");
        run(&mut s, "occurrence.ground", json!({"occurrence": socket, "grounded": true}));
        pin(&mut s, "ball", "Ball", "Socket", "Stick", Vec3::ZERO, Vec3::Z);
        // A point 1 mm across, on the tip's sphere about the joint.
        let off = Vec3::new(dx, dy, 21.0);
        let to = off * (21.0 / off.len());
        let r = reach(&mut s, "Stick", tip, json!({"point": pt(to)}));
        assert_eq!((&r["reached"], &r["moved"]), (&json!(true), &json!(true)), "toward ({dx}, {dy}): {r}");
        assert!(place(&s, "Stick", tip).dist(to) < 1e-6, "toward ({dx}, {dy}): {:?}", place(&s, "Stick", tip));
        // The least turn: a point off the axis moves about as far as the tip, not round it.
        let side = Vec3::new(10.0, 0.0, 21.0);
        assert!(place(&s, "Stick", side).dist(side) < 1.5, "toward ({dx}, {dy}) it spun: {:?}", place(&s, "Stick", side));
    }
    // A cone of 10°: a target 30° over stops at the limit, toward it, and names it.
    let mut s = Session::default();
    part(&mut s, "Socket", [-1.0, -1.0, -3.0]);
    part(&mut s, "Stick", [-1.0, -1.0, 20.0]);
    let socket = occ(&s, "Socket");
    run(&mut s, "occurrence.ground", json!({"occurrence": socket, "grounded": true}));
    pin(&mut s, "ball", "Ball", "Socket", "Stick", Vec3::ZERO, Vec3::Z);
    run(&mut s, "joint.limits", json!({"joint": "Ball", "index": 1, "min": -10, "max": 10}));
    let a = 30f64.to_radians();
    let r = reach(&mut s, "Stick", tip, json!({"point": [21.0 * a.sin(), 0.0, 21.0 * a.cos()]}));
    assert_eq!(r["reached"], false, "{r}");
    let at = place(&s, "Stick", tip);
    assert!((at.z / 21.0).acos().to_degrees() - 10.0 < 1e-6 && at.x > 0.0 && at.y.abs() < 1e-6, "{at:?} {r}");
    assert_eq!(r["stopped_by_limit"][0]["joint"], "Ball", "{r}");
}

/// The four-bar's rocker reaches a point on its swing, the crank and coupler following with the
/// loop closed; past its toggle position it stops there, and the loop is named.
#[test]
fn a_four_bar_reaches_with_its_rocker() {
    let (mut s, _, q0) = four_bar();
    let o4 = Vec3::new(45.0, 0.0, 0.0);
    let start = q0.y.atan2(q0.x - 45.0);
    let swing = |deg: f64| o4 + Vec3::new((start + deg.to_radians()).cos(), (start + deg.to_radians()).sin(), 0.0) * 40.0;
    let r = run(&mut s, "joint.reach", json!({"occurrence": "Rocker", "at": pt(q0), "point": pt(swing(-10.0))}));
    assert_eq!((&r["reached"], &r["stopped_by_loop"]), (&json!(true), &Value::Null), "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(place(&s, "Rocker", q0).dist(swing(-10.0)) < 1e-6 && place(&s, "Coupler", q0).dist(swing(-10.0)) < 1e-6, "{r}");
    // The crank turned to where the loop closes with the rocker there (its pin on the coupler's circle).
    let th = (90.0 + value(&mut s, "Crank")).to_radians();
    let crank_pin = Vec3::new(20.0 * th.cos(), 20.0 * th.sin(), 0.0);
    assert!((crank_pin.dist(swing(-10.0)) - 50.0).abs() < 1e-6 && place(&s, "Coupler", Vec3::new(0.0, 20.0, 0.0)).dist(crank_pin) < 1e-6, "{r}");
    // Past its toggle position (|Q| = 30) the loop stops it there.
    let (mut s, _, q0) = four_bar();
    let r = run(&mut s, "joint.reach", json!({"occurrence": "Rocker", "at": pt(q0), "point": pt(swing(100.0))}));
    assert_eq!((&r["reached"], &r["stopped_by_loop"]), (&json!(false), &json!({"joint": "Rocker pin"})), "{r}");
    // Its joints have no limits, so none is named.
    assert_eq!(r["stopped_by_limit"], Value::Null, "{r}");
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let limit = ((900.0f64 - 3625.0) / 3600.0).acos().to_degrees() - start.to_degrees();
    let at = value(&mut s, "Rocker");
    assert!(at > limit - 0.5 && at <= limit + 1e-6, "stopped at {at}°, the toggle is at {limit}°: {r}");
    let [m1, m2] = meet(o4, 40.0, Vec3::ZERO, 30.0);
    assert!(place(&s, "Rocker", q0).dist(m1).min(place(&s, "Rocker", q0).dist(m2)) < 0.5, "{r}");
}

#[test]
fn reach_refuses_bad_params_and_says_why_nothing_moves() {
    let mut s = arm(false, 30.0);
    for bad in [
        json!({"occurrence": "Fore"}),
        json!({"occurrence": "Nope", "point": [0, 0, 0]}),
        json!({"occurrence": "Fore", "point": [1e300, 0, 0]}),
        json!({"occurrence": "Fore", "point": [0, 0, "x"]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "z": [0, 0, 0]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "axis": [1, 0, 0]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "joints": "Elbow"}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "joints": ["Nope"]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "at": [f64::MAX, 0, 0]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "from": {"joint": "Elbow", "values": [10]}}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "from": [{"joint": "Nope", "values": [10]}]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "from": [{"joint": "Elbow", "values": [10, 20]}]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "from": [{"joint": "Elbow", "values": ["x"]}]}),
        // A value that is no number beside one that is: not passed over to leave the number that fits.
        json!({"occurrence": "Fore", "point": [0, 0, 0], "from": [{"joint": "Elbow", "values": [10, "x"]}]}),
        json!({"occurrence": "Fore", "point": [0, 0, 0], "from": [{"joint": "Elbow", "values": [1e9]}]}),
    ] {
        assert!(s.execute("joint.reach", &bad).is_err(), "{bad}");
    }
    // A grounded part, a part no joint places, and joints that don't move it: nothing moves, and
    // the answer says why.
    part(&mut s, "Loose", [200.0, 0.0, 0.0]);
    for (p, why) in [
        (json!({"occurrence": "Base", "point": [5, 5, 5]}), "fixed in place"),
        (json!({"occurrence": "Loose", "point": [5, 5, 5]}), "not connected"),
        (json!({"occurrence": "Upper", "point": [5, 5, 5], "joints": ["Elbow"]}), "none of the joints given"),
    ] {
        let r = run(&mut s, "joint.reach", p.clone());
        assert_eq!((&r["reached"], &r["moved"]), (&json!(false), &json!(false)), "{p}: {r}");
        assert!(r["reason"].as_str().is_some_and(|w| w.contains(why)), "{p}: {r}");
    }
}

/// Reach still publishes fresh full diagnostics after a lightweight refresh, including stationary
/// mouse frames, loop-limited targets and undo/redo. Full analysis runs once only when requested.
#[test]
fn reach_preserves_full_diagnostics_without_repeating_dof_analysis() {
    let (mut s, _, q0) = four_bar();
    let o4 = Vec3::new(45.0, 0.0, 0.0);
    let start = q0.y.atan2(q0.x - 45.0);
    for (angle, quiet) in [(-10.0_f64, false), (-5.0, true), (-5.0, true), (100.0, false)] {
        let a = start + angle.to_radians();
        let to = o4 + Vec3::new(40.0 * a.cos(), 40.0 * a.sin(), 0.0);
        super::solve_counts::take();
        let r = reach(&mut s, "Rocker", q0, json!({"point":pt(to), "continuous":true, "quiet":quiet}));
        assert_eq!(super::solve_counts::take().1, usize::from(!quiet));
        assert_eq!(r["reached"], angle != 100.0, "{r}");
        assert_eq!(r["stopped_by_loop"].is_object(), angle == 100.0, "{r}");
        let expected = super::sol_json(&solidcraft_doc::joints::solve(&s.doc, &s.model.state()), &json!({"quiet":quiet}));
        for key in ["dof", "fixed", "errors", "conflicts"] {
            assert_eq!(r.get(key), expected.get(key), "{key}: {r}");
        }
        let listed = run(&mut s, "joint.list", json!({}));
        let full = super::sol_json(&solidcraft_doc::joints::solve(&s.doc, &s.model.state()), &json!({}));
        assert_eq!(listed["dof"], full["dof"]);
    }
    for command in ["edit.undo", "edit.redo"] {
        run(&mut s, command, json!({}));
        let full = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        assert!(full.conflicts.is_empty() && full.errors.is_empty());
        for part in ["Crank", "Coupler", "Rocker"] {
            assert_eq!(full.dof.get(&occ(&s, part)), Some(&1), "{command}: {part}");
        }
    }
}

/// A joint drag frame (a quiet, continuous reach) solves the joints once and does not walk every
/// feature again to pin body names: the refresh after it keeps the reach's own solve, and the
/// pinning pass finds the features and the model as it left them (#6325). A body made after the
/// frames is still pinned, and so is a renamed one.
#[test]
fn reach_frames_solve_once_and_skip_body_name_pinning() {
    let (mut s, _, q0) = four_bar();
    let o4 = Vec3::new(45.0, 0.0, 0.0);
    let start = q0.y.atan2(q0.x - 45.0);
    let passes = s.body_name_passes;
    for angle in [-4.0_f64, -8.0, -12.0] {
        let a = start + angle.to_radians();
        let to = o4 + Vec3::new(40.0 * a.cos(), 40.0 * a.sin(), 0.0);
        let evaluations = s.evaluations;
        super::solve_counts::take();
        let r = reach(&mut s, "Rocker", q0, json!({"point":pt(to), "continuous":true, "quiet":true}));
        assert_eq!((&r["reached"], &r["moved"]), (&json!(true), &json!(true)), "{r}");
        assert_eq!(s.evaluations, evaluations, "a placement change keeps the model");
        assert_eq!(super::solve_counts::take(), (1, 0), "the refresh keeps the reach's solve");
        assert_eq!(s.body_name_passes, passes, "nothing new to pin");
        // The kept solve is the one a fresh solve gives.
        let full = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        let mut next = (*s.doc).clone();
        next.apply_joint_solution(&full);
        for part in ["Crank", "Coupler", "Rocker"] {
            let at = |d: &solidcraft_doc::Document| solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(d, occ(&s, part)), q0);
            assert!(at(&s.doc).dist(at(&next)) < 1e-9, "{angle}°: {part} is where a new solve puts it");
        }
    }
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0], "name": "Late"}));
    assert_eq!(s.body_name_passes, passes + 1);
    let late = s.doc.find_feature("Late").map(|f| f.body_names.clone()).unwrap_or_default();
    assert_eq!(late.len(), 1, "the new body's name is pinned: {late:?}");
    assert!(s.doc.reserved_body_names.contains(&late[0]), "{late:?}");
    run(&mut s, "body.rename", json!({"body": late[0].clone(), "name": "Renamed"}));
    assert_eq!(s.doc.find_feature("Late").map(|f| f.body_names.clone()), Some(vec!["Renamed".to_string()]));
    assert!(s.model.state().body("Renamed").is_some());
}

/// A drag's frame that jumps past a limit on a closed loop ends on the limit, not short of it:
/// the part must show how far it goes while it is held.
#[test]
fn a_drag_frame_past_a_limit_on_a_closed_loop_ends_on_the_limit() {
    let (mut s, _, q0) = four_bar();
    run(&mut s, "joint.limits", json!({"joint": "Rocker", "min": -12, "max": 12}));
    let o4 = Vec3::new(45.0, 0.0, 0.0);
    let start = q0.y.atan2(q0.x - 45.0);
    let rocker = occ(&s, "Rocker");
    let local = solidcraft_doc::apply_point(&solidcraft_doc::mat_inverse(&solidcraft_doc::joints::occurrence_world(&s.doc, rocker)).unwrap(), q0);
    let st = s.model.state();
    let mut pose = (*s.doc).clone();
    let mut carry = solidcraft_doc::joints::FrameCarry::default();
    for angle in [-6.0_f64, -40.0, -40.0] {
        let a = start + angle.to_radians();
        let to = o4 + Vec3::new(40.0 * a.cos(), 40.0 * a.sin(), 0.0);
        let r = solidcraft_doc::joints::Reach { occurrence: rocker, point: local, to, axis: None, joints: None, path: Vec::new() };
        assert!(super::reach_preview(&mut pose, &st, &r, &mut carry).is_ok());
    }
    let value = pose.assembly.joints.iter().find(|j| j.name == "Rocker").map_or(0.0, |j| j.values[0]).to_degrees();
    assert!((value.abs() - 12.0).abs() < 1e-3, "{value}");
}

/// #6325: a joint drag's frames reach on a copy of the design (`reach_preview`), and its release
/// applies the last one as `joint.reach` from the values that frame started from: the design
/// ends where the frames showed it, closed loop and all, in one undo step.
#[test]
fn a_reach_from_the_last_frames_start_ends_where_the_frames_showed() {
    let (mut s, _, q0) = four_bar();
    let o4 = Vec3::new(45.0, 0.0, 0.0);
    let start = q0.y.atan2(q0.x - 45.0);
    let design = s.doc.clone();
    let rocker = occ(&s, "Rocker");
    let local = solidcraft_doc::apply_point(&solidcraft_doc::mat_inverse(&solidcraft_doc::joints::occurrence_world(&s.doc, rocker)).unwrap(), q0);
    let mut pose = (*s.doc).clone();
    let mut carry = solidcraft_doc::joints::FrameCarry::default();
    let mut last = None;
    for angle in [-6.0_f64, -12.0, -18.0] {
        let a = start + angle.to_radians();
        let to = o4 + Vec3::new(40.0 * a.cos(), 40.0 * a.sin(), 0.0);
        let before = pose.assembly.joints.iter().map(|j| (j.id, j.values.clone())).collect();
        let r = solidcraft_doc::joints::Reach { occurrence: rocker, point: local, to, axis: None, joints: None, path: Vec::new() };
        assert!(super::reach_preview(&mut pose, &s.model.state(), &r, &mut carry).is_ok());
        last = Some((before, to));
    }
    assert!(std::sync::Arc::ptr_eq(&design, &s.doc), "frames leave the design alone");
    let (before, to) = last.unwrap();
    let from = super::reach_from(&s.doc, &before);
    assert!(from.as_array().is_some_and(|f| !f.is_empty()), "{from}");
    let steps = s.undo.len();
    let r = reach(&mut s, "Rocker", q0, json!({"point": pt(to), "quiet": true, "continuous": true, "from": from}));
    assert_eq!((&r["reached"], &r["moved"]), (&json!(true), &json!(true)), "{r}");
    assert_eq!(s.undo.len(), steps + 1);
    for part in ["Crank", "Coupler", "Rocker"] {
        let at = |d: &solidcraft_doc::Document| solidcraft_doc::apply_point(&solidcraft_doc::joints::occurrence_world(d, occ(&s, part)), q0);
        assert!(at(&s.doc).dist(at(&pose)) < 1e-6, "{part}: {:?} vs the frame's {:?}", at(&s.doc), at(&pose));
    }
    for j in &s.doc.assembly.joints {
        let shown = pose.assembly.joints.iter().find(|k| k.id == j.id).map(|k| k.values.clone()).unwrap_or_default();
        assert!(j.values.iter().zip(&shown).all(|(a, b)| (a - b).abs() < 1e-6), "{}: {:?} vs {shown:?}", j.name, j.values);
    }
}

/// Successive large pointer goals must use the new joint values at every perturbation and
/// solve, keep all loop pins together and preserve mobility reports through undo/redo.
#[test]
fn quarter_turn_reaches_keep_the_four_bar_closed_and_diagnostics_current() {
    let (mut s, p0, q0) = four_bar();
    let ground = place(&s, "Ground", Vec3::ZERO);
    for (i, point) in
        [Vec3::new(-20.0, 0.0, 0.0), Vec3::new(0.0, -20.0, 0.0), Vec3::new(20.0, 0.0, 0.0), Vec3::new(0.0, 20.0, 0.0)].into_iter().enumerate()
    {
        let r = reach(&mut s, "Crank", p0, json!({"point":pt(point), "continuous":true, "quiet":i % 2 == 0}));
        assert_eq!((&r["reached"], &r["stopped_by_loop"], &r["stopped_by_limit"]), (&json!(true), &Value::Null, &Value::Null), "{r}");
        assert!(place(&s, "Crank", p0).dist(point) < 1e-5, "{r}");
        let rocker = place(&s, "Rocker", q0);
        let [a, b] = meet(point, 50.0, Vec3::new(45.0, 0.0, 0.0), 40.0);
        assert!(rocker.dist(a).min(rocker.dist(b)) < 1e-5, "{r}");
        assert!(place(&s, "Coupler", q0).dist(rocker) < 1e-6, "{r}");
        assert!(place(&s, "Coupler", p0).dist(point) < 1e-5, "{r}");
        assert_eq!(place(&s, "Ground", Vec3::ZERO), ground);
        let full = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        assert!(full.errors.is_empty() && full.conflicts.is_empty(), "{full:?}");
        for part in ["Crank", "Coupler", "Rocker"] {
            assert_eq!(full.dof.get(&occ(&s, part)), Some(&1));
        }
        let steps = s.undo.len();
        let again = reach(&mut s, "Crank", p0, json!({"point":pt(point), "continuous":true, "quiet":true}));
        assert_eq!((&again["reached"], &again["moved"], s.undo.len()), (&json!(true), &json!(false), steps), "{again}");
    }
    for (command, point) in [("edit.undo", Vec3::new(20.0, 0.0, 0.0)), ("edit.redo", p0)] {
        run(&mut s, command, json!({}));
        assert!(place(&s, "Crank", p0).dist(point) < 1e-5, "{command}");
        let full = solidcraft_doc::joints::solve(&s.doc, &s.model.state());
        assert!(full.errors.is_empty() && full.conflicts.is_empty(), "{command}: {full:?}");
        for part in ["Crank", "Coupler", "Rocker"] {
            assert_eq!(full.dof.get(&occ(&s, part)), Some(&1));
        }
    }
}

/// A grounded block A and a block B on a slider toward it, 10 mm clear, in one contact set:
/// `contact` turns contact sets on or off. The slider stops 10 mm past the touch.
fn blocks_on_a_rail(contact: bool) -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let a = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "joint.limits", json!({"joint": "Rail", "index": 0, "min": -10, "max": 100}));
    run(&mut s, "contact.create", json!({"occurrences": ["A", "B"]}));
    run(&mut s, "contact.enable_sets", json!({"enabled": contact}));
    s
}

/// A drag (joint.reach) stops where a contact set says two parts touch, as joint.drive does, and
/// says so, not the limit further on; with contact sets off it goes on to that limit (#14682).
#[test]
fn a_reach_stops_where_a_contact_set_touches() {
    // B's near face, in its own coordinates; the drag pulls its centre 25 mm toward A, 15 mm past
    // where B touches A.
    let near_face = Vec3::new(20.0, 5.0, 5.0);
    let pull = |s: &mut Session| {
        let from = place(s, "B", Vec3::new(25.0, 5.0, 5.0));
        let to = from - Vec3::new(25.0, 0.0, 0.0);
        reach(s, "B", Vec3::new(25.0, 5.0, 5.0), json!({"point": pt(to)}))
    };
    let mut s = blocks_on_a_rail(true);
    let start = place(&s, "B", near_face).x;
    assert!((start - 20.0).abs() < 1e-6, "B starts 10 mm clear of A: {start}");
    let r = pull(&mut s);
    assert!(r["stopped_by_contact"].is_object(), "{r}");
    assert!(r["stopped_by_limit"].is_null(), "the contact stopped it, not the limit: {r}");
    assert_eq!(r["reached"], false, "{r}");
    let x = place(&s, "B", near_face).x;
    assert!((10.0 - 1e-6..10.0 + 1e-3).contains(&x), "B stops touching A, not through it: its face at x = {x}");
    let collisions = run(&mut s, "contact.check", json!({}));
    assert!(collisions["collisions"].as_array().is_some_and(Vec::is_empty), "{collisions}");
    // Contact sets off: the same drag goes through A, to the slider's limit.
    let mut s = blocks_on_a_rail(false);
    let r = pull(&mut s);
    assert!(r["stopped_by_contact"].is_null(), "{r}");
    assert!(r["stopped_by_limit"].is_array(), "{r}");
    let x = place(&s, "B", near_face).x;
    assert!(x.abs() < 1e-6, "B's face at x = {x}, not at the limit, 0");
}

#[test]
fn body_centre_uses_exact_body_bounds() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    let c = super::body_centre(&s, 0);
    assert!((c.x - 5.0).abs() < 1e-6 && (c.y - 10.0).abs() < 1e-6 && (c.z - 15.0).abs() < 1e-6, "got {c:?}");
}
