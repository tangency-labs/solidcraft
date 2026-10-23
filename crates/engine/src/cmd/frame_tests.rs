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

// ---------------------------------------------------------------------------------------------
// Trim/Extend (frame.trim)

/// A 45 × 45 bar: its volume is 2025 mm² × its length.
fn bar45() -> Value {
    json!({"kind": "bar", "width": 45, "height": 45})
}

const BAR45: f64 = 45.0 * 45.0;

/// The member bodies a `frame.member` run made (not every body of the model), in line order.
fn made(r: &Value) -> Vec<String> {
    r["created_bodies"].as_array().unwrap().iter().map(|b| b.as_str().unwrap().to_string()).collect()
}

/// A member of 45 × 45 bar per line, each line its own 3D sketch line (a separate feature per
/// group, so each group's bodies are known by its `body_name`).
fn members_3d(s: &mut Session, name: &str, lines: &[([f64; 3], [f64; 3])]) -> Vec<String> {
    run(s, "sketch.create", json!({"plane": "XY", "name": name}));
    run(s, "sketch.options", json!({"sketch_3d": true}));
    for (a, b) in lines {
        run(s, "sketch.line3d", json!({"points": [a, b]}));
    }
    run(s, "sketch.finish", json!({}));
    made(&run(s, "frame.member", json!({"sketch": name, "profile": bar45(), "body_name": name})))
}

/// The T-slot tower of #4003 in 45 × 45 bar, footprint `w`: 4 uprights 1500 long at the corners,
/// rings of 4 rails (bottoms at 0, 700 and 1455) flush with the outside, and 2 cross members in the
/// middle ring at x = w / 2. Every line runs through its profile's centre, 22.5 in from the
/// outside. Returns (uprights, rails, cross members).
fn tower(s: &mut Session, w: f64) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (c, f) = (22.5, w - 22.5);
    let uprights = members_3d(
        s,
        "Upright",
        &[([c, c, 0.0], [c, c, 1500.0]), ([f, c, 0.0], [f, c, 1500.0]), ([f, f, 0.0], [f, f, 1500.0]), ([c, f, 0.0], [c, f, 1500.0])],
    );
    let mut lines = Vec::new();
    for z in [22.5, 722.5, 1477.5] {
        lines.extend([([c, c, z], [f, c, z]), ([c, f, z], [f, f, z]), ([c, c, z], [c, f, z]), ([f, c, z], [f, f, z])]);
    }
    let rails = members_3d(s, "Rail", &lines);
    let cross = members_3d(s, "Cross", &[([w / 2.0, c, 722.5], [w / 2.0, f, 722.5]), ([w / 2.0 + 100.0, c, 722.5], [w / 2.0 + 100.0, f, 722.5])]);
    (uprights, rails, cross)
}

/// Butt the rails against the uprights, then the cross members against the middle ring's rails
/// along X (the 5th and 6th rails).
fn butt_tower(s: &mut Session, uprights: &[String], rails: &[String], cross: &[String]) -> (Value, Value) {
    let a = run(s, "frame.trim", json!({"members": rails, "to": uprights}));
    let b = run(s, "frame.trim", json!({"members": cross, "to": [rails[4], rails[5]]}));
    (a, b)
}

/// Each body's length along the axis its bounding box is longest on.
fn lengths(s: &mut Session) -> std::collections::BTreeMap<String, f64> {
    bodies(s).into_iter().map(|b| (b.0, (0..3).map(|k| b.3[k] - b.2[k]).fold(0.0, f64::max))).collect()
}

