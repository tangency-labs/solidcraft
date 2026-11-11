#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use solidcraft_geom::prelude::*;
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
fn members_are_made_of_a_standard_size_by_name() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": {"size": "RSC 203x89"}}));
    let bs = bodies(&mut s);
    let names: Vec<&str> = bs.iter().map(|b| b.0.as_str()).collect();
    assert_eq!(names, ["RSC 203x89", "RSC 203x89 (2)", "RSC 203x89 (3)", "RSC 203x89 (4)"]);
    // Each member is the size's section, 37.9 cm² in the BS 4 table, the length of its line.
    let props = run(&mut s, "frame.profile.properties", json!({"profile": {"size": "RSC 203x89"}}));
    let area = f(&props["area"]);
    assert!(rel(area, 3790.0) < 0.005, "{props}");
    assert_eq!(props["size"], json!("RSC 203x89"));
    assert_eq!(props["profile"]["flange_slope"], json!(5.0), "{props}");
    let v = total_volume(&mut s);
    assert!(rel(v, area * 2000.0) < MEASURED, "{v} vs {}", area * 2000.0);
    // The bottom line runs along +X: the channel, 88.9 wide and 203.2 deep, is centred on it.
    assert!(bs.iter().any(|b| near3(b.2, [0.0, -44.45, -101.6]) && near3(b.3, [600.0, 44.45, 101.6])), "{bs:?}");
    // The design keeps the size's name and dimensions, and reopens to the same members.
    let saved = s.doc.to_json();
    assert!(saved.contains("RSC 203x89") && saved.contains("5 deg"), "{saved}");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    let after = bodies(&mut reopened);
    assert_eq!(after.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), names);
    assert!(rel(total_volume(&mut reopened), v) < 1e-9);
    // The reported profile, slope in degrees, is the same section given by its dimensions.
    let again = run(&mut s, "frame.profile.properties", json!({"profile": props["profile"]}));
    assert!(rel(f(&again["area"]), area) < 1e-12 && rel(f(&again["iyy"]), f(&props["iyy"])) < 1e-12, "{again}");
    // In an inch design the size is still in millimetres.
    run(&mut s, "document.units", json!({"units": "in"}));
    let inch = run(&mut s, "frame.profile.properties", json!({"profile": {"size": "SHS 100x100x5"}}));
    assert!((f(&inch["width"]) - 100.0).abs() < 1e-9, "{inch}");
}

#[test]
fn standard_sizes_are_listed_and_refused_with_the_reason() {
    let mut s = Session::default();
    let out = run(&mut s, "frame.profile.list", json!({}));
    let sizes = out["sizes"].as_array().unwrap();
    for family in sizes {
        let names = family["names"].as_array().unwrap();
        assert!(!names.is_empty() && !family["source"].as_str().unwrap_or_default().is_empty(), "{family}");
        // The first and last size of each family make a section of their kind.
        for name in [&names[0], &names[names.len() - 1]] {
            let r = run(&mut s, "frame.profile.properties", json!({"profile": {"size": name}}));
            assert_eq!(r["profile"]["kind"], family["kind"], "{r}");
        }
    }
    let rsc = sizes.iter().find(|f| f["family"] == "RSC").unwrap();
    for name in ["RSC 203x89", "RSC 152x76"] {
        assert!(rsc["names"].as_array().unwrap().contains(&json!(name)), "{rsc}");
    }
    let only = run(&mut s, "frame.profile.list", json!({"family": "SHS"}));
    assert_eq!(only["sizes"].as_array().map(Vec::len), Some(1));
    assert!(s.execute("frame.profile.list", &json!({"family": "XYZ"})).unwrap_err().to_string().contains("`family`"));
    rectangle_frame(&mut s);
    for (profile, why) in [
        (json!({"size": "RSC 203x89", "height": 200}), "brings its own dimensions"),
        (json!({"size": "RSC 999x99"}), "unknown size"),
        (json!({"size": "RSC 203x89", "kind": "rect_tube"}), "is a channel profile"),
    ] {
        let err = s.execute("frame.member", &json!({"sketch": "Frame", "profile": profile})).unwrap_err().to_string();
        assert!(err.contains(why), "{profile}: {err}");
    }
    assert!(bodies(&mut s).is_empty());
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
fn a_located_member_keeps_its_profile_point_on_the_line_when_the_section_changes() {
    let place = |width: f64, height: f64, locate: Value| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        let p = json!({"sketch": "Rail", "profile": {"kind": "angle", "width": width, "height": height, "thickness": 5}, "locate": locate});
        run(&mut s, "frame.member", p);
        bodies(&mut s).remove(0)
    };
    // The angle is drawn about its heel: locate [0, 0] puts the heel on the line, as the
    // bottom_left anchor does, and it stays there when the legs grow.
    let small = place(60.0, 40.0, json!([0, 0]));
    assert!(near3(small.2, [0.0, 0.0, 0.0]) && near3(small.3, [1000.0, 60.0, 40.0]), "{small:?}");
    let large = place(80.0, 50.0, json!([0, 0]));
    assert!(near3(large.2, [0.0, 0.0, 0.0]) && near3(large.3, [1000.0, 80.0, 50.0]), "{large:?}");
    // A point 10 across and 5 up from the heel on the line moves the profile back by that much.
    let off = place(60.0, 40.0, json!([10, 5]));
    assert!(near3(off.2, [0.0, -10.0, -5.0]) && near3(off.3, [1000.0, 50.0, 35.0]), "{off:?}");
    // A bad point is refused with the reason.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    for bad in [json!([1]), json!(["a", 0]), json!([0, 1e9])] {
        let err = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "locate": bad})).unwrap_err().to_string();
        assert!(err.contains("`locate`"), "{bad}: {err}");
    }
    // A design file with a point the command would refuse fails the feature, with the reason.
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "locate": [0, 0]}));
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    fn far(v: &mut Value) {
        match v {
            Value::Object(o) => {
                if let Some(l) = o.get_mut("locate") {
                    *l = json!([1e8, 0.0]);
                }
                o.values_mut().for_each(far);
            }
            Value::Array(a) => a.iter_mut().for_each(far),
            _ => {}
        }
    }
    far(&mut saved);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
    let fid = reopened.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let failing = reopened.model.result(fid).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(failing.contains("`locate`"), "{failing}");
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

