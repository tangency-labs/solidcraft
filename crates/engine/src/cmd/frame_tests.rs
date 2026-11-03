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

// ---------------------------------------------------------------------------------------------
// Trim/Extend (frame.trim)

/// A 45 × 45 bar: its volume is 2025 mm² × its length.
fn bar45() -> Value {
    json!({"kind": "bar", "width": 45, "height": 45})
}

/// The section area of [`bar45`]: a member's volume is this × its length.
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

#[test]
fn a_side_given_must_face_the_member() {
    let mut s = Session::default();
    // A post along Y at x = 0 and a rail along X from x = 100 that falls short of it. The post's
    // profile x is world −X (y × d with y up and d along +Y), so its left side faces +X.
    let post = members_3d(&mut s, "Post", &[([0.0, -100.0, 0.0], [0.0, 100.0, 0.0])]);
    let rail = members_3d(&mut s, "Rail", &[([100.0, 0.0, 0.0], [300.0, 0.0, 0.0])]);
    // Its start extends to the post's side facing it (x = 22.5) by default, and so with that side.
    let f = run(&mut s, "frame.trim", json!({"members": rail, "to": post}))["feature"].clone();
    let left = json!({"members": rail, "to": [{"member": post[0], "side": "left"}]});
    run(&mut s, "timeline.redefine", json!({"feature": f, "command": "frame.trim", "params": left}));
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
    // The moved target's original sketch line is stale too: it must not silently trim
    // another member to the side where the target used to be.
    let err = s.execute("frame.trim", &json!({"members": [rails[2]], "to": [rails[0]]})).unwrap_err().to_string();
    assert!(err.contains("changed after"), "{err}");
    // Not while a sketch is open.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("frame.trim", &json!({"members": [rails[1]], "to": uprights})).is_err());
}

