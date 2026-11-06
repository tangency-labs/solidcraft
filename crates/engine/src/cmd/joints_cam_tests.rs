//! Cams: a revolute joint drives a follower that rides the cam's profile. Expected values are
//! analytic: an eccentric circle of radius R whose centre is e from its axis puts a knife-edge
//! follower on a line h from the axis at e·cos θ + √(R² − (h − e·sin θ)²), a roller of radius r
//! where a knife on the circle of radius R + r would be, a flat face at e·cos θ + R; an
//! oscillating follower's roller sits where its arm's circle meets the circle of radius R + r.

use super::*;
use solidcraft_doc::joints::Coupling;
use solidcraft_doc::joints::cam::CamProfile;

/// The cam's eccentricity and radius (mm).
const E: f64 = 10.0;
const R: f64 = 30.0;

fn joint_id(s: &Session, name: &str) -> u64 {
    s.doc.assembly.joints.iter().find(|j| j.name == name).map(|j| j.id).unwrap_or(0)
}

/// A joint's value as `joint.list` gives it (degrees or mm).
fn shown(s: &mut Session, joint: &str) -> f64 {
    let l = run(s, "joint.list", json!({"quiet": true}));
    let j = l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == joint).cloned().unwrap_or_default();
    j["values"][0]["value"].as_f64().unwrap_or(f64::NAN)
}

/// Where a knife edge on a line `h` from the axis stands on the circle of radius `r` about the
/// cam's centre turned by `deg`.
fn knife(deg: f64, h: f64, r: f64) -> f64 {
    let t = deg.to_radians();
    E * t.cos() + (r * r - (h - E * t.sin()).powi(2)).sqrt()
}

/// A grounded base, and a cam: a disc of radius `rc` (a parameter, 30 mm) about (10, 0), 5 mm thick,
/// drawn in sketch `CamSketch` of its own component, on a revolute joint `Cam` about Z through the
/// origin (the cam is its b side).
fn cam_on_base() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "rc", "expression": "30 mm"}));
    run(&mut s, "parameters.add", json!({"name": "th", "expression": "5 mm"}));
    part(&mut s, "Base", [-90.0, -90.0, -20.0]);
    let g = occ(&s, "Base");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Cam"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "CamSketch"}));
    run(&mut s, "sketch.circle.center", json!({"center": [E, 0], "radius": R, "id": "c1"}));
    run(&mut s, "sketch.dimension", json!({"entities": ["c1"], "type": "radius", "value": "rc"}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": "th"}));
    pin(&mut s, "revolute", "Cam", "Base", "Cam", Vec3::ZERO, Vec3::Z);
    s
}

/// A tappet (a 20 mm bar, 4 wide) sliding along +X on the line y = `h`, its end at x = `tip`, on a
/// slider `Tappet`.
fn tappet(s: &mut Session, h: f64, tip: f64) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": "Tappet"}));
    run(s, "solid.box", json!({"length": 20, "width": 4, "height": 5, "corner": [tip, h - 2.0, 0]}));
    pin(s, "slider", "Tappet", "Base", "Tappet", Vec3::new(0.0, h, 2.5), Vec3::X);
}

fn close(got: f64, want: f64, tol: f64) -> bool {
    (got - want).abs() < tol
}

/// A knife-edge tappet on the eccentric circle, its line through the axis and 6 mm past it, rises
/// and falls as e·cos θ + √(R² − (h − e·sin θ)²); the tappet's body goes with it.
#[test]
fn a_knife_edge_tappet_rides_an_eccentric_circle() {
    for h in [0.0, 6.0] {
        let mut s = cam_on_base();
        let tip = knife(0.0, h, R);
        tappet(&mut s, h, tip);
        let r = run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
        assert_eq!((r["link"].clone(), r["reverse"].clone()), (json!(0), json!(false)), "{r}");
        assert!(close(r["follower_offset"].as_f64().unwrap_or(f64::NAN), h, 1e-9), "{r}");
        // It stays where it is, and its travel over a turn is the cam's.
        assert!(close(shown(&mut s, "Tappet"), 0.0, 1e-9), "{r}");
        let over_a_turn: Vec<f64> = (0..3600).map(|k| knife(f64::from(k) / 10.0, h, R)).collect();
        let rise = over_a_turn.iter().fold(f64::MIN, |a, b| a.max(*b)) - over_a_turn.iter().fold(f64::MAX, |a, b| a.min(*b));
        assert!(close(r["lift"]["rise"].as_f64().unwrap_or(f64::NAN), rise, 2e-3), "{r} vs {rise}");
        for deg in [30.0f64, 90.0, 135.0, 200.0, 300.0, -45.0, 400.0] {
            let d = run(&mut s, "joint.drive", json!({"joint": "Cam", "value": deg}));
            assert_eq!(d["conflicts"].as_array().map(Vec::len), Some(0), "{d}");
            let want = knife(deg, h, R) - tip;
            assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "h {h} θ {deg}: {} vs {want}", shown(&mut s, "Tappet"));
            let end = place(&s, "Tappet", Vec3::new(tip, h, 2.5));
            assert!(close(end.x, tip + want, 1e-3) && close(end.y, h, 1e-9), "{end:?}");
        }
    }
}

