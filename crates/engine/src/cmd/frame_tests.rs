#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use std::f64::consts::PI;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

/// The areas of the active sketch's profiles, largest first.
fn profile_areas(s: &Session) -> Vec<f64> {
    let id = s.active_sketch.unwrap();
    let mut a: Vec<f64> = s.model.state().sketch(id).unwrap().profiles.iter().map(|p| p.area).collect();
    a.sort_by(|x, y| y.total_cmp(x));
    a
}

#[test]
fn list_gives_every_kind_with_a_working_example() {
    let mut s = Session::default();
    let out = run(&mut s, "frame.profile.list", json!({}));
    let kinds = out["kinds"].as_array().unwrap();
    let ids: Vec<&str> = kinds.iter().filter_map(|k| k["kind"].as_str()).collect();
    assert_eq!(ids, ["bar", "round_bar", "rect_tube", "round_tube", "angle", "channel", "i_beam"]);
    for k in kinds {
        let r = run(&mut s, "frame.profile.properties", json!({"profile": k["example"]}));
        assert!(f(&r["area"]) > 0.0, "{}", k["kind"]);
    }
}

#[test]
fn properties_fill_defaults_and_give_mass_per_metre() {
    let mut s = Session::default();
    let r = run(
        &mut s,
        "frame.profile.properties",
        json!({"profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 0}, "density": 7850}),
    );
    let area = 40.0 * 40.0 - 34.0 * 34.0;
    assert!(rel(f(&r["area"]), area) < 1e-12, "{r}");
    assert!(rel(f(&r["mass_per_m"]), area * 1e-6 * 7850.0) < 1e-12, "{r}");
    assert!(rel(f(&r["ixx"]), (40f64.powi(4) - 34f64.powi(4)) / 12.0) < 1e-12, "{r}");
    let r = run(&mut s, "frame.profile.properties", json!({"profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}));
    assert_eq!(r["profile"]["outer_radius"], json!(6.0), "{r}");
    assert!(r.get("mass_per_m").is_none(), "{r}");
}

#[test]
fn bad_profiles_are_refused_with_the_reason() {
    let mut s = new_sketch();
    for (p, why) in [
        (json!({}), "`profile` is required"),
        (json!({"profile": {"kind": "hexagon"}}), "`profile`"),
        (json!({"profile": {"kind": "rect_tube", "width": 10, "height": 10, "wall": 5}}), "wall"),
        (json!({"profile": {"kind": "round_bar", "diameter": -1}}), "diameter"),
        (json!({"profile": {"kind": "round_bar", "diameter": 10}, "density": -1}), "density"),
        (json!({"profile": {"kind": "round_bar", "diameter": 10}, "density": "steel"}), "density"),
    ] {
        let err = s.execute("frame.profile.properties", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
    }
    for p in [
        json!({"profile": {"kind": "round_bar", "diameter": 10}, "at": [1e300, 0]}),
        json!({"profile": {"kind": "round_bar", "diameter": 10}, "at": "here"}),
        json!({"profile": {"kind": "round_bar", "diameter": 10}, "anchor": "middle"}),
        json!({"profile": {"kind": "round_bar", "diameter": 10}, "angle": "a lot"}),
        json!({"profile": {"kind": "round_bar", "diameter": 10}, "fixed": 3}),
    ] {
        assert!(s.execute("frame.profile.insert", &p).is_err(), "{p} is refused");
    }
    // Nothing was drawn.
    assert!(profile_areas(&s).is_empty());
}

#[test]
fn insert_needs_a_sketch() {
    let mut s = Session::default();
    assert!(s.execute("frame.profile.insert", &json!({"profile": {"kind": "round_bar", "diameter": 10}})).is_err());
}

#[test]
fn inserted_tube_is_one_profile_with_its_hole() {
    let mut s = new_sketch();
    let r = run(&mut s, "frame.profile.insert", json!({"profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}));
    let area = f(&r["section"]["area"]);
    // Outer radius 6, inner radius 3.
    let want = (1600.0 - (4.0 - PI) * 36.0) - (34.0 * 34.0 - (4.0 - PI) * 9.0);
    assert!(rel(area, want) < 1e-12, "{r}");
    // Four lines and four arcs outside, the same inside.
    assert_eq!(r["created"]["curves"].as_array().unwrap().len(), 16, "{r}");
    // The sketch has the wall and, as a region of its own, the inside.
    let areas = profile_areas(&s);
    assert_eq!(areas.len(), 2, "{areas:?}");
    assert!(areas.iter().any(|a| rel(*a, want) < 1e-9), "the wall is the tube's section: {areas:?}");
}

#[test]
fn inserted_channel_extrudes_to_its_area_times_length() {
    let mut s = new_sketch();
    let r = run(
        &mut s,
        "frame.profile.insert",
        json!({"profile": {"kind": "channel", "height": 60, "width": 45, "web": 5, "flange": 6}, "at": [100, 50], "anchor": "bottom_left"}),
    );
    let area = f(&r["section"]["area"]);
    let areas = profile_areas(&s);
    assert!(areas.iter().any(|a| rel(*a, area) < 1e-9), "{areas:?} vs {area}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 500, "profiles": [{"point": [101.5, 51.5]}]}));
    let m = run(&mut s, "inspect.measure", json!({}));
    let v = f(&m["total"]["volume_mm3"]);
    // The kernel measures arcs through their facets: within 1e-4.
    assert!(rel(v, area * 500.0) < 1e-4, "volume {v} vs {}", area * 500.0);
    // The bottom-left corner sits on (100, 50): the body spans 100…145 and 50…110.
    let bb = &m["bodies"][0]["bbox"];
    let near = |v: &Value, want: f64| (f(v) - want).abs() < 1e-6;
    assert!(near(&bb["min"][0], 100.0) && near(&bb["min"][1], 50.0) && near(&bb["max"][0], 145.0) && near(&bb["max"][1], 110.0), "{bb}");
}

#[test]
fn inserted_profile_turns_about_its_anchor_and_stays_fixed() {
    let mut s = new_sketch();
    let r = run(
        &mut s,
        "frame.profile.insert",
        json!({"profile": {"kind": "angle", "width": 60, "height": 40, "thickness": 5}, "anchor": "bottom_left", "angle": PI / 2.0}),
    );
    let id = s.active_sketch.unwrap();
    let sk = s.model.state().sketch(id).unwrap().sketch.clone();
    let created: Vec<&str> = r["created"]["curves"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(!created.is_empty());
    for c in sk.curves.iter().filter(|c| created.contains(&c.id.as_str())) {
        assert!(c.fixed, "{} is locked", c.id);
    }
    // Turned a quarter about the heel at the origin: the 60 mm leg runs up the y axis, the 40 mm
    // one along −x.
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &sk.points {
        lo = [lo[0].min(p.pos.x), lo[1].min(p.pos.y)];
        hi = [hi[0].max(p.pos.x), hi[1].max(p.pos.y)];
    }
    assert!((lo[0] + 40.0).abs() < 1e-9 && hi[0].abs() < 1e-9 && lo[1].abs() < 1e-9 && (hi[1] - 60.0).abs() < 1e-9, "{lo:?} {hi:?}");
    // Unlocked on request.
    let r =
        run(&mut s, "frame.profile.insert", json!({"profile": {"kind": "round_tube", "diameter": 20, "wall": 2}, "at": [200, 0], "fixed": false}));
    assert_eq!(r["created"]["curves"].as_array().unwrap().len(), 2, "two circles: {r}");
    let sk = s.model.state().sketch(id).unwrap().sketch.clone();
    assert!(sk.curves.iter().filter(|c| r["created"]["curves"].as_array().unwrap().contains(&json!(c.id))).all(|c| !c.fixed));
}

#[test]
fn insert_is_one_undo_step() {
    let mut s = new_sketch();
    run(&mut s, "frame.profile.insert", json!({"profile": {"kind": "i_beam", "height": 100, "width": 50, "web": 4.5, "flange": 7}}));
    assert_eq!(profile_areas(&s).len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(profile_areas(&s).is_empty());
}

// ---------------------------------------------------------------------------------------------
// Members

/// How close a measured volume comes: the kernel measures curved faces through their facets.
const MEASURED: f64 = 1e-3;

/// The section area of a 40 × 40 × 3 rectangular tube (outer radius 6, inner 3).
fn tube_area() -> f64 {
    (1600.0 - (4.0 - PI) * 36.0) - (34.0 * 34.0 - (4.0 - PI) * 9.0)
}

fn tube() -> Value {
    json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3})
}

/// A body's name, volume, bounding box and centre of mass.
type Measured = (String, f64, [f64; 3], [f64; 3], [f64; 3]);

fn bodies(s: &mut Session) -> Vec<Measured> {
    let m = run(s, "inspect.measure", json!({}));
    let v3 = |v: &Value| [f(&v[0]), f(&v[1]), f(&v[2])];
    m["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (b["name"].as_str().unwrap().to_string(), f(&b["volume_mm3"]), v3(&b["bbox"]["min"]), v3(&b["bbox"]["max"]), v3(&b["center_of_mass"]))
        })
        .collect()
}

fn total_volume(s: &mut Session) -> f64 {
    bodies(s).iter().map(|b| b.1).sum()
}

fn near3(got: [f64; 3], want: [f64; 3]) -> bool {
    got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6)
}

/// A 600 × 400 rectangle on XY, as the frame sketch `Frame`.
fn rectangle_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(s, "sketch.finish", json!({}));
}

#[test]
fn members_follow_every_line_of_a_rectangle() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let r = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 4, "{r}");
    let names: Vec<&str> = bs.iter().map(|b| b.0.as_str()).collect();
    assert_eq!(names, ["Rect Tube 40x40x3", "Rect Tube 40x40x3 (2)", "Rect Tube 40x40x3 (3)", "Rect Tube 40x40x3 (4)"]);
    // Each member is the section run the length of its line: 2 × 600 + 2 × 400.
    let v = total_volume(&mut s);
    assert!(rel(v, tube_area() * 2000.0) < MEASURED, "{v} vs {}", tube_area() * 2000.0);
    // The bottom line runs along +X: the profile is centred on it, square to it.
    assert!(bs.iter().any(|b| near3(b.2, [0.0, -20.0, -20.0]) && near3(b.3, [600.0, 20.0, 20.0])), "{bs:?}");
    // The right line runs along Y at x = 600.
    assert!(bs.iter().any(|b| near3(b.2, [580.0, 0.0, -20.0]) && near3(b.3, [620.0, 400.0, 20.0])), "{bs:?}");
}

#[test]
fn members_follow_a_parameter_edit() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "600 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], ["W", 0], ["W", 400], [0, 400]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    assert!(rel(total_volume(&mut s), tube_area() * 2000.0) < MEASURED);
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "800 mm"}));
    // The two 600 mm members are 800 mm now.
    let v = total_volume(&mut s);
    assert!(rel(v, tube_area() * 2400.0) < MEASURED, "{v} vs {}", tube_area() * 2400.0);
}

#[test]
fn members_along_3d_sketch_lines() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tower"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1500], [600, 0, 1500]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Tower", "profile": tube()}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    // The upright: square to Z, the profile's x along world X.
    assert!(bs.iter().any(|b| near3(b.2, [-20.0, -20.0, 0.0]) && near3(b.3, [20.0, 20.0, 1500.0])), "{bs:?}");
    // The rail along X at the top.
    assert!(bs.iter().any(|b| near3(b.2, [0.0, -20.0, 1480.0]) && near3(b.3, [600.0, 20.0, 1520.0])), "{bs:?}");
    let v = total_volume(&mut s);
    assert!(rel(v, tube_area() * 2100.0) < MEASURED, "{v} vs {} {bs:?}", tube_area() * 2100.0);
}