#[test]
fn a_rotated_square_member_is_not_rebuilt_even_when_its_bounds_match() {
    let mut s = Session::default();
    let m = members_3d(&mut s, "Bar", &[([0.0, 0.0, 0.0], [100.0, 0.0, 0.0])]);
    run(&mut s, "solid.move", json!({"bodies": m, "axis": [1, 0, 0], "angle": 90}));
    let saved = s.model.saved(&s.doc).unwrap();
    let doc = s.doc.to_json();
    assert!(solidcraft_doc::Model::adopt(&s.doc, &saved).is_ok(), "the new cache is adopted");
    let mut legacy = solidcraft_doc::SavedModel { records: saved.records.clone(), bodies: saved.bodies.clone() };
    for state in legacy.records["states"].as_array_mut().unwrap() {
        for member in state["frame_members"].as_array_mut().unwrap() {
            member.as_object_mut().unwrap().remove("unmodified");
        }
    }
    assert!(solidcraft_doc::Model::adopt(&s.doc, &legacy).is_err(), "old identity checks require reevaluation");
    for mut session in [
        s,
        Session::new(solidcraft_doc::Document::from_json(&doc).unwrap()),
        Session::opened(solidcraft_doc::Document::from_json(&doc).unwrap(), Some(&saved)),
        Session::opened(solidcraft_doc::Document::from_json(&doc).unwrap(), Some(&legacy)),
    ] {
        let body = session.model.state().body("Bar").unwrap().body.clone();
        let features = session.doc.features.len();
        let err = session
            .execute(
                "frame.trim",
                &json!({"members": m,
            "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("changed after"), "{err}");
        assert_eq!(session.doc.features.len(), features);
        assert!(body.same(&session.model.state().body("Bar").unwrap().body));
    }
}

#[test]
fn a_transferred_member_cannot_be_rebuilt_in_its_original_component_frame() {
    let mut s = Session::default();
    let m = members_3d(&mut s, "Bar", &[([0.0, 0.0, 0.0], [100.0, 0.0, 0.0])]);
    let c = run(&mut s, "component.create", json!({"name": "Child", "grounded": false}));
    let component = c["component"].as_u64().unwrap();
    run(&mut s, "component.move_bodies", json!({"component": component, "bodies": m}));
    run(&mut s, "occurrence.move", json!({"occurrence": c["occurrence"], "translate": [100, 0, 0], "capture": true}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let saved = s.model.saved(&s.doc).unwrap();
    let json = s.doc.to_json();
    assert!(solidcraft_doc::Model::adopt(&s.doc, &saved).is_ok());
    for mut session in [s, Session::opened(solidcraft_doc::Document::from_json(&json).unwrap(), Some(&saved))] {
        let body = session.model.state().body("Bar").unwrap().clone();
        assert!(body.transferred && body.component == component);
        let before = session.doc.to_json();
        let err = session
            .execute(
                "frame.trim",
                &json!({"members": m,
            "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("changed after"), "{err}");
        assert_eq!(session.doc.to_json(), before);
        let state = session.model.state();
        let after = state.body("Bar").unwrap();
        assert!(after.body.same(&body.body) && after.component == component && after.transferred);
    }
}

#[test]
fn trims_undo_reopen_and_keep_their_members_through_the_saved_model() {
    let mut s = Session::default();
    let (uprights, rails, cross) = tower(&mut s, 600.0);
    let full = s.doc.to_json();
    butt_tower(&mut s, &uprights, &rails, &cross);
    let before = s.doc.to_json();
    // Reopened from its JSON (evaluated again) and from its saved model (adopted as saved).
    let json = s.doc.to_json();
    let saved = s.model.saved(&s.doc).expect("a saved model");
    assert!(solidcraft_doc::Model::adopt(&s.doc, &saved).is_ok(), "the cache must actually be adopted");
    for adopted in [None, Some(&saved)] {
        let mut r = Session::opened(solidcraft_doc::Document::from_json(&json).unwrap(), adopted);
        assert_eq!(r.doc.to_json(), before);
        // The members are still known to a trim after reopening: the upright's top end moves
        // down to z = 1499.
        run(&mut r, "frame.trim", json!({"members": [uprights[0]], "to": [{"plane": {"origin": [0, 0, 1499], "normal": [0, 0, 1]}}]}));
    }
    // A model saved before members were recorded knows none: it is not adopted, the design is
    // evaluated again, and a trim still finds its members.
    let mut old = solidcraft_doc::SavedModel { records: saved.records.clone(), bodies: saved.bodies.clone() };
    for st in old.records["states"].as_array_mut().unwrap() {
        st.as_object_mut().unwrap().remove("frame_members");
    }
    let mut r = Session::opened(solidcraft_doc::Document::from_json(&json).unwrap(), Some(&old));
    run(&mut r, "frame.trim", json!({"members": [uprights[0]], "to": [{"plane": {"origin": [0, 0, 1499], "normal": [0, 0, 1]}}]}));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), full, "both trims undone");
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn a_renamed_trimmed_member_stays_trimmed() {
    let mut s = Session::default();
    let (uprights, rails, cross) = tower(&mut s, 600.0);
    butt_tower(&mut s, &uprights, &rails, &cross);
    run(&mut s, "body.rename", json!({"body": rails[0], "name": "Front rail"}));
    assert!(s.model.state().frame_member("Front rail").is_some());
    assert!(s.model.state().frame_member(&cross[0]).is_some());
}

#[test]
fn a_member_name_given_again_means_the_new_member() {
    // A member removed, and a new member along another line given its name.
    let mut s = Session::default();
    let old = members_3d(&mut s, "Rail", &[([0.0, 0.0, 0.0], [100.0, 0.0, 0.0])]);
    run(&mut s, "solid.remove", json!({"bodies": old}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Second"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 200, 0], [100, 200, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let new = made(&run(&mut s, "frame.member", json!({"sketch": "Second", "profile": bar45(), "body_name": "Rail"})));
    assert_eq!(old, new);
    // The name is the new member's: it extends to x = 150, where it runs.
    run(&mut s, "frame.trim", json!({"members": new, "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}));
    assert_eq!(s.model.state().frame_member(&new[0]).unwrap().body, new[0]);
}

#[test]
fn a_trimmed_member_keeps_its_face_names() {
    // What later features picked on a member stays picked when a trim goes in before them.
    let mut s = Session::default();
    let m = members_3d(&mut s, "Bar", &[([0.0, 0.0, 0.0], [100.0, 0.0, 0.0])]);
    let names = |s: &Session| solidcraft_doc::naming::face_names(s.model.state().body(&m[0]).unwrap()).to_vec();
    let before = names(&s);
    run(&mut s, "frame.trim", json!({"members": m, "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}));
    assert_eq!(names(&s), before);
}

#[test]
fn a_slanted_trim_keeps_each_side_face_attached_to_the_same_side() {
    let mut s = Session::default();
    let m = members_3d(&mut s, "Bar", &[([0.0, 0.0, 0.0], [100.0, 0.0, 0.0])]);
    let side_names = |s: &Session| {
        let st = s.model.state();
        let b = st.body("Bar").unwrap();
        solidcraft_doc::naming::face_names(b)
            .iter()
            .enumerate()
            .filter_map(|(k, n)| {
                let normal = b.body.planar_face(k)?.normal();
                (normal.x.abs() < 1e-9).then_some((n.clone(), normal))
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before = side_names(&s);
    assert_eq!(before.len(), 4);
    run(
        &mut s,
        "frame.trim",
        json!({"members": m,
        "to": {"plane": {"origin": [80, 0, 0], "normal": [1, 0, 1]}}}),
    );
    let after = side_names(&s);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for (name, normal) in before {
        assert!(after[&name].dot(normal) > 1.0 - 1e-9, "{name}: {:?} vs {normal:?}", after[&name]);
    }
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
        json!([{"plane": {"face": [0, 0, 0], "body": "Upright", "index": 1e300}}]),
        json!([{"plane_ref": {"type": "custom", "plane": {"origin": [0, 0, 0], "x_dir": [0, 0, 0], "y_dir": [0, 0, 0]}}}]),
        json!([{"plane_ref": {"type": "custom", "plane": {"origin": [1e308, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 1e-300, 0]}}}]),
        json!([{"plane_ref": {"type": "offset", "base": {"type": "origin", "name": "XY"}, "distance": "1e400 mm"}}]),
        json!([{"plane_ref": 7}]),
        json!([{"member": "nope", "side": "left"}]),
        json!([{"member": {"feature": "Member1"}, "side": "left"}]),
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

/// What a trim recipe must give: its bodies, their total volume and box, or a refusal.
enum Trimmed {
    Built { bodies: usize, volume: f64, bbox: Option<([f64; 3], [f64; 3])> },
    Refused(&'static str),
}

/// Trims land each member's ends where a hand-derived answer puts them: the members' total
/// volume and box after each recipe, worked out from where the ends must be, or the refusal the
/// last step must give. Member targets follow their lines through parameter edits, face and plane
/// targets follow the face or plane through moves, transfers and renames, and a world plane means
/// the same place for a member drawn in a moved component.
#[test]
fn trims_put_member_ends_where_the_hand_derived_answer_says() {
    use Trimmed::{Built, Refused};
    let cases = [
        (
            // x+y=0 and x-y=30 cross inside y in [-22.5,22.5].
            "crossing ends refused",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [30, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [0, 0, 0], "normal": [1, 1, 0]}}, "end": "start"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [30, 0, 0], "normal": [1, 1, 0]}}, "end": "end"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [30, 0, 0], "normal": [1, -1, 0]}}, "end": "end"}]
            ]),
            Refused("ends cross"),
        ),
        (
            // Mean length 150-(-20)=170 over 10x10 section.
            "extend start and slanted end",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [100, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 10, "height": 10}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 1]}}}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [-20, 0, 0], "normal": [1, 0, 0]}}, "end": "start"}]
            ]),
            Built { bodies: 1, volume: 17000.0, bbox: Some(([-20.0, -5.0, -5.0], [155.0, 5.0, 5.0])) },
        ),
        (
            // 10 squared section extends to x=150-2; V=100*148.
            "extend with gap",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [100, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 10, "height": 10}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}, "gap": 2}]
            ]),
            Built { bodies: 1, volume: 14800.0, bbox: Some(([0.0, -5.0, -5.0], [148.0, 5.0, 5.0])) },
        ),
        (
            // Plate V=200*200*30; post V=2025*(300-30).
            "face target follows edit",
            json!([
                ["parameters.add", {"name": "T", "expression": "10 mm"}],
                ["solid.box", {"length": 200, "width": 200, "height": "T", "corner": [0, 0, 0], "body_name": "Plate"}],
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 100, 100], [100, 100, 300]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["frame.trim", {"members": ["Post"], "to": {"plane": {"face": [50, 50, 10]}}}],
                ["parameters.change", {"name": "T", "expression": "30 mm"}]
            ]),
            Built { bodies: 2, volume: 1746750.0, bbox: Some(([0.0, 0.0, 0.0], [200.0, 200.0, 300.0])) },
        ),
        (
            // 2 posts of 400 mm plus a rail of 800-90=710 mm, all section 2025 mm2.
            "footprint parameter",
            json!([
                ["parameters.add", {"name": "W", "expression": "600 mm"}],
                ["sketch.create", {"plane": "XY", "name": "Posts"}],
                ["sketch.line", {"points": [[22.5, 0], [22.5, 400]]}],
                ["sketch.line", {"points": [["W - 22.5 mm", 0], ["W - 22.5 mm", 400]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "Posts", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["sketch.create", {"plane": "XY", "name": "Rail"}],
                ["sketch.line", {"points": [[22.5, 200], ["W - 22.5 mm", 200]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "Rail", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Rail"}],
                ["frame.trim", {"members": ["Rail"], "to": ["Post", "Post (2)"]}],
                ["parameters.change", {"name": "W", "expression": "800 mm"}]
            ]),
            Built { bodies: 3, volume: 3057750.0, bbox: Some(([0.0, 0.0, -22.5], [800.0, 400.0, 22.5])) },
        ),
        (
            // Post V=2025*200; rail V=2025*(300-32.5).
            "moved member face",
            json!([
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, -100, 0], [0, 100, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 0, 0], [300, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["solid.move", {"bodies": ["Post"], "translate": [10, 0, 0]}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"face": [32.5, 0, 0]}}}]
            ]),
            Built { bodies: 2, volume: 946687.5, bbox: None },
        ),
        (
            // 45 squared section, every section ray retains 30 mm; V=2025*30.
            "parallel slanted ends",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [30, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [0, 0, 0], "normal": [1, 1, 0]}}, "end": "start"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [30, 0, 0], "normal": [1, 1, 0]}}, "end": "end"}]
            ]),
            Built { bodies: 1, volume: 60750.0, bbox: Some(([-22.5, -22.5, -22.5], [52.5, 22.5, 22.5])) },
        ),
        (
            // Rail runs x=150..300; V=2025*(200+150).
            "renamed construction plane",
            json!([
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, -100, 0], [0, 100, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 0, 0], [300, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["construct.plane.offset", {"base": "YZ", "offset": 150, "name": "Cut"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": "Cut"}}],
                ["timeline.rename", {"feature": "Cut", "name": "Renamed cut"}]
            ]),
            Built { bodies: 2, volume: 708750.0, bbox: None },
        ),
        (
            // Replacement is 150 mm long at y=200, with section 2025 mm2.
            "reused member name",
            json!([
                ["sketch.create", {"plane": "XY", "name": "RailLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [100, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "RailLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Rail"}],
                ["solid.remove", {"bodies": ["Rail"]}],
                ["sketch.create", {"plane": "XY", "name": "ReplacementLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 200, 0], [100, 200, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "ReplacementLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Rail"}],
                ["frame.trim", {"members": ["Rail"], "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}]
            ]),
            Built { bodies: 1, volume: 303750.0, bbox: Some(([0.0, 177.5, -22.5], [150.0, 222.5, 22.5])) },
        ),
        (
            // Root sketch maps into component frame; world member stays on X, from 160 to 300 after trimming.
            "rotated component world plane",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 0, 0], [300, 0, 0]]}],
                ["sketch.finish", {}],
                ["component.create", {"name": "Frame", "grounded": false}],
                ["occurrence.move", {"occurrence": "Frame:1", "axis": [0, 0, 1], "angle": "90 deg", "translate": [40, 70, 0], "capture": true}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [160, 0, 0], "normal": [1, 0, 0]}}}]
            ]),
            Built { bodies: 1, volume: 283500.0, bbox: Some(([160.0, -22.5, -22.5], [300.0, 22.5, 22.5])) },
        ),
        (
            // A rotation changes body identity despite equal bounds of a square section.
            "rotated member refused",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [100, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["solid.move", {"bodies": ["Bar"], "axis": [1, 0, 0], "angle": 90}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}]
            ]),
            Refused("changed after"),
        ),
        (
            // x+z=150 over centred 10x10 section; mean length 150.
            "slanted end",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [100, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 10, "height": 10}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 1]}}}]
            ]),
            Built { bodies: 1, volume: 15000.0, bbox: Some(([0.0, -5.0, -5.0], [155.0, 5.0, 5.0])) },
        ),
        (
            // Transferred member cannot be rebuilt in its original component frame.
            "transferred member refused",
            json!([
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, 0, 0], [100, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["component.create", {"name": "Child", "grounded": false}],
                ["component.move_bodies", {"component": "Child", "bodies": ["Bar"]}],
                ["occurrence.move", {"occurrence": "Child:1", "translate": [100, 0, 0], "capture": true}],
                ["component.activate", {"component": "root"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"origin": [150, 0, 0], "normal": [1, 0, 0]}}}]
            ]),
            Refused("changed after"),
        ),
        (
            // Transfer retains post at x=0, rail ends at x=22.5; V=2025*(200+277.5).
            "transferred target face",
            json!([
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, -100, 0], [0, 100, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 0, 0], [300, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["component.create", {"name": "Child"}],
                ["component.move_bodies", {"component": "Child", "bodies": ["Post"]}],
                ["component.activate", {"component": "root"}],
                ["frame.trim", {"members": ["Bar"], "to": {"plane": {"face": [22.5, 0, 0]}}}]
            ]),
            Built { bodies: 2, volume: 966937.5, bbox: None },
        ),
        (
            // The root sketch line stays at world (100,100), z=100..300. A world plane at z=60 extends the post to length 240; V=2025*240. Component translation changes only the local representation.
            "translated component world plane",
            json!([
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 100, 100], [100, 100, 300]]}],
                ["sketch.finish", {}],
                ["component.create", {"name": "Frame", "grounded": false}],
                ["occurrence.move", {"occurrence": "Frame:1", "translate": [0, 0, 100], "capture": true}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["frame.trim", {"members": ["Post"], "to": {"plane": {"origin": [0, 0, 60], "normal": [0, 0, 1]}}}]
            ]),
            Built { bodies: 1, volume: 486000.0, bbox: Some(([77.5, 77.5, 60.0], [122.5, 122.5, 300.0])) },
        ),
        (
            // Post length 200, rail 300-22.5; V=2025*(200+277.5).
            "trim to member side",
            json!([
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, -100, 0], [0, 100, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 0, 0], [300, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": ["Post"]}]
            ]),
            Built { bodies: 2, volume: 966937.5, bbox: Some(([-22.5, -100.0, -22.5], [300.0, 100.0, 22.5])) },
        ),
        (
            // The far side normal points away from the rail.
            "wrong side refused",
            json!([
                ["sketch.create", {"plane": "XY", "name": "PostLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[0, -100, 0], [0, 100, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "PostLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Post"}],
                ["sketch.create", {"plane": "XY", "name": "BarLine"}],
                ["sketch.options", {"sketch_3d": true}],
                ["sketch.line3d", {"points": [[100, 0, 0], [300, 0, 0]]}],
                ["sketch.finish", {}],
                ["frame.member", {"sketch": "BarLine", "profile": {"kind": "bar", "width": 45, "height": 45}, "body_name": "Bar"}],
                ["frame.trim", {"members": ["Bar"], "to": {"member": "Post", "side": "right"}}]
            ]),
            Refused("faces away"),
        ),
    ];
    for (what, steps, want) in cases {
        let mut s = Session::default();
        let steps = steps.as_array().unwrap();
        let (last, before) = steps.split_last().unwrap();
        for step in before {
            run(&mut s, step[0].as_str().unwrap(), step[1].clone());
        }
        match want {
            Refused(why) => {
                let err = s.execute(last[0].as_str().unwrap(), &last[1]).unwrap_err().to_string();
                assert!(err.contains(why), "{what}: {err}");
            }
            Built { bodies: count, volume, bbox } => {
                run(&mut s, last[0].as_str().unwrap(), last[1].clone());
                let got = bodies(&mut s);
                assert_eq!(got.len(), count, "{what}: {got:?}");
                let v: f64 = got.iter().map(|b| b.1).sum();
                assert!(rel(v, volume) < 1e-6, "{what}: volume {v}, want {volume}");
                if let Some((lo, hi)) = bbox {
                    let min = got.iter().fold([f64::INFINITY; 3], |m, b| std::array::from_fn(|i| m[i].min(b.2[i])));
                    let max = got.iter().fold([f64::NEG_INFINITY; 3], |m, b| std::array::from_fn(|i| m[i].max(b.3[i])));
                    assert!(near3(min, lo) && near3(max, hi), "{what}: box {min:?} {max:?}, want {lo:?} {hi:?}");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Corners (frame.corner)

/// A frame of 45 × 45 bar on XY round a W × 400 rectangle (W a parameter, 600 mm), its lines
/// through the bars' middles, each from the corner before it: `Bar` along +X at y = 0, `Bar (2)`
/// along +Y at x = W, `Bar (3)` along −X at y = 400, `Bar (4)` along −Y at x = 0.
fn square_frame(s: &mut Session) -> Vec<String> {
    run(s, "parameters.add", json!({"name": "W", "expression": "600 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.line", json!({"points": [[0, 0], ["W", 0]]}));
    run(s, "sketch.line", json!({"points": [["W", 0], ["W", 400]]}));
    run(s, "sketch.line", json!({"points": [["W", 400], [0, 400]]}));
    run(s, "sketch.line", json!({"points": [[0, 400], [0, 0]]}));
    run(s, "sketch.finish", json!({}));
    made(&run(s, "frame.member", json!({"sketch": "Frame", "profile": bar45(), "body_name": "Bar"})))
}

/// Mitre each corner of the square frame, the cut faces `gap` apart; the corner features.
fn mitre_square(s: &mut Session, bars: &[String], gap: f64) -> Vec<Value> {
    (0..4).map(|k| run(s, "frame.corner", json!({"members": [bars[k], bars[(k + 1) % 4]], "gap": gap}))["feature"].clone()).collect()
}

/// The angle (degrees) between the line of member `name`, along `dir`, and the normal of its cut
/// at `end` (0 its start, 1 its end): 0 for a square end, 45 for a mitre of a right angle.
fn cut_angle(s: &Session, name: &str, end: usize, dir: [f64; 3]) -> f64 {
    let st = s.model.state();
    let pl = st.frame_member(name).unwrap().ends[end].unwrap();
    let d = solidcore_record::Vec3::new(dir[0], dir[1], dir[2]).normalized().unwrap();
    pl.normal().dot(d).abs().min(1.0).acos().to_degrees()
}

#[test]
fn a_square_frame_mitres_at_its_four_corners() {
    let mut s = Session::default();
    let bars = square_frame(&mut s);
    // Before: each bar runs square from corner to corner, so they overlap 45 × 45 × 45 at each.
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [0.0, -22.5, -22.5]) && near3(bs[&bars[0]].1, [600.0, 22.5, 22.5]), "{:?}", bs[&bars[0]]);
    let r = run(&mut s, "frame.corner", json!({"members": [bars[0], bars[1]]}));
    assert_eq!(r["changed_bodies"], json!([bars[0], bars[1]]), "{r}");
    for k in 1..4 {
        run(&mut s, "frame.corner", json!({"members": [bars[k], bars[(k + 1) % 4]]}));
    }
    // After: each bar reaches the outer corners, 22.5 past its line's ends, and stops at the inner
    // ones, 22.5 short of them. A plane cut through the middle line's end leaves the volume of the
    // section times the line's length: 2025 × (2 × 600 + 2 × 400).
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [-22.5, -22.5, -22.5]) && near3(bs[&bars[0]].1, [622.5, 22.5, 22.5]), "{:?}", bs[&bars[0]]);
    assert!(near3(bs[&bars[1]].0, [577.5, -22.5, -22.5]) && near3(bs[&bars[1]].1, [622.5, 422.5, 22.5]), "{:?}", bs[&bars[1]]);
    assert!(near3(bs[&bars[2]].0, [-22.5, 377.5, -22.5]) && near3(bs[&bars[2]].1, [622.5, 422.5, 22.5]), "{:?}", bs[&bars[2]]);
    assert!(near3(bs[&bars[3]].0, [-22.5, -22.5, -22.5]) && near3(bs[&bars[3]].1, [22.5, 422.5, 22.5]), "{:?}", bs[&bars[3]]);
    for b in bodies(&mut s) {
        let want = BAR45 * if b.0 == bars[0] || b.0 == bars[2] { 600.0 } else { 400.0 };
        assert!(rel(b.1, want) < MEASURED, "{b:?} vs {want}");
    }
    // Both ends of every bar are cut at 45° to its line.
    for (bar, dir) in bars.iter().zip([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]) {
        for end in 0..2 {
            let a = cut_angle(&s, bar, end, dir);
            assert!((a - 45.0).abs() < 1e-9, "{bar} end {end}: {a}°");
        }
    }
    // The mitred bars don't overlap: joined, they keep the whole volume, 2025 × 2000.
    run(&mut s, "solid.combine", json!({"target": bars[0], "tools": [bars[1], bars[2], bars[3]]}));
    let v = total_volume(&mut s);
    assert!(rel(v, BAR45 * 2000.0) < MEASURED, "{v}");
}

#[test]
fn a_mitre_gap_shortens_each_bar_by_its_share_at_the_middle_line() {
    let mut s = Session::default();
    let bars = square_frame(&mut s);
    mitre_square(&mut s, &bars, 3.0);
    // The cut faces stand 3 apart, each 1.5 back from the mitre plane along its normal, which
    // leans 45° from the line: each end moves 1.5 / cos 45° = 1.5 √2 along it. A bar is
    // 2 × 1.5 √2 = 3 √2 shorter at its middle line, and at its outer edge.
    let e = 1.5 * 2f64.sqrt();
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [-22.5 + e, -22.5, -22.5]) && near3(bs[&bars[0]].1, [622.5 - e, 22.5, 22.5]), "{:?}", bs[&bars[0]]);
    for b in bodies(&mut s) {
        let len = if b.0 == bars[0] || b.0 == bars[2] { 600.0 } else { 400.0 };
        let want = BAR45 * (len - 2.0 * e);
        assert!(rel(b.1, want) < MEASURED, "{b:?} vs {want}");
    }
}

#[test]
fn mitres_follow_a_wider_frame() {
    let mut s = Session::default();
    let bars = square_frame(&mut s);
    mitre_square(&mut s, &bars, 0.0);
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "800 mm"}));
    // The long bars grow to 800 + 45 outside; the right bar moves with its line.
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [-22.5, -22.5, -22.5]) && near3(bs[&bars[0]].1, [822.5, 22.5, 22.5]), "{:?}", bs[&bars[0]]);
    assert!(near3(bs[&bars[1]].0, [777.5, -22.5, -22.5]) && near3(bs[&bars[1]].1, [822.5, 422.5, 22.5]), "{:?}", bs[&bars[1]]);
    let v = total_volume(&mut s);
    assert!(rel(v, BAR45 * (2.0 * 800.0 + 2.0 * 400.0)) < MEASURED, "{v}");
}