/// The follower's line is through its tip, wherever its slider's origin is across the line; a
/// guide of its own (grounded, the slider's b side) makes the tappet the slider's a side, and it
/// rides the same way.
#[test]
fn a_tappet_on_a_guide_of_its_own_rides_its_tip_s_line() {
    let h = 6.0;
    let tip = knife(0.0, h, R);
    let mut s = cam_on_base();
    part(&mut s, "Guide", [70.0, 20.0, 0.0]);
    let g = occ(&s, "Guide");
    run(&mut s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Tappet"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 4, "height": 5, "corner": [tip, h - 2.0, 0]}));
    // The slider's origins 9 mm across from the tappet's line, a the tappet, b the guide.
    pin(&mut s, "slider", "Tappet", "Tappet", "Guide", Vec3::new(0.0, h + 9.0, 2.5), Vec3::X);
    let r = run(
        &mut s,
        "joint.cam",
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "tip": {"occurrence": "Tappet", "point": [tip, h, 2.5]}}),
    );
    assert!(close(r["follower_offset"].as_f64().unwrap_or(f64::NAN), h, 1e-9), "{r}");
    for deg in [70.0f64, 160.0, 290.0] {
        run(&mut s, "joint.drive", json!({"joint": "Cam", "value": deg}));
        let end = place(&s, "Tappet", Vec3::new(tip, h, 2.5));
        assert!(close(end.x, knife(deg, h, R), 1e-3), "θ {deg}: {end:?} vs {}", knife(deg, h, R));
    }
}

/// A roller of radius 5 rides the circle of radius R + 5; a flat face e·cos θ + R, and when the
/// cam's radius parameter grows by 2 the flat face stands 2 further out at every angle.
#[test]
fn rollers_and_flat_faces_ride_their_offset_circles() {
    let mut s = cam_on_base();
    let tip = knife(0.0, 0.0, R + 5.0);
    tappet(&mut s, 0.0, tip);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": "5 mm"}));
    for deg in [60.0f64, 150.0, 250.0] {
        run(&mut s, "joint.drive", json!({"joint": "Cam", "value": deg}));
        let want = knife(deg, 0.0, R + 5.0) - tip;
        assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "roller θ {deg}: {} vs {want}", shown(&mut s, "Tappet"));
    }
    let mut s = cam_on_base();
    tappet(&mut s, 3.0, E + R);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "flat"}));
    for deg in [60.0f64, 150.0, 250.0] {
        run(&mut s, "joint.drive", json!({"joint": "Cam", "value": deg}));
        let want = E * deg.to_radians().cos() - E;
        assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "flat θ {deg}: {} vs {want}", shown(&mut s, "Tappet"));
    }
    // Editing the profile: the table is worked out again.
    run(&mut s, "parameters.change", json!({"name": "rc", "expression": "32 mm"}));
    let want = E * 250f64.to_radians().cos() - E + 2.0;
    assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "after the edit: {} vs {want}", shown(&mut s, "Tappet"));
}