#[test]
fn a_profile_sketch_is_the_section() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tube section"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 17}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Post"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    // The material: the ring between the circles (nested loops alternate).
    run(&mut s, "frame.member", json!({"sketch": "Post", "profile_sketch": "Tube section"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 1);
    assert_eq!(bs[0].0, "Tube section");
    let ring = PI * (400.0 - 289.0) * 1000.0;
    assert!(rel(bs[0].1, ring) < MEASURED, "{} vs {ring}", bs[0].1);
    // Picked: the inner disc alone.
    run(&mut s, "frame.member", json!({"sketch": "Post", "profile_sketch": "Tube section", "profiles": [{"point": [0, 0]}]}));
    // A second feature of the same profile numbers on from the first.
    let disc = bodies(&mut s).into_iter().find(|b| b.0 != "Tube section").unwrap();
    assert_eq!(disc.0, "Tube section (2)");
    assert!(rel(disc.1, PI * 289.0 * 1000.0) < MEASURED, "{disc:?}");
}

#[test]
fn anchor_angle_and_mirror_place_the_profile() {
    let place = |extra: Value| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch": "Rail", "profile": {"kind": "angle", "width": 60, "height": 40, "thickness": 5}, "anchor": "bottom_left"});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        run(&mut s, "frame.member", p);
        bodies(&mut s).remove(0)
    };
    // Along +X the profile's x is world +Y and its y world +Z: the heel on the line, the legs
    // running +Y (60) and +Z (40).
    let plain = place(json!({}));
    assert!(near3(plain.2, [0.0, 0.0, 0.0]) && near3(plain.3, [1000.0, 60.0, 40.0]), "{plain:?}");
    // Turned a quarter: the 60 mm leg up, the 40 mm one toward −Y.
    let b = place(json!({"angle": "90 deg"}));
    assert!(near3(b.2, [0.0, -40.0, 0.0]) && near3(b.3, [1000.0, 0.0, 60.0]), "{b:?}");
    // Mirrored: the same box, the heel at the far side (the mass sits past the middle).
    let m = place(json!({"mirror": true}));
    assert!(near3(m.2, [0.0, 0.0, 0.0]) && near3(m.3, [1000.0, 60.0, 40.0]), "{m:?}");
    assert!(plain.4[1] < 30.0 && m.4[1] > 30.0, "{plain:?} {m:?}");
}