#[test]
fn the_tower_butts_its_rails_between_the_uprights() {
    let mut s = Session::default();
    let (uprights, rails, cross) = tower(&mut s, 600.0);
    let (a, b) = butt_tower(&mut s, &uprights, &rails, &cross);
    assert_eq!(a["changed_bodies"].as_array().unwrap().len(), 12, "{a}");
    assert_eq!(b["changed_bodies"].as_array().unwrap().len(), 2, "{b}");
    let len = lengths(&mut s);
    // 4 × 1500 uprights, untouched; 12 rails and 2 cross members of 600 − 2 × 45 = 510.
    for u in &uprights {
        assert!((len[u] - 1500.0).abs() < 1e-6, "{u}: {len:?}");
    }
    for m in rails.iter().chain(&cross) {
        assert!((len[m] - 510.0).abs() < 1e-6, "{m}: {len:?}");
    }
    let v = total_volume(&mut s);
    let want = BAR45 * (4.0 * 1500.0 + 14.0 * 510.0);
    assert!(rel(v, want) < MEASURED, "{v} vs {want}");
    // The first rail runs along X at the bottom, flush with the outside: x from 45 to 555.
    let bs = boxes(&mut s);
    assert!(near3(bs[&rails[0]].0, [45.0, 0.0, 0.0]) && near3(bs[&rails[0]].1, [555.0, 45.0, 45.0]), "{:?}", bs[&rails[0]]);
    // A cross member spans the inner faces of the two rails: y from 45 to 555.
    assert!(near3(bs[&cross[0]].0, [277.5, 45.0, 700.0]) && near3(bs[&cross[0]].1, [322.5, 555.0, 745.0]), "{:?}", bs[&cross[0]]);
}