/// The cam's top edge picked on its body gives the table its sketch gives, and follows the
/// design the same way.
#[test]
fn picked_edges_give_the_sketch_s_table() {
    let table = |profile: Value| {
        let mut s = cam_on_base();
        tappet(&mut s, 0.0, E + R);
        run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": profile}));
        s.doc.assembly.links.first().map(|l| (l.lift.clone(), l.offset)).unwrap_or_default()
    };
    let (sketch, a) = table(json!({"sketch": "CamSketch"}));
    // Picked on the rim a quarter turn from where the extruded circle's seam meets it: a pick on
    // that vertex could be either edge.
    let (edges, b) = table(json!({"edges": [[E, R, 5]]}));
    assert_eq!((sketch.len(), edges.len()), (360, 360));
    assert!(sketch.iter().zip(&edges).all(|(x, y)| close(*x, *y, 1e-3)) && close(a, b, 1e-3));
    // A second disc of the cam's component touching it in the same plane is not part of its loop.
    let mut s = cam_on_base();
    run(&mut s, "component.activate", json!({"component": "Cam"}));
    run(&mut s, "solid.cylinder", json!({"base": [E + R + 20.0, 0, 0], "axis": [0, 0, 1], "radius": 20, "height": 5}));
    tappet(&mut s, 0.0, E + R);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[E - R, 0, 5]]}}));
    let touching = s.doc.assembly.links.first().map(|l| l.lift.clone()).unwrap_or_default();
    assert!(touching.len() == 360 && touching.iter().zip(&sketch).all(|(x, y)| close(*x, *y, 1e-3)), "{touching:?}");
    let mut s = cam_on_base();
    tappet(&mut s, 0.0, E + R);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[E, R, 5]]}}));
    assert!(matches!(
        s.doc.assembly.links.first().and_then(|l| l.coupling.clone()),
        Some(Coupling::Cam(c)) if matches!(&c.profile, CamProfile::Edges { names, .. } if names.first().is_some_and(|n| !n.is_empty()))
    ));
    run(&mut s, "parameters.change", json!({"name": "rc", "expression": "32 mm"}));
    // A thicker cam: the picked edge is found at its new height, and its loop with it.
    run(&mut s, "parameters.change", json!({"name": "th", "expression": "7 mm"}));
    let l = run(&mut s, "joint.list", json!({"quiet": true}));
    assert!(l["links"][0].get("error").is_none(), "{l}");
    run(&mut s, "joint.drive", json!({"joint": "Cam", "value": 90}));
    let want = knife(90.0, 0.0, 32.0) - (E + R);
    assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "{} vs {want}", shown(&mut s, "Tappet"));
}

