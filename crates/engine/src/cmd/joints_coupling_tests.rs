//! Gears, racks and screws: motion links whose ratio comes from radii or a pitch. Expected values
//! are analytic: a gear train turns by the ratio of its radii, a rack travels r·θ, a screw
//! advances its pitch per turn.

use super::*;
use solidcraft_doc::joints::{Coupling, Size};

/// A joint's value as `joint.list` gives it (degrees or mm).
fn shown(s: &mut Session, joint: &str) -> f64 {
    let l = run(s, "joint.list", json!({"quiet": true}));
    let j = l["joints"].as_array().into_iter().flatten().find(|j| j["name"] == joint).cloned().unwrap_or_default();
    j["values"][0]["value"].as_f64().unwrap_or(f64::NAN)
}

fn close_to(got: f64, want: f64) -> bool {
    (got - want).abs() < 1e-6
}

/// A round disc (a gear's pitch circle) of radius `r` about a vertical axis through (x, 0), 5 mm
/// thick, in a component of its own, on a revolute joint to the grounded base.
fn disc(s: &mut Session, name: &str, x: f64, r: &str) {
    loose_disc(s, name, x, r);
    pin(s, "revolute", name, "Base", name, Vec3::new(x, 0.0, 0.0), Vec3::Z);
}

/// The disc alone, on no joint.
fn loose_disc(s: &mut Session, name: &str, x: f64, r: &str) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name, "grounded": false}));
    run(s, "solid.cylinder", json!({"base": [x, 0, 0], "axis": [0, 0, 1], "radius": r, "height": 5}));
}

fn base(s: &mut Session) {
    part(s, "Base", [-60.0, -60.0, -20.0]);
    let g = occ(s, "Base");
    run(s, "occurrence.ground", json!({"occurrence": g, "grounded": true}));
}

/// Gears of pitch radii r1 (a parameter), 20 and 15 in a row: G1 meshes G2 (radii picked on their
/// discs), G2 meshes G3 (radii typed).
fn gear_train() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "r1", "expression": "10 mm"}));
    base(&mut s);
    disc(&mut s, "G1", 0.0, "r1");
    disc(&mut s, "G2", 30.0, "20 mm");
    disc(&mut s, "G3", 65.0, "15 mm");
    let r = run(
        &mut s,
        "joint.gear",
        json!({"a": "G1", "b": "G2", "ra": {"occurrence": "G1", "circle": [10, 0, 2.5]}, "rb": {"occurrence": "G2", "circle": [50, 0, 2.5]}}),
    );
    assert_eq!((r["link"].clone(), r["ratio"].as_f64().map(|x| close_to(x, -0.5))), (json!(0), Some(true)), "{r}");
    run(&mut s, "joint.gear", json!({"a": "G2", "b": "G3", "ra": 20, "rb": "15 mm"}));
    s
}

/// A point on a disc's rim, where the disc is placed.
fn rim(s: &Session, name: &str, x: f64, r: f64) -> Vec3 {
    place(s, name, Vec3::new(x + r, 0.0, 2.5))
}

fn turned(x: f64, r: f64, deg: f64) -> Vec3 {
    let a = deg.to_radians();
    Vec3::new(x + r * a.cos(), r * a.sin(), 2.5)
}