/// A 300 mm post up Z from the origin of `profile`, its member body `Post`, made by the feature
/// `Post member` from the 3D sketch `Post path`.
fn capped_post(profile: Value) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Post path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Post path", "name": "Post member", "body_name": "Post", "profile": profile}));
    s
}

/// The face of `body` through `point` facing along `normal`, as the view picks it.
fn face_pick(s: &Session, body: &str, point: [f64; 3], normal: [f64; 3]) -> Value {
    let st = s.model.state();
    let b = st.body(body).unwrap();
    let at = solidcore_record::Vec3::new(point[0], point[1], point[2]);
    let n = solidcore_record::Vec3::new(normal[0], normal[1], normal[2]);
    let i = (0..b.body.face_count())
        .find(|i| b.body.planar_face(*i).is_some_and(|pl| pl.normal().dot(n) > 0.999 && (pl.origin - at).dot(n).abs() < 1e-6))
        .unwrap();
    json!({"body": body, "index": i, "face": point})
}

fn square_tube() -> Value {
    json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 0})
}

fn measured(s: &mut Session, name: &str) -> Measured {
    bodies(s).into_iter().find(|b| b.0 == name).unwrap_or_else(|| panic!("no body {name}"))
}

#[test]
fn an_end_cap_is_flush_with_the_outside_unless_offset() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let r = run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    assert_eq!(r["created_bodies"], json!(["Post End Cap"]), "{r}");
    // No offset: the plate is the profile's outside, 40 x 40, 6 thick, on top.
    let cap = measured(&mut s, "Post End Cap");
    assert!(rel(cap.1, 40.0 * 40.0 * 6.0) < 1e-9, "{cap:?}");
    assert!(near3(cap.2, [-20.0, -20.0, 300.0]) && near3(cap.3, [20.0, 20.0, 306.0]), "{cap:?}");
    // The bottom end faces down: its cap hangs below the post. Offset by half its thickness of
    // 4: a 36 x 36 plate.
    let bottom = face_pick(&s, "Post", [18.5, 0.0, 0.0], [0.0, 0.0, -1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [bottom], "thickness": 4, "ratio": 0.5}));
    let cap = measured(&mut s, "Post End Cap (2)");
    assert!(near3(cap.2, [-18.0, -18.0, -4.0]) && near3(cap.3, [18.0, 18.0, 0.0]), "{cap:?}");
}

/// A cap made while another is suppressed doesn't take that one's name: restored, each keeps its
/// own, so what refers to the second cap by name still finds it.
#[test]
fn an_end_cap_made_while_another_is_suppressed_keeps_a_name_of_its_own() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let bottom = face_pick(&s, "Post", [18.5, 0.0, 0.0], [0.0, 0.0, -1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6, "name": "Top cap"}));
    run(&mut s, "timeline.suppress", json!({"feature": "Top cap"}));
    let r = run(&mut s, "frame.end_cap", json!({"faces": [bottom], "thickness": 4, "name": "Bottom cap"}));
    assert_eq!(r["created_bodies"], json!(["Post End Cap (2)"]), "{r}");
    run(&mut s, "timeline.suppress", json!({"feature": "Top cap", "suppressed": false}));
    // The top cap on top, the bottom one below the post, each under the name it was made with.
    assert!(measured(&mut s, "Post End Cap").2[2] > 299.0);
    assert!(measured(&mut s, "Post End Cap (2)").3[2] < 1e-6);
}

#[test]
fn end_cap_corner_chamfers_cut_each_corner() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 5, "offset": 2, "chamfer": 4}));
    // 36 x 36 less four 4 x 4 corner triangles.
    let v = measured(&mut s, "Post End Cap").1;
    assert!(rel(v, (36.0 * 36.0 - 4.0 * 8.0) * 5.0) < 1e-9, "{v}");
}

#[test]
fn end_cap_on_a_rounded_tube_keeps_the_rounding_less_the_inset() {
    // Outer radius 6 (twice the wall): inset 3 leaves 34 x 34 with corners of radius 3.
    let mut s = capped_post(tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let before = s.doc.to_json();
    let e = s.execute("frame.end_cap", &json!({"faces": [top.clone()], "thickness": 6, "ratio": 0.5, "chamfer": 2})).unwrap_err().to_string();
    assert!(e.contains("at least the corner rounding"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "frame.end_cap", json!({"faces": [top.clone()], "thickness": 6, "ratio": 0.5}));
    let v = measured(&mut s, "Post End Cap").1;
    assert!(rel(v, (34.0 * 34.0 - (4.0 - PI) * 9.0) * 6.0) < MEASURED, "{v}");
    run(&mut s, "edit.undo", json!({}));
    // A chamfer as long as the rounding cuts it away: four 3 x 3 triangles off the 34 square.
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6, "ratio": 0.5, "chamfer": 3}));
    let v = measured(&mut s, "Post End Cap").1;
    assert!(rel(v, (34.0 * 34.0 - 4.0 * 4.5) * 6.0) < 1e-9, "{v}");
}