#[test]
fn a_sixty_degree_corner_cuts_each_bar_at_sixty_degrees() {
    // Two bars 200 long from the origin, along X and at 60° to it in XY.
    let mut s = Session::default();
    let (c, sn) = (100.0, 100.0 * 3f64.sqrt());
    let bars = members_3d(&mut s, "Bar", &[([0.0, 0.0, 0.0], [200.0, 0.0, 0.0]), ([0.0, 0.0, 0.0], [c, sn, 0.0])]);
    let f = run(&mut s, "frame.corner", json!({"members": bars}))["feature"].clone();
    // The corner plane holds the bisector, at 30° to each bar, so each cut leans 90° − 30° = 60°
    // from square. Across the 45 wide bar along X the cut runs from x = −22.5 / tan 30° (the outer
    // edge, y = −22.5) to +22.5 / tan 30° (the inner one): 22.5 √3 either side of the corner.
    assert!((cut_angle(&s, &bars[0], 0, [1.0, 0.0, 0.0]) - 60.0).abs() < 1e-9);
    assert!((cut_angle(&s, &bars[1], 0, [c, sn, 0.0]) - 60.0).abs() < 1e-9);
    let o = 22.5 * 3f64.sqrt();
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [-o, -22.5, -22.5]) && near3(bs[&bars[0]].1, [200.0, 22.5, 22.5]), "{:?}", bs[&bars[0]]);
    // The cut passes through the middle line's end: each bar keeps 2025 × 200.
    for b in bodies(&mut s) {
        assert!(rel(b.1, BAR45 * 200.0) < MEASURED, "{b:?}");
    }
    // A gap of 4: each cut face 2 back along the normal, which leans 60° from the line, so the end
    // moves 2 / cos 60° = 4 along it; each bar keeps 2025 × 196.
    let p = json!({"members": bars, "gap": "4 mm"});
    run(&mut s, "timeline.redefine", json!({"feature": f, "command": "frame.corner", "params": p}));
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [-o + 4.0, -22.5, -22.5]), "{:?}", bs[&bars[0]]);
    for b in bodies(&mut s) {
        assert!(rel(b.1, BAR45 * 196.0) < MEASURED, "{b:?}");
    }
}