#[test]
fn bad_members_are_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Arc"}));
    let arc = run(&mut s, "sketch.arc.three_point", json!({"start": [0, 0], "end": [100, 0], "through": [50, 30]}));
    let arc_id = arc["created"]["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Empty"}));
    run(&mut s, "sketch.finish", json!({}));
    for (p, why) in [
        (json!({"sketch": "Frame"}), "`profile`"),
        (json!({"sketch": "Frame", "profile": tube(), "profile_sketch": "Frame"}), "one of them"),
        (json!({"sketch": "Frame", "profile_sketch": "Frame"}), "must not be the frame sketch"),
        (json!({"sketch": "Nowhere", "profile": tube()}), "no sketch"),
        (json!({"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 10, "height": 10, "wall": 5}}), "wall"),
        (json!({"sketch": "Frame", "profile": tube(), "lines": []}), "`lines`"),
        (json!({"sketch": "Frame", "profile": tube(), "lines": ["nope"]}), "nope"),
        (json!({"sketch": "Arc", "profile": tube(), "lines": [arc_id]}), "not a straight line"),
        (json!({"sketch": "Arc", "profile": tube()}), "no lines"),
        (json!({"sketch": "Empty", "profile": tube()}), "no lines"),
        (json!({"sketch": "Frame", "profile": tube(), "anchor": "middle"}), "`anchor`"),
        (json!({"sketch": "Frame", "profile": tube(), "angle": "a lot"}), "angle"),
        (json!({"sketch": "Frame", "profile": tube(), "mirror": 3}), "`mirror`"),
        (json!({"sketch": "Frame", "profile": tube(), "names": ["Rail"]}), "`names`"),
        (json!({"sketch": "Frame", "profile": tube(), "names": {"l1": " "}}), "`names`"),
    ] {
        let err = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
    }
    assert!(bodies(&mut s).is_empty(), "nothing was made");
    // Not while a sketch is open.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube()})).is_err());
}

#[test]
fn members_undo_and_survive_save_and_reopen() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "anchor": "bottom", "angle": "15 deg", "mirror": true, "body_name": "Rail"}),
    );
    let before = bodies(&mut s);
    assert_eq!(before.len(), 4);
    assert_eq!(before[0].0, "Rail");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let after = bodies(&mut reopened);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(b.0, a.0);
        assert!(rel(a.1, b.1) < 1e-9 && near3(a.2, b.2) && near3(a.3, b.3), "{b:?} vs {a:?}");
    }
    run(&mut s, "edit.undo", json!({}));
    assert!(bodies(&mut s).is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(bodies(&mut s).len(), 4);
}