#[test]
fn an_end_cap_on_an_i_beam_closes_its_whole_end_square() {
    // An I-beam is open, so its cap is the 50 x 100 rectangle round it, in by 1: 48 x 98.
    let mut s = capped_post(json!({"kind": "i_beam", "height": 100, "width": 50, "web": 4.5, "flange": 7}));
    let post = measured(&mut s, "Post");
    let mid = [(post.2[0] + post.3[0]) / 2.0, (post.2[1] + post.3[1]) / 2.0, 300.0];
    let top = face_pick(&s, "Post", mid, [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6, "offset": 1}));
    let cap = measured(&mut s, "Post End Cap");
    assert!(rel(cap.1, 48.0 * 98.0 * 6.0) < 1e-9, "{cap:?}");
    // A 2 mm chamfer cuts the rectangle's four corners.
    run(&mut s, "edit.undo", json!({}));
    let top = face_pick(&s, "Post", mid, [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top.clone()], "thickness": 6, "offset": 1, "chamfer": 2}));
    let v = measured(&mut s, "Post End Cap").1;
    assert!(rel(v, (48.0 * 98.0 - 4.0 * 2.0) * 6.0) < 1e-9, "{v}");
    // An inset of half the beam's width leaves no plate, and nothing changes.
    run(&mut s, "edit.undo", json!({}));
    let before = s.doc.to_json();
    let e = s.execute("frame.end_cap", &json!({"faces": [top], "thickness": 6, "offset": 25})).unwrap_err().to_string();
    assert!(e.contains("leaves no plate"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn an_inward_end_cap_cuts_the_member_back() {
    let mut s = capped_post(json!({"kind": "round_tube", "diameter": 40, "wall": 3}));
    let ring = PI * (400.0 - 289.0);
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let r = run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 4, "ratio": 0.5, "direction": "inward"}));
    assert_eq!(r["changed_bodies"], json!(["Post"]), "{r}");
    let post = measured(&mut s, "Post");
    assert!(rel(post.1, ring * 296.0) < MEASURED, "{post:?}");
    assert!((post.3[2] - 296.0).abs() < 1e-6, "{post:?}");
    // Radius 20 less an inset of 2, 4 thick, flush with the old end.
    let cap = measured(&mut s, "Post End Cap");
    assert!(rel(cap.1, PI * 18.0 * 18.0 * 4.0) < MEASURED, "{cap:?}");
    assert!((cap.2[2] - 296.0).abs() < 1e-6 && (cap.3[2] - 300.0).abs() < 1e-6, "{cap:?}");
}

/// The post's body moved into a component placed 50 along Y, capped from the top of the design:
/// the cap is authored in the design's frame, the post is stored in its component's. The inward
/// cut is made where the post is, so it is cut back by the cap's thickness, not missed.
fn a_post_in_a_placed_component() -> Session {
    let mut s = capped_post(square_tube());
    let holder = run(&mut s, "component.create", json!({"name": "Holder", "grounded": false}))["component"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": "Holder:1", "translate": [0, 50, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    std::sync::Arc::make_mut(&mut s.doc).body_components.insert("Post".into(), holder);
    s.refresh();
    s
}

#[test]
fn an_inward_end_cap_cuts_a_member_in_a_placed_component_where_it_stands() {
    let mut s = a_post_in_a_placed_component();
    let post = measured(&mut s, "Post");
    assert!(near3(post.2, [-20.0, 30.0, 0.0]) && near3(post.3, [20.0, 70.0, 300.0]), "{post:?}");
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let top = json!({"body": top["body"], "index": top["index"], "face": [18.5, 50.0, 300.0]});
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 4, "direction": "inward"}));
    // 40 x 40 x 3 tube, 296 long after the cut; the cap 40 x 40 x 4 where the end was.
    let post = measured(&mut s, "Post");
    assert!(near3(post.2, [-20.0, 30.0, 0.0]) && near3(post.3, [20.0, 70.0, 296.0]), "{post:?}");
    assert!(rel(post.1, (1600.0 - 34.0 * 34.0) * 296.0) < 1e-9, "{post:?}");
    let cap = measured(&mut s, "Post End Cap");
    assert!(near3(cap.2, [-20.0, 30.0, 296.0]) && near3(cap.3, [20.0, 70.0, 300.0]), "{cap:?}");
}

/// Moving the occurrence a capped member is placed by rebuilds the cap: the incremental model
/// gives what a model built from scratch does.
#[test]
fn moving_a_capped_members_occurrence_rebuilds_the_cap_as_from_scratch() {
    let mut s = a_post_in_a_placed_component();
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let top = json!({"body": top["body"], "index": top["index"], "face": [18.5, 50.0, 300.0]});
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 4, "direction": "inward"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Holder:1", "translate": [30, 0, 0]}));
    let warm = bodies(&mut s);
    let mut cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let cold = bodies(&mut cold);
    assert_eq!(warm.len(), cold.len(), "{warm:?} vs {cold:?}");
    for (w, c) in warm.iter().zip(&cold) {
        assert!(w.0 == c.0 && rel(w.1, c.1) < 1e-9 && near3(w.2, c.2) && near3(w.3, c.3), "{w:?} vs {c:?}");
    }
}