#[test]
fn a_corner_extends_members_that_stop_short_of_it() {
    // Lines that stop 10 short of the corner at (0, 0): the bars grow to it, as a trim extends.
    let mut s = Session::default();
    let bars = members_3d(&mut s, "Bar", &[([10.0, 0.0, 0.0], [300.0, 0.0, 0.0]), ([0.0, 10.0, 0.0], [0.0, 300.0, 0.0])]);
    run(&mut s, "frame.corner", json!({"members": bars}));
    let bs = boxes(&mut s);
    assert!(near3(bs[&bars[0]].0, [-22.5, -22.5, -22.5]) && near3(bs[&bars[0]].1, [300.0, 22.5, 22.5]), "{:?}", bs[&bars[0]]);
    for b in bodies(&mut s) {
        assert!(rel(b.1, BAR45 * 300.0) < MEASURED, "{b:?}");
    }
}

/// Two lines `len` long meeting at the origin `deg` apart, one along −X into it and one out of it,
/// with 45 × 45 bar anchored by its bottom left corner: each bar lies outside the angle, its line
/// along its inner edge, and the bars' bottoms on Z 0.
fn outline_corner(s: &mut Session, deg: f64, len: f64) -> Vec<String> {
    let (c, sn) = (deg.to_radians().cos(), deg.to_radians().sin());
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.line", json!({"points": [[len, 0.0], [0.0, 0.0]]}));
    run(s, "sketch.line", json!({"points": [[0.0, 0.0], [len * c, len * sn]]}));
    run(s, "sketch.finish", json!({}));
    made(&run(s, "frame.member", json!({"sketch": "Frame", "profile": bar45(), "anchor": "bottom_left", "body_name": "Bar"})))
}