// ---------------------------------------------------------------------------------------------
// Regressions from the adversarial review of #4003

/// The lines of sketch `name`, as (id, start, end) in sketch coordinates.
fn sketch_lines(s: &Session, name: &str) -> Vec<(String, [f64; 2], [f64; 2])> {
    let id = s.doc.find_feature(name).unwrap().id;
    let sk = s.doc.sketch(id).unwrap();
    sk.curves
        .iter()
        .filter_map(|c| match c.kind {
            solidcraft_sketch::CurveKind::Line { a, b } => {
                let (p, q) = (sk.points[a].pos, sk.points[b].pos);
                Some((c.id.clone(), [p.x, p.y], [q.x, q.y]))
            }
            _ => None,
        })
        .collect()
}

/// The id of the line of sketch `name` with both ends at x = `x`.
fn line_at_x(s: &Session, name: &str, x: f64) -> String {
    sketch_lines(s, name).into_iter().find(|(_, a, b)| (a[0] - x).abs() < 1e-9 && (b[0] - x).abs() < 1e-9).unwrap().0
}

/// The id of the line of sketch `name` with both ends at y = `y`.
fn line_at_y(s: &Session, name: &str, y: f64) -> String {
    sketch_lines(s, name).into_iter().find(|(_, a, b)| (a[1] - y).abs() < 1e-9 && (b[1] - y).abs() < 1e-9).unwrap().0
}

fn delete_line(s: &mut Session, sketch: &str, line: &str) {
    run(s, "sketch.edit", json!({"sketch": sketch}));
    run(s, "sketch.delete", json!({"entities": [line]}));
    run(s, "sketch.finish", json!({}));
}

/// Each body's bounding box by name.
fn boxes(s: &mut Session) -> std::collections::BTreeMap<String, ([f64; 3], [f64; 3])> {
    bodies(s).into_iter().map(|b| (b.0, (b.2, b.3))).collect()
}

#[test]
fn projected_edges_are_not_member_lines() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "body_name": "Plate"}));
    // A sketch on the plate's top face projects its edges; the one line drawn is the only member.
    run(&mut s, "sketch.create", json!({"plane": {"face": [50, 50, 10]}, "name": "On plate"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [90, 10]]}));
    run(&mut s, "sketch.finish", json!({}));
    // The plate's four edges are in the sketch, projected.
    assert_eq!(sketch_lines(&s, "On plate").len(), 5);
    run(&mut s, "frame.member", json!({"sketch": "On plate", "profile": tube()}));
    let members: Vec<Measured> = bodies(&mut s).into_iter().filter(|b| b.0 != "Plate").collect();
    assert_eq!(members.len(), 1, "{members:?}");
    assert!(rel(members[0].1, tube_area() * 80.0) < MEASURED, "{members:?}");
}

#[test]
fn a_profile_sketch_on_a_face_is_its_drawn_section() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 100, "width": 100, "height": 10, "body_name": "Plate"}));
    run(&mut s, "sketch.create", json!({"plane": {"face": [50, 50, 10]}, "name": "Section on plate"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 17}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Post"}));
    run(&mut s, "sketch.line", json!({"points": [[200, 0], [200, 1000]]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(sketch_lines(&s, "Section on plate").len(), 4, "the plate's edges, projected");
    run(&mut s, "frame.member", json!({"sketch": "Post", "profile_sketch": "Section on plate"}));
    let member = bodies(&mut s).into_iter().find(|b| b.0 != "Plate").unwrap();
    assert!(rel(member.1, PI * (400.0 - 289.0) * 1000.0) < MEASURED, "{member:?}");
}

#[test]
fn members_keep_their_names_when_a_line_is_deleted() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    let before = boxes(&mut s);
    assert_eq!(before.len(), 4);
    let right = line_at_x(&s, "Frame", 600.0);
    delete_line(&mut s, "Frame", &right);
    let after = boxes(&mut s);
    assert_eq!(after.len(), 3, "{after:?}");
    // Each member left is still called what it was, and is where it was.
    for (name, bb) in &after {
        assert_eq!(before.get(name), Some(bb), "{name} moved: {before:?} → {after:?}");
    }
}

#[test]
fn a_renamed_member_keeps_its_name_through_line_edits() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    let top = boxes(&mut s).into_iter().find(|(_, (lo, _))| (lo[1] - 380.0).abs() < 1e-6).unwrap();
    run(&mut s, "body.rename", json!({"body": top.0, "name": "Top rail"}));
    let before = boxes(&mut s);
    assert_eq!(before.get("Top rail"), Some(&top.1));
    let left = line_at_x(&s, "Frame", 0.0);
    delete_line(&mut s, "Frame", &left);
    let after = boxes(&mut s);
    assert_eq!(after.len(), 3);
    for (name, bb) in &after {
        assert_eq!(before.get(name), Some(bb), "{name}: {before:?} → {after:?}");
    }
    // It survives saving and reopening.
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(boxes(&mut reopened), after);
}

#[test]
fn a_numbered_member_name_is_kept_when_the_first_feature_goes() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom, top) = (line_at_y(&s, "Frame", 0.0), line_at_y(&s, "Frame", 400.0));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "lines": [bottom], "profile": tube(), "name": "First"}));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "lines": [top], "profile": tube(), "name": "Second"}));
    assert_eq!(boxes(&mut s).keys().collect::<Vec<_>>(), ["Rect Tube 40x40x3", "Rect Tube 40x40x3 (2)"]);
    run(&mut s, "timeline.delete", json!({"feature": "First"}));
    assert_eq!(boxes(&mut s).keys().collect::<Vec<_>>(), ["Rect Tube 40x40x3 (2)"]);
}