/// Editing a cap down to its second end leaves that cap its own name: the first end's name goes
/// with the end, so what was set on it (a material, a reference) doesn't move to the other cap.
#[test]
fn an_end_cap_edit_that_drops_an_end_keeps_each_remaining_cap_its_name() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let bottom = face_pick(&s, "Post", [18.5, 0.0, 0.0], [0.0, 0.0, -1.0]);
    let r = run(&mut s, "frame.end_cap", json!({"faces": [top, bottom.clone()], "thickness": 6, "name": "Caps"}));
    assert_eq!(r["created_bodies"], json!(["Post End Cap", "Post End Cap (2)"]), "{r}");
    run(&mut s, "timeline.redefine", json!({"feature": "Caps", "command": "frame.end_cap", "params": {"faces": [bottom], "thickness": 6}}));
    let st = s.model.state();
    assert!(st.body("Post End Cap").is_none(), "{:?}", st.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    drop(st);
    assert!(measured(&mut s, "Post End Cap (2)").3[2] < 1e-6, "the bottom cap keeps its name");
}

/// Editing a cap from one end onto another: the new end's cap is not given the removed end's
/// name, which stays kept back as a renamed body's is.
#[test]
fn an_end_cap_edit_onto_another_end_does_not_hand_it_the_removed_ends_name() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let bottom = face_pick(&s, "Post", [18.5, 0.0, 0.0], [0.0, 0.0, -1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6, "name": "Cap"}));
    run(&mut s, "timeline.redefine", json!({"feature": "Cap", "command": "frame.end_cap", "params": {"faces": [bottom], "thickness": 6}}));
    assert!(s.model.state().body("Post End Cap").is_none());
    assert!(measured(&mut s, "Post End Cap (2)").3[2] < 1e-6, "the bottom end's cap has a name of its own");
}

/// An end added before one the cap already closes doesn't take that one's name: the kept end's
/// cap keeps it, and the new one is numbered after it.
#[test]
fn an_end_added_before_a_kept_end_leaves_that_cap_its_name() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let bottom = face_pick(&s, "Post", [18.5, 0.0, 0.0], [0.0, 0.0, -1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [bottom.clone()], "thickness": 6, "name": "Cap"}));
    run(&mut s, "timeline.redefine", json!({"feature": "Cap", "command": "frame.end_cap", "params": {"faces": [top, bottom], "thickness": 6}}));
    assert!(measured(&mut s, "Post End Cap").3[2] < 1e-6, "the bottom cap keeps its name");
    assert!(measured(&mut s, "Post End Cap (2)").2[2] > 299.0, "the new top cap is numbered after it");
}

#[test]
fn end_caps_follow_member_edits_renames_undo_and_reopen() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let bottom = face_pick(&s, "Post", [18.5, 0.0, 0.0], [0.0, 0.0, -1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top, bottom], "thickness": 6, "ratio": 0.5, "name": "Caps"}));
    run(&mut s, "body.rename", json!({"body": "Post", "name": "Column"}));
    let params = json!({"sketch": "Post path", "body_name": "Column", "profile": {"kind": "rect_tube", "width": 60, "height": 40, "wall": 3, "outer_radius": 0}});
    run(&mut s, "timeline.redefine", json!({"feature": "Post member", "command": "frame.member", "params": params}));
    let cap = measured(&mut s, "Post End Cap");
    assert!(rel(cap.1, 54.0 * 34.0 * 6.0) < 1e-9, "{cap:?}");
    assert!(near3(cap.2, [-27.0, -17.0, 300.0]), "{cap:?}");
    assert!(measured(&mut s, "Post End Cap (2)").2[2] < -5.9);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(rel(reopened.model.state().body("Post End Cap").unwrap().mesh().measure().volume, 54.0 * 34.0 * 6.0) < 1e-7);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().body("Post End Cap").is_none());
    run(&mut s, "edit.redo", json!({}));
    assert!(rel(measured(&mut s, "Post End Cap").1, 34.0 * 34.0 * 6.0) < 1e-9);
}