/// An arm of 50 about (−50, 40) with a roller of 5: its roller's centre is where the arm's circle
/// meets the circle of radius R + 5 about the cam's centre, the meeting it turns into first from
/// pointing away from the cam's axis.
#[test]
fn an_oscillating_roller_follower_swings_to_the_offset_circle() {
    let (pivot, arm, roller) = (Vec3::new(-50.0, 40.0, 2.5), 50.0, 5.0);
    let away = pivot.y.atan2(pivot.x);
    let wrap = |a: f64| (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    let angle = |p: Vec3| (p.y - pivot.y).atan2(p.x - pivot.x);
    // Where the roller's centre is at cam angle `deg`, turning the way `turn` says.
    let centre = |deg: f64, turn: f64| {
        let c = Vec3::new(E * deg.to_radians().cos(), E * deg.to_radians().sin(), 2.5);
        let [p, q] = meet(pivot, arm, c, R + roller);
        let swept = |x: Vec3| (turn * (angle(x) - away)).rem_euclid(std::f64::consts::TAU);
        if swept(p) < swept(q) { p } else { q }
    };
    let [p, q] = meet(pivot, arm, Vec3::new(E, 0.0, 2.5), R + roller);
    let start = if wrap(angle(p) - away).abs() < wrap(angle(q) - away).abs() { p } else { q };
    let turn = wrap(angle(start) - away).signum();
    let mut s = cam_on_base();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Arm"}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 5, "corner": [start.x - 2.0, start.y - 2.0, 0]}));
    pin(&mut s, "revolute", "Arm", "Base", "Arm", pivot, Vec3::Z);
    // An oscillating follower needs its tip.
    assert!(s.execute("joint.cam", &json!({"a": "Cam", "b": "Arm", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": 5})).is_err());
    let r = run(
        &mut s,
        "joint.cam",
        json!({"a": "Cam", "b": "Arm", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": 5,
            "tip": {"occurrence": "Arm", "point": [start.x, start.y, 2.5]}}),
    );
    assert_eq!((r["reverse"].clone(), r["lift"]["unit"].clone()), (json!(turn < 0.0), json!("deg")), "{r}");
    for deg in [40.0f64, 120.0, 210.0, 330.0] {
        run(&mut s, "joint.drive", json!({"joint": "Cam", "value": deg}));
        let want = wrap(angle(centre(deg, turn)) - angle(start)).to_degrees();
        assert!(close(shown(&mut s, "Arm"), want, 1e-3), "θ {deg}: {} vs {want}", shown(&mut s, "Arm"));
        let at = place(&s, "Arm", start);
        assert!(at.dist(centre(deg, turn)) < 1e-3, "θ {deg}: {at:?}");
    }
    // The roller's round face picked on its arm is where it touches, and its radius: the same table.
    let table = s.doc.assembly.links.first().map(|l| l.lift.clone()).unwrap_or_default();
    let mut t = cam_on_base();
    run(&mut t, "component.activate", json!({"component": "root"}));
    run(&mut t, "component.create", json!({"name": "Arm"}));
    run(&mut t, "solid.cylinder", json!({"base": [start.x, start.y, 0], "axis": [0, 0, 1], "radius": roller, "height": 5}));
    pin(&mut t, "revolute", "Arm", "Base", "Arm", pivot, Vec3::Z);
    let face = [start.x + roller, start.y, 2.5];
    run(
        &mut t,
        "joint.cam",
        json!({"a": "Cam", "b": "Arm", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": {"occurrence": "Arm", "circle": face}}),
    );
    let picked = t.doc.assembly.links.first().map(|l| l.lift.clone()).unwrap_or_default();
    assert!(picked.len() == 360 && picked.iter().zip(&table).all(|(x, y)| close(*x, *y, 1e-6)));
}

/// A stationary cam: the profile on the grounded part, a rotor turning about it with the tappet's
/// guide on it. Turned by +θ against the cam, the tappet stands where it would on a cam turned by
/// −θ. A guide on the cam's own (grounded) side would turn with the profile: refused.
#[test]
fn a_stationary_cam_drives_a_follower_on_a_turning_rotor() {
    let h = 6.0;
    let tip = knife(0.0, h, R);
    let mut s = cam_on_base();
    // The cam is the fixed part: ground it, and take the base off its joint.
    let (cam, base) = (occ(&s, "Cam"), occ(&s, "Base"));
    run(&mut s, "joint.delete", json!({"joint": "Cam"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": cam, "grounded": true}));
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": false}));
    pin(&mut s, "revolute", "Turret", "Cam", "Base", Vec3::ZERO, Vec3::Z);
    tappet(&mut s, h, tip);
    run(&mut s, "joint.cam", json!({"a": "Turret", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    for deg in [50.0f64, 140.0, 260.0] {
        let d = run(&mut s, "joint.drive", json!({"joint": "Turret", "value": deg}));
        assert_eq!(d["conflicts"].as_array().map(Vec::len), Some(0), "{d}");
        let want = knife(-deg, h, R) - tip;
        assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "θ {deg}: {} vs {want}", shown(&mut s, "Tappet"));
    }
    let mut s = cam_on_base();
    let (cam, base) = (occ(&s, "Cam"), occ(&s, "Base"));
    run(&mut s, "joint.delete", json!({"joint": "Cam"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": cam, "grounded": true}));
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": false}));
    part(&mut s, "Rotor", [-90.0, 60.0, -20.0]);
    pin(&mut s, "revolute", "Turret", "Cam", "Rotor", Vec3::ZERO, Vec3::Z);
    run(&mut s, "occurrence.ground", json!({"occurrence": base, "grounded": true}));
    tappet(&mut s, h, tip);
    let e = s.execute("joint.cam", &json!({"a": "Turret", "b": "Tappet", "profile": {"sketch": "CamSketch"}})).map_err(|e| e.to_string()).err();
    assert!(e.as_deref().is_some_and(|e| e.contains("turns with the cam")), "{e:?}");
}

/// A motion study turning the cam plays the tappet through its analytic travel.
#[test]
fn a_cam_plays_in_a_motion_study() {
    let mut s = cam_on_base();
    let tip = knife(0.0, 0.0, R);
    tappet(&mut s, 0.0, tip);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    run(&mut s, "motion.study", json!({"name": "Turn", "steps": 10, "keys": [{"joint": "Cam", "points": [[0, 0], [10, 360]]}]}));
    for step in [0, 2, 5, 7, 10] {
        run(&mut s, "motion.play", json!({"study": "Turn", "step": step}));
        let want = knife(36.0 * f64::from(step), 0.0, R) - tip;
        assert!(close(shown(&mut s, "Tappet"), want, 1e-3), "step {step}: {} vs {want}", shown(&mut s, "Tappet"));
    }
}

/// The tappet is the piston of a slider-crank: a 40 mm rod from its end to a 20 mm crank about
/// (95, 0). Turning the cam moves the tappet, and the loop turns the crank to where the rod's
/// circle meets the crank's, on the branch it started on. A stop in a contact set with the cam and
/// the tappet halts the cam where the tappet reaches it; the cam and the flat tappet only touch.
#[test]
fn a_cam_drives_a_closed_loop_and_stops_at_a_contact() {
    let mut s = cam_on_base();
    let tip = knife(0.0, 0.0, R);
    tappet(&mut s, 0.0, tip);
    let (q, end) = (Vec3::new(95.0, 0.0, 2.5), Vec3::new(tip + 20.0, 0.0, 2.5));
    let crank_pin = |x: f64| meet(q, 20.0, Vec3::new(x, 0.0, 2.5), 40.0).into_iter().find(|p| p.y > 0.0).unwrap_or(Vec3::ZERO);
    let pin0 = crank_pin(end.x);
    for (name, at) in [("Rod", end), ("Crank", pin0)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [at.x - 1.0, at.y - 1.0, 10]}));
    }
    pin(&mut s, "revolute", "Crank", "Base", "Crank", q, Vec3::Z);
    pin(&mut s, "revolute", "Wrist", "Tappet", "Rod", end, Vec3::Z);
    pin(&mut s, "revolute", "Big end", "Rod", "Crank", pin0, Vec3::Z);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    for deg in [45.0f64, 120.0, 180.0] {
        let d = run(&mut s, "joint.drive", json!({"joint": "Cam", "value": deg}));
        assert_eq!(d["conflicts"].as_array().map(Vec::len), Some(0), "{d}");
        let x = end.x + knife(deg, 0.0, R) - tip;
        let at = place(&s, "Crank", pin0);
        assert!(at.dist(crank_pin(x)) < 1e-3, "θ {deg}: {at:?} vs {:?}", crank_pin(x));
    }
    // A flat tappet, a stop 5 mm past where its end is at the top, and contact on.
    let mut s = cam_on_base();
    tappet(&mut s, 0.0, E + R);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "flat"}));
    run(&mut s, "joint.drive", json!({"joint": "Cam", "value": 180}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Stop"}));
    run(&mut s, "solid.box", json!({"length": 3, "width": 4, "height": 5, "corner": [E + R + 15.0, -2, 0]}));
    let st = occ(&s, "Stop");
    run(&mut s, "occurrence.ground", json!({"occurrence": st, "grounded": true}));
    run(&mut s, "contact.create", json!({"occurrences": ["Cam", "Tappet", "Stop"]}));
    // Touching the cam stops nothing: half a turn moves the tappet 20 mm back up, as far as the stop.
    let d = run(&mut s, "joint.drive", json!({"joint": "Cam", "value": 90}));
    assert!(d["stopped_by_contact"].is_null(), "{d}");
    let d = run(&mut s, "joint.drive", json!({"joint": "Cam", "value": 0}));
    assert!(d["stopped_by_contact"].is_object(), "{d}");
    let reached = shown(&mut s, "Tappet");
    assert!(reached > -5.5 && reached < -4.5, "{reached}");
}

/// What a cam can't be made from is refused, and a table edited by hand never puts a NaN into a
/// joint's value.
#[test]
fn hostile_cams_are_refused() {
    let mut s = cam_on_base();
    tappet(&mut s, 0.0, E + R);
    let refused = |s: &mut Session, p: Value| s.execute("joint.cam", &p).map_err(|e| e.to_string()).err().unwrap_or_else(|| panic!("accepted {p}"));
    // An open curve makes no closed profile.
    run(&mut s, "component.activate", json!({"component": "Cam"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Open"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 5], [10, 20]]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = refused(&mut s, json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "Open"}}));
    assert!(e.contains("closed profile"), "{e}");
    // Two closed edges are not one loop; a line past the cam misses it.
    let e = refused(&mut s, json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[E + R, 0, 5], [E + R, 0, 0]]}}));
    assert!(e.contains("closed loop"), "{e}");
    let mut far = cam_on_base();
    tappet(&mut far, 50.0, 0.0);
    let e = refused(&mut far, json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    assert!(e.contains("misses"), "{e}");
    for p in [
        json!({"a": "Tappet", "b": "Cam", "profile": {"sketch": "CamSketch"}}),
        json!({"a": "Cam", "b": "Cam", "profile": {"sketch": "CamSketch"}}),
        json!({"a": "Cam", "b": "Tappet"}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch", "index": 7}}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"edges": []}}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"edges": [[1, "x", 0]]}}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"edges": vec![[E + R, 0.0, 5.0]; 65]}}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller"}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": -1}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": "nope"}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": 1e300}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "spring"}),
        json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "link": 3}),
    ] {
        refused(&mut s, p);
    }
    assert!(s.doc.assembly.links.is_empty());
    // A table edited by hand: NaN leaves the follower where it is, any length reads.
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}}));
    let before = solidcraft_doc::joints::joint_values(&s.doc.assembly);
    for lift in [vec![f64::NAN; 360], vec![1.0, 2.0, 3.0], vec![f64::INFINITY]] {
        let mut d = (*s.doc).clone();
        if let Some(l) = d.assembly.links.first_mut() {
            l.lift = lift;
        }
        let vals = solidcraft_doc::joints::joint_values(&d.assembly);
        assert!(vals.values().flatten().all(|v| v.is_finite()), "{vals:?}");
        // The follower keeps where it is.
        assert_eq!(vals.get(&joint_id(&s, "Tappet")), d.assembly.joints.iter().find(|j| j.name == "Tappet").map(|j| &j.values), "{before:?}");
    }
    // A table on a plain link is not read.
    let mut d = (*s.doc).clone();
    if let Some(l) = d.assembly.links.first_mut() {
        (l.coupling, l.ratio, l.offset) = (None, 0.0, 3.0);
    }
    assert_eq!(solidcraft_doc::joints::joint_values(&d.assembly).get(&joint_id(&s, "Tappet")).and_then(|q| q.first()).copied(), Some(3.0));
    // A roller's picked face must turn about an axis parallel to the cam's, a tip given or not.
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "Pin"}));
    run(&mut s, "solid.cylinder", json!({"base": [60, 30, 0], "axis": [1, 0, 0], "radius": 3, "height": 10}));
    run(&mut s, "joint.delete", json!({"link": 0}));
    let tilted = json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller",
        "radius": {"occurrence": "Pin", "circle": [65, 33, 0]}, "tip": {"occurrence": "Tappet", "point": [E + R, 0, 2.5]}});
    let e = refused(&mut s, tilted);
    assert!(e.contains("roller's axis"), "{e}");
}