#[test]
fn trimmed_rails_follow_a_wider_footprint() {
    // A 2D frame on XY, its width a parameter: two posts along Y and a rail along X between them.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "600 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Posts"}));
    run(&mut s, "sketch.line", json!({"points": [[22.5, 0], [22.5, 400]]}));
    run(&mut s, "sketch.line", json!({"points": [["W - 22.5 mm", 0], ["W - 22.5 mm", 400]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(&mut s, "sketch.line", json!({"points": [[22.5, 200], ["W - 22.5 mm", 200]]}));
    run(&mut s, "sketch.finish", json!({}));
    let posts = made(&run(&mut s, "frame.member", json!({"sketch": "Posts", "profile": bar45(), "body_name": "Post"})));
    let rail = made(&run(&mut s, "frame.member", json!({"sketch": "Rail", "profile": bar45(), "body_name": "Rail"})));
    run(&mut s, "frame.trim", json!({"members": rail, "to": posts}));
    assert!((lengths(&mut s)["Rail"] - 510.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "800 mm"}));
    // The rail follows: 800 − 2 × 45 = 710, still between the posts' inner faces.
    let len = lengths(&mut s);
    assert!((len["Rail"] - 710.0).abs() < 1e-6, "{len:?}");
    let bs = boxes(&mut s);
    assert!(near3(bs["Rail"].0, [45.0, 177.5, -22.5]) && near3(bs["Rail"].1, [755.0, 222.5, 22.5]), "{:?}", bs["Rail"]);
}

#[test]
fn a_member_extends_to_a_plane_with_a_gap_and_cuts_at_a_slant() {
    let mut s = Session::default();
    let bar = json!({"kind": "bar", "width": 10, "height": 10});
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Line"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let m = made(&run(&mut s, "frame.member", json!({"sketch": "Line", "profile": bar, "body_name": "Bar"})));
    // Extended to x = 150, 2 short of it: 148 long.
    let f =
        run(&mut s, "frame.trim", json!({"members": m, "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}, "gap": 2}))["feature"].clone();
    let b = &bodies(&mut s)[0];
    assert!(near3(b.2, [0.0, -5.0, -5.0]) && near3(b.3, [148.0, 5.0, 5.0]), "{b:?}");
    assert!(rel(b.1, 100.0 * 148.0) < MEASURED, "{b:?}");
    // A plane at 45° through (150, 0, 0): x + z = 150. The end runs from x = 145 (top) to 155
    // (bottom); over the square section the length averages 150.
    let slant = json!({"members": m, "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 1]}}});
    run(&mut s, "timeline.redefine", json!({"feature": f, "command": "frame.trim", "params": slant}));
    let b = &bodies(&mut s)[0];
    assert!(near3(b.2, [0.0, -5.0, -5.0]) && near3(b.3, [155.0, 5.0, 5.0]), "{b:?}");
    assert!(rel(b.1, 100.0 * 150.0) < MEASURED, "{b:?}");
    // The start end, asked for, moves back to x = −20 (the plane's side away from the bar).
    run(&mut s, "frame.trim", json!({"members": m, "to": {"plane": {"origin": [-20, 0, 0], "normal": [1, 0, 0]}}, "end": "start"}));
    let b = &bodies(&mut s)[0];
    assert!(near3(b.2, [-20.0, -5.0, -5.0]) && rel(b.1, 100.0 * 170.0) < MEASURED, "{b:?}");
}

#[test]
fn a_side_given_must_face_the_member() {
    let mut s = Session::default();
    // A post along Y at x = 0 and a rail along X from x = 100 that falls short of it. The post's
    // profile x is world −X (y × d with y up and d along +Y), so its left side faces +X.
    let post = members_3d(&mut s, "Post", &[([0.0, -100.0, 0.0], [0.0, 100.0, 0.0])]);
    let rail = members_3d(&mut s, "Rail", &[([100.0, 0.0, 0.0], [300.0, 0.0, 0.0])]);
    // Its start extends to the post's side facing it (x = 22.5) by default, and so with that side.
    let f = run(&mut s, "frame.trim", json!({"members": rail, "to": post}))["feature"].clone();
    assert!(near3(boxes(&mut s)["Rail"].0, [22.5, -22.5, -22.5]), "{:?}", boxes(&mut s)["Rail"]);
    let left = json!({"members": rail, "to": [{"member": post[0], "side": "left"}]});
    run(&mut s, "timeline.redefine", json!({"feature": f, "command": "frame.trim", "params": left}));
    assert!(near3(boxes(&mut s)["Rail"].0, [22.5, -22.5, -22.5]));
    // The far side faces away from the rail, and the top runs along it: both refused.
    for side in ["right", "top"] {
        let p = json!({"members": rail, "to": [{"member": post[0], "side": side}]});
        let err = s.execute("timeline.redefine", &json!({"feature": f, "command": "frame.trim", "params": p})).unwrap_err().to_string();
        assert!(err.contains("faces away"), "{side}: {err}");
    }
}

#[test]
fn bad_trims_are_refused_with_the_reason() {
    let mut s = Session::default();
    let (uprights, rails, cross) = tower(&mut s, 600.0);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    for (p, why) in [
        (json!({"to": uprights}), "`members`"),
        (json!({"members": rails}), "`to`"),
        (json!({"members": [], "to": uprights}), "`members`"),
        (json!({"members": ["Block"], "to": uprights}), "not a frame member"),
        (json!({"members": rails, "to": ["Block"]}), "not a frame member"),
        (json!({"members": [rails[0], rails[0]], "to": uprights}), "twice"),
        (json!({"members": rails, "to": uprights, "end": "middle"}), "`end`"),
        (json!({"members": rails, "to": uprights, "gap": -1}), "`gap`"),
        (json!({"members": rails, "to": uprights, "gap": "a lot"}), "gap"),
        (json!({"members": rails, "to": [{"member": uprights[0], "side": "front"}]}), "`side`"),
        // The uprights' feature is Member1; the sketch it runs along is named Upright.
        (json!({"members": [{"feature": "Member1", "line": "nope"}], "to": rails}), "nope"),
        (json!({"members": [{"feature": "Upright", "line": "l1"}], "to": rails}), "not a frame member feature"),
        // A cross member meets no upright.
        (json!({"members": cross, "to": uprights}), "meets none"),
        (json!({"members": [rails[0]], "to": [rails[0]]}), "itself"),
        // The first rail's start meets the upright, and YZ (x = 0) is nearer its start too.
        (json!({"members": [rails[0]], "to": [uprights[0], {"plane": "YZ"}]}), "both trim"),
    ] {
        let err = s.execute("frame.trim", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
    }
    // A member a later feature changed is not rebuilt (its change would be lost).
    run(&mut s, "solid.move", json!({"bodies": [rails[0]], "translate": [0, 0, 5]}));
    let err = s.execute("frame.trim", &json!({"members": [rails[0]], "to": uprights})).unwrap_err().to_string();
    assert!(err.contains("changed after"), "{err}");
    // As a target it still serves, a butt joint being worked out from the target's line and
    // profile: the bottom ring's first rail along Y now starts at its inner side, y = 45.
    run(&mut s, "frame.trim", json!({"members": [rails[2]], "to": [rails[0]]}));
    assert!(near3(boxes(&mut s)[&rails[2]].0, [0.0, 45.0, 0.0]), "{:?}", boxes(&mut s)[&rails[2]]);
    // Not while a sketch is open.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("frame.trim", &json!({"members": [rails[1]], "to": uprights})).is_err());
}

#[test]
fn trims_undo_reopen_and_keep_their_members_through_the_saved_model() {
    let mut s = Session::default();
    let (uprights, rails, cross) = tower(&mut s, 600.0);
    let full = total_volume(&mut s);
    butt_tower(&mut s, &uprights, &rails, &cross);
    let before = bodies(&mut s);
    // Reopened from its JSON (evaluated again) and from its saved model (adopted as saved).
    let json = s.doc.to_json();
    let saved = s.model.saved(&s.doc).expect("a saved model");
    for adopted in [None, Some(&saved)] {
        let mut r = Session::opened(solidcraft_doc::Document::from_json(&json).unwrap(), adopted);
        let after = bodies(&mut r);
        assert_eq!(before.len(), after.len());
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(b.0, a.0);
            assert!(rel(a.1, b.1) < 1e-9 && near3(a.2, b.2) && near3(a.3, b.3), "{b:?} vs {a:?}");
        }
        // The members are still known to a trim after reopening: the upright's top end moves
        // down to z = 1499.
        run(&mut r, "frame.trim", json!({"members": [uprights[0]], "to": [{"plane": {"origin": [0, 0, 1499], "normal": [0, 0, 1]}}]}));
        assert!((lengths(&mut r)[&uprights[0]] - 1499.0).abs() < 1e-6);
    }
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(rel(total_volume(&mut s), full) < 1e-9, "both trims undone");
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    for (b, a) in before.iter().zip(&bodies(&mut s)) {
        assert!(b.0 == a.0 && rel(a.1, b.1) < 1e-9 && near3(a.2, b.2) && near3(a.3, b.3), "{b:?} vs {a:?}");
    }
}

#[test]
fn a_renamed_trimmed_member_stays_trimmed() {
    let mut s = Session::default();
    let (uprights, rails, cross) = tower(&mut s, 600.0);
    butt_tower(&mut s, &uprights, &rails, &cross);
    run(&mut s, "body.rename", json!({"body": rails[0], "name": "Front rail"}));
    let len = lengths(&mut s);
    assert!((len["Front rail"] - 510.0).abs() < 1e-6, "{len:?}");
    assert!((len[&cross[0]] - 510.0).abs() < 1e-6, "{len:?}");
}

/// Hostile values in every parameter of `frame.trim`, on a real tower so they get past the member
/// lookups: refused, never a panic or an internal error.
#[test]
fn hostile_trims_are_refused_not_crashed() {
    let mut s = Session::default();
    let (uprights, rails, _) = tower(&mut s, 600.0);
    let hostile = [
        json!(null),
        json!(1e308),
        json!(-1e308),
        json!(f64::MAX),
        json!("1e400 mm"),
        json!("-1e300 mm"),
        json!(""),
        json!([]),
        json!({}),
        json!([[]]),
        json!([{"feature": 1e300, "line": []}]),
        json!([{"member": {"feature": "Member1", "line": "l1"}, "side": 7}]),
        json!([{"plane": {"origin": [1e308, 0, 0], "normal": [0, 0, 0]}}]),
        json!([{"plane": {"origin": [0, 0, 0], "normal": [1e-300, 0, 0]}}]),
        json!([{"plane": {"face": [1e308, 1e308, 1e308]}}]),
        json!("x".repeat(100_000)),
    ];
    let good = json!({"members": [rails[0]], "to": uprights});
    for key in ["members", "to", "end", "gap"] {
        for v in &hostile {
            let mut p = good.clone();
            p[key] = v.clone();
            if let Err(crate::EngineError::Internal(id, msg)) = s.execute("frame.trim", &p) {
                panic!("{id} {p}: {msg}");
            }
        }
    }
    // Planes the members run along, through or far from: refused or built, never a panic.
    for plane in [
        json!({"origin": [0, 0, 0], "normal": [0, 1, 0]}),
        json!({"origin": [0, 0, 1e6], "normal": [1, 0, 1e-7]}),
        json!({"origin": [300, 0, 0], "normal": [1, 0, 0]}),
    ] {
        let p = json!({"members": [rails[0]], "to": {"plane": plane}, "gap": 1000});
        match s.execute("frame.trim", &p) {
            Err(crate::EngineError::Internal(id, msg)) => panic!("{id} {p}: {msg}"),
            // Built: take it back, so the next plane trims the rail as the tower made it.
            Ok(_) => run(&mut s, "edit.undo", json!({})),
            Err(_) => json!(null),
        };
    }
}