#[test]
fn end_caps_refuse_bad_picks_and_values_atomically() {
    let mut s = capped_post(square_tube());
    run(&mut s, "solid.box", json!({"corner": [100, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let side = face_pick(&s, "Post", [20.0, 0.0, 150.0], [1.0, 0.0, 0.0]);
    let block = face_pick(&s, "Block", [105.0, 5.0, 10.0], [0.0, 0.0, 1.0]);
    // The same end face picked at another point on it is still the same end.
    let top_again = face_pick(&s, "Post", [0.0, 18.5, 300.0], [0.0, 0.0, 1.0]);
    let good = json!({"faces": [top.clone()], "thickness": 6});
    let cases = [
        (json!({"faces": [side]}), "not an end"),
        (json!({"faces": [block]}), "not on a frame member"),
        (json!({"faces": [top.clone(), top.clone()]}), "twice"),
        (json!({"faces": [top, top_again.clone()]}), "twice"),
        (json!({"faces": [top_again.clone(), top_again], "direction": "inward"}), "twice"),
        (json!({"faces": []}), ""),
        (json!({"offset": 20}), "leaves no plate"),
        (json!({"ratio": 4}), "leaves no plate"),
        (json!({"offset": 1, "ratio": 0.5}), "not both"),
        (json!({"chamfer": 25}), "too large"),
        (json!({"thickness": -1}), "thickness"),
        (json!({"thickness": "1e99 mm"}), ""),
        (json!({"thickness": 0}), "thickness"),
        (json!({"ratio": "-1"}), "ratio"),
        (json!({"direction": "sideways"}), ""),
        (json!({"direction": "inward", "thickness": 300, "offset": 0}), "longer member"),
        (json!({"unsupported": true}), ""),
    ];
    for (change, why) in cases {
        let mut p = good.clone();
        for (k, v) in change.as_object().unwrap() {
            p[k] = v.clone();
        }
        let before = s.doc.to_json();
        let e = s.execute("frame.end_cap", &p).map(|_| ()).unwrap_err().to_string();
        assert!(e.contains(why), "{p}: {e}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
    // An angle is open too: its cap is the 40 x 40 square round it, in by 1 (half of 2).
    let mut s = capped_post(json!({"kind": "angle", "width": 40, "height": 40, "thickness": 4}));
    let end = face_pick(&s, "Post", [-18.0, -19.0, 300.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [end], "thickness": 2, "ratio": 0.5}));
    let v = measured(&mut s, "Post End Cap").1;
    assert!(rel(v, 38.0 * 38.0 * 2.0) < 1e-9, "{v}");
}

/// Members longer than 810 mm mirrored as bodies measure as their sources, across a named plane
/// and across a plane by origin and normal: SMLib's mirror gave each straight edge as a B-spline
/// 1,234,567 edge lengths long, which the copy's measure refused ("invalid B-rep record: flat
/// arrays", #14413, #15379).
#[test]
fn long_members_mirrored_as_bodies_measure_as_their_sources() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rails"}));
    run(&mut s, "sketch.line", json!({"points": [[100, 0], [100, 1100]]}));
    run(&mut s, "sketch.line", json!({"points": [[300, -200], [300, 1800]]}));
    run(&mut s, "sketch.finish", json!({}));
    let channel = json!({"kind": "channel", "height": 152.4, "width": 76.2, "web": 6.4, "flange": 9, "root_radius": 12.2, "toe_radius": 2.4});
    run(&mut s, "frame.member", json!({"sketch": "Rails", "lines": ["l1"], "profile": channel, "body_name": "Channel"}));
    run(&mut s, "frame.member", json!({"sketch": "Rails", "lines": ["l2"], "profile": tube(), "body_name": "Tube"}));
    run(&mut s, "solid.mirror", json!({"bodies": ["Channel", "Tube"], "plane": "YZ"}));
    run(&mut s, "solid.mirror", json!({"bodies": ["Channel"], "plane": {"origin": [1000, 0, 0], "normal": [1, 0, 0]}}));
    let bs = bodies(&mut s);
    let get = |name: &str| bs.iter().find(|b| b.0 == name).cloned().unwrap_or_else(|| panic!("no {name} in {bs:?}"));
    for (source, copy, x0) in [("Channel", "Channel (2)", 0.0), ("Tube", "Tube (2)", 0.0), ("Channel", "Channel (3)", 1000.0)] {
        let (a, b) = (get(source), get(copy));
        assert!(rel(b.1, a.1) < 1e-9, "{copy}: volume {} against {}", b.1, a.1);
        // Mirrored in x = x0: x runs the other way, y and z stay.
        assert!(near3(b.2, [2.0 * x0 - a.3[0], a.2[1], a.2[2]]) && near3(b.3, [2.0 * x0 - a.2[0], a.3[1], a.3[2]]), "{copy}: {b:?} from {a:?}");
    }
}

/// A frame member feature mirrors and patterns as a feature: each copy is every member it made,
/// placed by the mirror or the pattern, and the copies follow the member's lines. Mirroring the
/// feature was refused ("cannot be patterned or mirrored yet").
#[test]
fn a_member_feature_mirrors_and_patterns_and_follows_its_lines() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "700 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rails"}));
    run(&mut s, "sketch.line", json!({"points": [[100, 0], [100, "L"]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Rails", "profile": square_tube(), "name": "Rail member", "body_name": "Rail"}));
    run(&mut s, "solid.mirror", json!({"features": ["Rail member"], "plane": "YZ"}));
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Rail member"], "dir1": [0, 0, 1], "count1": 2, "spacing1": 300}));
    let check = |s: &mut Session, length: f64| {
        let bs = bodies(s);
        assert_eq!(bs.len(), 3, "{bs:?}");
        for b in &bs {
            assert!(rel(b.1, 444.0 * length) < 1e-9, "{b:?}");
        }
        let at = |lo: [f64; 3], hi: [f64; 3]| bs.iter().any(|b| near3(b.2, lo) && near3(b.3, hi));
        assert!(at([80.0, 0.0, -20.0], [120.0, length, 20.0]), "the rail: {bs:?}");
        assert!(at([-120.0, 0.0, -20.0], [-80.0, length, 20.0]), "its mirror in x = 0: {bs:?}");
        assert!(at([80.0, 0.0, 280.0], [120.0, length, 320.0]), "its copy 300 up: {bs:?}");
    };
    check(&mut s, 700.0);
    run(&mut s, "parameters.change", json!({"name": "L", "expression": "760 mm"}));
    check(&mut s, 760.0);
}

/// The side of the capped post that faces +X.
fn post_side(s: &Session) -> Value {
    face_pick(s, "Post", [20.0, 0.0, 150.0], [1.0, 0.0, 0.0])
}

fn turn_step(s: &Session) -> &solidcraft_doc::Feature {
    s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMemberTurn { .. })).unwrap()
}

#[test]
fn rotate_member_adds_a_rotate_step() {
    let mut s = capped_post(square_tube());
    let face = post_side(&s);
    let r = run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));
    assert!(r.to_string().contains("Rotate1"), "{r}");
    let info = run(&mut s, "timeline.inspect", json!({"feature": "Rotate1"}));
    assert!(info.to_string().contains("frame_member_turn"), "{info}");
}