/// A cam saves and opens with its definition and table; a link saved before cams reads as a
/// plain link.
#[test]
fn cams_save_and_open_with_their_table() {
    let mut s = cam_on_base();
    tappet(&mut s, 0.0, E + R);
    run(&mut s, "joint.cam", json!({"a": "Cam", "b": "Tappet", "profile": {"sketch": "CamSketch"}, "follower": "roller", "radius": 2}));
    run(&mut s, "joint.drive", json!({"joint": "Cam", "value": 30}));
    let text = s.doc.to_json();
    let back = solidcraft_doc::format::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.assembly.links, s.doc.assembly.links);
    assert_eq!(back.assembly.links.first().map(|l| l.lift.len()), Some(360));
    let l = run(&mut s, "joint.list", json!({}));
    let link = &l["links"][0];
    assert!(link.get("ratio").is_none() && link["lift"]["unit"] == "mm" && link["coupling"]["type"] == "cam", "{link}");
    assert!(link["coupling"].get("lift").is_none(), "{link}");
    let mut v: Value = serde_json::from_str(&text).unwrap_or_default();
    for l in v["assembly"]["links"].as_array_mut().into_iter().flatten() {
        l.as_object_mut().map(|o| (o.remove("coupling"), o.remove("lift")));
    }
    let old = solidcraft_doc::format::read(&v.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(old.assembly.links.iter().all(|l| l.coupling.is_none() && l.lift.is_empty()));
}