#[test]
fn an_asymmetric_profile_turns_with_its_3d_line() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tower"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1500], [600, 0, 1500]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Tower", "profile": {"kind": "bar", "width": 60, "height": 10}}));
    let bs = bodies(&mut s);
    // The post: the bar's width (its x) along world X, its height along Y.
    assert!(bs.iter().any(|b| near3(b.2, [-30.0, -5.0, 0.0]) && near3(b.3, [30.0, 5.0, 1500.0])), "{bs:?}");
    // The rail along +X: width along +Y, height up.
    assert!(bs.iter().any(|b| near3(b.2, [0.0, -30.0, 1495.0]) && near3(b.3, [600.0, 30.0, 1505.0])), "{bs:?}");
}

#[test]
fn a_post_a_hair_off_vertical_is_oriented_as_a_vertical_one() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Posts"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [1e-5, 0, 1000]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Posts", "profile": {"kind": "bar", "width": 60, "height": 10}}));
    let b = bodies(&mut s).remove(0);
    // Width along X as for a true vertical, not along Y.
    assert!((b.3[0] - b.2[0] - 60.0).abs() < 1e-3 && (b.3[1] - b.2[1] - 10.0).abs() < 1e-3, "{b:?}");
}

#[test]
fn thick_sections_build_with_their_default_radii() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [500, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    for profile in [
        json!({"kind": "angle", "width": 20, "height": 20, "thickness": 10}),
        json!({"kind": "channel", "height": 50, "width": 10, "web": 6, "flange": 2}),
        json!({"kind": "channel", "height": 20, "width": 40, "web": 5, "flange": 8}),
        json!({"kind": "i_beam", "height": 100, "width": 10, "web": 6, "flange": 5}),
    ] {
        run(&mut s, "frame.member", json!({"sketch": "Rail", "profile": profile}));
    }
    assert_eq!(bodies(&mut s).len(), 4);
}

#[test]
fn profile_dimensions_follow_parameters() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "T", "expression": "3 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let square = |t: f64| (1600.0 - (40.0 - 2.0 * t).powi(2)) * 1000.0;
    let sharp = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": "T", "outer_radius": 0});
    let props = run(&mut s, "frame.profile.properties", json!({"profile": sharp}));
    assert!(rel(f(&props["area"]), square(3.0) / 1000.0) < 1e-12, "{props}");
    run(&mut s, "frame.member", json!({"sketch": "Rail", "profile": sharp}));
    assert!(rel(total_volume(&mut s), square(3.0)) < 1e-9);
    run(&mut s, "parameters.change", json!({"name": "T", "expression": "2 mm"}));
    assert!(rel(total_volume(&mut s), square(2.0)) < 1e-9, "{}", total_volume(&mut s));
    // A parameter that makes the profile not fit fails the feature, naming the dimension.
    let err = s.execute("parameters.change", &json!({"name": "T", "expression": "25 mm"})).err().map(|e| e.to_string()).unwrap_or_default();
    let fid = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let failing = s.model.result(fid).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(err.contains("wall") || failing.contains("wall"), "{err} / {failing}");
}

#[test]
fn bare_profile_numbers_are_in_the_design_units() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Rail", "profile": {"kind": "bar", "width": 2, "height": 1}}));
    let b = bodies(&mut s).remove(0);
    // 2 in across (world Y) and 1 in up (world Z).
    assert!((b.3[1] - b.2[1] - 50.8).abs() < 1e-6 && (b.3[2] - b.2[2] - 25.4).abs() < 1e-6, "{b:?}");
    let props = run(&mut s, "frame.profile.properties", json!({"profile": {"kind": "round_bar", "diameter": 1}}));
    assert!((f(&props["width"]) - 25.4).abs() < 1e-9, "{props}");
}

#[test]
fn members_follow_a_frame_sketch_in_another_component() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let c = run(&mut s, "component.create", json!({"name": "Frame parts", "grounded": false}))["component"].as_u64().unwrap();
    let o = s.doc.occurrence_of(c).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": o, "translate": [0, 0, 100], "capture": true}));
    // Made in the moved component, the members still lie on the root's frame sketch.
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 4);
    for b in &bs {
        assert!((b.2[2] + 20.0).abs() < 1e-6 && (b.3[2] - 20.0).abs() < 1e-6, "{b:?}");
    }
}

#[test]
fn listing_a_line_twice_is_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let bottom = line_at_y(&s, "Frame", 0.0);
    let err = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "lines": [bottom, bottom]})).unwrap_err().to_string();
    assert!(err.contains("listed twice"), "{err}");
}