#[test]
fn a_gear_train_of_three_turns_by_its_radii() {
    let mut s = gear_train();
    let r = run(&mut s, "joint.drive", json!({"joint": "G1", "value": 90}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // ω2 = −ω1·r1/r2, ω3 = −ω2·r2/r3 = ω1·r1/r3: external gears turn the other way.
    assert!(close_to(shown(&mut s, "G2"), -45.0) && close_to(shown(&mut s, "G3"), 60.0), "{}", run(&mut s, "joint.list", json!({})));
    for (name, x, r, deg) in [("G1", 0.0, 10.0, 90.0), ("G2", 30.0, 20.0, -45.0), ("G3", 65.0, 15.0, 60.0)] {
        assert!(near(rim(&s, name, x, r), turned(x, r, deg)), "{name}: {:?}", rim(&s, name, x, r));
    }
    // The picked radius follows the design: G1 grows to 12, so G2 turns 90·12/20.
    run(&mut s, "parameters.change", json!({"name": "r1", "expression": "12 mm"}));
    assert!(close_to(shown(&mut s, "G2"), -54.0) && close_to(shown(&mut s, "G3"), 72.0), "{}", run(&mut s, "joint.list", json!({})));
    let l = run(&mut s, "joint.list", json!({}));
    assert!(close_to(l["links"][0]["ratio"].as_f64().unwrap_or(f64::NAN), -0.6), "{l}");
    assert_eq!(l["links"][0]["coupling"]["type"], "gear", "{l}");
    // A motion study playing G1 turns the train the same way.
    run(&mut s, "motion.study", json!({"name": "Spin", "steps": 10, "keys": [{"joint": "G1", "points": [[0, 0], [10, 30]]}]}));
    run(&mut s, "motion.play", json!({"study": "Spin", "step": 10}));
    assert!(close_to(shown(&mut s, "G3"), 24.0), "{}", shown(&mut s, "G3"));
    // An internal (ring) gear turns its mate the same way; the redefined link leaves G3 where it
    // is (G2 at -18°), and G1 turning on 30° more turns G2 on -18° and G3 on -24°.
    let r = run(&mut s, "joint.gear", json!({"a": "G2", "b": "G3", "ra": 20, "rb": 15, "internal": true, "link": 1}));
    assert_eq!(r["link"], 1, "{r}");
    assert!(close_to(shown(&mut s, "G3"), 24.0), "{}", run(&mut s, "joint.list", json!({})));
    run(&mut s, "joint.drive", json!({"joint": "G1", "value": 60}));
    assert!(close_to(shown(&mut s, "G2"), -36.0) && close_to(shown(&mut s, "G3"), 0.0), "{}", run(&mut s, "joint.list", json!({})));
    // Undo takes the redefinition back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.assembly.links.get(1).and_then(|l| l.coupling.clone()).map(|c| matches!(c, Coupling::Gear { internal: false, .. })), Some(true));
}

#[test]
fn gears_save_and_open_with_their_definition() {
    let mut s = gear_train();
    run(&mut s, "joint.drive", json!({"joint": "G1", "value": 30}));
    let text = s.doc.to_json();
    let back = solidcraft_doc::format::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.assembly.links, s.doc.assembly.links);
    assert!(matches!(back.assembly.links.first().and_then(|l| l.coupling.as_ref()), Some(Coupling::Gear { ra: Size::Circle(_), .. })));
    // A link saved before couplings (no `coupling`) opens as a plain link.
    let mut v: Value = serde_json::from_str(&text).unwrap_or_default();
    for l in v["assembly"]["links"].as_array_mut().into_iter().flatten() {
        l.as_object_mut().map(|o| o.remove("coupling"));
    }
    let old = solidcraft_doc::format::read(&v.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(old.assembly.links.iter().all(|l| l.coupling.is_none()) && old.assembly.links.len() == 2);
}

/// A coupling command put in place of a motion link takes the sizes the call leaves out from the
/// coupling it replaces: an edit dialog can't re-offer a picked round face, so an edited gear
/// keeps the radii picked on its discs, a rack and a screw their radius and pitch.
#[test]
fn an_edited_coupling_keeps_the_sizes_it_is_not_given() {
    let mut s = gear_train();
    run(&mut s, "joint.drive", json!({"joint": "G1", "value": 60})); // G2 at -30°
    let l = run(&mut s, "joint.list", json!({}));
    let (ra, rb) = (l["links"][0]["coupling"]["ra"].clone(), l["links"][0]["coupling"]["rb"].clone());
    assert!(ra.is_object() && rb.is_object(), "{l}"); // the round faces picked on the discs
    let r = run(&mut s, "joint.gear", json!({"a": "G1", "b": "G2", "link": 0}));
    assert_eq!(r["link"], 0, "{r}");
    let l = run(&mut s, "joint.list", json!({}));
    assert_eq!((l["links"][0]["coupling"]["ra"].clone(), l["links"][0]["coupling"]["rb"].clone()), (ra, rb), "{l}");
    // The kept radii drive as before: G1 at 90° puts G2 at -45° (r1/r2 = 10/20).
    run(&mut s, "joint.drive", json!({"joint": "G1", "value": 90}));
    assert!(close_to(shown(&mut s, "G2"), -45.0), "{}", shown(&mut s, "G2"));
    // A rack redefined without a radius keeps the radius picked on the pinion's round face, and
    // the rack stays put: 8 mm turns it 8·π/4 per quarter turn, before and after.
    let mut s = rack();
    run(&mut s, "joint.rack_pinion", json!({"a": "Pinion", "b": "Rail", "radius": {"occurrence": "Pinion", "circle": [8, 0, 2]}}));
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 45}));
    let before = run(&mut s, "joint.list", json!({}));
    let radius = before["links"][0]["coupling"]["radius"].clone();
    assert!(radius.is_object(), "{before}");
    let r = run(&mut s, "joint.rack_pinion", json!({"a": "Pinion", "b": "Rail", "link": 0}));
    assert_eq!(r["link"], 0, "{r}");
    assert_eq!(run(&mut s, "joint.list", json!({}))["links"][0]["coupling"]["radius"], radius);
    assert!(close_to(shown(&mut s, "Rail"), 8.0 * std::f64::consts::FRAC_PI_4), "{}", shown(&mut s, "Rail"));
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 90}));
    assert!(close_to(shown(&mut s, "Rail"), 8.0 * std::f64::consts::FRAC_PI_2), "{}", shown(&mut s, "Rail"));
    // A screw redefined without a pitch keeps the pitch it was given.
    let mut s = Session::default();
    base(&mut s);
    part(&mut s, "Nut", [-1.0, -1.0, 0.0]);
    pin(&mut s, "cylindrical", "Thread", "Base", "Nut", Vec3::ZERO, Vec3::Z);
    run(&mut s, "joint.screw", json!({"joint": "Thread", "pitch": "2 mm"}));
    let r = run(&mut s, "joint.screw", json!({"joint": "Thread", "link": 0, "reverse": true}));
    assert_eq!(r["link"], 0, "{r}");
    assert_eq!(s.doc.assembly.links[0].coupling, Some(Coupling::Screw { pitch: Size::Expr("2 mm".into()), reverse: true }));
}