#[test]
fn rotate_member_refuses_a_face_not_on_a_member() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    let face = face_pick(&s, "Cube", [10.0, 5.0, 5.0], [1.0, 0.0, 0.0]);
    let before = s.doc.to_json();
    let e = s.execute("frame.rotate_member", &json!({"face": face, "angle": "30 deg"})).unwrap_err().to_string();
    assert!(e.contains("Select a face of a frame member"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn rotate_step_saves_and_reopens() {
    let mut s = capped_post(square_tube());
    let face = post_side(&s);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let (a, b) = (turn_step(&s), turn_step(&reopened));
    let (
        solidcraft_doc::FeatureKind::FrameMemberTurn { member: ma, angle: aa },
        solidcraft_doc::FeatureKind::FrameMemberTurn { member: mb, angle: ab },
    ) = (&a.kind, &b.kind)
    else {
        panic!("not a turn");
    };
    assert_eq!((ma, aa), (mb, ab));
    assert_eq!(aa.as_ref() as &str, "30 deg");
    assert!(!ma.line.is_empty());
}

#[test]
fn a_rotate_step_reports_a_lost_member() {
    let mut s = capped_post(square_tube());
    let face = post_side(&s);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));
    let step = turn_step(&s).id;
    run(&mut s, "timeline.suppress", json!({"feature": "Post member"}));
    let e = s.model.result(step).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("Its frame member is gone"), "{e}");
    run(&mut s, "timeline.suppress", json!({"feature": "Post member", "suppressed": false}));
    assert!(s.model.result(step).is_some_and(|r| r.error.is_none()));
    if let Some(f) = s.doc_mut().feature_mut(step)
        && let solidcraft_doc::FeatureKind::FrameMemberTurn { member, .. } = &mut f.kind
    {
        member.line = "nope".into();
    }
    s.refresh();
    let e = s.model.result(step).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("Line `nope` of its frame member is gone"), "{e}");
}

#[test]
fn rotate_member_refuses_a_member_in_another_occurrence() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part"}));
    let occurrence = part["occurrence"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Post path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Post path", "name": "Post member", "body_name": "Post", "profile": square_tube()}));
    let mut face = post_side(&s);
    face["path"] = json!([occurrence]);
    let e = s.execute("frame.rotate_member", &json!({"face": face, "angle": "30 deg"})).unwrap_err().to_string();
    assert!(e.contains("Activate the member's component"), "{e}");
}

/// A 40 × 40 square tube on the sloped line (0, 0, 0)–(300, 0, 300), its member body `Brace`,
/// made by the feature `Brace member`. Its profile's x runs along world Y.
fn brace() -> Session {
    brace_at("origin")
}

/// [`brace`] with the profile point `anchor` on the line.
fn brace_at(anchor: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Brace path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [300, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Brace path", "name": "Brace member", "body_name": "Brace", "profile": square_tube(), "anchor": anchor}),
    );
    s
}

/// The side of the brace whose outward normal is the profile's x turned `deg` about the line.
fn brace_side(s: &Session, deg: f64) -> Value {
    let t = deg.to_radians();
    let y = [-1.0 / 2f64.sqrt(), 0.0, 1.0 / 2f64.sqrt()];
    let n = [t.sin() * y[0], t.cos(), t.sin() * y[2]];
    face_pick(s, "Brace", [150.0 + 20.0 * n[0], 20.0 * n[1], 150.0 + 20.0 * n[2]], n)
}

fn brace_box(s: &mut Session) -> ([f64; 3], [f64; 3]) {
    let b = measured(s, "Brace");
    (b.2, b.3)
}

fn same_box(a: ([f64; 3], [f64; 3]), b: ([f64; 3], [f64; 3])) -> bool {
    near3(a.0, b.0) && near3(a.1, b.1)
}

/// The brace turned once by `angle` from a fresh design.
fn brace_turned(angle: &str) -> ([f64; 3], [f64; 3]) {
    let mut s = brace();
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": angle}));
    brace_box(&mut s)
}

#[test]
fn a_rotate_step_turns_its_member_about_the_profile_centre() {
    let mut s = brace();
    let flat = measured(&mut s, "Brace");
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    let turned = measured(&mut s, "Brace");
    // Square on, the brace is 40 wide across Y; turned 45° its corners stick out 40√2.
    assert!(rel(flat.3[1] - flat.2[1], 40.0) < 1e-9, "{flat:?}");
    assert!(rel(turned.3[1] - turned.2[1], 40.0 * 2f64.sqrt()) < 1e-9, "{turned:?}");
    assert!(rel(turned.1, flat.1) < 1e-9, "{turned:?} vs {flat:?}");
    // It spins about the profile's centre: its centre of mass stays put.
    assert!(near3(turned.4, flat.4), "{turned:?} vs {flat:?}");
}

/// The pivot is the profile's centre wherever the profile sits on the line: a brace laid by its
/// bottom-left corner spins in place, not about the line.
#[test]
fn an_anchored_member_turns_about_its_profile_centre() {
    let mut s = brace_at("bottom_left");
    let flat = measured(&mut s, "Brace");
    // The tube's centre is its centre of mass; its +x side faces +Y, 20 out from it.
    let c = flat.4;
    let face = face_pick(&s, "Brace", [c[0], c[1] + 20.0, c[2]], [0.0, 1.0, 0.0]);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    let turned = measured(&mut s, "Brace");
    assert!(!same_box((turned.2, turned.3), (flat.2, flat.3)), "{turned:?}");
    assert!(near3(turned.4, flat.4), "{turned:?} vs {flat:?}");
}