/// Edit Feature keeps each member's name, a renamed one too, through a change of profile; giving
/// the members a name renames them all.
#[test]
fn editing_the_feature_keeps_member_names_until_it_names_them() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}))["feature"].clone();
    let top = boxes(&mut s).into_iter().find(|(_, (lo, _))| (lo[1] - 380.0).abs() < 1e-6).unwrap().0;
    run(&mut s, "body.rename", json!({"body": top, "name": "Top rail"}));
    let names: Vec<String> = boxes(&mut s).into_keys().collect();
    let bigger = json!({"kind": "rect_tube", "width": 50, "height": 50, "wall": 3});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": {"sketch": "Frame", "profile": bigger}}));
    let after = boxes(&mut s);
    assert_eq!(after.keys().cloned().collect::<Vec<_>>(), names);
    // The top rail is the 50 mm tube now, centred on the line at y = 400.
    assert!(near3(after["Top rail"].0, [0.0, 375.0, -25.0]) && near3(after["Top rail"].1, [600.0, 425.0, 25.0]), "{after:?}");
    let params = json!({"sketch": "Frame", "profile": bigger, "body_name": "Rail"});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    assert_eq!(boxes(&mut s).into_keys().collect::<Vec<_>>(), ["Rail", "Rail (2)", "Rail (3)", "Rail (4)"]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(boxes(&mut s).into_keys().collect::<Vec<_>>(), names);
}

// Mounting and bolted end plates (#7485).
fn plate_post() -> Session {
    plate_post_in(Session::default())
}

fn plate_post_in(mut s: Session) -> Session {
    run(&mut s, "parameters.add", json!({"name":"PostHeight","expression":"300 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XZ","name":"PostPath"}));
    run(&mut s, "sketch.line", json!({"points":[[200,0],[200,"-PostHeight"]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch":"PostPath","profile":{"kind":"rect_tube","width":40,"height":40,"wall":3,"outer_radius":0},"body_name":"Post","name":"MemberFeature"}),
    );
    s
}

/// A point on the material of the cap, including a hollow cap whose centroid is in its bore.
fn plate_pick(s: &Session, body: &str, high: bool) -> Value {
    let state = s.world_state();
    let body = state.body(body).unwrap();
    let face =
        body.body.faces(0.001).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.z * if high { 1. } else { -1. } > 0.999)).unwrap();
    let mesh = body.mesh();
    let (triangle, _) = mesh.triangles.iter().zip(&mesh.tri_face).find(|(_, index)| **index as usize == face.index).unwrap();
    let [a, b, c] = mesh.tri(triangle).unwrap();
    json!({"face":(a+b+c)/3.,"body":body.name,"index":face.index})
}

fn rectangular_plate(s: &Session, high: bool) -> Value {
    let mut p = plate_pick(s, "Post", high);
    p["width"] = json!(100);
    p["height"] = json!(80);
    p["thickness"] = json!(8);
    p["body_name"] = json!(if high { "EndPlate" } else { "BasePlate" });
    p
}

#[test]
fn mounting_plates_are_centred_on_hollow_member_ends_and_leave_the_member_alone() {
    let mut s = plate_post();
    let before = bodies(&mut s)[0].clone();
    let mut p = rectangular_plate(&s, true);
    p["holes"] = json!({"pattern":"rectangle","diameter":10,"pitch_x":70,"pitch_y":50});
    run(&mut s, "frame.plate", p);
    let p = rectangular_plate(&s, false);
    run(&mut s, "frame.plate", p);
    let bs = bodies(&mut s);
    let post = bs.iter().find(|b| b.0 == "Post").unwrap();
    assert_eq!(post, &before, "plate holes never cut the supporting member");
    let top = bs.iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!(near3(top.2, [150., -40., 300.]) && near3(top.3, [250., 40., 308.]), "{top:?}");
    assert!(rel(top.1, (100. * 80. - 4. * PI * 25.) * 8.) < MEASURED, "{top:?}");
    let bottom = bs.iter().find(|b| b.0 == "BasePlate").unwrap();
    assert!(near3(bottom.2, [150., -40., -8.]) && near3(bottom.3, [250., 40., 0.]), "{bottom:?}");
}

#[test]
fn round_bolted_end_plate_has_the_analytic_volume() {
    let mut s = plate_post();
    let mut p = plate_pick(&s, "Post", true);
    p.as_object_mut().unwrap().extend(json!({"shape":"round","diameter":120,"thickness":8,"body_name":"Flange","holes":{"pattern":"circle","diameter":10,"pitch_diameter":90,"count":6,"angle":"15 deg"}}).as_object().unwrap().clone());
    run(&mut s, "frame.plate", p);
    let flange = bodies(&mut s).into_iter().find(|b| b.0 == "Flange").unwrap();
    assert!(near3(flange.2, [140., -60., 300.]) && near3(flange.3, [260., 60., 308.]), "{flange:?}");
    assert!(rel(flange.1, PI * (3600. - 6. * 25.) * 8.) < MEASURED, "{flange:?}");
}

#[test]
fn plates_follow_frame_edits_and_renames_and_survive_undo_and_reopen() {
    let mut s = plate_post();
    let p = rectangular_plate(&s, true);
    run(&mut s, "frame.plate", p);
    run(&mut s, "parameters.change", json!({"name":"PostHeight","expression":"450 mm"}));
    run(&mut s, "body.rename", json!({"body":"Post","name":"Upright"}));
    let plate = bodies(&mut s).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!(near3(plate.2, [150., -40., 450.]) && near3(plate.3, [250., 40., 458.]), "{plate:?}");
    let json = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&json).unwrap());
    let after = bodies(&mut reopened).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert_eq!(after.0, plate.0);
    assert!(rel(after.1, plate.1) < 1e-9 && near3(after.2, plate.2) && near3(after.3, plate.3) && near3(after.4, plate.4));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let old = bodies(&mut s).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!((old.2[2] - 300.).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    assert!(bodies(&mut s).iter().any(|b| b.0 == "EndPlate" && (b.2[2] - 450.).abs() < 1e-6));
}