/// A pinion of radius 8 about +Z at the origin, a rack under it sliding along +X, and a stop the
/// rack runs into after 10 mm.
fn rack() -> Session {
    let mut s = Session::default();
    base(&mut s);
    disc(&mut s, "Pinion", 0.0, "8 mm");
    for (name, corner, length) in [("Rack", [-20.0, -14.0, 0.0], 40.0), ("Stop", [30.0, -14.0, 0.0], 4.0)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name, "grounded": false}));
        run(&mut s, "solid.box", json!({"length": length, "width": 4, "height": 4, "corner": corner}));
    }
    pin(&mut s, "slider", "Rail", "Base", "Rack", Vec3::new(0.0, -10.0, 0.0), Vec3::X);
    pin(&mut s, "rigid", "Stop", "Base", "Stop", Vec3::new(30.0, -10.0, 0.0), Vec3::Z);
    s
}

#[test]
fn a_rack_travels_r_times_theta() {
    let mut s = rack();
    let r = run(&mut s, "joint.rack_pinion", json!({"a": "Pinion", "b": "Rail", "radius": {"occurrence": "Pinion", "circle": [8, 0, 2]}}));
    // Per degree in the answer, as joint.list gives a link between an angle and a distance.
    assert!(close_to(r["ratio"].as_f64().unwrap_or(f64::NAN), 8f64.to_radians()), "{r}");
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 45}));
    let travel = 8.0 * std::f64::consts::FRAC_PI_4;
    assert!(close_to(shown(&mut s, "Rail"), travel), "{}", shown(&mut s, "Rail"));
    // The pinion turns counter-clockwise about +Z; its bottom, where the rack is, moves along +X.
    assert!(near(place(&s, "Rack", Vec3::new(-20.0, -14.0, 0.0)), Vec3::new(-20.0 + travel, -14.0, 0.0)));
    // Contact sets hold the rack at the stop, and the pinion with it: 10 mm is 10/8 rad.
    run(&mut s, "contact.create", json!({"occurrences": ["Rack", "Stop"]}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 180}));
    assert!(r["stopped_by_contact"].is_object(), "{r}");
    let (angle, travel) = (shown(&mut s, "Pinion"), shown(&mut s, "Rail"));
    assert!((angle.to_radians() - 1.25).abs() < 0.01, "{angle}");
    assert!(close_to(travel, 8.0 * angle.to_radians()), "{travel} at {angle}°");
    // Either may drive: the rack turning the pinion, reversed.
    let mut s = rack();
    let r = run(&mut s, "joint.rack_pinion", json!({"a": "Rail", "b": "Pinion", "radius": "8 mm", "reverse": true}));
    assert!(close_to(r["ratio"].as_f64().unwrap_or(f64::NAN), -1.0 / 8f64.to_radians()), "{r}");
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 5}));
    assert!(close_to(shown(&mut s, "Pinion"), -(5.0f64 / 8.0).to_degrees()), "{}", shown(&mut s, "Pinion"));
}