#[test]
fn acute_corners_of_bars_on_their_inner_edges_mitre() {
    // #8452: 30° apart, the middle lines (22.5 out from the lines, on the outside) cross on the
    // bisector 22.5 / sin 15° from the origin, 22.5 · cot 15° = 83.971 past the lines' ends,
    // more than the bar's diagonal √2 · 45 = 63.640.
    for deg in [15.0, 30.0, 60.0] {
        let mut s = Session::default();
        let bars = outline_corner(&mut s, deg, 200.0);
        run(&mut s, "frame.corner", json!({"members": bars}));
        // The cut holds the bisector, at θ/2 to each line, so it leans 90° − θ/2 from square.
        let lean = 90.0 - deg / 2.0;
        let r: f64 = deg.to_radians();
        assert!((cut_angle(&s, &bars[0], 1, [1.0, 0.0, 0.0]) - lean).abs() < 1e-9, "{deg}");
        assert!((cut_angle(&s, &bars[1], 0, [r.cos(), r.sin(), 0.0]) - lean).abs() < 1e-9, "{deg}");
        // The bisector runs through the origin, where the inner edges (the lines) end; the outer
        // edge, 45 out, ends 45 · cot(θ/2) past it, and the middle line 22.5 · cot(θ/2): each
        // bar keeps 2025 × (200 + 22.5 · cot(θ/2)).
        let cot = 1.0 / (r / 2.0).tan();
        let bs = boxes(&mut s);
        assert!(near3(bs[&bars[0]].0, [-45.0 * cot, -45.0, 0.0]) && near3(bs[&bars[0]].1, [200.0, 0.0, 45.0]), "{deg}: {:?}", bs[&bars[0]]);
        for b in bodies(&mut s) {
            assert!(rel(b.1, BAR45 * (200.0 + 22.5 * cot)) < MEASURED, "{deg}: {b:?}");
        }
    }
}