#[test]
fn plate_offsets_and_rotation_are_in_the_support_face_frame() {
    let mut s = plate_post();
    let mut p = rectangular_plate(&s, true);
    p["offset"] = json!(2);
    p["x_offset"] = json!(10);
    p["y_offset"] = json!(20);
    p["angle"] = json!("90 deg");
    run(&mut s, "frame.plate", p);
    let plate = bodies(&mut s).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    // The selected face's canonical frame is X/Y; offsets apply before the plate turns.
    assert!(near3(plate.2, [170., -30., 302.]) && near3(plate.3, [250., 70., 310.]), "{plate:?}");
}

#[test]
fn invalid_plates_and_hole_patterns_leave_the_design_unchanged() {
    let mut s = plate_post();
    let valid = rectangular_plate(&s, true);
    for patch in [
        json!({"thickness":0}),
        json!({"width":-1}),
        json!({"angle":1e20}),
        json!({"offset":1e20}),
        json!({"diameter":100}),
        json!({"shape":"round"}),
        json!({"index":99999}),
        json!({"path":[999]}),
        json!({"holes":{"pattern":"circle","diameter":10,"pitch_diameter":50,"count":129}}),
        json!({"holes":{"pattern":"circle","diameter":10,"pitch_diameter":50,"count":0}}),
        json!({"holes":{"pattern":"rectangle","diameter":10,"pitch_x":90,"pitch_y":50}}),
        json!({"holes":{"pattern":"rectangle","diameter":10,"pitch_x":5,"pitch_y":50}}),
        json!({"holes":{"pattern":"rectangle","diameter":10,"pitch_x":70,"pitch_y":50,"typo":3}}),
    ] {
        let mut p = valid.clone();
        p.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
        let before = s.doc.to_json();
        assert!(s.execute("frame.plate", &p).is_err(), "refuse {p}");
        assert_eq!(s.doc.to_json(), before, "failed plate is atomic: {p}");
    }
}

#[test]
fn losing_the_support_face_never_attaches_a_plate_to_a_nearby_body() {
    let mut s = plate_post();
    let p = rectangular_plate(&s, true);
    let id = run(&mut s, "frame.plate", p)["feature"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"corner":[180,-20,0],"length":40,"width":40,"height":300,"body_name":"Replacement"}));
    run(&mut s, "timeline.delete", json!({"features":["MemberFeature"]}));
    assert!(s.model.result(id).unwrap().error.is_some());
    assert!(s.model.state().body("EndPlate").is_none());
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.state().body("EndPlate").is_none());
}

#[test]
fn plates_in_a_moved_component_keep_their_attachment_and_copy_independently() {
    let mut s = Session::default();
    let comp = run(&mut s, "component.create", json!({"name":"Bracket","grounded":false}))["component"].as_u64().unwrap();
    s = plate_post_in(s);
    let occurrence = s.doc.occurrence_of(comp).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[100,200,300],"capture":true}));
    let mut p = plate_pick(&s, "Post", true);
    p.as_object_mut().unwrap().extend(json!({"width":100,"height":80,"thickness":8,"body_name":"Pad"}).as_object().unwrap().clone());
    run(&mut s, "frame.plate", p);
    let pad = bodies(&mut s).into_iter().find(|b| b.0 == "Pad").unwrap();
    assert!(near3(pad.2, [250., 160., 600.]) && near3(pad.3, [350., 240., 608.]), "{pad:?}");
    run(&mut s, "component.activate", json!({"component":"root"}));
    let copied = run(&mut s, "component.paste_new", json!({"component":comp,"translate":[500,0,0]}));
    let state = s.model.state();
    let copy = state.body("Pad (2)").unwrap();
    let original = state.body("Pad").unwrap();
    assert_ne!(copy.feature, original.feature);
    let feature = s.doc.feature(copy.feature).unwrap();
    let solidcraft_doc::FeatureKind::FramePlate { plane, member, .. } = &feature.kind else { panic!("copied plate") };
    let anchor = solidcraft_doc::face_anchor::plane_face_anchor(plane).unwrap();
    assert_eq!(anchor.target.body, "Post (2)");
    assert_eq!(anchor.component, copied["component"].as_u64().unwrap());
    assert_eq!(member.as_ref().unwrap().feature.0, anchor.target.origin_feature);
    assert!(matches!(s.doc.feature(anchor.target.origin_feature).unwrap().kind, solidcraft_doc::FeatureKind::FrameMember { .. }));
    let width = s.doc.feature(original.feature).unwrap().param_names[0].clone();
    run(&mut s, "parameters.change", json!({"name":width,"expression":"140 mm"}));
    let state = s.model.state();
    let original = state.body("Pad").unwrap().mesh().bounds();
    let copied_bounds = state.body("Pad (2)").unwrap().mesh().bounds();
    assert!((original.max.x - original.min.x - 140.).abs() < 1e-6);
    assert!((copied_bounds.max.x - copied_bounds.min.x - 100.).abs() < 1e-6);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.state().body("Pad (2)").is_some());
}