#[test]
fn a_screw_advances_its_pitch_per_turn() {
    let mut s = Session::default();
    base(&mut s);
    part(&mut s, "Nut", [-1.0, -1.0, 0.0]);
    pin(&mut s, "cylindrical", "Thread", "Base", "Nut", Vec3::ZERO, Vec3::Z);
    run(&mut s, "joint.screw", json!({"joint": "Thread", "pitch": 1.5}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Thread", "values": [720]}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    let travel = r["values"][1]["value"].as_f64().unwrap_or(f64::NAN);
    assert!(close_to(travel, 3.0), "{r}");
    assert!(near(place(&s, "Nut", Vec3::new(-1.0, -1.0, 0.0)), Vec3::new(-1.0, -1.0, 3.0)));
    // A left-hand thread goes the other way, from where the nut is.
    run(&mut s, "joint.screw", json!({"joint": "Thread", "pitch": 1.5, "reverse": true, "link": 0}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Thread", "values": [1080]}));
    assert!(close_to(r["values"][1]["value"].as_f64().unwrap_or(f64::NAN), 1.5), "{r}");
    // A revolute joint and a slider, the pitch a parameter: changing it changes the lead.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lead", "expression": "2 mm"}));
    base(&mut s);
    part(&mut s, "Shaft", [-1.0, -1.0, 0.0]);
    part(&mut s, "Carriage", [10.0, -1.0, 0.0]);
    pin(&mut s, "revolute", "Spin", "Base", "Shaft", Vec3::ZERO, Vec3::Z);
    pin(&mut s, "slider", "Lift", "Base", "Carriage", Vec3::new(11.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.screw", json!({"a": "Spin", "b": "Lift", "pitch": "lead"}));
    run(&mut s, "joint.drive", json!({"joint": "Spin", "value": 900}));
    assert!(close_to(shown(&mut s, "Lift"), 5.0), "{}", shown(&mut s, "Lift"));
    run(&mut s, "parameters.change", json!({"name": "lead", "expression": "4 mm"}));
    assert!(close_to(shown(&mut s, "Lift"), 10.0), "{}", shown(&mut s, "Lift"));
}

/// A motor gear turns the four-bar's crank: half as fast, the other way round, and the loop
/// follows.
#[test]
fn a_gear_drives_a_closed_loop() {
    let (mut s, _, q0) = four_bar();
    part(&mut s, "Motor", [80.0, -1.0, -5.0]);
    pin(&mut s, "revolute", "Motor", "Ground", "Motor", Vec3::new(80.0, 0.0, 0.0), Vec3::Z);
    run(&mut s, "joint.gear", json!({"a": "Motor", "b": "Crank", "ra": 10, "rb": 20}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Motor", "value": -120}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    assert!(close_to(shown(&mut s, "Crank"), 60.0), "{}", shown(&mut s, "Crank"));
    assert!(near(place(&s, "Rocker", q0), rocker_pin(0.0, 60.0, q0)), "{:?}", place(&s, "Rocker", q0));
}

/// A pinion moves the piston of a slider-crank: the piston travels r·θ along its bore and the
/// crank follows, the rod keeping its length.
#[test]
fn a_rack_drives_a_slider_crank_from_its_piston() {
    let (mut s, p0, s0) = slider_crank();
    part(&mut s, "Pinion", [0.0, -30.0, -5.0]);
    pin(&mut s, "revolute", "Pinion", "Ground", "Pinion", Vec3::new(0.0, -30.0, 0.0), Vec3::Z);
    run(&mut s, "joint.rack_pinion", json!({"a": "Pinion", "b": "Bore", "radius": 5}));
    let r = run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 60}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(r["stopped_by_loop"].is_null(), "{r}");
    let travel = 5.0 * 60f64.to_radians();
    assert!(close_to(shown(&mut s, "Bore"), travel), "{}", shown(&mut s, "Bore"));
    let wrist = place(&s, "Piston", s0);
    assert!(near(wrist, s0 + Vec3::X * travel), "{wrist:?}");
    let pin_at = place(&s, "Rod", p0);
    assert!((pin_at.dist(wrist) - 40.0).abs() < 1e-6 && (pin_at.len() - 15.0).abs() < 1e-6, "{pin_at:?}");
}

#[test]
fn couplings_refuse_what_they_cannot_couple() {
    let mut s = gear_train();
    let refused = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).err().map(|e| e.to_string()).unwrap_or_default();
    // G3 already follows G2; a second writer, or a cycle back to G1, is refused.
    assert!(refused(&mut s, "joint.gear", json!({"a": "G1", "b": "G3", "ra": 1, "rb": 1})).contains("already follows"));
    assert!(refused(&mut s, "joint.gear", json!({"a": "G3", "b": "G1", "ra": 1, "rb": 1})).contains("already drives"));
    assert!(refused(&mut s, "joint.gear", json!({"a": "G1", "b": "G1", "ra": 1, "rb": 1})).contains("two different"));
    // A gear needs turns, a rack a turn and a travel.
    assert!(refused(&mut s, "joint.rack_pinion", json!({"a": "G1", "b": "G3", "radius": 5})).contains("slides"));
    // Hostile sizes and indices never panic and change nothing.
    let before = s.doc.assembly.clone();
    for bad in [
        json!(-3),
        json!(0),
        json!(1e300),
        json!("nope"),
        json!("1/0"),
        json!({"circle": [1e308, 0, 0]}),
        json!({"circle": "x"}),
        json!([1]),
        Value::Null,
    ] {
        for (id, p) in [
            ("joint.gear", json!({"a": "G3", "b": "G1", "ra": bad, "rb": 1})),
            ("joint.gear", json!({"a": "G1", "b": "G2", "ra": 1, "rb": bad, "link": 0})),
            ("joint.rack_pinion", json!({"a": "G1", "b": "G2", "radius": bad})),
            ("joint.screw", json!({"joint": "G1", "pitch": bad})),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
        }
    }
    for bad in [json!(-1), json!(2), json!(1.5), json!(1e300), json!("0"), json!([0]), Value::Null] {
        let p = json!({"a": "G1", "b": "G2", "ra": 1, "rb": 1, "link": bad});
        assert!(s.execute("joint.gear", &p).is_err(), "{p}");
    }
    assert_eq!(s.doc.assembly, before);
}

/// Gears of radii 10 and 20 on the grounded base, turned to 30° and 40° before they are coupled.
fn meshed_at_30_and_40() -> Session {
    let mut s = Session::default();
    base(&mut s);
    disc(&mut s, "GA", 0.0, "10 mm");
    disc(&mut s, "GB", 30.0, "20 mm");
    run(&mut s, "joint.drive", json!({"joint": "GA", "value": 30}));
    run(&mut s, "joint.drive", json!({"joint": "GB", "value": 40}));
    let r = run(&mut s, "joint.gear", json!({"a": "GA", "b": "GB", "ra": 10, "rb": 20}));
    assert!(close_to(r["ratio"].as_f64().unwrap_or(f64::NAN), -0.5), "{r}");
    s
}

/// The first motion link's offset in degrees, and whether its gear is reversed.
fn gear_link(s: &Session) -> (f64, Option<bool>) {
    let l = s.doc.assembly.links.first();
    let reverse = l.and_then(|l| match &l.coupling {
        Some(Coupling::Gear { reverse, .. }) => Some(*reverse),
        _ => None,
    });
    (l.map(|l| l.offset.to_degrees()).unwrap_or(f64::NAN), reverse)
}

/// A gear link redefined in place (turned the other way with `reverse`) leaves the driven gear
/// where it is, as a new link does, so the gears stay in mesh at the pose they are in: GB stays
/// at 40° rather than jumping to 70° (the old offset kept, the flip pivoting about GA at 0°).
/// Undo, redo and saving keep each definition with its pose.
#[test]
fn a_redefined_gear_stays_in_mesh_where_it_is() {
    let mut s = meshed_at_30_and_40();
    // b = -0.5·a + 55: GB keeps its 40° at GA's 30°.
    let (offset, reverse) = gear_link(&s);
    assert!(close_to(offset, 55.0) && reverse == Some(false), "{offset} {reverse:?}");
    assert!(close_to(shown(&mut s, "GB"), 40.0), "{}", shown(&mut s, "GB"));
    let r = run(&mut s, "joint.gear", json!({"a": "GA", "b": "GB", "ra": 10, "rb": 20, "reverse": true, "link": 0}));
    assert!(r["link"] == 0 && close_to(r["ratio"].as_f64().unwrap_or(f64::NAN), 0.5), "{r}");
    // b = 0.5·a + 25.
    let (offset, reverse) = gear_link(&s);
    assert!(close_to(offset, 25.0) && reverse == Some(true), "{offset} {reverse:?}");
    assert!(close_to(shown(&mut s, "GA"), 30.0) && close_to(shown(&mut s, "GB"), 40.0), "{}", run(&mut s, "joint.list", json!({})));
    assert!(near(rim(&s, "GB", 30.0, 20.0), turned(30.0, 20.0, 40.0)), "{:?}", rim(&s, "GB", 30.0, 20.0));
    // It turns from there, now the same way as GA.
    run(&mut s, "joint.drive", json!({"joint": "GA", "value": 0}));
    assert!(close_to(shown(&mut s, "GB"), 25.0), "{}", shown(&mut s, "GB"));
    assert!(near(rim(&s, "GB", 30.0, 20.0), turned(30.0, 20.0, 25.0)), "{:?}", rim(&s, "GB", 30.0, 20.0));
    // Undo goes back through the drive and the redefinition, redo forward again.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let (offset, reverse) = gear_link(&s);
    assert!(close_to(offset, 55.0) && reverse == Some(false), "{offset} {reverse:?}");
    assert!(close_to(shown(&mut s, "GA"), 30.0) && close_to(shown(&mut s, "GB"), 40.0), "{}", run(&mut s, "joint.list", json!({})));
    run(&mut s, "edit.redo", json!({}));
    let (offset, reverse) = gear_link(&s);
    assert!(close_to(offset, 25.0) && reverse == Some(true), "{offset} {reverse:?}");
    assert!(close_to(shown(&mut s, "GB"), 40.0), "{}", shown(&mut s, "GB"));
    run(&mut s, "edit.redo", json!({}));
    assert!(close_to(shown(&mut s, "GA"), 0.0) && close_to(shown(&mut s, "GB"), 25.0), "{}", run(&mut s, "joint.list", json!({})));
    // Saved and opened again: the same link, the same pose.
    let back = solidcraft_doc::format::read(&s.doc.to_json()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.assembly.links, s.doc.assembly.links);
    let vals = solidcraft_doc::joints::joint_values(&back.assembly);
    let gb = back.assembly.joints.iter().find(|j| j.name == "GB").map(|j| j.id).unwrap_or(0);
    assert!(vals.get(&gb).and_then(|q| q.first()).is_some_and(|v| close_to(v.to_degrees(), 25.0)), "{vals:?}");
}

/// The other couplings redefined in place keep their driven joint where it is too.
#[test]
fn a_redefined_rack_keeps_the_rack_where_it_is() {
    let mut s = rack();
    run(&mut s, "joint.rack_pinion", json!({"a": "Pinion", "b": "Rail", "radius": 8}));
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 45}));
    let travel = 8.0 * std::f64::consts::FRAC_PI_4;
    run(&mut s, "joint.rack_pinion", json!({"a": "Pinion", "b": "Rail", "radius": 4, "reverse": true, "link": 0}));
    assert!(close_to(shown(&mut s, "Rail"), travel), "{}", shown(&mut s, "Rail"));
    run(&mut s, "joint.drive", json!({"joint": "Pinion", "value": 0}));
    assert!(close_to(shown(&mut s, "Rail"), travel + 4.0 * std::f64::consts::FRAC_PI_4), "{}", shown(&mut s, "Rail"));
}

/// A joint a motion link drives adds no freedom of its own: gears on a grounded base have one
/// between them, each turning (and draggable) with it; a part hinged on the driven gear has one
/// more; a cylindrical joint whose turn drives its travel (a screw) has one, not two.
#[test]
fn a_motion_link_takes_its_driven_joints_freedom_on_a_grounded_base() {
    let mut s = Session::default();
    base(&mut s);
    disc(&mut s, "GA", 0.0, "10 mm");
    disc(&mut s, "GB", 30.0, "20 mm");
    run(&mut s, "joint.gear", json!({"a": "GA", "b": "GB", "ra": 10, "rb": 20}));
    for (part, want) in [("Base", 0), ("GA", 1), ("GB", 1)] {
        assert_eq!(dof_of(&mut s, part), want, "{part}");
    }
    part(&mut s, "Arm", [45.0, -1.0, 5.0]);
    pin(&mut s, "revolute", "Arm", "GB", "Arm", Vec3::new(46.0, 0.0, 5.0), Vec3::Z);
    assert_eq!(dof_of(&mut s, "Arm"), 2);
    // The arm's hinge driven by GA too: still the one value.
    run(&mut s, "joint.motion_link", json!({"a": "GA", "b": "Arm", "ratio": 2}));
    assert_eq!(dof_of(&mut s, "Arm"), 1);
    let mut s = Session::default();
    base(&mut s);
    part(&mut s, "Nut", [-1.0, -1.0, 0.0]);
    pin(&mut s, "cylindrical", "Thread", "Base", "Nut", Vec3::ZERO, Vec3::Z);
    assert_eq!(dof_of(&mut s, "Nut"), 2);
    run(&mut s, "joint.screw", json!({"joint": "Thread", "pitch": 1.5}));
    assert_eq!(dof_of(&mut s, "Nut"), 1);
}

/// With nothing grounded, the first joint's A side stays put. A gear there (its joint to the
/// housing made from the gear) leaves the housing one turn about it, and the driven gear on the
/// housing turns with that same value once the gears are coupled: one, not two. With the
/// housing first, each gear has one.
#[test]
fn a_motion_link_takes_its_driven_joints_freedom_with_nothing_grounded() {
    for gear_first in [true, false] {
        let mut s = Session::default();
        part(&mut s, "Housing", [-10.0, -10.0, -20.0]);
        loose_disc(&mut s, "GA", 0.0, "10 mm");
        loose_disc(&mut s, "GB", 30.0, "20 mm");
        let (a, b) = if gear_first { ("GA", "Housing") } else { ("Housing", "GA") };
        pin(&mut s, "revolute", "JA", a, b, Vec3::ZERO, Vec3::Z);
        pin(&mut s, "revolute", "JB", "Housing", "GB", Vec3::new(30.0, 0.0, 0.0), Vec3::Z);
        let (fixed, free) = if gear_first { ("GA", "Housing") } else { ("Housing", "GA") };
        assert_eq!(dof_of(&mut s, fixed), 0, "gear first: {gear_first}");
        assert_eq!(dof_of(&mut s, free), 1, "gear first: {gear_first}");
        // Before the link GB turns on JB, and with the housing about GA when GA stays put.
        assert_eq!(dof_of(&mut s, "GB"), if gear_first { 2 } else { 1 }, "gear first: {gear_first}");
        run(&mut s, "joint.gear", json!({"a": "JA", "b": "JB", "ra": 10, "rb": 20}));
        for (part, want) in [(fixed, 0), (free, 1), ("GB", 1)] {
            assert_eq!(dof_of(&mut s, part), want, "{part}, gear first: {gear_first}");
        }
    }
}
