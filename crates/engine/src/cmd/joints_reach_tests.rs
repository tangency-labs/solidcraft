//! `joint.reach`: inverse kinematics against closed-form answers (planar arms), the nearest pose
//! out of reach, limits, and a four-bar reached through its rocker.

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use super::tests::{four_bar, meet, occ, part, pin, place, run};
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
    let steps = s.history.len();
    let r = reach(&mut s, "Fore", tip, json!({"point": pt(to)}));
    assert_eq!((&r["reached"], &r["moved"], s.history.len()), (&json!(true), &json!(false), steps), "{r}");
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
        let steps = s.history.len();
        let again = reach(&mut s, "Crank", p0, json!({"point":pt(point), "continuous":true, "quiet":true}));
        assert_eq!((&again["reached"], &again["moved"], s.history.len()), (&json!(true), &json!(false), steps), "{again}");
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