#[test]
fn a_root_plate_follows_later_rotation_and_translation_of_its_support_occurrence() {
    let mut s = Session::default();
    let comp = run(&mut s, "component.create", json!({"name":"PostPart","grounded":false}))["component"].as_u64().unwrap();
    s = plate_post_in(s);
    let occurrence = s.doc.occurrence_of(comp).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[100,200,300],"capture":true}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    let p = rectangular_plate(&s, true);
    run(&mut s, "frame.plate", p);
    run(
        &mut s,
        "occurrence.move",
        json!({"occurrence":occurrence,"space":"world","axis":[0,0,1],"angle":"90 deg","translate":[10,20,30],"capture":true}),
    );
    let plate = bodies(&mut s).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!(near3(plate.2, [-230., 270., 630.]) && near3(plate.3, [-150., 370., 638.]), "{plate:?}");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let after = bodies(&mut reopened).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!(near3(after.2, plate.2) && near3(after.3, plate.3));
}

#[test]
fn plate_dimensions_are_parameter_expressions_and_bare_lengths_use_design_units() {
    let mut s = plate_post();
    run(&mut s, "parameters.add", json!({"name":"PlateWidth","expression":"100 mm"}));
    let mut p = rectangular_plate(&s, true);
    p["width"] = json!("PlateWidth");
    run(&mut s, "frame.plate", p);
    run(&mut s, "parameters.change", json!({"name":"PlateWidth","expression":"140 mm"}));
    let plate = bodies(&mut s).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!(near3(plate.2, [130., -40., 300.]) && near3(plate.3, [270., 40., 308.]), "{plate:?}");
    let mut s = plate_post();
    s.doc_mut().units = "cm".into();
    let mut p = rectangular_plate(&s, true);
    p["width"] = json!(10);
    p["height"] = json!(8);
    p["thickness"] = json!(0.8);
    run(&mut s, "frame.plate", p);
    let plate = bodies(&mut s).into_iter().find(|b| b.0 == "EndPlate").unwrap();
    assert!(near3(plate.2, [150., -40., 300.]) && near3(plate.3, [250., 40., 308.]), "{plate:?}");
}

#[test]
fn a_reused_name_in_the_same_frame_feature_does_not_steal_a_plate() {
    let mut s = plate_post();
    let source = s.doc.find_feature("MemberFeature").unwrap().id;
    let old = sketch_lines(&s, "PostPath")[0].0.clone();
    let p = rectangular_plate(&s, true);
    let plate = run(&mut s, "frame.plate", p)["feature"].as_u64().unwrap();
    run(&mut s, "sketch.edit", json!({"sketch":"PostPath"}));
    run(&mut s, "sketch.line", json!({"points":[[400,0],[400,"-PostHeight"]]}));
    run(&mut s, "sketch.finish", json!({}));
    let new = sketch_lines(&s, "PostPath").into_iter().find(|line| line.0 != old).unwrap().0;
    let names = serde_json::Map::from_iter([(old, json!("Other")), (new, json!("Post"))]);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":source,"command":"frame.member","params":{"sketch":"PostPath","profile":{"kind":"rect_tube","width":40,"height":40,"wall":3,"outer_radius":0},"body_name":"Post","names":names}}),
    );
    assert!(s.model.result(plate).unwrap().error.is_some(), "the original line lost its body name; do not attach to the other line");
    assert!(s.model.state().body("EndPlate").is_none());
}

#[test]
fn a_plate_can_attach_to_a_member_from_a_line_added_after_the_member_feature() {
    let mut s = plate_post();
    run(&mut s, "sketch.edit", json!({"sketch":"PostPath"}));
    run(&mut s, "sketch.line", json!({"points":[[400,0],[400,"-PostHeight"]]}));
    run(&mut s, "sketch.finish", json!({}));
    let mut p = plate_pick(&s, "Post (2)", true);
    p.as_object_mut().unwrap().extend(json!({"width":100,"height":80,"thickness":8,"body_name":"New end plate"}).as_object().unwrap().clone());
    let id = run(&mut s, "frame.plate", p)["feature"].as_u64().unwrap();
    let solidcraft_doc::FeatureKind::FramePlate { member: Some(member), .. } = &s.doc.feature(id).unwrap().kind else { panic!("plate member") };
    let solidcraft_doc::FeatureKind::FrameMember { names, .. } = &s.doc.feature(member.feature.0).unwrap().kind else { panic!("member") };
    assert_eq!(names.get(&member.line).unwrap(), "Post (2)");
    let plate = bodies(&mut s).into_iter().find(|b| b.0 == "New end plate").unwrap();
    assert!(near3(plate.2, [350., -40., 300.]) && near3(plate.3, [450., 40., 308.]), "{plate:?}");
}

#[test]
fn saved_plate_planes_cannot_silently_ignore_a_wrapped_offset() {
    let mut s = plate_post();
    let p = rectangular_plate(&s, true);
    let id = run(&mut s, "frame.plate", p)["feature"].as_u64().unwrap();
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let feature = saved["features"].as_array_mut().unwrap().iter_mut().find(|f| f["id"].as_u64() == Some(id)).unwrap();
    let plane = feature["plane"].take();
    feature["plane"] = json!({"type":"offset","base":plane,"distance":"10 mm"});
    let reopened = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    assert!(reopened.model.result(id).unwrap().error.as_deref().unwrap().contains("direct persistent planar face"));
    assert!(reopened.model.state().body("EndPlate").is_none());
}