#[test]
fn rotate_follows_suppress_delete_marker_and_edit() {
    let mut s = brace();
    let flat = brace_box(&mut s);
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face.clone(), "angle": "45 deg"}));
    let turned = brace_box(&mut s);
    assert!(!same_box(turned, flat), "{turned:?}");

    run(&mut s, "timeline.suppress", json!({"feature": "Rotate1"}));
    assert!(same_box(brace_box(&mut s), flat), "suppressed");
    run(&mut s, "timeline.suppress", json!({"feature": "Rotate1", "suppressed": false}));
    assert!(same_box(brace_box(&mut s), turned), "unsuppressed");

    let at = s.doc.features.iter().position(|f| f.name == "Rotate1").unwrap();
    run(&mut s, "timeline.rollback", json!({"position": at}));
    assert!(same_box(brace_box(&mut s), flat), "rolled back before the step");
    run(&mut s, "timeline.rollback", json!({"position": "end"}));
    assert!(same_box(brace_box(&mut s), turned), "rolled forward");

    // The edit runs with the step suppressed, so the face is picked on the member as laid.
    run(&mut s, "timeline.redefine", json!({"feature": "Rotate1", "command": "frame.rotate_member", "params": {"face": face, "angle": "0 deg"}}));
    assert!(same_box(brace_box(&mut s), flat), "edited to 0 deg");
    run(&mut s, "timeline.redefine", json!({"feature": "Rotate1", "command": "frame.rotate_member", "params": {"face": face, "angle": "45 deg"}}));
    assert!(same_box(brace_box(&mut s), turned), "edited back to 45 deg");

    run(&mut s, "timeline.delete", json!({"feature": "Rotate1"}));
    assert!(same_box(brace_box(&mut s), flat), "deleted");
}

#[test]
fn the_later_rotate_step_wins() {
    let mut s = brace();
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "20 deg"}));
    let face = brace_side(&s, 20.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    let got = brace_box(&mut s);
    let want = brace_turned("45 deg");
    assert!(same_box(got, want), "{got:?} vs {want:?}");
    assert!(!same_box(got, brace_turned("20 deg")), "the earlier step doesn't win");
}

/// Two steps on the line, the later one's angle broken: the earlier one turns the member, and
/// is the line's turn as the design reads it.
#[test]
fn a_bad_later_rotate_step_leaves_the_earlier_one_standing() {
    let mut s = brace();
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "20 deg"}));
    let face = brace_side(&s, 20.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    let later = s.doc.features.iter().find(|f| f.name == "Rotate2").unwrap().id;
    if let Some(f) = s.doc_mut().feature_mut(later)
        && let solidcraft_doc::FeatureKind::FrameMemberTurn { angle, .. } = &mut f.kind
    {
        *angle = "nope".into();
    }
    s.refresh();
    let frame = s.doc.features.iter().find(|f| f.name == "Brace member").unwrap().id;
    let turns = solidcraft_doc::frame::member_turns(&s.doc, &s.doc.evaluation_values(), solidcraft_doc::FeatureId(frame));
    let angles: Vec<&str> = turns.values().map(|(e, _)| e.as_str()).collect();
    assert_eq!(angles, ["20 deg"]);
    let got = brace_box(&mut s);
    assert!(same_box(got, brace_turned("20 deg")), "{got:?}");
}

#[test]
fn a_rotate_angle_parameter_drives_the_turn() {
    let mut s = brace();
    let flat = brace_box(&mut s);
    run(&mut s, "parameters.add", json!({"name": "tilt", "expression": "20 deg"}));
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "tilt"}));
    let got = brace_box(&mut s);
    assert!(!same_box(got, flat) && same_box(got, brace_turned("20 deg")), "{got:?}");
    run(&mut s, "parameters.change", json!({"name": "tilt", "expression": "40 deg"}));
    let got = brace_box(&mut s);
    let want = brace_turned("40 deg");
    assert!(same_box(got, want), "{got:?} vs {want:?}");
}

/// A unitless parameter that gains an angle unit keeps its number (`30` to `30 rad`): the turn
/// follows the unit all the same.
#[test]
fn a_rotate_angle_parameter_given_a_unit_turns_again() {
    let mut s = brace();
    run(&mut s, "parameters.add", json!({"name": "tilt", "expression": "30", "unit": "none"}));
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "tilt"}));
    let before = brace_box(&mut s);
    run(&mut s, "parameters.change", json!({"name": "tilt", "expression": "30 rad", "unit": "rad"}));
    let got = brace_box(&mut s);
    let want = brace_turned("30 rad");
    assert!(!same_box(got, before) && same_box(got, want), "{before:?} {got:?} vs {want:?}");
}

#[test]
fn a_rotate_step_before_its_frame_turns_nothing() {
    let mut s = brace();
    let flat = brace_box(&mut s);
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    assert!(!same_box(brace_box(&mut s), flat));
    let step = turn_step(&s).id;
    let frame = s.doc.features.iter().position(|f| f.name == "Brace member").unwrap();
    run(&mut s, "timeline.reorder", json!({"feature": "Rotate1", "position": frame, "allow_errors": true}));
    let e = s.model.result(step).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("Its frame member must come earlier in the timeline"), "{e}");
    assert!(same_box(brace_box(&mut s), flat));
}