#[test]
fn an_acute_t_is_still_refused() {
    // A bar from the middle of another, 30° to it: the lines meet 150 from either end of the
    // first, far more than its section's size even after the 83.971 its outline anchor moves the
    // corner by.
    let mut s = Session::default();
    let (c, sn) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[300.0, 0.0], [0.0, 0.0]]}));
    run(&mut s, "sketch.line", json!({"points": [[150.0, 0.0], [150.0 + 200.0 * c, 200.0 * sn]]}));
    run(&mut s, "sketch.finish", json!({}));
    let bars = made(&run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": bar45(), "anchor": "bottom_left", "body_name": "Bar"})));
    let err = s.execute("frame.corner", &json!({"members": bars})).unwrap_err().to_string();
    assert!(err.contains("doesn't end at the corner"), "{err}");
}

#[test]
fn bad_corners_are_refused_with_the_reason() {
    let mut s = Session::default();
    let bars = square_frame(&mut s);
    // Two rails side by side, a rail far above the frame, and a post standing in the middle of
    // the bottom bar.
    let side = members_3d(&mut s, "Side", &[([0.0, 100.0, 0.0], [600.0, 100.0, 0.0])]);
    let high = members_3d(&mut s, "High", &[([600.0, 200.0, 300.0], [900.0, 200.0, 300.0])]);
    let post = members_3d(&mut s, "Post", &[([300.0, 0.0, 0.0], [300.0, 0.0, 500.0])]);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Block"}));
    for (p, why) in [
        (json!({}), "`members`"),
        (json!({"members": [bars[0]]}), "`members`"),
        (json!({"members": [bars[0], bars[1], bars[2]]}), "`members`"),
        (json!({"members": bars[0]}), "`members`"),
        (json!({"members": [bars[0], bars[0]]}), "twice"),
        (json!({"members": [bars[0], {"feature": "Member1", "line": "l1"}]}), "twice"),
        (json!({"members": [bars[0], "Block"]}), "not a frame member"),
        (json!({"members": [bars[0], bars[1]], "kind": "butt"}), "`kind`"),
        (json!({"members": [bars[0], bars[1]], "gap": -1}), "`gap`"),
        (json!({"members": [bars[0], bars[1]], "gap": 1001}), "`gap`"),
        (json!({"members": [bars[0], bars[1]], "gap": "wide"}), "gap"),
        // Parallel, both ways round.
        (json!({"members": [bars[0], side[0]]}), "parallel"),
        (json!({"members": [bars[0], bars[2]]}), "parallel"),
        // Crossing the right bar's line from 300 above it: they never touch.
        (json!({"members": [bars[1], high[0]]}), "don't meet"),
        // The post stands on the bottom bar 300 from either of its ends.
        (json!({"members": [bars[0], post[0]]}), "doesn't end at the corner"),
    ] {
        let err = s.execute("frame.corner", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
    }
    // A member a later feature changed is not rebuilt.
    run(&mut s, "solid.move", json!({"bodies": [bars[0]], "translate": [0, 0, 5]}));
    let err = s.execute("frame.corner", &json!({"members": [bars[0], bars[1]]})).unwrap_err().to_string();
    assert!(err.contains("changed after"), "{err}");
    // Not while a sketch is open.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("frame.corner", &json!({"members": [bars[1], bars[2]]})).is_err());
}

#[test]
fn corners_undo_and_reopen_with_their_members() {
    let mut s = Session::default();
    let bars = square_frame(&mut s);
    mitre_square(&mut s, &bars, 2.0);
    let want = total_volume(&mut s);
    run(&mut s, "edit.undo", json!({}));
    // The last corner taken back: the last bar's end and the first bar's start square again.
    assert!(near3(boxes(&mut s)[&bars[3]].0, [-22.5, 0.0, -22.5]), "{:?}", boxes(&mut s)[&bars[3]]);
    assert!(near3(boxes(&mut s)[&bars[0]].0, [0.0, -22.5, -22.5]), "{:?}", boxes(&mut s)[&bars[0]]);
    run(&mut s, "edit.redo", json!({}));
    assert!(rel(total_volume(&mut s), want) < 1e-12);
    // Saved and opened again, the corners hold.
    let mut t = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(rel(total_volume(&mut t), want) < MEASURED);
}

#[test]
fn hostile_corners_are_refused_not_crashed() {
    let mut s = Session::default();
    let bars = square_frame(&mut s);
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
        json!([null, null]),
        json!([{"feature": 1e300, "line": []}, {"feature": -1, "line": "l1"}]),
        json!([{"feature": "Member1", "line": "x".repeat(10_000)}, bars[0]]),
        json!(["mitre", "miter"]),
        json!("x".repeat(100_000)),
    ];
    let good = json!({"members": [bars[0], bars[1]]});
    for key in ["members", "kind", "gap"] {
        for v in &hostile {
            let mut p = good.clone();
            p[key] = v.clone();
            match s.execute("frame.corner", &p) {
                Err(crate::EngineError::Internal(id, msg)) => panic!("{id} {p}: {msg}"),
                Ok(_) => {
                    run(&mut s, "edit.undo", json!({}));
                }
                Err(_) => {}
            }
        }
    }
}