#[test]
fn an_end_cap_follows_a_turned_member() {
    let mut s = capped_post(square_tube());
    let top = face_pick(&s, "Post", [18.5, 0.0, 300.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let face = post_side(&s);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));
    let post = measured(&mut s, "Post");
    let cap = measured(&mut s, "Post End Cap");
    // Turned 30°, the 40 × 40 section spans 20 (cos 30° + sin 30°) each side in X and Y.
    let half = 20.0 * (30f64.to_radians().cos() + 30f64.to_radians().sin());
    assert!(near3(post.2, [-half, -half, 0.0]) && near3(post.3, [half, half, 300.0]), "{post:?}");
    assert!(near3([cap.2[0], cap.2[1], 0.0], [post.2[0], post.2[1], 0.0]), "{cap:?} vs {post:?}");
    assert!(near3([cap.3[0], cap.3[1], 0.0], [post.3[0], post.3[1], 0.0]), "{cap:?} vs {post:?}");
    assert!((cap.2[2] - 300.0).abs() < 1e-6 && (cap.3[2] - 306.0).abs() < 1e-6, "{cap:?}");
}

#[test]
fn a_turned_member_keeps_its_face_names() {
    let mut s = capped_post(square_tube());
    let face = post_side(&s);
    let index = face["index"].as_u64().unwrap() as usize;
    let name = solidcraft_doc::naming::face_names(s.model.state().body("Post").unwrap())[index].clone();
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));
    let st = s.model.state();
    let post = st.body("Post").unwrap();
    let now = solidcraft_doc::naming::face_names(post).iter().position(|n| *n == name).unwrap_or_else(|| panic!("no face {name}"));
    // The same side, turned 30° about Z with the member.
    let t = 30f64.to_radians();
    let n = post.body.planar_face(now).unwrap().normal();
    assert!(near3([n.x, n.y, n.z], [t.cos(), t.sin(), 0.0]), "{name}: {n:?}");
}

/// A Rotate step whose angle can't be worked out reports it itself; its Frame Member builds as
/// if the step weren't there (Ruling 8).
#[test]
fn a_rotate_step_with_a_bad_angle_errors_alone() {
    let mut s = brace();
    let flat = brace_box(&mut s);
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    assert!(!same_box(brace_box(&mut s), flat));
    let step = turn_step(&s).id;
    let frame = s.doc.features.iter().find(|f| f.name == "Brace member").unwrap().id;
    // Not a number, not an angle, and a parameter the design hasn't got.
    for (bad, says) in [("1 / 0 * 1 deg", ""), ("45 mm", ""), ("nope", "unknown parameter `nope`")] {
        if let Some(f) = s.doc_mut().feature_mut(step)
            && let solidcraft_doc::FeatureKind::FrameMemberTurn { angle, .. } = &mut f.kind
        {
            *angle = bad.into();
        }
        s.refresh();
        let e = s.model.result(step).and_then(|r| r.error.clone()).unwrap_or_default();
        assert!(!e.is_empty() && e.contains(says), "{bad}: the step reports its angle: {e}");
        assert!(s.model.result(frame).is_some_and(|r| r.error.is_none()), "{bad}: the frame builds");
        assert!(same_box(brace_box(&mut s), flat), "{bad}: unturned");
    }
}

fn brace_normals(s: &Session) -> Vec<solidcore_record::Vec3> {
    let st = s.model.state();
    let b = st.body("Brace").unwrap();
    (0..b.body.face_count()).filter_map(|i| b.body.planar_face(i)).map(|pl| pl.normal()).collect()
}

fn ref_line(s: &mut Session, plane: &str, points: Value) {
    run(s, "sketch.create", json!({"plane": plane, "name": "Ref"}));
    run(s, "sketch.line", json!({"points": points, "ids": ["ref"]}));
    run(s, "sketch.finish", json!({}));
}

#[test]
fn parallel_to_lines_a_sloped_member_side_up_with_a_line() {
    let mut s = brace();
    ref_line(&mut s, "XY", json!([[200, -50], [250, 0]]));
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "parallel_to": {"sketch": "Ref", "curve": "ref"}}));
    // Some side is now parallel to the line (its normal is perpendicular to it).
    assert!(brace_normals(&s).iter().any(|n| (n.x + n.y).abs() < 1e-9), "{:?}", brace_normals(&s));
    // The stored angle is the previous turn (0) plus a delta in (-90, 90].
    let solidcraft_doc::FeatureKind::FrameMemberTurn { angle, .. } = &turn_step(&s).kind else { panic!("not a turn") };
    let deg: f64 = angle.as_ref().trim_end_matches(" deg").trim_end_matches("deg").trim().parse().unwrap();
    assert!(deg > -90.0 - 1e-9 && deg <= 90.0 + 1e-9, "{angle}");
}

#[test]
fn parallel_to_refuses_an_end_face_a_curve_and_a_line_along_the_member() {
    let mut s = brace();
    ref_line(&mut s, "XY", json!([[200, -50], [200, 50]]));
    let h = 1.0 / 2f64.sqrt();
    let end = face_pick(&s, "Brace", [300.0, 18.5, 300.0], [h, 0.0, h]);
    let side = brace_side(&s, 0.0);
    let line = json!({"sketch": "Ref", "curve": "ref"});
    let e = s.execute("frame.rotate_member", &json!({"face": end, "parallel_to": line})).unwrap_err().to_string();
    assert!(e.contains("The member's face must be a flat side"), "{e}");
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Round"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "id": "ring"}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("frame.rotate_member", &json!({"face": side, "parallel_to": {"sketch": "Round", "curve": "ring"}})).unwrap_err().to_string();
    assert!(e.contains("Select a straight edge, a sketch line or an axis"), "{e}");
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Along"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [100, -100]], "ids": ["al"]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("frame.rotate_member", &json!({"face": side, "parallel_to": {"sketch": "Along", "curve": "al"}})).unwrap_err().to_string();
    assert!(e.contains("That line runs along the member: its sides are already parallel to it"), "{e}");
}
