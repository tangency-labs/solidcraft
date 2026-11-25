#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_doc::frame::Anchor;
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

/// A member's body name counts characters, as the refusal says: a hundred `ł` (200 bytes) is
/// taken, 129 characters are not.
#[test]
fn member_body_names_count_characters_not_bytes() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let long = "ł".repeat(100);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "body_name": long}));
    assert_eq!(bodies(&mut s)[0].0, long);
    let error = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "body_name": "ł".repeat(129)})).unwrap_err().to_string();
    assert!(error.contains("1…128 characters"), "{error}");
    // So does each name in `names`.
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "names": {"l1": long}}));
    let error = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "names": {"l1": "ł".repeat(129)}})).unwrap_err().to_string();
    assert!(error.contains("1…128 characters"), "{error}");
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
fn the_section_view_shows_where_the_member_goes_and_its_anchors() {
    let rail = || {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        s
    };
    let angle = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    let s = rail();
    let v = super::member_section_view(&s, &json!({"sketch": "Rail", "profile": angle, "anchor": "bottom_left"})).unwrap();
    let w = |p: Vec2| {
        let q = v.world(p);
        [q.x, q.y, q.z]
    };
    // The anchored point sits on the line's start; the section's x runs world +Y, its y +Z.
    assert!(near3(w(v.at), [0.0, 0.0, 0.0]) && near3([v.end.x, v.end.y, v.end.z], [1000.0, 0.0, 0.0]), "{v:?}");
    let anchors = v.anchors();
    assert_eq!(anchors.len(), 11);
    let at = |a: Anchor| anchors.iter().find(|(k, _)| *k == a).map(|(_, p)| *p).unwrap();
    assert!(near3(w(at(Anchor::TopRight)), [0.0, 60.0, 40.0]), "{anchors:?}");
    assert!(near3(w(at(Anchor::Top)), [0.0, 30.0, 40.0]), "{anchors:?}");
    // A world point back to the section point it came from.
    let p = at(Anchor::Right);
    let back = v.section_point(v.world(p));
    assert!((back.x - p.x).abs() < 1e-9 && (back.y - p.y).abs() < 1e-9);

    // Mirrored, a picked section point turned into `locate` is the point the made member puts on
    // the line: its top right on the line, the member runs to −Y and −Z of it.
    let mut s = rail();
    let mirrored = json!({"sketch": "Rail", "profile": angle, "mirror": true});
    let v = super::member_section_view(&s, &mirrored).unwrap();
    let top_right = v.anchors().into_iter().find(|(k, _)| *k == Anchor::TopRight).map(|(_, p)| p).unwrap();
    let mut p = mirrored.clone();
    p["locate"] = json!(v.locate_of(top_right));
    let again = super::member_section_view(&s, &p).unwrap();
    assert!((again.at.x - top_right.x).abs() < 1e-9 && (again.at.y - top_right.y).abs() < 1e-9, "{again:?}");
    // And back: the section point a `locate` puts on the line, mirrored or not.
    for view in [&v, &again] {
        let back = view.locate_point(view.locate_of(top_right));
        assert!((back.x - top_right.x).abs() < 1e-9 && (back.y - top_right.y).abs() < 1e-9, "{back:?}");
    }
    let back = again.locate_point(v.locate_of(top_right));
    assert!((back.x - again.at.x).abs() < 1e-9 && (back.y - again.at.y).abs() < 1e-9, "{back:?}");
    run(&mut s, "frame.member", p);
    let b = bodies(&mut s).remove(0);
    assert!(near3(b.2, [0.0, -60.0, -40.0]) && near3(b.3, [1000.0, 0.0, 0.0]), "{b:?}");

    // What the command would refuse shows nothing, with the reason; nothing is added.
    let s = rail();
    for bad in [
        json!({}),
        json!({"sketch": "Rail"}),
        json!({"sketch": "Nope", "profile": tube()}),
        json!({"sketch": "Rail", "profile": {"kind": "angle", "width": -1}}),
    ] {
        assert!(super::member_section_view(&s, &bad).is_err(), "{bad}");
    }
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })));
}

#[test]
fn the_section_view_reads_bare_profile_numbers_in_the_design_units() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let v = super::member_section_view(&s, &json!({"sketch": "Rail", "profile": {"kind": "bar", "width": 2, "height": 1}})).unwrap();
    let anchors = v.anchors();
    let at = |a: Anchor| v.world(anchors.iter().find(|(k, _)| *k == a).map(|(_, p)| *p).unwrap());
    // The member made from these values is 2 in across (world Y) and 1 in up (world Z).
    let (lo, hi) = (at(Anchor::BottomLeft), at(Anchor::TopRight));
    assert!((hi.y - lo.y - 50.8).abs() < 1e-6 && (hi.z - lo.z - 25.4).abs() < 1e-6, "{anchors:?}");
}

#[test]
fn the_section_view_sits_at_the_active_occurrence_with_its_pending_move() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Frame parts"}));
    let component = c["component"].as_u64().unwrap();
    let first = c["occurrence"].as_u64().unwrap();
    let second = run(&mut s, "occurrence.copy", json!({"occurrence": first}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [20, 30, 40], "capture": true}));
    run(&mut s, "component.activate", json!({"component": component, "occurrence": second, "path": [second]}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    // Not captured: shown where it was dragged, not yet stored.
    run(&mut s, "occurrence.move", json!({"occurrence": second, "translate": [5, 0, 0], "capture": false}));
    let v = super::member_section_view(&s, &json!({"sketch": "Rail", "profile": tube()})).unwrap();
    // The line as the second occurrence shows it: moved by [20, 30, 40] and then [5, 0, 0].
    assert!(near3([v.start.x, v.start.y, v.start.z], [25.0, 30.0, 40.0]), "{v:?}");
    assert!(near3([v.end.x, v.end.y, v.end.z], [1025.0, 30.0, 40.0]), "{v:?}");
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
    let _arc_id = arc["created"]["curves"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Points only"}));
    run(&mut s, "sketch.point", json!({"point": [7, 9]}));
    run(&mut s, "sketch.finish", json!({}));
    for (p, why) in [
        (json!({"sketch": "Frame"}), "`profile`"),
        (json!({"sketch": "Frame", "profile": tube(), "profile_sketch": "Frame"}), "one of them"),
        (json!({"sketch": "Frame", "profile_sketch": "Frame"}), "must not be the frame sketch"),
        (json!({"sketch": "Nowhere", "profile": tube()}), "no sketch"),
        (json!({"sketch": "Frame", "profile": {"kind": "rect_tube", "width": 10, "height": 10, "wall": 5}}), "wall"),
        (json!({"sketch": "Frame", "profile": tube(), "lines": []}), "`lines`"),
        (json!({"sketch": "Frame", "profile": tube(), "lines": ["nope"]}), "nope"),
        (json!({"sketch": "Arc", "profile": tube()}), "no lines"),
        (json!({"sketch": "Points only", "profile": tube()}), "no lines"),
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
    // Starting the member tool commits the open sketch first.
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let reply = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube()})).unwrap();
    assert!(reply.get("finished_sketch").is_some());
    assert!(s.active_sketch.is_none());
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

/// The first two lines of the rectangle frame, by id.
fn two_lines(s: &Session) -> (String, String) {
    let l = sketch_lines(s, "Frame");
    (l[0].0.clone(), l[1].0.clone())
}

#[test]
fn corner_settings_are_taken_and_saved() {
    use solidcraft_doc::frame::{CornerOverride, CornerType, MemberRef};
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (l1, l2) = two_lines(&s);
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "flip": true,
               "corners": [{"members": [l1, l2], "corner": "mitre", "flip": false}]}),
    );
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let kind = &reopened.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().kind;
    let solidcraft_doc::FeatureKind::FrameMember { corner, flip, corners, .. } = kind else { panic!("not a frame member") };
    assert_eq!((*corner, *flip), (CornerType::Butt, true));
    // Saved in the order today's rules lay them (the longer l1 runs through), with no `flip`.
    let members = vec![MemberRef { feature: None, line: l1 }, MemberRef { feature: None, line: l2 }];
    assert_eq!(*corners, [CornerOverride { members, kind: CornerType::Mitre, gap: None, coped: None }]);
}

#[test]
fn bad_corner_settings_are_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (l1, l2) = two_lines(&s);
    let ok = json!({"members": [l1, l2], "corner": "mitre"});
    let many: Vec<Value> = (0..=solidcraft_doc::frame::MAX_MEMBERS).map(|_| ok.clone()).collect();
    let cases = [
        ("corner", json!({"corner": "round"})),
        ("flip", json!({"flip": "yes"})),
        ("corners", json!({"corners": "mitre"})),
        ("corners", json!({"corners": [{"members": [l1], "corner": "mitre"}]})),
        ("corners", json!({"corners": [{"members": [l1, "nope"], "corner": "mitre"}]})),
        ("corners", json!({"corners": [{"members": [l1, l1], "corner": "mitre"}]})),
        ("corners", json!({"corners": [ok, {"members": [l2, l1], "corner": "butt"}]})),
        ("corners", json!({"corners": many})),
    ];
    for (key, extra) in cases {
        let mut p = json!({"sketch": "Frame", "profile": tube()});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let e = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(e.contains(&format!("`{key}`")), "{extra:.80}: {e}");
    }
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })));
}

#[test]
fn a_design_without_corner_settings_builds_as_before() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 4);
    assert!(bs.iter().any(|b| near3(b.2, [0.0, -20.0, -20.0]) && near3(b.3, [600.0, 20.0, 20.0])), "{bs:?}");
    let json = s.doc.to_json();
    for key in ["\"corner\"", "\"flip\"", "\"corners\""] {
        assert!(!json.contains(key), "{key} saved");
    }
}

/// A frame sketch `Y` of three lines from the origin, by id.
fn three_at_a_point(s: &mut Session) -> [String; 3] {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Y"}));
    for end in [[600, 0], [0, 400], [-300, -300]] {
        run(s, "sketch.line", json!({"points": [[0, 0], end]}));
    }
    run(s, "sketch.finish", json!({}));
    let l = sketch_lines(s, "Y");
    [l[0].0.clone(), l[1].0.clone(), l[2].0.clone()]
}

#[test]
fn auto_and_fit_ends_are_taken_and_saved() {
    use solidcraft_doc::frame::CornerType;
    let mut s = Session::default();
    let ids = three_at_a_point(&mut s);
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Y", "profile": tube(), "corner": "auto", "fit_ends": true,
               "corners": [{"members": ids, "corner": "auto", "flip": true}]}),
    );
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let kind = &reopened.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().kind;
    let solidcraft_doc::FeatureKind::FrameMember { corner, fit_ends, corners, .. } = kind else { panic!("not a frame member") };
    assert_eq!((*corner, *fit_ends), (CornerType::Auto, true));
    // Flip saved as the order: the corner's two laid first, the 600 and 424 long ones, swapped (the 424 one runs
    // through), the 400 one after them.
    assert_eq!(corners.len(), 1);
    assert_eq!((corners[0].lines().collect::<Vec<_>>(), corners[0].kind), (vec![&*ids[2], &*ids[0], &*ids[1]], CornerType::Auto));
}

#[test]
fn an_old_two_member_override_still_loads() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (l1, l2) = two_lines(&s);
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "corners": [{"members": [l1, l2], "corner": "butt"}]}),
    );
    // The design's override as a stage 1 build wrote it, by hand: two ids and no `flip`.
    let old: Value = serde_json::from_str(&format!(r#"[{{"members":["{l1}","{l2}"],"corner":"mitre"}}]"#)).unwrap();
    fn put(v: &mut Value, old: &Value) -> bool {
        match v {
            Value::Object(o) if o.contains_key("corners") => {
                o.insert("corners".into(), old.clone());
                true
            }
            Value::Object(o) => o.values_mut().any(|v| put(v, old)),
            Value::Array(a) => a.iter_mut().any(|v| put(v, old)),
            _ => false,
        }
    }
    let mut doc: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert!(put(&mut doc, &old), "no corners in {doc}");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&doc.to_string()).unwrap());
    let kind = &reopened.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().kind;
    let solidcraft_doc::FeatureKind::FrameMember { corners, .. } = kind else { panic!("not a frame member") };
    assert_eq!((corners[0].lines().collect::<Vec<_>>(), corners[0].kind), (vec![&*l1, &*l2], solidcraft_doc::frame::CornerType::Mitre));
    assert_eq!(bodies(&mut reopened).len(), 4);
}

#[test]
fn bad_auto_settings_are_refused() {
    let mut s = Session::default();
    let ids = three_at_a_point(&mut s);
    let many: Vec<String> = (0..=solidcraft_doc::frame::MAX_MEMBERS).map(|k| format!("l{k}")).collect();
    let cases = [
        ("fit_ends", json!({"fit_ends": "yes"})),
        ("corners", json!({"corners": [{"members": [ids[0]], "corner": "auto"}]})),
        ("corners", json!({"corners": [{"members": [ids[0], ids[1], ids[0]], "corner": "auto"}]})),
        ("corners", json!({"corners": [{"members": many, "corner": "auto"}]})),
        ("corner", json!({"corner": "automatic"})),
    ];
    for (key, extra) in cases {
        let mut p = json!({"sketch": "Y", "profile": tube()});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let e = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(e.contains(&format!("`{key}`")), "{extra:.80}: {e}");
    }
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })));
}

#[test]
fn a_design_without_fit_ends_saves_without_it() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "fit_ends": false}));
    assert!(!s.doc.to_json().contains("fit_ends"));
}

#[test]
fn a_three_tube_override_on_a_mitre_frame_is_refused() {
    let mut s = Session::default();
    let ids = three_at_a_point(&mut s);
    let p = json!({"sketch": "Y", "profile": tube(), "corner": "mitre", "corners": [{"members": ids, "corner": "butt"}]});
    let e = s.execute("frame.member", &p).unwrap_err().to_string();
    assert!(e.contains("`corners`"), "{e}");
    run(&mut s, "frame.member", json!({"sketch": "Y", "profile": tube(), "corner": "auto", "corners": [{"members": ids, "corner": "butt"}]}));
}

#[test]
fn a_hand_edited_three_tube_override_on_a_mitre_frame_fails() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let l = sketch_lines(&s, "Frame");
    let (a, b, c) = (&l[0].0, &l[1].0, &l[2].0);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "corners": [{"members": [a, b], "corner": "butt"}]}));
    let member = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let json = serde_json::from_str::<Value>(&s.doc.to_json()).unwrap().to_string();
    let from = format!(r#""members":[{{"line":"{a}"}},{{"line":"{b}"}}]"#);
    assert!(json.contains(&from), "{json}");
    let edited = json.replace(&from, &format!(r#""members":[{{"line":"{a}"}},{{"line":"{b}"}},{{"line":"{c}"}}]"#));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&edited).unwrap());
    let e = reopened.model.result(member).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("A corner override is not at a corner of the frame"), "{e}");
}

/// A frame sketch `V` of the lines through `points` (one line per pair of neighbours), with tube
/// members finished `corner`.
fn polyline_frame(points: Value, corner: &str) -> (Session, Result<Value, String>) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
    run(&mut s, "sketch.line", json!({"points": points}));
    run(&mut s, "sketch.finish", json!({}));
    let r = s.execute("frame.member", &json!({"sketch": "V", "profile": tube(), "corner": corner})).map_err(|e| e.to_string());
    (s, r)
}

#[test]
fn a_mitred_rectangle_meets_at_its_corners() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 4, "{bs:?}");
    // The bottom member runs out to the outer corners, and keeps its centreline's volume.
    let bottom = bs.iter().find(|b| near3(b.2, [-20.0, -20.0, -20.0]) && near3(b.3, [620.0, 20.0, 20.0])).unwrap_or_else(|| panic!("{bs:?}"));
    assert!(rel(bottom.1, tube_area() * 600.0) < MEASURED, "{bottom:?} vs {}", tube_area() * 600.0);
    assert!(bs.iter().any(|b| near3(b.2, [-20.0, -20.0, -20.0]) && near3(b.3, [20.0, 420.0, 20.0])), "{bs:?}");
    assert!(rel(total_volume(&mut s), tube_area() * 2000.0) < MEASURED);
}

#[test]
fn mitres_at_30_70_110_and_140_degrees() {
    for deg in [30.0f64, 70.0, 110.0, 140.0] {
        let (c, sn) = (deg.to_radians().cos(), deg.to_radians().sin());
        let (mut s, r) = polyline_frame(json!([[300, 0], [0, 0], [300.0 * c, 300.0 * sn]]), "mitre");
        r.unwrap_or_else(|e| panic!("{deg}: {e}"));
        let bs = bodies(&mut s);
        assert_eq!(bs.len(), 2, "{deg}: {bs:?}");
        for b in &bs {
            assert!(rel(b.1, tube_area() * 300.0) < MEASURED, "{deg}: {b:?} vs {}", tube_area() * 300.0);
        }
        // The member along X runs past the corner on its outer side, to where the mitre meets y = -20.
        let reach = -20.0 / (deg.to_radians() / 2.0).tan();
        assert!(bs.iter().any(|b| (b.2[0] - reach).abs() < 1e-6), "{deg}: {reach} {bs:?}");
    }
}

#[test]
fn a_corner_sharper_than_five_degrees_fails() {
    let a = 4.0f64.to_radians();
    let (mut s, r) = polyline_frame(json!([[300, 0], [0, 0], [300.0 * a.cos(), 300.0 * a.sin()]]), "mitre");
    let e = r.unwrap_err();
    assert!(e.contains("Corner at (0, 0, 0) is too sharp to finish"), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn members_in_line_are_left_square() {
    let (mut s, r) = polyline_frame(json!([[0, 0], [300, 0], [600, 0]]), "mitre");
    r.unwrap();
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    assert!(bs.iter().any(|b| near3(b.2, [0.0, -20.0, -20.0]) && near3(b.3, [300.0, 20.0, 20.0])), "{bs:?}");
    assert!(bs.iter().any(|b| near3(b.2, [300.0, -20.0, -20.0]) && near3(b.3, [600.0, 20.0, 20.0])), "{bs:?}");
}

#[test]
fn the_third_member_at_a_point_is_fitted_to_the_first_two() {
    // Three tubes end at (300, 0): the two along X, laid first, make the corner, in line, so both stay square; the
    // upright, laid last, is fitted flat to their top faces at y = 20, A·280 of it left.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "T"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [300, 0], [600, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "T", "profile": tube(), "corner": "mitre"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 3, "{bs:?}");
    for (lo, hi) in
        [([0.0, -20.0, -20.0], [300.0, 20.0, 20.0]), ([300.0, -20.0, -20.0], [600.0, 20.0, 20.0]), ([280.0, 20.0, -20.0], [320.0, 300.0, 20.0])]
    {
        assert!(bs.iter().any(|b| near3(b.2, lo) && near3(b.3, hi)), "{lo:?} {hi:?}: {bs:?}");
    }
    assert!(bs.iter().any(|b| b.2[1] > 1.0 && rel(b.1, tube_area() * 280.0) < MEASURED), "{bs:?}");
    assert!(overlap_all(&mut s) < 1e-3);
}

#[test]
fn an_end_cap_on_the_third_member_fitted_at_a_point_is_refused() {
    // The upright of `the_third_member_at_a_point_is_fitted_to_the_first_two`, in square tube: its fitted end lies
    // flat on the horizontals' top faces at y = 20, square to its line, and is no open end, as Auto's fitted ends.
    for corner in ["mitre", "butt", "coped_butt"] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "T"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [300, 0], [600, 0]]}));
        run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 300]], "ids": ["up"]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "frame.member", json!({"sketch": "T", "profile": square_tube(), "corner": corner, "names": {"up": "Up"}}));
        let face = face_pick(&s, "Up", [281.5, 20.0, 0.0], [0.0, -1.0, 0.0]);
        let e = s.execute("frame.end_cap", &json!({"faces": [face], "thickness": 6})).unwrap_err().to_string();
        assert!(e.contains("not an open end"), "{corner}: {e}");
    }
}

#[test]
fn a_3d_sketch_corner_is_mitred() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tower"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 600], [400, 300, 900]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Tower", "profile": tube(), "corner": "mitre"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    // The upright now reaches above its line's top, where the mitre's high side is.
    assert!(bs.iter().any(|b| b.3[2] > 600.0 + 1.0 && b.3[2] < 700.0 && rel(b.1, tube_area() * 600.0) < MEASURED), "{bs:?}");
    let slant = 340_000f64.sqrt();
    assert!(bs.iter().any(|b| rel(b.1, tube_area() * slant) < MEASURED), "{bs:?} vs {}", tube_area() * slant);
}

#[test]
fn mitred_members_keep_their_names() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre"}));
    let names: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert_eq!(names, ["Rect Tube 40x40x3", "Rect Tube 40x40x3 (2)", "Rect Tube 40x40x3 (3)", "Rect Tube 40x40x3 (4)"]);
}

/// The rectangle frame's bottom (600) and left (400) members, by their boxes.
fn bottom_and_left(s: &mut Session) -> (Measured, Measured) {
    let bs = bodies(s);
    let bottom = bs.iter().find(|b| b.3[0] > 500.0 && b.3[1] < 200.0).unwrap_or_else(|| panic!("{bs:?}")).clone();
    let left = bs.iter().find(|b| b.3[0] < 200.0 && b.3[1] > 300.0).unwrap_or_else(|| panic!("{bs:?}")).clone();
    (bottom, left)
}

#[test]
fn a_butted_rectangle_runs_its_long_sides_through() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt"}));
    let (bottom, left) = bottom_and_left(&mut s);
    // The 600 members run out to the 400s' outer faces; the 400s stop at their near faces.
    assert!((bottom.2[0] + 20.0).abs() < 1e-6 && (bottom.3[0] - 620.0).abs() < 1e-6, "{bottom:?}");
    assert!(rel(bottom.1, tube_area() * 640.0) < MEASURED, "{bottom:?} vs {}", tube_area() * 640.0);
    assert!((left.2[1] - 20.0).abs() < 1e-6 && (left.3[1] - 380.0).abs() < 1e-6, "{left:?}");
    assert!(rel(left.1, tube_area() * 360.0) < MEASURED, "{left:?} vs {}", tube_area() * 360.0);
    // Each cut face is named for the end it finishes.
    let st = s.model.state();
    let names = solidcraft_doc::naming::face_names(st.body(&bottom.0).unwrap());
    for role in ["corner_start", "corner_end"] {
        assert!(names.iter().any(|n| n.contains(role)), "{role} in {names:?}");
    }
}

#[test]
fn flip_runs_the_short_sides_through() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "flip": true}));
    let (bottom, left) = bottom_and_left(&mut s);
    assert!((left.2[1] + 20.0).abs() < 1e-6 && (left.3[1] - 420.0).abs() < 1e-6, "{left:?}");
    assert!((bottom.2[0] - 20.0).abs() < 1e-6 && (bottom.3[0] - 580.0).abs() < 1e-6, "{bottom:?}");
}

#[test]
fn which_member_runs_through_follows_drawing_order_not_pick_order() {
    // A diamond: every side equally long and at 45° to X, so only drawing order can choose.
    let build = |reverse: bool| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "D"}));
        run(&mut s, "sketch.line", json!({"points": [[0, -200], [200, 0], [0, 200], [-200, 0], [0, -200]]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut lines: Vec<String> = sketch_lines(&s, "D").into_iter().map(|l| l.0).collect();
        assert_eq!(lines.len(), 4);
        if reverse {
            lines.reverse();
        }
        let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
        run(&mut s, "frame.member", json!({"sketch": "D", "profile": tube(), "corner": "butt", "lines": lines, "names": names}));
        let faces: std::collections::BTreeMap<String, Vec<String>> = s
            .model
            .state()
            .bodies
            .iter()
            .map(|b| {
                let mut n = solidcraft_doc::naming::face_names(b).to_vec();
                n.sort();
                (b.name.clone(), n)
            })
            .collect();
        let measured: std::collections::BTreeMap<String, (f64, [f64; 3], [f64; 3])> =
            bodies(&mut s).into_iter().map(|b| (b.0, (b.1, b.2, b.3))).collect();
        (measured, faces)
    };
    let ((drawn, drawn_faces), (picked, picked_faces)) = (build(false), build(true));
    assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
    for (line, (v, lo, hi)) in &drawn {
        let (pv, plo, phi) = picked[line];
        assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{line}: drawn {drawn:?} picked {picked:?}");
    }
    assert_eq!(drawn_faces, picked_faces);
}

#[test]
fn a_coped_box_tube_keeps_slivers_in_the_through_member_radii() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let r = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "coped": true}));
    let (bottom, left) = bottom_and_left(&mut s);
    // T as in Butt; U's slivers fill T's outer radius (6), down to 20 − 6.
    assert!((bottom.2[0] + 20.0).abs() < 1e-6 && (bottom.3[0] - 620.0).abs() < 1e-6, "{r} {bottom:?}");
    assert!((left.2[1] - 14.0).abs() < 1e-3, "{left:?}");
    assert!((left.3[1] - 386.0).abs() < 1e-3, "{left:?}");
    assert_eq!(bodies(&mut s).len(), 4);
}

/// One corner coped on its own: an override's `coped` copes that butt only, and on a coped frame
/// an override's `coped: false` leaves that butt plain. The left post meets the bottom rail at its
/// start: coped there, its slivers fill the rail's outer radius down to y = 14, while its top end
/// butts square at the top rail's near face (380); and the other way round.
#[test]
fn one_corner_is_coped_by_its_override() {
    for (frame_coped, corner_coped, want) in [(false, true, (14.0, 380.0)), (true, false, (20.0, 386.0))] {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        let (rails, posts) = rails_and_posts(&s);
        let corners = json!([{"members": [&rails[0], &posts[0]], "kind": "butt", "coped": corner_coped}]);
        let r =
            run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "coped": frame_coped, "corners": corners}));
        let (_, left) = bottom_and_left(&mut s);
        assert!((left.2[1] - want.0).abs() < 1e-3 && (left.3[1] - want.1).abs() < 1e-3, "frame coped {frame_coped}: {r} {left:?}");
        // frame.multi_trim sets the same corner's Coped in place.
        let fid = r["feature"].clone();
        run(&mut s, "frame.multi_trim", json!({"feature": fid, "members": [&rails[0], &posts[0]], "kind": "butt", "coped": frame_coped}));
        let (_, left) = bottom_and_left(&mut s);
        let back = if frame_coped { 14.0 } else { 20.0 };
        assert!((left.2[1] - back).abs() < 1e-3, "multi_trim coped {frame_coped}: {left:?}");
    }
}

/// Angle iron coped at a rectangle's butts fills each corner: an angle is open on one side, and a cope that stopped
/// halfway through the through member would leave its upright leg short of the other's. Each post runs on to both
/// rails' far faces (450 mm of section, 50 × 5 + 45 × 5) less what lies in each rail there: where both bottom legs
/// are (5 mm high) 50 × 50, above it the two upright legs' 5 × 5, 45 mm high. The rails run through, 650 mm whole.
#[test]
fn a_coped_angle_fills_the_corner_without_gaps() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let angle = json!({"kind": "angle", "width": 50, "height": 50, "thickness": 5, "root_radius": 0, "toe_radius": 0});
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": angle, "corner": "butt", "coped": true}));
    let area = 475.0;
    let corner = 50.0 * 50.0 * 5.0 + 5.0 * 5.0 * 45.0;
    let b = bodies(&mut s);
    for rail in [&b[0], &b[2]] {
        assert!(rel(rail.1, area * 650.0) < 1e-9, "{rail:?}");
    }
    for post in [&b[1], &b[3]] {
        assert!(rel(post.1, area * 450.0 - 2.0 * corner) < 1e-9, "the post fills each corner: {post:?}");
        // Its upright leg reaches the rails' upright legs (outer faces at y = -25 and 425, 5 mm thick).
        assert!((post.2[1] + 20.0).abs() < 1e-6 && (post.3[1] - 420.0).abs() < 1e-6, "{post:?}");
    }
}

#[test]
fn a_coped_round_tube_is_fishmouthed() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let profile = json!({"kind": "round_tube", "diameter": 40, "wall": 3});
    let r = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": profile, "corner": "butt", "coped": true}));
    let (bottom, left) = bottom_and_left(&mut s);
    assert!((bottom.2[0] + 20.0).abs() < 1e-6 && (bottom.3[0] - 620.0).abs() < 1e-6, "{r} {bottom:?}");
    // U's tips reach T's centreline at its sides.
    assert!(left.2[1].abs() < 1e-3 && (left.3[1] - 400.0).abs() < 1e-3, "{left:?}");
    // A·360 + 2∫(20 − √(400 − z²)) dA over the annulus: each tip the depth T's outside leaves.
    assert!(rel(left.1, 129_458.17) < MEASURED, "{left:?} vs 129458.17");
}

/// The line of sketch `name` that rises off y = 0.
fn line_off_x_axis(s: &Session, name: &str) -> String {
    sketch_lines(s, name).into_iter().find(|(_, a, b)| a[1].max(b[1]) > 1.0).unwrap().0
}

#[test]
fn a_coped_member_too_short_to_reach_is_refused() {
    // 15 mm of U ends inside T: what is left of it lies on T's far side only.
    let (mut s, r) = polyline_frame(json!([[400, 0], [0, 0], [0, 15]]), "coped_butt");
    let u = line_off_x_axis(&s, "V");
    let e = r.unwrap_err();
    assert!(e.contains(&format!("the member on line `{u}` was cut away completely")), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_short_coped_brace_keeps_its_own_side() {
    let deg = 30.0f64.to_radians();
    let (mut s, r) = polyline_frame(json!([[400, 0], [0, 0], [70.0 * deg.cos(), 70.0 * deg.sin()]]), "coped_butt");
    r.unwrap_or_else(|e| panic!("{e}"));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    // U is the one that does not run along T; it stays above T's near face, less T's radius.
    let u = bs.iter().find(|b| b.3[0] - b.2[0] < 300.0).unwrap_or_else(|| panic!("{bs:?}"));
    assert!(u.2[1] > 14.0 - 1e-3, "{u:?}");
}

#[test]
fn a_short_coped_round_tube_keeps_its_fishmouth() {
    // 25 mm of round U comes 5 mm out of T; its tips reach T's centreline.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
    run(&mut s, "sketch.line", json!({"points": [[400, 0], [0, 0], [0, 25]]}));
    run(&mut s, "sketch.finish", json!({}));
    let profile = json!({"kind": "round_tube", "diameter": 40, "wall": 3});
    run(&mut s, "frame.member", json!({"sketch": "V", "profile": profile, "corner": "butt", "coped": true}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    let u = bs.iter().find(|b| b.3[0] - b.2[0] < 300.0).unwrap_or_else(|| panic!("{bs:?}"));
    assert!(u.2[1].abs() < 1e-3 && (u.3[1] - 25.0).abs() < 1e-3, "{u:?}");
}

#[test]
fn a_coped_butt_frame_matches_the_old_coped_corner_type() {
    // Coped (#17854) on a Butt rectangle: the bodies the old Coped corner type (`coped_butt`) made, recorded at
    // 4f4304424. The 600s run through to the 400s' outer faces; the 400s are coped to them, their slivers filling the
    // 600s' outer radii (6), down to y = 14 and up to 386.
    let base = [
        ("Rect Tube 40x40x3", 269_326.722_128_267, [-20.0, -20.0, -20.0], [620.0, 20.0, 20.0]),
        ("Rect Tube 40x40x3 (2)", 152_468.289_152_276, [580.0, 14.0, -20.0], [620.0, 386.0, 20.0]),
        ("Rect Tube 40x40x3 (3)", 269_326.722_128_268, [-20.0, 380.0, -20.0], [620.0, 420.0, 20.0]),
        ("Rect Tube 40x40x3 (4)", 152_468.289_152_276, [-20.0, 14.0, -20.0], [20.0, 386.0, 20.0]),
    ];
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "coped": true}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), base.len(), "{bs:?}");
    for (b, (name, volume, lo, hi)) in bs.iter().zip(base) {
        assert!(b.0 == name && rel(b.1, volume) < 1e-8 && near3(b.2, lo) && near3(b.3, hi), "{b:?} vs {name} {volume} {lo:?} {hi:?}");
    }
}

/// A plus `Plus`: h (0, 0)…(400, 0) and v (200, −100)…(200, 100), or with `tee` only v's top half, ending on h's
/// middle; in 40 tube (outer radius 6) finished Butt, with Coped or not, h named `H` and v `V`.
fn coped_or_not(tee: bool, coped: bool) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[200, if tee { 0 } else { -100 }], [200, 100]], "ids": ["v"]}));
    run(&mut s, "sketch.finish", json!({}));
    let p = json!({"sketch": "Plus", "profile": tube(), "corner": "butt", "coped": coped, "names": {"h": "H", "v": "V"}});
    run(&mut s, "frame.member", p);
    s
}

#[test]
fn coped_fits_a_tee_to_the_through_tubes_shape() {
    // v ends on h's middle (ruling O1): Butt fits it to h's squared tool, flat on h's face at y = 20; Coped to h's
    // real shape, so v keeps what lies in h's outer radii, down to y = 14 where they are.
    let mut fitted = Vec::new();
    for coped in [false, true] {
        let mut s = coped_or_not(true, coped);
        let v = measured(&mut s, "V");
        let o = overlap_all(&mut s);
        assert!(o < 1e-3, "coped {coped}: the frame overlaps by {o} mm³");
        fitted.push(v);
    }
    let (butt, coped) = (&fitted[0], &fitted[1]);
    assert!((butt.2[1] - 20.0).abs() < 1e-6 && (coped.2[1] - 14.0).abs() < 1e-3, "{butt:?} {coped:?}");
    assert!(coped.1 > butt.1 + 1.0, "{butt:?} {coped:?}");
}

#[test]
fn coped_cuts_a_crossing_and_an_overlap_to_the_tubes_shape() {
    // v crosses h and is cut there into `V` and `V · H`, each fitted to h: under Coped to h's real shape, each
    // keeping what lies in h's outer radii, 6 nearer h's line.
    let pieces = |coped: bool| {
        let mut s = coped_or_not(false, coped);
        let o = overlap_all(&mut s);
        assert!(o < 1e-3, "coped {coped}: the frame overlaps by {o} mm³");
        ["V", "V · H"].map(|n| measured(&mut s, n))
    };
    let (butt, coped) = (pieces(false), pieces(true));
    for (b, c) in butt.iter().zip(&coped) {
        let near_h = |m: &Measured| m.2[1].abs().min(m.3[1].abs());
        assert!(c.1 > b.1 + 1.0 && (near_h(b) - 20.0).abs() < 1e-6 && (near_h(c) - 14.0).abs() < 1e-3, "{b:?} {c:?}");
    }
    // The brace 30 above the rail, pairs in no sketch plane: notched by the rail's real shape, it keeps what lies in
    // the rail's outer radii.
    let notched = |coped: bool| {
        let mut s = rail_and_brace(30.0, "butt", json!({"coped": coped}));
        let bs = bodies(&mut s);
        assert_eq!(bs.len(), 2, "{bs:?}");
        let o = overlap_all(&mut s);
        assert!(o < 1e-3, "coped {coped}: the tubes overlap by {o} mm³: {bs:?}");
        body_named(&bs, "T (2)").1
    };
    let (butt, coped) = (notched(false), notched(true));
    assert!(coped > butt + 1.0, "{butt} {coped}");
}

#[test]
fn coped_leaves_mitres_alone() {
    // A Mitre frame with Coped builds exactly as without: its tubes, volumes, boxes and face names.
    let build = |coped: bool| {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "coped": coped}));
        let faces: Vec<Vec<String>> = s.model.state().bodies.iter().map(|b| solidcraft_doc::naming::face_names(b).to_vec()).collect();
        (bodies(&mut s), faces)
    };
    let ((plain, plain_faces), (coped, coped_faces)) = (build(false), build(true));
    assert_eq!(plain.len(), 4);
    assert_eq!(plain, coped);
    assert_eq!(plain_faces, coped_faces);
}

#[test]
fn an_old_coped_butt_design_reads_as_butt_with_coped() {
    use solidcraft_doc::FeatureKind;
    use solidcraft_doc::frame::CornerType;
    // What the frame says: its type, Coped, and its override's type.
    fn read(doc: &solidcraft_doc::Document) -> (CornerType, bool, Vec<CornerType>) {
        let f = doc.features.iter().find(|f| matches!(f.kind, FeatureKind::FrameMember { .. })).unwrap();
        let FeatureKind::FrameMember { corner, coped, corners, .. } = &f.kind else { panic!("not a frame member") };
        (*corner, *coped, corners.iter().map(|o| o.kind).collect())
    }
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (l1, l2) = two_lines(&s);
    // A script: the frame's and an override's `coped_butt`.
    let corners = json!([{"members": [l1, l2], "corner": "coped_butt"}]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "coped_butt", "corners": corners}));
    assert_eq!(read(&s.doc), (CornerType::Butt, true, vec![CornerType::Butt]));
    let saved = s.doc.to_json();
    assert!(!saved.contains("coped_butt") && saved.contains("\"coped\": true"), "{saved}");
    // A saved design: a Mitre frame with a Butt override, either of them `coped_butt`.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let corners = json!([{"members": [l1, l2], "corner": "butt"}]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "corners": corners}));
    let json = s.doc.to_json();
    for (old, want) in
        [("\"butt\"", (CornerType::Mitre, true, vec![CornerType::Butt])), ("\"mitre\"", (CornerType::Butt, true, vec![CornerType::Butt]))]
    {
        assert!(json.contains(old), "{json}");
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&json.replace(old, "\"coped_butt\"")).unwrap());
        assert_eq!(read(&reopened.doc), want, "{old}");
        assert_eq!(bodies(&mut reopened).len(), 4, "{old}");
    }
    // An edit that sets `coped_butt` (an older script's `timeline.edit`): read the same way.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "corners": corners}));
    let fid = first_frame(&s);
    run(&mut s, "timeline.edit", json!({"feature": fid, "set": {"corner": "coped_butt"}}));
    assert_eq!(read(&s.doc), (CornerType::Butt, true, vec![CornerType::Butt]));
}

#[test]
fn a_coped_that_is_not_true_or_false_is_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    for bad in [json!("yes"), json!(1), json!([true]), json!({"on": true})] {
        let e = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "coped": bad})).unwrap_err().to_string();
        assert!(e.contains("`coped` must be true or false"), "{bad}: {e}");
    }
    assert!(s.doc.features.iter().all(|f| !matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })));
}

#[test]
fn butts_at_70_and_110_degrees_meet_flush() {
    for deg in [70.0f64, 110.0] {
        let (c, sn) = (deg.to_radians().cos(), deg.to_radians().sin());
        let (mut s, r) = polyline_frame(json!([[400, 0], [0, 0], [300.0 * c, 300.0 * sn]]), "butt");
        r.unwrap_or_else(|e| panic!("{deg}: {e}"));
        let bs = bodies(&mut s);
        assert_eq!(bs.len(), 2, "{deg}: {bs:?}");
        // T (the 400) runs h / sin φ past the corner to U's outer face; U stops that far short.
        let (t, u) = (tube_area() * (400.0 + 20.0 / sn), tube_area() * (300.0 - 20.0 / sn));
        assert!(bs.iter().any(|b| rel(b.1, t) < MEASURED), "{deg}: T {t} in {bs:?}");
        assert!(bs.iter().any(|b| rel(b.1, u) < MEASURED), "{deg}: U {u} in {bs:?}");
    }
}

#[test]
fn a_butt_at_150_degrees_is_flush() {
    let deg = 150.0f64.to_radians();
    let (mut s, r) = polyline_frame(json!([[400, 0], [0, 0], [300.0 * deg.cos(), 300.0 * deg.sin()]]), "butt");
    r.unwrap();
    let bs = bodies(&mut s);
    let t = bs.iter().find(|b| b.3[0] > 300.0).unwrap_or_else(|| panic!("{bs:?}"));
    let want = tube_area() * (400.0 + 20.0 / deg.sin());
    assert!(rel(t.1, want) < MEASURED, "{t:?} vs {want}");
    // T is cut on U's outer face: the plane 20 from the corner along m = (−sin 30°, −cos 30°),
    // facing −m. T's face y = +20 meets it furthest back: 0.5 (x + 10) + 0.866 (20 + 17.32) = 0,
    // so x = −20 (1 + cos 30°) / sin 150° = −74.641.
    let min_x = -20.0 * (1.0 + 30f64.to_radians().cos()) / deg.sin();
    assert!((t.2[0] - min_x).abs() < 1e-6, "{t:?} vs {min_x}");
}

#[test]
fn a_nearly_straight_butt_is_refused() {
    // At 175° T would run back 458 mm, past the far end of the 300 mm U.
    let deg = 175.0f64.to_radians();
    let (mut s, r) = polyline_frame(json!([[400, 0], [0, 0], [300.0 * deg.cos(), 300.0 * deg.sin()]]), "butt");
    let e = r.unwrap_err();
    assert!(e.contains("Corner at (0, 0, 0) is too flat to butt"), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn an_override_changes_only_its_corner() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom_line, left_line) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "mitre",
               "corners": [{"members": [bottom_line, left_line], "corner": "butt", "flip": true}]}),
    );
    let (bottom, left) = bottom_and_left(&mut s);
    // Flipped, the shorter left member runs through at the bottom; the bottom's far end is mitred.
    assert!((left.2[1] + 20.0).abs() < 1e-6, "{left:?}");
    assert!((bottom.2[0] - 20.0).abs() < 1e-6 && (bottom.3[0] - 620.0).abs() < 1e-6, "{bottom:?}");
}

#[test]
fn an_override_follows_its_corner_through_a_dimension_edit() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "W", "expression": "600 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], ["W", 0], ["W", 400], [0, 400]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    let (bottom_line, left_line) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "mitre",
               "corners": [{"members": [left_line, bottom_line], "corner": "butt", "flip": true}]}),
    );
    run(&mut s, "parameters.change", json!({"name": "W", "expression": "800 mm"}));
    let bs = bodies(&mut s);
    let bottom = bs.iter().find(|b| b.3[0] > 700.0 && b.3[1] < 200.0).unwrap_or_else(|| panic!("{bs:?}"));
    let left = bs.iter().find(|b| b.3[0] < 200.0 && b.3[1] > 300.0).unwrap_or_else(|| panic!("{bs:?}"));
    assert!((left.2[1] + 20.0).abs() < 1e-6, "{left:?}");
    assert!((bottom.2[0] - 20.0).abs() < 1e-6 && (bottom.3[0] - 820.0).abs() < 1e-6, "{bottom:?}");
}

/// An L of `l1` (0, 0)…(300, 0) and `l2` (0, 0)…(0, H), H = 200 mm, in sketch `L`.
fn l_corner(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "H", "expression": "200 mm"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [300, 0]], "ids": ["l1"]}));
    run(s, "sketch.line", json!({"points": [[0, 0], [0, "H"]], "ids": ["l2"]}));
    run(s, "sketch.finish", json!({}));
}

/// The saved corner overrides of the session's Frame Member.
fn saved_corners(s: &Session) -> Vec<solidcraft_doc::frame::CornerOverride> {
    let f = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap();
    let solidcraft_doc::FeatureKind::FrameMember { corners, .. } = &f.kind else { panic!("not a frame member") };
    corners.clone()
}

#[test]
fn a_saved_order_fixes_the_through_member() {
    // l2 first runs through, the shorter one: its tube reaches past the corner to l1's outer face (y from -20), and
    // l1 stops at l2's near face (x from 20). Made longer than l1, it still runs through: the order is absolute.
    let mut s = Session::default();
    l_corner(&mut s);
    let corners = json!([{"members": ["l2", "l1"], "kind": "butt"}]);
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "butt", "corners": corners, "names": {"l1": "A", "l2": "B"}}));
    for h in [200.0, 400.0] {
        run(&mut s, "parameters.change", json!({"name": "H", "expression": format!("{h} mm")}));
        let (a, b) = (measured(&mut s, "A"), measured(&mut s, "B"));
        assert!(near3(b.2, [-20.0, -20.0, -20.0]) && near3(b.3, [20.0, h, 20.0]), "{h}: {b:?}");
        assert!(near3(a.2, [20.0, -20.0, -20.0]) && near3(a.3, [300.0, 20.0, 20.0]), "{h}: {a:?}");
        assert!(rel(b.1 / a.1, (h + 20.0) / 280.0) < 1e-6, "{h}: {a:?} {b:?}");
    }
}

#[test]
fn flip_is_turned_into_an_order() {
    // Today's rules run the longer l1 through; Flip runs l2 through, saved as the order l2, l1, never as `flip`.
    let mut s = Session::default();
    l_corner(&mut s);
    let corners = json!([{"members": ["l1", "l2"], "corner": "butt", "flip": true}]);
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "butt", "corners": corners}));
    let saved = saved_corners(&s);
    assert_eq!(saved.iter().map(|c| c.lines().collect::<Vec<_>>()).collect::<Vec<_>>(), [["l2", "l1"]]);
    let json = serde_json::to_string(&saved).unwrap();
    assert!(!json.contains("flip") && json.contains(r#""kind":"butt""#), "{json}");
}

#[test]
fn a_corner_member_of_another_frame_is_refused() {
    let mut s = Session::default();
    l_corner(&mut s);
    let corners = json!([{"members": [{"feature": "Other", "line": "l1"}, "l2"], "kind": "butt"}]);
    let e = s.execute("frame.member", &json!({"sketch": "L", "profile": tube(), "corner": "butt", "corners": corners})).unwrap_err().to_string();
    assert!(e.contains("not supported yet: a corner with a member of another frame"), "{e}");
}

#[test]
fn an_override_whose_corner_has_gone_fails() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
    run(&mut s, "sketch.line", json!({"points": [[400, 0], [0, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 300]], "ids": ["b"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "V", "profile": tube(), "corners": [{"members": ["a", "b"], "corner": "butt"}]}));
    assert_eq!(bodies(&mut s).len(), 2);
    // Line b drawn again 50 mm up: a and b no longer meet.
    run(&mut s, "sketch.edit", json!({"sketch": "V"}));
    run(&mut s, "sketch.delete", json!({"entities": ["b"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 50], [0, 300]], "ids": ["b"]}));
    let finished = s.execute("sketch.finish", &json!({})).map_err(|e| e.to_string());
    let member = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let e = finished.err().or_else(|| s.model.result(member).and_then(|r| r.error.clone())).unwrap_or_default();
    assert!(e.contains("A corner override is not at a corner of the frame"), "{e} {:?}", sketch_lines(&s, "V"));
}

#[test]
fn a_member_too_short_for_its_cuts_fails_by_name() {
    let (mut s, r) = polyline_frame(json!([[0, 300], [0, 0], [30, 0], [30, 300]]), "butt");
    let short = line_at_y(&s, "V", 0.0);
    let e = r.unwrap_err();
    assert!(e.contains(&format!("the member on line `{short}` was cut away completely")), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_member_cut_into_pieces_fails_by_name() {
    // A 10 mm member mitred at both ends keeps only the section within 5 mm of its inner side:
    // a channel whose web is on its outer side keeps its two flanges, apart.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 300], [0, 0], [10, 0], [10, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    let short = line_at_y(&s, "V", 0.0);
    let channel = json!({"kind": "channel", "height": 60, "width": 45, "web": 5, "flange": 6});
    let e = s.execute("frame.member", &json!({"sketch": "V", "profile": channel, "mirror": true, "corner": "mitre"})).unwrap_err().to_string();
    assert!(e.contains(&format!("the member on line `{short}` is cut into pieces by its corners")), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_bottom_left_anchor_butts_flush_with_the_placed_section() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "anchor": "bottom_left"}));
    let bs = bodies(&mut s);
    // The members sit off their lines: find them by their runs along X and Y.
    let t = bs.iter().find(|b| b.3[0] - b.2[0] > 500.0 && b.2[1] < 100.0).unwrap_or_else(|| panic!("{bs:?}"));
    let u = bs.iter().find(|b| b.3[1] - b.2[1] > 300.0 && b.2[0] < 100.0).unwrap_or_else(|| panic!("{bs:?}"));
    // T ends flush with U's outer face, and U ends on T's near face.
    assert!((t.2[0] - u.2[0]).abs() < 1e-6, "{t:?} {u:?}");
    assert!((u.2[1] - t.3[1]).abs() < 1e-6, "{t:?} {u:?}");
}

#[test]
fn a_hand_edited_override_list_is_refused_not_a_panic() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom_line, left_line) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "mitre",
               "corners": [{"members": [bottom_line, left_line], "corner": "butt"}]}),
    );
    let member = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    /// The saved override list: the array under `corners` beside `corner`.
    fn overrides(v: &mut Value) -> Option<&mut Vec<Value>> {
        if v.get("corner").is_some() && v.get("corners").is_some_and(Value::is_array) {
            return v.get_mut("corners")?.as_array_mut();
        }
        match v {
            Value::Object(m) => m.values_mut().find_map(overrides),
            Value::Array(a) => a.iter_mut().find_map(overrides),
            _ => None,
        }
    }
    let saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let one = overrides(&mut saved.clone()).unwrap()[0].clone();
    for (edit, why) in [
        (vec![json!({"members": [bottom_line, "nope"], "corner": "butt"})], "A corner override is not at a corner of the frame"),
        (vec![json!({"members": [bottom_line, bottom_line], "corner": "butt"})], "A corner override is not at a corner of the frame"),
        (vec![one; solidcraft_doc::frame::MAX_MEMBERS + 1], "corner overrides"),
    ] {
        let mut file = saved.clone();
        *overrides(&mut file).unwrap() = edit;
        let reopened = Session::new(solidcraft_doc::Document::from_json(&file.to_string()).unwrap());
        let e = reopened.model.result(member).and_then(|r| r.error.clone()).unwrap_or_default();
        assert!(e.contains(why), "{why}: {e}");
    }
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

// ---------------------------------------------------------------------------------------------
// Members between pairs of points (#15705)

/// A 600 × 400 × 400 box `Box` (feature `Box`) from the rectangle `Base` on XY: its corners are
/// the points the pairs join.
fn box_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 400, "name": "Box", "body_name": "Box"}));
}

/// The upright at the box's corner (600, 0), as `pairs` takes it.
fn upright() -> Value {
    json!({"a": {"vertex": [600, 0, 0]}, "b": {"vertex": [600, 0, 400]}})
}

/// The rail along the box's top front edge, as `pairs` takes it.
fn top_rail() -> Value {
    json!({"a": {"vertex": [0, 0, 400]}, "b": {"vertex": [600, 0, 400]}})
}

/// The bounding boxes of the bodies other than the box, by name.
fn member_boxes(s: &mut Session) -> std::collections::BTreeMap<String, ([f64; 3], [f64; 3])> {
    boxes(s).into_iter().filter(|(name, _)| name != "Box").collect()
}

fn has_box(boxes: &std::collections::BTreeMap<String, ([f64; 3], [f64; 3])>, lo: [f64; 3], hi: [f64; 3]) -> bool {
    boxes.values().any(|(l, h)| near3(*l, lo) && near3(*h, hi))
}

/// The kind of the feature `id` (as a command's reply gives it).
fn member_kind(s: &Session, id: &Value) -> solidcraft_doc::FeatureKind {
    s.doc.feature(id.as_u64().unwrap()).unwrap().kind.clone()
}

#[test]
fn members_between_point_pairs() {
    let mut s = Session::default();
    box_frame(&mut s);
    let r = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}));
    let members = member_boxes(&mut s);
    assert_eq!(members.len(), 2, "{members:?}");
    // The upright: square to Z, the profile's x along world X.
    assert!(has_box(&members, [580.0, -20.0, 0.0], [620.0, 20.0, 400.0]), "{members:?}");
    // The rail along +X: the profile's x along +Y, its y up.
    assert!(has_box(&members, [0.0, -20.0, 380.0], [600.0, 20.0, 420.0]), "{members:?}");
    let solidcraft_doc::FeatureKind::FrameMember { path_sketch, pairs, next_pair, .. } = member_kind(&s, &r["feature"]) else { panic!() };
    assert_eq!(path_sketch, None);
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair1", "pair2"]);
    assert_eq!(next_pair, 3);
}

#[test]
fn point_pairs_and_sketch_lines_in_one_frame_member() {
    let mut s = Session::default();
    box_frame(&mut s);
    let bottom = line_at_y(&s, "Base", 0.0);
    let r = run(&mut s, "frame.member", json!({"sketch": "Base", "lines": [bottom], "profile": tube(), "pairs": [upright()]}));
    let members = member_boxes(&mut s);
    assert_eq!(members.len(), 2, "{members:?}");
    assert!(has_box(&members, [0.0, -20.0, -20.0], [600.0, 20.0, 20.0]), "{members:?}");
    assert!(has_box(&members, [580.0, -20.0, 0.0], [620.0, 20.0, 400.0]), "{members:?}");
    let ids = solidcraft_doc::frame::member_line_ids(&s.model.state(), &member_kind(&s, &r["feature"])).unwrap();
    assert_eq!(ids, [bottom, "pair1".to_string()]);
}

#[test]
fn a_pair_member_follows_its_point() {
    let mut s = Session::default();
    box_frame(&mut s);
    run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright()]}));
    run(&mut s, "timeline.edit", json!({"feature": "Box", "set": {"extent.distance": "500"}}));
    let members = member_boxes(&mut s);
    assert!(has_box(&members, [580.0, -20.0, 0.0], [620.0, 20.0, 500.0]), "the upright reaches the box's new top: {members:?}");
}

/// A pair on sketch points follows them when the sketch is edited.
#[test]
fn a_pair_member_follows_its_sketch_points() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    run(&mut s, "sketch.point", json!({"point": [0, -100], "id": "m1"}));
    run(&mut s, "sketch.point", json!({"point": [600, -100], "id": "m2"}));
    run(&mut s, "sketch.finish", json!({}));
    let pair = json!({"a": {"sketch": "Marks", "point": "m1"}, "b": {"sketch": "Marks", "point": "m2"}});
    run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [pair]}));
    let members = member_boxes(&mut s);
    assert!(has_box(&members, [0.0, -120.0, -20.0], [600.0, -80.0, 20.0]), "{members:?}");
    run(&mut s, "sketch.edit", json!({"sketch": "Marks"}));
    run(&mut s, "sketch.move_point", json!({"point": "m2", "to": [800, -100]}));
    run(&mut s, "sketch.finish", json!({}));
    let members = member_boxes(&mut s);
    assert!(has_box(&members, [0.0, -120.0, -20.0], [800.0, -80.0, 20.0]), "{members:?}");
}

#[test]
fn a_pair_member_keeps_its_name_when_another_pair_goes() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}))["feature"].clone();
    let solidcraft_doc::FeatureKind::FrameMember { names, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(names.keys().collect::<Vec<_>>(), ["pair1", "pair2"]);
    let rail = member_boxes(&mut s).into_iter().find(|(_, (lo, _))| (lo[2] - 380.0).abs() < 1e-6).unwrap();
    assert_eq!(names["pair2"], rail.0);
    let mut kept = top_rail();
    kept["id"] = json!("pair2");
    let params = json!({"profile": tube(), "pairs": [kept], "next_pair": 3});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    let after = member_boxes(&mut s);
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after.get(&rail.0), Some(&rail.1), "the rail keeps its name and place: {after:?}");
    let solidcraft_doc::FeatureKind::FrameMember { pairs, next_pair, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair2"]);
    assert_eq!(next_pair, 3);
}

#[test]
fn an_end_cap_closes_a_pair_member() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": square_tube(), "pairs": [upright()], "body_name": "Post"}))["feature"].clone();
    let top = face_pick(&s, "Post", [618.5, 0.0, 400.0], [0.0, 0.0, 1.0]);
    let r = run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let cap = measured(&mut s, "Post End Cap");
    assert!(near3(cap.2, [580.0, -20.0, 400.0]) && near3(cap.3, [620.0, 20.0, 406.0]), "{cap:?}");
    let solidcraft_doc::FeatureKind::FrameEndCap { ends, .. } = member_kind(&s, &r["feature"]) else { panic!() };
    assert_eq!(ends[0].member.line, "pair1");
    assert_eq!(ends[0].member.feature.0, fid.as_u64().unwrap());
}

#[test]
fn pairs_are_refused_when() {
    let mut s = Session::default();
    box_frame(&mut s);
    let same = json!({"a": {"vertex": [600, 0, 0]}, "b": {"vertex": [600, 0, 0]}});
    let fixed = json!({"a": {"point": [0, 0, 0]}, "b": {"vertex": [600, 0, 400]}});
    let edge = json!({"a": {"vertex": [600, 0, 0]}, "b": {"edge": [300, 0, 0]}});
    let many: Vec<Value> = (0..=1000).map(|_| upright()).collect();
    let with_id = |pair: Value, id: &str| {
        let mut pair = pair;
        pair["id"] = json!(id);
        pair
    };
    for (p, why) in [
        (json!({"profile": tube(), "pairs": [same]}), "no length"),
        (json!({"profile": tube(), "pairs": [fixed]}), "`pairs[0].a` must be a point"),
        (json!({"profile": tube(), "pairs": [upright(), edge]}), "`pairs[1].b` must be a point"),
        (json!({"profile": tube(), "pairs": many}), "at most 1000"),
        (json!({"profile": tube()}), "`sketch`"),
        (json!({"profile": tube(), "pairs": []}), "`pairs`"),
        (json!({"profile": tube(), "pairs": "pair1"}), "`pairs`"),
        (json!({"profile": tube(), "pairs": [{"a": {"vertex": [600, 0, 0]}}]}), "`pairs[0].b`"),
        (json!({"profile": tube(), "pairs": [with_id(upright(), "pair9")], "next_pair": 3}), "pair9"),
        (json!({"profile": tube(), "pairs": [with_id(upright(), "pair1"), with_id(top_rail(), "pair1")], "next_pair": 3}), "pair1"),
        (json!({"profile": tube(), "pairs": [with_id(upright(), "rail")]}), "rail"),
        (json!({"profile": tube(), "pairs": [upright()], "next_pair": -1}), "`next_pair`"),
    ] {
        let err = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{why}: {err}");
    }
    assert!(member_boxes(&mut s).is_empty(), "nothing was made");
}

#[test]
fn a_pair_whose_point_is_gone_fails_the_feature_naming_it() {
    let mut s = Session::default();
    box_frame(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Marks"}));
    run(&mut s, "sketch.point", json!({"point": [600, 0], "id": "m1"}));
    run(&mut s, "sketch.finish", json!({}));
    let on_mark = json!({"a": {"sketch": "Marks", "point": "m1"}, "b": {"vertex": [600, 0, 400]}});
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [on_mark, top_rail()]}))["feature"].as_u64().unwrap();
    let before = member_boxes(&mut s);
    assert_eq!(before.len(), 2, "{before:?}");
    run(&mut s, "sketch.edit", json!({"sketch": "Marks"}));
    run(&mut s, "sketch.delete", json!({"entities": ["m1"]}));
    run(&mut s, "sketch.finish", json!({}));
    let failing = s.model.result(fid).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(failing.contains("pair1"), "{failing}");
    assert!(member_boxes(&mut s).is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(member_boxes(&mut s), before);
}

#[test]
fn a_pair_member_in_a_rotated_component_turns_like_a_line_member() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Tilted", "grounded": false}))["component"].as_u64().unwrap();
    let o = s.doc.occurrence_of(c).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": o, "axis": [1, 0, 0], "angle": "30 deg", "capture": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let wire = run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [600, 0, 0]]}))["created"]["wires"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let bar = json!({"kind": "bar", "width": 60, "height": 10});
    run(&mut s, "frame.member", json!({"sketch": "Path", "profile": bar, "body_name": "Line"}));
    let pair = json!({"a": {"sketch": "Path", "point": format!("{wire}:0")}, "b": {"sketch": "Path", "point": format!("{wire}:1")}});
    run(&mut s, "frame.member", json!({"profile": bar, "pairs": [pair], "body_name": "Pair"}));
    let (line, pair) = (measured(&mut s, "Line"), measured(&mut s, "Pair"));
    // Tilted 30° about X with its component: the bar's 60 mm width rises 30 mm out of the XY plane.
    assert!(line.3[2] - line.2[2] > 30.0, "{line:?}");
    assert!(near3(line.2, pair.2) && near3(line.3, pair.3), "{line:?} vs {pair:?}");
}

#[test]
fn a_pasted_frame_member_with_pairs_lands_with_its_source() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}))["feature"].clone();
    let source: Vec<_> = member_boxes(&mut s).into_values().collect();
    run(&mut s, "timeline.copy", json!({"features": [fid]}));
    run(&mut s, "timeline.paste", json!({}));
    let all: Vec<_> = member_boxes(&mut s).into_values().collect();
    assert_eq!(all.len(), 4, "{all:?}");
    for (lo, hi) in &source {
        assert_eq!(all.iter().filter(|(l, h)| near3(*l, *lo) && near3(*h, *hi)).count(), 2, "{all:?}");
    }
}

/// The stored `pairs` of a frame member, as JSON.
fn stored_pairs(s: &Session, id: &Value) -> Value {
    serde_json::to_value(&s.doc.feature(id.as_u64().unwrap()).unwrap().kind).unwrap()["pairs"].clone()
}

#[test]
fn editing_a_frame_member_keeps_pair_ids_and_never_reuses_one() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}))["feature"].clone();
    let before = stored_pairs(&s, &fid);
    let solidcraft_doc::FeatureKind::FrameMember { names, .. } = member_kind(&s, &fid) else { panic!() };
    // timeline.edit: the feature is edited in place, so a removed pair's id stays spent.
    run(&mut s, "timeline.edit", json!({"feature": fid, "set": {"pairs": [before[1].clone()]}}));
    let solidcraft_doc::FeatureKind::FrameMember { pairs, next_pair, names: after, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair2"]);
    assert_eq!(serde_json::to_value(&pairs[0]).unwrap(), before[1]);
    assert_eq!(next_pair, 3);
    assert_eq!(after["pair2"], names["pair2"]);
    // timeline.redefine without `next_pair`: the same.
    let kept = json!({"id": "pair2", "a": top_rail()["a"], "b": top_rail()["b"]});
    let params = json!({"profile": tube(), "pairs": [kept, upright()]});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    let solidcraft_doc::FeatureKind::FrameMember { pairs, next_pair, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair2", "pair3"]);
    assert_eq!(next_pair, 4);
    // Dropping the newest pair and adding another still never reuses `pair3`.
    let params = json!({"profile": tube(), "pairs": [{"id": "pair2", "a": top_rail()["a"], "b": top_rail()["b"]}]});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    let params = json!({"profile": tube(), "pairs": [{"id": "pair2", "a": top_rail()["a"], "b": top_rail()["b"]}, upright()]});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    let solidcraft_doc::FeatureKind::FrameMember { pairs, next_pair, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair2", "pair4"]);
    assert_eq!(next_pair, 5);
}

#[test]
fn a_member_id_used_twice_is_refused() {
    let mut s = Session::default();
    box_frame(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rails"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 500]], "ids": ["pair1"]}));
    run(&mut s, "sketch.finish", json!({}));
    let e =
        s.execute("frame.member", &json!({"sketch": "Rails", "lines": ["pair1"], "profile": tube(), "pairs": [upright()]})).unwrap_err().to_string();
    assert!(e.contains("`pair1` is used twice"), "{e}");
}

/// The rail along the box's bottom front edge, as `pairs` takes it.
fn bottom_rail() -> Value {
    json!({"a": {"vertex": [0, 0, 0]}, "b": {"vertex": [600, 0, 0]}})
}

fn remove(s: &mut Session, members: Value) -> Value {
    run(s, "frame.remove_member", json!({ "members": members }))
}

#[test]
fn removing_a_pair_member_takes_out_its_pair() {
    let mut s = Session::default();
    box_frame(&mut s);
    // The upright and the top rail share the corner (600, 0, 400).
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}))["feature"].clone();
    let solidcraft_doc::FeatureKind::FrameMember { names, .. } = member_kind(&s, &fid) else { panic!() };
    let rail = member_boxes(&mut s).into_iter().find(|(name, _)| *name == names["pair2"]).unwrap();
    let stored = stored_pairs(&s, &fid);
    let r = remove(&mut s, json!([{"feature": fid, "line": "pair1"}]));
    assert_eq!(r["feature"], fid, "{r}");
    let solidcraft_doc::FeatureKind::FrameMember { pairs, names: after, next_pair, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair2"]);
    assert_eq!(stored_pairs(&s, &fid), json!([stored[1]]), "pair2, the shared point with it, is as it was");
    assert_eq!(after.keys().collect::<Vec<_>>(), ["pair2"]);
    assert_eq!(after["pair2"], names["pair2"]);
    assert_eq!(next_pair, 3, "a removed pair's id is never given again");
    let members = member_boxes(&mut s);
    assert_eq!(members.len(), 1, "{members:?}");
    assert_eq!(members.get(&rail.0), Some(&rail.1), "the rail keeps its name and place: {members:?}");
}

#[test]
fn removing_a_line_member_lists_the_other_lines() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}))["feature"].clone();
    let before = boxes(&mut s);
    let bottom = line_at_y(&s, "Frame", 0.0);
    remove(&mut s, json!([{"feature": fid, "line": bottom}]));
    let solidcraft_doc::FeatureKind::FrameMember { lines, names, path_sketch, .. } = member_kind(&s, &fid) else { panic!() };
    let others: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).filter(|id| *id != bottom).collect();
    assert_eq!(lines, others);
    assert!(path_sketch.is_some());
    assert!(!names.contains_key(&bottom), "{names:?}");
    let after = boxes(&mut s);
    assert_eq!(after.len(), 3, "{after:?}");
    for (name, bb) in &after {
        assert_eq!(before.get(name), Some(bb), "{name} moved or was renamed: {before:?} → {after:?}");
    }
    assert_eq!(sketch_lines(&s, "Frame").len(), 4, "the line stays in its sketch");
}

#[test]
fn removing_the_last_member_deletes_the_frame_member() {
    let mut s = Session::default();
    box_frame(&mut s);
    let made = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright()]}));
    let fid = made["feature"].clone();
    let kind = member_kind(&s, &fid);
    let undo_steps = s.undo.len();
    let r = remove(&mut s, json!([{"feature": made["name"], "line": "pair1"}]));
    assert_eq!(r["deleted"], made["name"], "{r}");
    assert_eq!(s.undo.len(), undo_steps + 1, "one undo step");
    assert!(s.doc.feature(fid.as_u64().unwrap()).is_none());
    assert!(member_boxes(&mut s).is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(member_kind(&s, &fid), kind, "one undo step brings it back, its member and name too");
    assert_eq!(member_boxes(&mut s).len(), 1);
}

#[test]
fn removing_every_line_of_a_mixed_frame_member_keeps_its_pairs() {
    let mut s = Session::default();
    box_frame(&mut s);
    let bottom = line_at_y(&s, "Base", 0.0);
    let fid = run(&mut s, "frame.member", json!({"sketch": "Base", "lines": [bottom], "profile": tube(), "pairs": [upright()]}))["feature"].clone();
    remove(&mut s, json!([{"feature": fid, "line": bottom}]));
    let solidcraft_doc::FeatureKind::FrameMember { lines, path_sketch, pairs, .. } = member_kind(&s, &fid) else { panic!() };
    assert!(lines.is_empty() && path_sketch.is_none(), "no lines left, not every line: {lines:?} {path_sketch:?}");
    assert_eq!(pairs.len(), 1);
    assert_eq!(member_boxes(&mut s).len(), 1);
}

#[test]
fn undo_and_redo_of_remove_tube() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}))["feature"].clone();
    let before = member_kind(&s, &fid);
    let undo_steps = s.undo.len();
    remove(&mut s, json!([{"feature": fid, "line": "pair1"}]));
    assert_eq!(s.undo.len(), undo_steps + 1, "one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(member_kind(&s, &fid), before, "both members, pair1's name back");
    assert_eq!(member_boxes(&mut s).len(), 2);
    run(&mut s, "edit.redo", json!({}));
    let solidcraft_doc::FeatureKind::FrameMember { pairs, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.len(), 1);
    assert_eq!(member_boxes(&mut s).len(), 1);
}

#[test]
fn an_end_cap_on_a_removed_member_reports_a_broken_reference() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid =
        run(&mut s, "frame.member", json!({"profile": square_tube(), "pairs": [upright(), bottom_rail()], "body_name": "Post"}))["feature"].clone();
    let top = face_pick(&s, "Post", [618.5, 0.0, 400.0], [0.0, 0.0, 1.0]);
    let cap = run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}))["feature"].as_u64().unwrap();
    remove(&mut s, json!([{"feature": fid, "line": "pair1"}]));
    assert!(s.doc.feature(cap).is_some(), "the End Cap is not deleted");
    let error = s.model.result(cap).and_then(|r| r.error.clone());
    assert!(error.is_some(), "the End Cap shows its broken reference");
}

#[test]
fn remove_member_refuses() {
    let mut s = Session::default();
    box_frame(&mut s);
    let a = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright()]}))["feature"].clone();
    let b = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [top_rail()]}))["feature"].clone();
    let doc = s.doc.clone();
    let undo_steps = s.undo.len();
    let many: Vec<Value> = (0..=1000).map(|_| json!({"feature": a, "line": "pair1"})).collect();
    for (p, why) in [
        (json!({"members": [{"feature": "Nowhere", "line": "pair1"}]}), "Nowhere"),
        (json!({"members": [{"feature": "Box", "line": "pair1"}]}), "not a Frame Member"),
        (json!({"members": [{"feature": a, "line": "pair9"}]}), "pair9"),
        (json!({"members": [{"feature": a, "line": "pair1"}, {"feature": b, "line": "pair1"}]}), "one Frame Member"),
        (json!({"members": []}), "`members`"),
        (json!({}), "`members`"),
        (json!({"members": many}), "1000"),
        (json!({"members": [{"feature": a}]}), "`members[0]`"),
    ] {
        let err = s.execute("frame.remove_member", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{why}: {err}");
    }
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), serde_json::to_value(&*doc).unwrap(), "nothing changed");
    assert_eq!(s.undo.len(), undo_steps);
}

/// The bounding boxes of the bodies feature `fid` made, by name.
fn feature_boxes(s: &mut Session, fid: u64) -> std::collections::BTreeMap<String, ([f64; 3], [f64; 3])> {
    let made: Vec<String> = s.model.state().bodies.iter().filter(|b| b.feature == fid).map(|b| b.name.clone()).collect();
    boxes(s).into_iter().filter(|(name, _)| made.contains(name)).collect()
}

/// A pasted Frame Member names its tubes by position: removing one, by Remove Tube or by Edit
/// Feature, leaves every other tube its name, so what refers to them stays on them.
#[test]
fn removing_a_tube_from_a_pasted_frame_member_keeps_the_other_tubes_names() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail(), bottom_rail()]}))["feature"].clone();
    run(&mut s, "timeline.copy", json!({"features": [fid]}));
    let copy = run(&mut s, "timeline.paste", json!({}))["pasted"][0]["id"].as_u64().unwrap();
    assert!(!s.doc.feature(copy).unwrap().body_names.is_empty(), "a copy names its bodies by position");
    let before = feature_boxes(&mut s, copy);
    assert_eq!(before.len(), 3, "{before:?}");
    remove(&mut s, json!([{"feature": copy, "line": "pair1"}]));
    let after = feature_boxes(&mut s, copy);
    assert_eq!(after.len(), 2, "{after:?}");
    for (name, bb) in &after {
        assert_eq!(before.get(name), Some(bb), "{name} moved or was renamed: {before:?} → {after:?}");
    }
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(feature_boxes(&mut s, copy), before);
    // Edit Feature dropping the first pair: the same.
    let kept = |pair: Value, id: &str| {
        let mut pair = pair;
        pair["id"] = json!(id);
        pair
    };
    let params = json!({"profile": tube(), "pairs": [kept(top_rail(), "pair2"), kept(bottom_rail(), "pair3")]});
    run(&mut s, "timeline.redefine", json!({"feature": copy, "command": "frame.member", "params": params}));
    let after = feature_boxes(&mut s, copy);
    assert_eq!(after.len(), 2, "{after:?}");
    for (name, bb) in &after {
        assert_eq!(before.get(name), Some(bb), "{name} moved or was renamed: {before:?} → {after:?}");
    }
}

/// A removed pair's id is never given again: not by an Edit Feature that names a lower
/// `next_pair`, not by a new Frame Member that names ids, and `timeline.edit` can't lower it.
#[test]
fn a_removed_pair_id_cannot_be_brought_back() {
    let mut s = Session::default();
    box_frame(&mut s);
    let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright(), top_rail()]}))["feature"].clone();
    remove(&mut s, json!([{"feature": fid, "line": "pair1"}]));
    let kept = json!({"id": "pair2", "a": top_rail()["a"], "b": top_rail()["b"]});
    let params = json!({"profile": tube(), "pairs": [kept, upright()], "next_pair": 1});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    let solidcraft_doc::FeatureKind::FrameMember { pairs, next_pair, .. } = member_kind(&s, &fid) else { panic!() };
    assert_eq!(pairs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pair2", "pair3"]);
    assert_eq!(next_pair, 4);
    let mut given = upright();
    given["id"] = json!("pair1");
    let e = s.execute("frame.member", &json!({"profile": tube(), "pairs": [given]})).unwrap_err().to_string();
    assert!(e.contains("given by the command"), "{e}");
    let doc = s.doc.clone();
    let mut past = stored_pairs(&s, &fid)[1].clone();
    past["id"] = json!("pair4");
    for set in [json!({"next_pair": 2}), json!({"next_pair": 3}), json!({"pairs": [past]})] {
        let e = s.execute("timeline.edit", &json!({"feature": fid, "set": set})).unwrap_err().to_string();
        assert!(e.contains("next_pair"), "{set}: {e}");
    }
    // Nor by naming it: neither edit gives the removed `pair1` to another tube.
    let mut revived = stored_pairs(&s, &fid)[1].clone();
    revived["id"] = json!("pair1");
    let e = s.execute("timeline.edit", &json!({"feature": fid, "set": {"pairs": [revived]}})).unwrap_err().to_string();
    assert!(e.contains("`pair1` was removed"), "{e}");
    let params = json!({"profile": tube(), "pairs": [{"id": "pair1", "a": upright()["a"], "b": upright()["b"]}]});
    let e = s.execute("timeline.redefine", &json!({"feature": fid, "command": "frame.member", "params": params})).unwrap_err().to_string();
    assert!(e.contains("`pair1` was removed"), "{e}");
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), serde_json::to_value(&*doc).unwrap(), "nothing changed");
}

/// A pair on a vertex of a body in another component follows that component when its
/// occurrence moves: the cached member is not served at its old place.
#[test]
fn a_pair_member_follows_a_vertex_in_a_moved_component() {
    let mut s = Session::default();
    let b = run(&mut s, "component.create", json!({"name": "B", "grounded": false}))["component"].as_u64().unwrap();
    box_frame(&mut s);
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [upright()]}));
    let members = member_boxes(&mut s);
    assert!(has_box(&members, [580.0, -20.0, 0.0], [620.0, 20.0, 400.0]), "{members:?}");
    let o = s.doc.occurrence_of(b).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": o, "translate": [0, 0, 100]}));
    let members = member_boxes(&mut s);
    assert!(has_box(&members, [580.0, -20.0, 100.0], [620.0, 20.0, 500.0]), "the upright moved with B: {members:?}");
    let mut cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(member_boxes(&mut cold), members, "as a model built from scratch");
}

/// In a rotated component a pair between two body vertices comes out as the 3D sketch line
/// member between the same points does, turned the same way.
#[test]
fn a_vertex_pair_in_a_rotated_component_turns_like_a_line_member() {
    let mut s = Session::default();
    let c = run(&mut s, "component.create", json!({"name": "Tilted", "grounded": false}))["component"].as_u64().unwrap();
    box_frame(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 400], [600, 0, 400]]}));
    run(&mut s, "sketch.finish", json!({}));
    let bar = json!({"kind": "bar", "width": 60, "height": 10});
    run(&mut s, "frame.member", json!({"sketch": "Path", "profile": bar, "body_name": "Line"}));
    run(&mut s, "frame.member", json!({"profile": bar, "pairs": [top_rail()], "body_name": "Pair"}));
    let o = s.doc.occurrence_of(c).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": o, "axis": [1, 0, 0], "angle": "30 deg"}));
    let (line, pair) = (measured(&mut s, "Line"), measured(&mut s, "Pair"));
    // Tilted 30° about X with its component: the bar's 60 mm width rises 30 mm out of the XY plane.
    assert!(line.3[2] - line.2[2] > 30.0, "{line:?}");
    assert!(near3(line.2, pair.2) && near3(line.3, pair.3), "{line:?} vs {pair:?}");
}

// ---------------------------------------------------------------------------------------------
// Corners of members between pairs of points (#4003, #15705)

/// The frame members' measures, the box left out.
fn members_measured(s: &mut Session) -> Vec<Measured> {
    bodies(s).into_iter().filter(|b| b.0 != "Box").collect()
}

/// The member whose bounding box is `lo`..`hi`.
fn member_at(bs: &[Measured], lo: [f64; 3], hi: [f64; 3]) -> Measured {
    bs.iter().find(|b| near3(b.2, lo) && near3(b.3, hi)).unwrap_or_else(|| panic!("{lo:?}..{hi:?} in {bs:?}")).clone()
}

#[test]
fn two_pair_members_meeting_at_a_point_are_mitred() {
    let mut s = Session::default();
    box_frame(&mut s);
    // The top rail and the upright end together at (600, 0, 400).
    run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [top_rail(), upright()], "corner": "mitre"}));
    let bs = members_measured(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    // Each runs out to the corner's outer side, where the 45° mitre meets x = 620 and z = 420, and
    // keeps its centreline's volume.
    let rail = member_at(&bs, [0.0, -20.0, 380.0], [620.0, 20.0, 420.0]);
    assert!(rel(rail.1, tube_area() * 600.0) < MEASURED, "{rail:?} vs {}", tube_area() * 600.0);
    let post = member_at(&bs, [580.0, -20.0, 0.0], [620.0, 20.0, 420.0]);
    assert!(rel(post.1, tube_area() * 400.0) < MEASURED, "{post:?} vs {}", tube_area() * 400.0);
}

#[test]
fn a_sketch_line_and_a_pair_member_butt_the_shorter_against_the_longer() {
    let mut s = Session::default();
    box_frame(&mut s);
    // The base's 600 bottom line and the 400 upright end together at (600, 0, 0).
    let bottom = line_at_y(&s, "Base", 0.0);
    let frame = |extra: Value| {
        let mut p = json!({"sketch": "Base", "lines": [bottom], "profile": tube(), "pairs": [upright()]});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    let fid = run(&mut s, "frame.member", frame(json!({"corner": "butt"})))["feature"].as_u64().unwrap();
    let bs = members_measured(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    // The longer line runs through to the upright's outer face; the upright stops on its top.
    let line = member_at(&bs, [0.0, -20.0, -20.0], [620.0, 20.0, 20.0]);
    assert!(rel(line.1, tube_area() * 620.0) < MEASURED, "{line:?} vs {}", tube_area() * 620.0);
    let post = member_at(&bs, [580.0, -20.0, 20.0], [620.0, 20.0, 400.0]);
    assert!(rel(post.1, tube_area() * 380.0) < MEASURED, "{post:?} vs {}", tube_area() * 380.0);
    // Flipped, by a Mitre frame's override named by the line's and the pair's ids, the pair runs through.
    run(&mut s, "timeline.delete", json!({"feature": fid}));
    let over = json!({"corner": "mitre", "corners": [{"members": ["pair1", bottom], "corner": "butt", "flip": true}]});
    run(&mut s, "frame.member", frame(over));
    let bs = members_measured(&mut s);
    member_at(&bs, [0.0, -20.0, -20.0], [580.0, 20.0, 20.0]);
    member_at(&bs, [580.0, -20.0, -20.0], [620.0, 20.0, 400.0]);
}

#[test]
fn a_tie_runs_the_sketch_line_through_then_the_first_pair() {
    let mut s = Session::default();
    box_frame(&mut s);
    // The 400 left line (along Y) and a 400 post (along Z) at the origin: equally long, equally far
    // from the sketch's X axis. The sketch's line comes before every pair.
    let left = line_at_x(&s, "Base", 0.0);
    let post = json!({"a": {"vertex": [0, 0, 0]}, "b": {"vertex": [0, 0, 400]}});
    run(&mut s, "frame.member", json!({"sketch": "Base", "lines": [left], "profile": tube(), "pairs": [post], "corner": "butt"}));
    let bs = members_measured(&mut s);
    member_at(&bs, [-20.0, -20.0, -20.0], [20.0, 400.0, 20.0]);
    member_at(&bs, [-20.0, -20.0, 20.0], [20.0, 20.0, 400.0]);
    // Two pairs alike (the upright and the 400 top side at (600, 0, 400)): the first runs through.
    let side = json!({"a": {"vertex": [600, 0, 400]}, "b": {"vertex": [600, 400, 400]}});
    for (pairs, through) in [
        (json!([upright(), side]), ([580.0, -20.0, 0.0], [620.0, 20.0, 420.0])),
        (json!([side, upright()]), ([580.0, -20.0, 380.0], [620.0, 400.0, 420.0])),
    ] {
        let fid = run(&mut s, "frame.member", json!({"profile": tube(), "pairs": pairs, "corner": "butt"}))["feature"].as_u64().unwrap();
        let bs = feature_boxes(&mut s, fid);
        assert!(bs.values().any(|b| near3(b.0, through.0) && near3(b.1, through.1)), "{pairs}: {bs:?}");
        run(&mut s, "timeline.delete", json!({"feature": fid}));
    }
}

#[test]
fn removing_a_tube_drops_the_corner_overrides_that_name_it() {
    let mut s = Session::default();
    box_frame(&mut s);
    let bottom = line_at_y(&s, "Base", 0.0);
    let p = json!({"sketch": "Base", "lines": [bottom], "profile": tube(), "pairs": [upright(), top_rail()], "corner": "mitre",
                   "corners": [{"members": [bottom, "pair1"], "corner": "butt"}, {"members": ["pair1", "pair2"], "corner": "butt"}]});
    let fid = run(&mut s, "frame.member", p)["feature"].clone();
    remove(&mut s, json!([{"feature": fid, "line": "pair1"}]));
    let solidcraft_doc::FeatureKind::FrameMember { corners, .. } = member_kind(&s, &fid) else { panic!() };
    assert!(corners.is_empty(), "{corners:?}");
    assert_eq!(member_boxes(&mut s).len(), 2, "the other tubes build: the overrides no longer point at a corner that has gone");
}

// ---------------------------------------------------------------------------------------------
// Auto corners (#4003 stage 2): each joint by its angle, and the frame's own tubes cut to fit

/// A `polyline_frame` of a 400 line along +X to the origin and a 300 line out of it at `theta` degrees, "auto".
fn auto_vee(theta: f64) -> (Session, Result<Value, String>) {
    let t = theta.to_radians();
    polyline_frame(json!([[400, 0], [0, 0], [300.0 * t.cos(), 300.0 * t.sin()]]), "auto")
}

/// The long (400, reaching x > 300) and the short member of an `auto_vee`.
fn long_and_short(s: &mut Session) -> (Measured, Measured) {
    let bs = bodies(s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    let long = bs.iter().find(|b| b.3[0] > 300.0).unwrap_or_else(|| panic!("{bs:?}")).clone();
    let short = bs.iter().find(|b| b.0 != long.0).unwrap().clone();
    (long, short)
}

#[test]
fn auto_at_20_degrees_is_an_angled_butt() {
    // The 300 leaves at 160°: 20° from running on in line with the 400 (along −X). The table's angled butt.
    let (mut s, r) = auto_vee(160.0);
    r.unwrap();
    let (long, short) = long_and_short(&mut s);
    // The long tube is cut square to its own line at the corner (x = 0), so its box runs x 0…400 and ±20
    // (the half-width) across: [0, −20, −20]…[400, 20, 20], and its volume stays A·400.
    assert!(near3(long.2, [0.0, -20.0, -20.0]) && near3(long.3, [400.0, 20.0, 20.0]), "{long:?}");
    assert!(rel(long.1, tube_area() * 400.0) < MEASURED, "{long:?} vs {}", tube_area() * 400.0);
    // The short tube is cut on that same plane x = 0, square to the long line, keeping x ≤ 0: it sits on the
    // long tube's square end, so its box ends at x = 0 (left square, it would reach 20·sin 20° = 6.84 past it).
    // The plane passes through its line at the corner, through the section's centroid, so its volume is A·300.
    assert!(short.3[0].abs() < 1e-6, "{short:?}");
    assert!(rel(short.1, tube_area() * 300.0) < MEASURED, "{short:?} vs {}", tube_area() * 300.0);
    // The 400 is drawn first, (400,0)→(0,0): the corner is its end and the 300's start.
    let st = s.model.state();
    for (b, role) in [(&long, "corner_end"), (&short, "corner_start")] {
        let names = solidcraft_doc::naming::face_names(st.body(&b.0).unwrap());
        assert!(names.iter().any(|n| n.contains(role)), "{role} in {names:?}");
    }
}

#[test]
fn auto_at_45_and_140_degrees_mitres() {
    for swing in [45.0f64, 140.0] {
        let (mut s, r) = auto_vee(180.0 - swing);
        r.unwrap_or_else(|e| panic!("{swing}: {e}"));
        let (long, short) = long_and_short(&mut s);
        // A mitre's plane passes through both lines at the corner: each keeps A × its line length.
        assert!(rel(long.1, tube_area() * 400.0) < MEASURED, "{swing}: {long:?} vs {}", tube_area() * 400.0);
        assert!(rel(short.1, tube_area() * 300.0) < MEASURED, "{swing}: {short:?} vs {}", tube_area() * 300.0);
        // The inside angle is 180 − swing; the 400 runs past the corner on its outer side (y = −20) to where the
        // bisecting plane meets it: x = −20 / tan(inside / 2): −8.284 at 45°, −54.950 at 140°.
        let reach = -20.0 / ((180.0 - swing).to_radians() / 2.0).tan();
        assert!((long.2[0] - reach).abs() < 1e-6, "{swing}: {reach} {long:?}");
    }
    // A swing of 176° (4° inside) is a mitre too sharp to finish, as stage 1 says.
    let (mut s, r) = auto_vee(4.0);
    let e = r.unwrap_err();
    assert!(e.contains("Corner at (0, 0, 0) is too sharp to finish"), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn auto_at_70_90_110_degrees_butts_and_fits() {
    for swing in [70.0f64, 90.0, 110.0] {
        let (mut s, r) = auto_vee(180.0 - swing);
        r.unwrap_or_else(|e| panic!("{swing}: {e}"));
        let (long, short) = long_and_short(&mut s);
        // T, the 400, runs through to U's outer face: h / sin φ = 20 / sin φ past the corner along its line
        // (21.28 at 70° and 110°, 20 at 90°).
        let sn = swing.to_radians().sin();
        let t = tube_area() * (400.0 + 20.0 / sn);
        assert!(rel(long.1, t) < MEASURED, "{swing}: T {long:?} vs {t}");
        // U, the 300, is fitted to T's squared-off box: flat on T's near face, a plane 20 from T's line which meets
        // U's line 20 / sin φ from the corner. So U keeps A·(300 − 20 / sin φ).
        let u = tube_area() * (300.0 - 20.0 / sn);
        assert!(rel(short.1, u) < MEASURED, "{swing}: U {short:?} vs {u}");
    }
}

#[test]
fn auto_rectangle_runs_its_long_sides_through() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto"}));
    // As stage 1's Butt rectangle: the 600s run out to the 400s' outer faces (x −20…620, A·640), and the 400s
    // stop on the 600s' near faces (y 20…380, A·360), fitted flat to the squared box tube.
    let (bottom, left) = bottom_and_left(&mut s);
    assert!((bottom.2[0] + 20.0).abs() < 1e-6 && (bottom.3[0] - 620.0).abs() < 1e-6, "{bottom:?}");
    assert!(rel(bottom.1, tube_area() * 640.0) < MEASURED, "{bottom:?} vs {}", tube_area() * 640.0);
    assert!((left.2[1] - 20.0).abs() < 1e-6 && (left.3[1] - 380.0).abs() < 1e-6, "{left:?}");
    assert!(rel(left.1, tube_area() * 360.0) < MEASURED, "{left:?} vs {}", tube_area() * 360.0);
    let st = s.model.state();
    let names = solidcraft_doc::naming::face_names(st.body(&bottom.0).unwrap());
    for role in ["corner_start", "corner_end"] {
        assert!(names.iter().any(|n| n.contains(role)), "{role} in {names:?}");
    }
    // The left's fitted faces are named for the end they finish and the line they fit: (0,400)→(0,0) starts
    // against the top and ends against the bottom.
    let names = solidcraft_doc::naming::face_names(st.body(&left.0).unwrap());
    for (role, y) in [("corner_start", 400.0), ("corner_end", 0.0)] {
        let fits = format!("{role}:{}:face:", line_at_y(&s, "Frame", y));
        assert!(names.iter().any(|n| n.contains(&fits)), "{fits} in {names:?}");
    }
    // In round tube the 400s are fitted, not cut flat: a fishmouth whose tips reach the 600s' centrelines at
    // their sides (y 0…400), A·400 less, at each end, the part inside the 600: stage 1's coped value,
    // A·360 + 2∫(20 − √(400 − z²)) dA = 129458.17.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let round = json!({"kind": "round_tube", "diameter": 40, "wall": 3});
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": round, "corner": "auto"}));
    let (_, left) = bottom_and_left(&mut s);
    assert!(left.2[1].abs() < 1e-3 && (left.3[1] - 400.0).abs() < 1e-3, "{left:?}");
    assert!(rel(left.1, 129_458.17) < MEASURED, "{left:?} vs 129458.17");
}

/// `rectangle_frame` with the profile sketch `Ring`: circles of radius 20 and 17 about its origin.
fn ring_frame(s: &mut Session) {
    rectangle_frame(s);
    run(s, "sketch.create", json!({"plane": "YZ", "name": "Ring"}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 17}));
    run(s, "sketch.finish", json!({}));
}

/// Mirror (or a half turn) puts a round section's seam on the frame's outer side, where each 600's flat end
/// touches the 400 fitted to it along the 400's outer side; SMLib's check refused the fitted 400 there ("Face Near
/// missing Seam"). Under Auto each section builds as the unmirrored, unturned one: a picked ring, a round tube and a
/// rectangular tube, with the values of `auto_rectangle_runs_its_long_sides_through`.
#[test]
fn auto_builds_a_mirrored_or_turned_section() {
    let ring = PI * (400.0 - 289.0);
    let picked = json!({"profile_sketch": "Ring", "profiles": [{"point": [18.5, 0]}]});
    let round = json!({"profile": {"kind": "round_tube", "diameter": 40, "wall": 3}});
    // (section, its area, the 400s' y range and volume): fishmouthed to the 600s' centrelines in a round section,
    // flat on their near faces in the rectangular tube.
    let sections = [
        (picked, ring, (0.0, 400.0), 129_458.17),
        (round, ring, (0.0, 400.0), 129_458.17),
        (json!({"profile": tube()}), tube_area(), (20.0, 380.0), tube_area() * 360.0),
    ];
    for (section, area, (y0, y1), short) in sections {
        for variant in [json!({"mirror": true}), json!({"angle": "180 deg"})] {
            let mut s = Session::default();
            ring_frame(&mut s);
            let mut p = json!({"sketch": "Frame", "corner": "auto"});
            for (k, v) in section.as_object().unwrap().iter().chain(variant.as_object().unwrap()) {
                p[k] = v.clone();
            }
            if let Err(e) = s.execute("frame.member", &p) {
                panic!("{p}: {e}");
            }
            assert_eq!(bodies(&mut s).len(), 4, "{p}");
            let (bottom, left) = bottom_and_left(&mut s);
            assert!(near3(bottom.2, [-20.0, -20.0, -20.0]) && near3(bottom.3, [620.0, 20.0, 20.0]), "{p}: {bottom:?}");
            assert!(rel(bottom.1, area * 640.0) < MEASURED, "{p}: {bottom:?} vs {}", area * 640.0);
            assert!((left.2[1] - y0).abs() < 1e-3 && (left.3[1] - y1).abs() < 1e-3, "{p}: {left:?}");
            assert!(rel(left.1, short) < MEASURED, "{p}: {left:?} vs {short}");
        }
    }
}

/// The New Frame Member dialog's picked ring, anchored at its bottom, turned 15° and mirrored (stage 1's dialog test
/// `a_picked_section_and_the_placement`), builds under Auto: a ring mirrored is the same ring, so each member is the
/// unmirrored one's, to the volume and the box.
#[test]
fn auto_builds_the_dialogs_mirrored_picked_ring() {
    let build = |mirror: bool| {
        let mut s = Session::default();
        ring_frame(&mut s);
        let p = json!({"sketch": "Frame", "profile_sketch": "Ring", "profiles": [{"point": [18.5, 0]}], "corner": "auto",
                       "anchor": "bottom", "angle": "15 deg", "mirror": mirror, "body_name": "Rail"});
        if let Err(e) = s.execute("frame.member", &p) {
            panic!("{p}: {e}");
        }
        bodies(&mut s)
    };
    let (mirrored, plain) = (build(true), build(false));
    let names: Vec<&str> = mirrored.iter().map(|b| b.0.as_str()).collect();
    assert_eq!(names, ["Rail", "Rail (2)", "Rail (3)", "Rail (4)"]);
    for (m, p) in mirrored.iter().zip(&plain) {
        assert!(m.0 == p.0 && rel(m.1, p.1) < MEASURED && near3(m.2, p.2) && near3(m.3, p.3), "{m:?} vs {p:?}");
    }
}

/// The 600 × 400 rectangle `Frame` with a cross-member from (300, 0) to (300, 400).
fn cross_member_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(s, "sketch.line", json!({"points": [[300, 0], [300, 400]]}));
    run(s, "sketch.finish", json!({}));
}

#[test]
fn auto_fits_a_t_junction() {
    let mut s = Session::default();
    cross_member_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 5, "{bs:?}");
    // The bottom and top pass through x = 300: the cross-member's ends are fitted flat to their near faces,
    // y = 20 and y = 380, leaving 360 of it.
    let cross = bs.iter().find(|b| b.2[0] > 250.0 && b.3[0] < 350.0).unwrap_or_else(|| panic!("{bs:?}"));
    assert!(near3(cross.2, [280.0, 20.0, -20.0]) && near3(cross.3, [320.0, 380.0, 20.0]), "{cross:?}");
    assert!(rel(cross.1, tube_area() * 360.0) < MEASURED, "{cross:?} vs {}", tube_area() * 360.0);
}

/// The 600 × 400 rectangle `Frame` with both diagonals: `d1` (0, 0)…(600, 400) drawn first, then `d2` (0, 400)…(600, 0).
fn x_brace_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(s, "sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d1"]}));
    run(s, "sketch.line", json!({"points": [[0, 400], [600, 0]], "ids": ["d2"]}));
    run(s, "sketch.finish", json!({}));
}

/// The overlap (mm³) between every two bodies.
fn overlap_all(s: &mut Session) -> f64 {
    let names: Vec<String> = bodies(s).into_iter().map(|b| b.0).collect();
    let i = run(s, "inspect.interference", json!({"bodies": names}));
    i["interferences"].as_array().map_or(0.0, |all| all.iter().map(|x| f(&x["volume_mm3"])).sum())
}

#[test]
fn an_auto_x_brace_keeps_one_diagonal_whole() {
    let mut s = Session::default();
    x_brace_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "auto", "names": {"d1": "D1", "d2": "D2"}}));
    // The four sides, d1 whole, d2 in two pieces.
    assert_eq!(bodies(&mut s).len(), 7, "{:?}", bodies(&mut s));
    // The diagonals tie (same length, same angle off X): d1, drawn first, is laid first and stays whole; d2 is cut at
    // (300, 200), its first piece keeping its name, the later one named after it and the whole tube.
    let (d1, d2, d2_d1) = (measured(&mut s, "D1"), measured(&mut s, "D2"), measured(&mut s, "D2 · D1"));
    // Worked by hand. s along d1 (direction (3, 2)/√13), u across it (n = (−2, 3)/√13); L = √(600² + 400²) = 721.110.
    // Square tube, A = 40² − 34² = 444: material of height 40 at 17 < |u| ≤ 20, 6 (two walls) at |u| ≤ 17.
    // At (0, 0) d1 is fitted to the bottom tube (y ≤ 20 goes) and the left one (x ≤ 20): at u it starts at the larger
    // of s_x(u) = (20 + 0.5547u)/0.83205 and s_y(u) = (20 − 0.83205u)/0.5547, which cross at u = 5.547. That end
    // takes C = ∫ height(u)·max(s_x, s_y) du = 40·191.417 + 6·1006.616 + 6·361.380 + 40·109.111 = 20229.04 (each
    // piece of the integrand linear, so exact by the trapezium rule). The far corner is the same turned half round.
    // d1 = A·L − 2C = 320172.95 − 40458.08 = 279714.87; its box x 20…580, y 20…380.
    assert!(rel(d1.1, 279_714.873_823) < 1e-6, "{d1:?}");
    assert!(near3(d1.2, [20.0, 20.0, -20.0]) && near3(d1.3, [580.0, 380.0, 20.0]), "{d1:?}");
    // Each piece of d2 runs half of it, L/2 = 360.555, from its corner (fitted the same way: C) to d1's near face.
    // The diagonals cross at sin θ = 12/13, so that face is 20/(12/13) = 21.667 back along d2's centre line, and the
    // one plane cut takes A × 21.667 off: A·(360.555 − 21.667) − C = 130237.44. The first piece's corners on that
    // face are at x = 300 (u = 20 across d2) and y = 200 (u = −20): its box is x 20…300, y 200…380; the second
    // piece, turned half round about (300, 200), x 300…580, y 20…200.
    for (piece, lo, hi) in [(&d2, [20.0, 200.0, -20.0], [300.0, 380.0, 20.0]), (&d2_d1, [300.0, 20.0, -20.0], [580.0, 200.0, 20.0])] {
        assert!(rel(piece.1, 130_237.436_911) < 1e-6, "{piece:?}");
        assert!(near3(piece.2, lo) && near3(piece.3, hi), "{piece:?}");
    }
    // Nothing overlaps: each piece stops at d1, each end at a corner at the tubes laid there before it.
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the frame overlaps by {o} mm³");
    // The whole diagonal is exactly as it is without the other one.
    let mut alone = Session::default();
    run(&mut alone, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut alone, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(&mut alone, "sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d1"]}));
    run(&mut alone, "sketch.finish", json!({}));
    run(&mut alone, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "auto", "names": {"d1": "D1"}}));
    let lone = measured(&mut alone, "D1");
    assert!(rel(lone.1, d1.1) < 1e-9 && near3(lone.2, d1.2) && near3(lone.3, d1.3), "{lone:?} {d1:?}");
}

#[test]
fn adding_and_removing_a_crossing_keeps_other_names() {
    let mut s = Session::default();
    x_brace_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "names": {"d1": "D1", "d2": "D2"}}));
    let before = built(&mut s);
    assert_eq!(before.0.len(), 7, "{before:?}");
    // A brace up x = 150 between the rails: square to them, it is laid before the diagonals (33.7° off square), stays
    // whole and cuts d1 at (150, 100) and d2 at (150, 300). d1 is two pieces, d2 three: 4 + 1 + 2 + 3 bodies.
    run(&mut s, "sketch.edit", json!({"sketch": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[150, 0], [150, 400]], "ids": ["brace"]}));
    run(&mut s, "sketch.finish", json!({}));
    let during = built(&mut s);
    assert_eq!(during.0.len(), 10, "{:?}", during.0.keys());
    // Every body keeps its name: a cut line's first piece keeps the line's, a new piece is named after its line and
    // the brace. The sides, untouched, keep their faces' names too.
    for name in before.0.keys() {
        assert!(during.0.contains_key(name), "{name} is gone: {:?}", during.0.keys());
    }
    for name in before.0.keys().filter(|n| !n.starts_with('D')) {
        assert_eq!(before.1[name], during.1[name], "{name}");
    }
    let brace = during.0.keys().find(|n| !before.0.contains_key(*n) && !n.contains('·')).unwrap().clone();
    for piece in [format!("D1 · {brace}"), format!("D2 · {brace}")] {
        assert!(during.0.contains_key(&piece), "no {piece}: {:?}", during.0.keys());
    }
    // Taking the brace out again brings back the same bodies, names and face names.
    delete_line(&mut s, "Frame", "brace");
    let after = built(&mut s);
    assert_eq!(before.0.keys().collect::<Vec<_>>(), after.0.keys().collect::<Vec<_>>());
    for (name, (v, lo, hi)) in &before.0 {
        let (av, alo, ahi) = after.0[name];
        assert!(rel(av, *v) < 1e-9 && near3(alo, *lo) && near3(ahi, *hi), "{name}: {:?} vs {:?}", before.0[name], after.0[name]);
    }
    assert_eq!(before.1, after.1);
}

/// A plus on `Plus`: h (0, 0)…(400, 0), the longer, laid first and whole; v (200, −100)…(200, `top`) cut at (200, 0),
/// its later piece `V · H` running from there to y = `top`. h's faces are at y = ±20. With the frame's feature.
fn short_plus(top: f64) -> (Session, Value) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[200.0, -100.0], [200.0, top]], "ids": ["v"]}));
    run(&mut s, "sketch.finish", json!({}));
    let fid =
        run(&mut s, "frame.member", json!({"sketch": "Plus", "profile": tube(), "corner": "auto", "names": {"h": "H", "v": "V"}}))["feature"].clone();
    (s, fid)
}

#[test]
fn a_short_piece_past_a_crossing_keeps_what_lies_past_the_tube() {
    // Ruling C4: an overhang past a crossing is an ordinary layout. The piece keeps what lies past h's far face, y = 20.
    for top in [10.0, 30.0, 50.0] {
        let (mut s, _) = short_plus(top);
        let (got, faces) = built(&mut s);
        // v's first piece runs from its free end to h's near face, y = −20, whatever the overhang.
        assert!(rel(got["V"].0, tube_area() * 80.0) < MEASURED, "{top}: {got:?}");
        assert!(near3(got["V"].1, [180.0, -100.0, -20.0]) && near3(got["V"].2, [220.0, -20.0, 20.0]), "{top}: {got:?}");
        if top < 20.0 {
            // Wholly inside h: no body for it, and nothing overlaps.
            assert_eq!(got.keys().collect::<Vec<_>>(), ["H", "V"], "{top}");
        } else {
            // Past h: y 20…top, fitted flat to h's face.
            let piece = &got["V · H"];
            assert!(rel(piece.0, tube_area() * (top - 20.0)) < MEASURED, "{top}: {piece:?}");
            assert!(near3(piece.1, [180.0, 20.0, -20.0]) && near3(piece.2, [220.0, top, 20.0]), "{top}: {piece:?}");
            assert!(faces["V · H"].iter().any(|n| n.contains("corner_start:h:face:")), "{top}: {:?}", faces["V · H"]);
        }
        let o = overlap_all(&mut s);
        assert!(o < 1e-3, "{top}: the tubes overlap by {o} mm³");
    }
}

#[test]
fn a_rename_keeps_names_pinned_when_a_piece_has_no_body() {
    // v's piece past h lies wholly inside h and has no body: a rename still pins each name to its member (the
    // piece keeps its own), rather than naming the frame's bodies by position from then on.
    let (mut s, fid) = short_plus(10.0);
    run(&mut s, "body.rename", json!({"body": "V", "name": "Post"}));
    let names = pinned_names(&s, &fid);
    assert_eq!(names.get("v").map(String::as_str), Some("Post"), "{names:?}");
    assert_eq!(names.get("h").map(String::as_str), Some("H"), "{names:?}");
    assert_eq!(names.get("v>h").map(String::as_str), Some("V · H"), "{names:?}");
    assert!(s.doc.feature(fid.as_u64().unwrap()).unwrap().body_names.is_empty());
    let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert_eq!(made, ["H", "Post"]);
}

#[test]
fn an_error_about_a_piece_names_its_line() {
    // The piece of v past h, (200, 0)…(200, 15), is the coped tube at its corner with w (200, 15)…(400, 15), which
    // runs through: it lies wholly inside w, and the error names it by its line and the tube it is past, never `v>h`.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[200, -100], [200, 15]], "ids": ["v"]}));
    run(&mut s, "sketch.line", json!({"points": [[200, 15], [400, 15]], "ids": ["w"]}));
    run(&mut s, "sketch.finish", json!({}));
    // w first: it runs through (an override's order is absolute, #17854).
    let corners = json!([{"members": ["w", "v"], "kind": "butt"}]);
    let p = json!({"sketch": "Plus", "profile": tube(), "corner": "auto", "coped": true, "corners": corners});
    let e = s.execute("frame.member", &p).unwrap_err().to_string();
    assert!(e.contains("the piece of `v` past `h` was cut away completely") && !e.contains("v>h"), "{e}");
}

#[test]
fn end_cap_goes_on_a_pieces_line_end_and_not_its_cut_end() {
    // A plus: h (0, 0)…(400, 0), the longer, laid first and whole; v (200, −100)…(200, 100) cut at (200, 0). Its
    // later piece `V · H` runs from h's near face y = 20 (fitted) to its line's end y = 100 (square, open).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[200, -100], [200, 100]], "ids": ["v"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Plus", "profile": square_tube(), "corner": "auto", "names": {"h": "H", "v": "V"}}));
    let piece = measured(&mut s, "V · H");
    assert!(near3(piece.2, [180.0, 20.0, -20.0]) && near3(piece.3, [220.0, 100.0, 20.0]), "{piece:?}");
    // On its line's end: the profile's outside, 40 × 40, 6 thick, beyond y = 100.
    let top = face_pick(&s, "V · H", [218.5, 100.0, 0.0], [0.0, 1.0, 0.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let cap = measured(&mut s, "V · H End Cap");
    assert!(near3(cap.2, [180.0, 100.0, -20.0]) && near3(cap.3, [220.0, 106.0, 20.0]), "{cap:?}");
    // Its cut end at the crossing is fitted to h: not an open end.
    let cut = face_pick(&s, "V · H", [218.5, 20.0, 0.0], [0.0, -1.0, 0.0]);
    let e = s.execute("frame.end_cap", &json!({"faces": [cut], "thickness": 6})).unwrap_err().to_string();
    assert!(e.contains("not an open end"), "{e}");
}

#[test]
fn a_crossing_tubes_far_end_override_still_applies() {
    // Review Focus 2: a (0, 0)…(600, 0) and b (600, 0)…(600, 400) meet at (600, 0), where an override may run b through
    // (Flip). A brace c (300, −400)…(300, 400), longer than a, is laid first and cuts a at (300, 0): a's far end is
    // now its piece `a>c`'s, and the override, naming the lines a and b, still finds it.
    let build = |flip: bool| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "L"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [600, 0]], "ids": ["a"]}));
        run(&mut s, "sketch.line", json!({"points": [[600, 0], [600, 400]], "ids": ["b"]}));
        run(&mut s, "sketch.line", json!({"points": [[300, -400], [300, 400]], "ids": ["c"]}));
        run(&mut s, "sketch.finish", json!({}));
        let corners = json!([{"members": ["a", "b"], "corner": "auto", "flip": flip}]);
        let names = json!({"a": "a", "b": "b", "c": "c"});
        run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "auto", "names": names, "corners": corners}));
        boxes(&mut s)
    };
    // Unflipped, the longer a (by its whole line) runs through: its piece out to b's outer face x = 620, b fitted
    // to its near face y = 20.
    let bs = build(false);
    assert!(near3(bs["a · c"].0, [320.0, -20.0, -20.0]) && near3(bs["a · c"].1, [620.0, 20.0, 20.0]), "{bs:?}");
    assert!(near3(bs["b"].0, [580.0, 20.0, -20.0]) && near3(bs["b"].1, [620.0, 400.0, 20.0]), "{bs:?}");
    // Flipped, b runs through to a's outer face y = −20, and a's piece is fitted to b's near face x = 580.
    let bs = build(true);
    assert!(near3(bs["a · c"].0, [320.0, -20.0, -20.0]) && near3(bs["a · c"].1, [580.0, 20.0, 20.0]), "{bs:?}");
    assert!(near3(bs["b"].0, [580.0, -20.0, -20.0]) && near3(bs["b"].1, [620.0, 400.0, 20.0]), "{bs:?}");
    // a's first piece runs from its free start to c's near face x = 280, either way.
    assert!(near3(bs["a"].0, [0.0, -20.0, -20.0]) && near3(bs["a"].1, [280.0, 20.0, 20.0]), "{bs:?}");
}

#[test]
fn three_tubes_through_a_point_and_one_ending_there_never_overlap() {
    // Review Focus 4: d (0, 0)…(600, 400), h (0, 200)…(600, 200) and v (300, 0)…(300, 400) all pass through
    // (300, 200), where e (300, 200)…(450, −100) ends. No closed shape: laid longest first, d (721), h, v, e. d stays
    // whole, h and v are cut there; every piece end and e's end is fitted to d and the ends laid before it, never to a
    // sibling: 1 + 2 + 2 + 1 bodies, none overlapping.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Star"}));
    for (id, points) in [("d", [[0, 0], [600, 400]]), ("h", [[0, 200], [600, 200]]), ("v", [[300, 0], [300, 400]]), ("e", [[300, 200], [450, -100]])]
    {
        run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Star", "profile": tube(), "corner": "auto", "names": {"d": "d", "h": "h", "v": "v", "e": "e"}}));
    let names: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert_eq!(names, ["d", "h", "h · d", "v", "v · d", "e"]);
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the tubes overlap by {o} mm³");
}

/// The 600 × 400 rectangle `Frame` and its diagonal `d` (0, 0)…(600, 400).
fn diagonal_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(s, "sketch.line", json!({"points": [[0, 0], [600, 400]], "ids": ["d"]}));
    run(s, "sketch.finish", json!({}));
}

/// A body's volume and box.
type Tube = (f64, [f64; 3], [f64; 3]);

/// Each body's volume and box but `skip`'s, sorted by box.
fn volumes_and_boxes(s: &mut Session, skip: &[&str]) -> Vec<Tube> {
    let mut out: Vec<Tube> = bodies(s).into_iter().filter(|b| !skip.contains(&b.0.as_str())).map(|b| (b.1, b.2, b.3)).collect();
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.2.partial_cmp(&b.2).unwrap()));
    out
}

/// The plain 600 × 400 rectangle's four tubes finished `corner`: volume and box, sorted by box.
fn plain_rectangle(profile: Value, corner: &str) -> Vec<Tube> {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": profile, "corner": corner}));
    volumes_and_boxes(&mut s, &[])
}

fn assert_same_tubes(got: &[Tube], want: &[Tube], what: &str) {
    assert_eq!(got.len(), want.len(), "{what}: {got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(rel(g.0, w.0) < 1e-9 && near3(g.1, w.1) && near3(g.2, w.2), "{what}: {got:?} vs {want:?}");
    }
}

/// Square tube 40 × 40 × 3 (no radii): A = 40² − 34².
const SQUARE_TUBE_AREA: f64 = 444.0;
/// The diagonal (0, 0)…(600, 400) in square tube fitted at both ends to the tubes filling x ≤ 20 and y ≤ 20 round
/// (0, 0) (and x ≥ 580 and y ≥ 380 round (600, 400)), as worked by hand in `an_auto_x_brace_keeps_one_diagonal_whole`:
/// A·L − 2C.
const FITTED_DIAGONAL: f64 = 279_714.873_823;

#[test]
fn a_diagonal_into_a_mitred_rectangle_fits_the_corners() {
    // Three tubes end at (0, 0) and at (600, 400): two sides and d. The sides, laid first (outer tubes, longest
    // first), make the mitre exactly as the plain rectangle's (stage 1: the bottom runs -20…620 in x, A·600, the left
    // -20…420 in y, A·400); d is cut to fit them, whatever Fit Ends says. The mitred bottom (|y| ≤ 20, x ≥ y) and
    // left (|x| ≤ 20, y ≥ x) fill y ≤ 20 or x ≤ 20 round the corner, as Auto's butted sides do: d is the diagonal
    // Auto fits there, FITTED_DIAGONAL, its box x 20…580, y 20…380, and no tube overlaps another.
    let plain = plain_rectangle(square_tube(), "mitre");
    let a = SQUARE_TUBE_AREA;
    for (lo, hi, len) in [([-20.0, -20.0, -20.0], [620.0, 20.0, 20.0], 600.0), ([-20.0, -20.0, -20.0], [20.0, 420.0, 20.0], 400.0)] {
        assert!(plain.iter().any(|b| rel(b.0, a * len) < MEASURED && near3(b.1, lo) && near3(b.2, hi)), "{plain:?}");
    }
    for fit_ends in [false, true] {
        let mut s = Session::default();
        diagonal_frame(&mut s);
        let p = json!({"sketch": "Frame", "profile": square_tube(), "corner": "mitre", "fit_ends": fit_ends, "names": {"d": "D"}});
        run(&mut s, "frame.member", p);
        assert_same_tubes(&volumes_and_boxes(&mut s, &["D"]), &plain, &format!("fit_ends {fit_ends}"));
        let d = measured(&mut s, "D");
        assert!(rel(d.1, FITTED_DIAGONAL) < 1e-6, "fit_ends {fit_ends}: {d:?}");
        assert!(near3(d.2, [20.0, 20.0, -20.0]) && near3(d.3, [580.0, 380.0, 20.0]), "fit_ends {fit_ends}: {d:?}");
        let o = overlap_all(&mut s);
        assert!(o < 1e-3, "fit_ends {fit_ends}: the frame overlaps by {o} mm³");
    }
}

#[test]
fn a_diagonal_into_a_butted_or_coped_rectangle_fits_the_corners() {
    // As the mitre: the sides make the corner exactly as the plain rectangle's; d is fitted to them. The butted (or
    // coped) sides fill y ≤ 20 or x ≤ 20 round the corner too (a coped tube's tool runs to its line's end), so d's
    // box is x 20…580, y 20…380. An older `coped_butt` is Butt with Coped (#17854), which fits d to the sides' real
    // shape: d keeps what lies in their outer radii (6), its box x 14…586, y 14…386.
    for (corner, lo, hi) in [("butt", 20.0, 380.0), ("coped_butt", 14.0, 386.0)] {
        let plain = plain_rectangle(tube(), corner);
        let mut s = Session::default();
        diagonal_frame(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": corner, "names": {"d": "D"}}));
        assert_same_tubes(&volumes_and_boxes(&mut s, &["D"]), &plain, corner);
        let d = measured(&mut s, "D");
        assert!(near3(d.2, [lo, lo, -20.0]) && near3(d.3, [600.0 - lo, hi, 20.0]), "{corner}: {d:?}");
        let o = overlap_all(&mut s);
        assert!(o < 1e-3, "{corner}: the frame overlaps by {o} mm³");
    }
}

/// The X brace (`x_brace_frame`) in square tube finished `corner` (Mitre, Butt or Coped), d1 named `D1`, d2 `D2`,
/// crossings split as Auto splits them (#4003): the sides as the plain rectangle's (the diagonals' ends there are fitted
/// to them, the 3+-end rule), d1 whole and fitted into its corners, FITTED_DIAGONAL, box x 20…580, y 20…380, and d2 cut
/// at (300, 200) into `D2` and `D2 · D1`, each fitted to its corner and to d1, as worked by hand in
/// `an_auto_x_brace_keeps_one_diagonal_whole`: A·(360.555 − 21.667) − C = 130237.44, boxes x 20…300, y 200…380 and
/// x 300…580, y 20…200. No tube overlaps another.
fn assert_x_brace_split(corner: &str) {
    let mut s = Session::default();
    x_brace_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": corner, "names": {"d1": "D1", "d2": "D2"}}));
    assert_eq!(bodies(&mut s).len(), 7, "{corner}: {:?}", bodies(&mut s));
    assert_same_tubes(&volumes_and_boxes(&mut s, &["D1", "D2", "D2 · D1"]), &plain_rectangle(square_tube(), corner), corner);
    let d1 = measured(&mut s, "D1");
    assert!(rel(d1.1, FITTED_DIAGONAL) < 1e-6, "{corner}: {d1:?}");
    assert!(near3(d1.2, [20.0, 20.0, -20.0]) && near3(d1.3, [580.0, 380.0, 20.0]), "{corner}: {d1:?}");
    for (name, lo, hi) in [("D2", [20.0, 200.0, -20.0], [300.0, 380.0, 20.0]), ("D2 · D1", [300.0, 20.0, -20.0], [580.0, 200.0, 20.0])] {
        let piece = measured(&mut s, name);
        assert!(rel(piece.1, 130_237.436_911) < 1e-6, "{corner}: {piece:?}");
        assert!(near3(piece.2, lo) && near3(piece.3, hi), "{corner}: {piece:?}");
    }
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "{corner}: the frame overlaps by {o} mm³");
}

#[test]
fn a_mitre_x_brace_splits_one_diagonal() {
    assert_x_brace_split("mitre");
}

#[test]
fn a_butt_x_brace_splits_one_diagonal() {
    assert_x_brace_split("butt");
}

#[test]
fn a_coped_butt_x_brace_splits_one_diagonal() {
    assert_x_brace_split("coped_butt");
}

#[test]
fn a_crossing_override_with_flip_keeps_the_other_diagonal_whole_on_a_mitre_frame() {
    assert_d2_stays_whole(json!([{"members": ["d1", "d2"], "corner": "mitre", "flip": true}]));
}

#[test]
fn an_override_order_picks_the_crossing_line_that_stays_whole() {
    // The order alone, no Flip: d2, first, stays whole.
    assert_d2_stays_whole(json!([{"members": ["d2", "d1"], "kind": "mitre"}]));
}

fn assert_d2_stays_whole(corners: Value) {
    // A crossing override names the lines through the point; on a Mitre frame too only its order counts: d2 stays
    // whole and d1 is cut, `D1` and `D1 · D2`, the X brace turned over (d2 is d1 mirrored across x = 300): the same
    // volumes, d2's box x 20…580, y 20…380, d1's pieces x 20…300, y 20…200 and x 300…580, y 200…380.
    let mut s = Session::default();
    x_brace_frame(&mut s);
    let p = json!({"sketch": "Frame", "profile": square_tube(), "corner": "mitre", "corners": corners, "names": {"d1": "D1", "d2": "D2"}});
    run(&mut s, "frame.member", p);
    assert_eq!(bodies(&mut s).len(), 7, "{:?}", bodies(&mut s));
    let d2 = measured(&mut s, "D2");
    assert!(rel(d2.1, FITTED_DIAGONAL) < 1e-6 && near3(d2.2, [20.0, 20.0, -20.0]) && near3(d2.3, [580.0, 380.0, 20.0]), "{d2:?}");
    for (name, lo, hi) in [("D1", [20.0, 20.0, -20.0], [300.0, 200.0, 20.0]), ("D1 · D2", [300.0, 200.0, -20.0], [580.0, 380.0, 20.0])] {
        let piece = measured(&mut s, name);
        assert!(rel(piece.1, 130_237.436_911) < 1e-6 && near3(piece.2, lo) && near3(piece.3, hi), "{piece:?}");
    }
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the frame overlaps by {o} mm³");
}

#[test]
fn mitre_crossings_are_the_same_whatever_the_pick_order() {
    let build = |reverse: bool| {
        let mut s = Session::default();
        x_brace_frame(&mut s);
        let mut lines: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).collect();
        assert_eq!(lines.len(), 6);
        if reverse {
            lines.reverse();
        }
        let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "mitre", "lines": lines, "names": names}));
        built(&mut s)
    };
    let ((drawn, drawn_faces), (picked, picked_faces)) = (build(false), build(true));
    // d1 stays whole and d2 is cut in two whichever way the lines are picked.
    assert!(drawn.len() == 7 && drawn.contains_key("d1") && drawn.contains_key("d2 · d1"), "{drawn:?}");
    assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
    for (line, (v, lo, hi)) in &drawn {
        let (pv, plo, phi) = picked[line];
        assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{line}: drawn {drawn:?} picked {picked:?}");
    }
    assert_eq!(drawn_faces, picked_faces);
}

#[test]
fn a_none_x_brace_keeps_its_crossings_overlapping() {
    // None finishes nothing and cuts no crossing: six square tubes, each A·L. The diagonals (L = √(600² + 400²)) are
    // whole, each box its line ± (11.094, 16.641) across it (20 × (2, 3)/√13), and overlap where they cross.
    let mut s = Session::default();
    x_brace_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "none", "names": {"d1": "D1", "d2": "D2"}}));
    assert_eq!(bodies(&mut s).len(), 6, "{:?}", bodies(&mut s));
    let (h, w) = (11.094_003_924_504_584, 16.641_005_886_756_876);
    let l = 721.110_255_092_797_8;
    for (name, lo, hi) in [("D1", [-h, -w, -20.0], [600.0 + h, 400.0 + w, 20.0]), ("D2", [-h, -w, -20.0], [600.0 + h, 400.0 + w, 20.0])] {
        let d = measured(&mut s, name);
        assert!(rel(d.1, SQUARE_TUBE_AREA * l) < 1e-6 && near3(d.2, lo) && near3(d.3, hi), "{d:?}");
    }
    assert_same_tubes(&volumes_and_boxes(&mut s, &["D1", "D2"]), &plain_rectangle(square_tube(), "none"), "sides");
    assert!(overlap_all(&mut s) > 1000.0);
}

/// A sketch `Lines` of the lines `(id, [start, end])`, and its square tube members finished `corner`, each named its
/// id in capitals.
fn lines_frame(lines: &[(&str, [[f64; 2]; 2])], corner: &str) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Lines"}));
    for (id, points) in lines {
        run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let names: serde_json::Map<String, Value> = lines.iter().map(|(id, _)| (id.to_string(), json!(id.to_uppercase()))).collect();
    run(&mut s, "frame.member", json!({"sketch": "Lines", "profile": square_tube(), "corner": corner, "names": names}));
    s
}

#[test]
fn a_butt_corner_between_pieces_runs_the_longer_line_through() {
    // A chevron, a (0, 0)…(450, 0) and b (0, 0)…(0, 350), and a rail r (100, −150)…(−50, 300) along 3x + y = 150,
    // crossing a at (50, 0) and b at (0, 150). r is laid first (longest, 474.3), so it stays whole and a and b are
    // cut, leaving pieces of 50 (A) and 150 (B) at the apex. As Auto ranks them, by their lines, a (450) runs
    // through, not B's longer piece: A keeps x ≥ −20 (b's outer face), B y ≥ 20 (A's near face). Each is fitted
    // to r, whose tube fills 3x + y ≥ 150 − 20√10 = 86.754 near them: A ends at x = (86.754 + 20)/3 = 35.585 (at
    // y = −20), B at y = 86.754 + 60 = 146.754 (at x = −20).
    let mut s =
        lines_frame(&[("a", [[0.0, 0.0], [450.0, 0.0]]), ("b", [[0.0, 0.0], [0.0, 350.0]]), ("r", [[100.0, -150.0], [-50.0, 300.0]])], "butt");
    let near = 150.0 - 20.0 * 10f64.sqrt();
    let (a, b) = (measured(&mut s, "A"), measured(&mut s, "B"));
    assert!(near3(a.2, [-20.0, -20.0, -20.0]) && near3(a.3, [(near + 20.0) / 3.0, 20.0, 20.0]), "{a:?}");
    assert!(near3(b.2, [-20.0, 20.0, -20.0]) && near3(b.3, [20.0, near + 60.0, 20.0]), "{b:?}");
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the frame overlaps by {o} mm³");
}

#[test]
fn three_ends_at_a_point_take_their_lines_laying_order_past_a_crossing() {
    // x (0, 0)…(500, 0), y (0, 0)…(0, 450) and z (0, 0)…(0, −300) end at the origin; the rail r (50, −300)…(50, 300),
    // laid first (longest), crosses x, leaving a 50 piece X at the origin. Laid by their lines, as Auto lays them,
    // x and y come first and make the corner, x (500) running through: X keeps x ≥ −20 (y's outer face) and ends at
    // r's tube (x = 30), Y keeps y ≥ 20, and z is fitted to them, y ≤ −20. (Laid by piece, y and z would make an
    // in-line corner and X be fitted to them.)
    let lines = [
        ("x", [[0.0, 0.0], [500.0, 0.0]]),
        ("y", [[0.0, 0.0], [0.0, 450.0]]),
        ("z", [[0.0, 0.0], [0.0, -300.0]]),
        ("r", [[50.0, -300.0], [50.0, 300.0]]),
    ];
    let mut s = lines_frame(&lines, "butt");
    for (name, lo, hi) in [
        ("X", [-20.0, -20.0, -20.0], [30.0, 20.0, 20.0]),
        ("Y", [-20.0, 20.0, -20.0], [20.0, 450.0, 20.0]),
        ("Z", [-20.0, -300.0, -20.0], [20.0, -20.0, 20.0]),
    ] {
        let m = measured(&mut s, name);
        assert!(near3(m.2, lo) && near3(m.3, hi), "{m:?}");
    }
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the frame overlaps by {o} mm³");
}

#[test]
fn a_post_ending_where_a_mitre_x_brace_crosses_is_fitted() {
    // The X brace (`assert_x_brace_split`) and a post p (300, 0)…(300, 200) ending where the diagonals cross. As
    // under Auto, the post (laid after them) is fitted there to d1 and d2's two pieces, whose filled tubes cover
    // y ≥ 200 − 20√13/3 − (2/3)|x − 300| = 175.963 − (2/3)|x − 300| over it; its foot, on the bottom's middle, is
    // fitted to the bottom (ruling O1), flat on its face y = 20. Its box x 280…320, y 20…175.963, and A·155.963 less
    // the V, 2·(2/3)·(6·17²/2 + 40·(20² − 17²)/2) = 4116. The sides, d1 and d2's split are the X brace's.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    for (id, points) in [("d1", [[0, 0], [600, 400]]), ("d2", [[0, 400], [600, 0]]), ("p", [[300, 0], [300, 200]])] {
        run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "mitre", "names": {"d1": "D1", "d2": "D2", "p": "P"}}));
    assert_eq!(bodies(&mut s).len(), 8, "{:?}", bodies(&mut s));
    assert_same_tubes(&volumes_and_boxes(&mut s, &["D1", "D2", "D2 · D1", "P"]), &plain_rectangle(square_tube(), "mitre"), "sides");
    let d1 = measured(&mut s, "D1");
    assert!(rel(d1.1, FITTED_DIAGONAL) < 1e-6 && near3(d1.2, [20.0, 20.0, -20.0]) && near3(d1.3, [580.0, 380.0, 20.0]), "{d1:?}");
    for (name, lo, hi) in [("D2", [20.0, 200.0, -20.0], [300.0, 380.0, 20.0]), ("D2 · D1", [300.0, 20.0, -20.0], [580.0, 200.0, 20.0])] {
        let piece = measured(&mut s, name);
        assert!(rel(piece.1, 130_237.436_911) < 1e-6 && near3(piece.2, lo) && near3(piece.3, hi), "{piece:?}");
    }
    let top = 200.0 - 20.0 * 13f64.sqrt() / 3.0;
    let p = measured(&mut s, "P");
    assert!(rel(p.1, SQUARE_TUBE_AREA * (top - 20.0) - 4116.0) < 1e-6 && near3(p.2, [280.0, 20.0, -20.0]) && near3(p.3, [320.0, top, 20.0]), "{p:?}");
    let bottom = bodies(&mut s).into_iter().find(|b| b.3[1] < 21.0 && b.2[0] < 0.0).unwrap().0;
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["P", "D1", "D2", "D2 · D1", bottom]}));
    let o: f64 = i["interferences"].as_array().map_or(0.0, |all| all.iter().map(|x| f(&x["volume_mm3"])).sum());
    assert!(o < 1e-3, "the post, the diagonals and the bottom overlap by {o} mm³");
}

#[test]
fn a_diagonal_into_a_mitred_or_butted_rectangle_is_the_same_whatever_the_pick_order() {
    for corner in ["mitre", "butt"] {
        let build = |reverse: bool| {
            let mut s = Session::default();
            diagonal_frame(&mut s);
            let mut lines: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).collect();
            assert_eq!(lines.len(), 5);
            if reverse {
                lines.reverse();
            }
            let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
            run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": corner, "lines": lines, "names": names}));
            built(&mut s)
        };
        let ((drawn, drawn_faces), (picked, picked_faces)) = (build(false), build(true));
        assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
        for (line, (v, lo, hi)) in &drawn {
            let (pv, plo, phi) = picked[line];
            assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{corner} {line}: drawn {drawn:?} picked {picked:?}");
        }
        assert_eq!(drawn_faces, picked_faces, "{corner}");
        // d's fitted faces are named for its ends and the sides they fit.
        for role in ["corner_start:", "corner_end:"] {
            assert!(drawn_faces["d"].iter().any(|n| n.contains(role)), "{corner} {role}: {:?}", drawn_faces["d"]);
        }
    }
}

#[test]
fn a_diagonal_in_a_none_frame_is_left_square() {
    // None finishes no corner: five square tubes, each A·L, the diagonal's box its line ± (11.094, 16.641) across
    // it (20 × (2, 3)/√13).
    let mut s = Session::default();
    diagonal_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "none", "names": {"d": "D"}}));
    let (h, w) = (11.094_003_924_504_584, 16.641_005_886_756_876);
    let l = 721.110_255_092_797_8;
    let d = measured(&mut s, "D");
    assert!(rel(d.1, tube_area() * l) < MEASURED && near3(d.2, [-h, -w, -20.0]) && near3(d.3, [600.0 + h, 400.0 + w, 20.0]), "{d:?}");
    let sides = volumes_and_boxes(&mut s, &["D"]);
    assert_eq!(sides.len(), 4, "{sides:?}");
    for (len, lo, hi) in [
        (400.0, [-20.0, 0.0, -20.0], [20.0, 400.0, 20.0]),
        (600.0, [0.0, -20.0, -20.0], [600.0, 20.0, 20.0]),
        (600.0, [0.0, 380.0, -20.0], [600.0, 420.0, 20.0]),
        (400.0, [580.0, 0.0, -20.0], [620.0, 400.0, 20.0]),
    ] {
        assert!(sides.iter().any(|b| rel(b.0, tube_area() * len) < MEASURED && near3(b.1, lo) && near3(b.2, hi)), "{len} {lo:?}: {sides:?}");
    }
    assert!(overlap_all(&mut s) > 1000.0);
}

#[test]
fn a17_pairs_on_a_group_tube_are_fitted_with_fit_ends_off() {
    // Ruling A17 holds whatever the Fit Ends setting: `pairs_ending_on_their_own_frames_tube_are_fitted_to_it_not_chained`
    // with Fit Ends off. The pairs' node (300, 0, 0) is on the bottom tube's middle: each brace is fitted to it
    // (y ≥ 20) and the later one to the earlier, never mitred inside it. No tube of the frame overlaps another.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    points(&mut s, &[("N", [300.0, 0.0, 0.0]), ("U", [300.0, 300.0, 0.0]), ("D", [500.0, 200.0, 0.0])]);
    let pairs = json!([pair("N", "U"), pair("N", "D")]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "pairs": pairs, "profile": tube(), "corner": "auto", "fit_ends": false, "body_name": "B"}));
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 6, "{bs:?}");
    let braces: Vec<&Measured> = bs.iter().filter(|b| b.3[1] < 350.0 && b.2[0] > 250.0 && b.3[0] < 560.0).collect();
    assert_eq!(braces.len(), 2, "{bs:?}");
    for b in braces {
        assert!(b.2[1] >= 20.0 - 1e-6, "{b:?}");
    }
    let o = overlap(&mut s, "B");
    assert!(o < 1e-3, "the frame's tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn a_dense_auto_grid_is_refused_not_built() {
    // Hostile input: 40 lines along X (0…410) and 40 along Y (0…400), 10 mm apart, cross 1600 times. Under Auto each
    // crossing cuts one of its two lines: 80 + 1600 = 1680 pieces, past the 1000 members a feature may have. Refused
    // at once, saying why, whether the frame is made on the grid or its sketch is edited into one.
    let grid = |s: &mut Session| {
        for k in 0..40 {
            let at = 5 + 10 * k;
            run(s, "sketch.line", json!({"points": [[0, at], [410, at]]}));
            run(s, "sketch.line", json!({"points": [[at, 0], [at, 400]]}));
        }
    };
    let why = "counting the pieces crossings make";
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Grid"}));
    grid(&mut s);
    run(&mut s, "sketch.finish", json!({}));
    let e = s.execute("frame.member", &json!({"sketch": "Grid", "profile": tube(), "corner": "auto"})).unwrap_err().to_string();
    assert!(e.contains(why), "{e}");
    // Made on two lines, then the grid drawn into its sketch: the feature fails.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Grid"}));
    run(&mut s, "sketch.line", json!({"points": [[0, -100], [400, -100]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, -200], [400, -200]]}));
    run(&mut s, "sketch.finish", json!({}));
    let fid = run(&mut s, "frame.member", json!({"sketch": "Grid", "profile": tube(), "corner": "auto"}))["feature"].as_u64().unwrap();
    run(&mut s, "sketch.edit", json!({"sketch": "Grid"}));
    grid(&mut s);
    let finished = s.execute("sketch.finish", &json!({})).map_err(|e| e.to_string());
    let e = finished.err().or_else(|| s.model.result(fid).and_then(|r| r.error.clone())).unwrap_or_default();
    assert!(e.contains(why), "{e}");
}

#[test]
fn a_line_id_with_a_greater_than_sign_is_a_line_of_its_own() {
    // Ruling C3: a sketch id may hold `>`, and only Auto's crossings make pieces. An open U of a (0, 0)…(600, 0),
    // b (600, 0)…(600, 400) and `a>b` (600, 400)…(0, 400) has no crossing: three tubes, numbered in drawing order as
    // any lines are, Mitre or Auto. Remove Tube of b keeps `a>b`, still a tube of its own with its own name.
    for corner in ["mitre", "auto"] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "U"}));
        for (id, points) in [("a", [[0, 0], [600, 0]]), ("b", [[600, 0], [600, 400]]), ("a>b", [[600, 400], [0, 400]])] {
            run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        let fid = run(&mut s, "frame.member", json!({"sketch": "U", "profile": tube(), "corner": corner}))["feature"].clone();
        let names: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
        assert_eq!(names, ["Rect Tube 40x40x3", "Rect Tube 40x40x3 (2)", "Rect Tube 40x40x3 (3)"], "{corner}");
        remove(&mut s, json!([{"feature": fid, "line": "b"}]));
        let solidcraft_doc::FeatureKind::FrameMember { lines, .. } = member_kind(&s, &fid) else { panic!() };
        assert_eq!(lines, ["a", "a>b"], "{corner}");
        let names: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
        assert_eq!(names, ["Rect Tube 40x40x3", "Rect Tube 40x40x3 (3)"], "{corner}");
    }
}

#[test]
fn auto_crossings_are_the_same_whatever_the_pick_order() {
    let build = |reverse: bool| {
        let mut s = Session::default();
        x_brace_frame(&mut s);
        let mut lines: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).collect();
        assert_eq!(lines.len(), 6);
        if reverse {
            lines.reverse();
        }
        let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "lines": lines, "names": names}));
        built(&mut s)
    };
    let ((drawn, drawn_faces), (picked, picked_faces)) = (build(false), build(true));
    // d1 stays whole and d2 is cut in two whichever way the lines are picked.
    assert!(drawn.len() == 7 && drawn.contains_key("d1") && drawn.contains_key("d2 · d1"), "{drawn:?}");
    assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
    for (line, (v, lo, hi)) in &drawn {
        let (pv, plo, phi) = picked[line];
        assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{line}: drawn {drawn:?} picked {picked:?}");
    }
    assert_eq!(drawn_faces, picked_faces);
}

/// The X brace frame made under Auto, d1 named `D1` and d2 `D2`: d1 whole, d2 cut into `d2` and `d2>d1`.
fn x_brace_member(s: &mut Session) -> Value {
    x_brace_frame(s);
    run(s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "names": {"d1": "D1", "d2": "D2"}}))["feature"].clone()
}

/// The frame member `fid`'s pinned names.
fn pinned_names(s: &Session, fid: &Value) -> std::collections::BTreeMap<String, String> {
    let solidcraft_doc::FeatureKind::FrameMember { names, .. } = member_kind(s, fid) else { panic!() };
    names
}

/// Sets the pinned names of the frame member `fid` as `change` says, without building it again.
fn change_pins(s: &mut Session, fid: &Value, change: impl FnOnce(&mut std::collections::BTreeMap<String, String>)) {
    if let Some(solidcraft_doc::FeatureKind::FrameMember { names, .. }) = s.doc_mut().feature_mut(fid.as_u64().unwrap()).map(|f| &mut f.kind) {
        change(names);
    }
}

#[test]
fn piece_names_are_pinned_at_creation() {
    let mut s = Session::default();
    let fid = x_brace_member(&mut s);
    let names = pinned_names(&s, &fid);
    // Every member is pinned, the later piece too, under its piece id with the name it was given.
    assert_eq!(names.len(), 7, "{names:?}");
    assert_eq!(names.get("d2>d1").map(String::as_str), Some("D2 · D1"), "{names:?}");
    let made: std::collections::BTreeSet<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert_eq!(made, names.values().cloned().collect(), "{names:?}");
}

#[test]
fn renaming_the_crossing_tube_doesnt_rename_the_pieces() {
    // Also for a frame pinned before its piece was: the rename pins every member as it is now.
    for unpinned in [false, true] {
        let mut s = Session::default();
        let fid = x_brace_member(&mut s);
        if unpinned {
            change_pins(&mut s, &fid, |names| {
                names.remove("d2>d1");
            });
            s.refresh();
            assert!(bodies(&mut s).iter().any(|b| b.0 == "D2 · D1"));
        }
        run(&mut s, "body.rename", json!({"body": "D1", "name": "Brace"}));
        let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
        assert!(made.contains(&"Brace".to_string()) && made.contains(&"D2 · D1".to_string()), "{unpinned}: {made:?}");
        let names = pinned_names(&s, &fid);
        assert_eq!(names.get("d1").map(String::as_str), Some("Brace"), "{unpinned}: {names:?}");
        assert_eq!(names.get("d2>d1").map(String::as_str), Some("D2 · D1"), "{unpinned}: {names:?}");
    }
}

#[test]
fn member_pins_pair_each_body_with_its_own_member() {
    let mut s = Session::default();
    let fid = x_brace_member(&mut s);
    let id = fid.as_u64().unwrap();
    assert_eq!(crate::cmd::browser::member_pins(&s, id), Some(pinned_names(&s, &fid)));
    // Pins that no longer agree with the bodies made (here d1's and d2's swapped, not yet built) pair nothing, so a
    // rename never pins a name to another member.
    change_pins(&mut s, &fid, |names| {
        names.insert("d1".into(), "D2".into());
        names.insert("d2".into(), "D1".into());
    });
    assert_eq!(crate::cmd::browser::member_pins(&s, id), None);
}

#[test]
fn a_corners_entry_with_a_piece_id_is_refused() {
    let mut s = Session::default();
    x_brace_frame(&mut s);
    // An override names lines and pairs, never a piece: refused, naming `corners`.
    let corners = json!([{"members": ["d2>d1", "d1"], "corner": "auto"}]);
    let e = s.execute("frame.member", &json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "corners": corners})).unwrap_err().to_string();
    assert!(e.contains("`corners`") && e.contains("d2>d1") && e.contains("piece"), "{e}");
    // A crossing override names the lines through the point; its type isn't used (ruling C2), only Flip: d2 stays whole.
    let corners = json!([{"members": ["d1", "d2"], "corner": "mitre", "flip": true}]);
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "corners": corners, "names": {"d1": "D1", "d2": "D2"}}),
    );
    let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert!(made.contains(&"D1 · D2".to_string()) && !made.contains(&"D2 · D1".to_string()), "{made:?}");
}

#[test]
fn a_corners_entry_naming_a_line_whose_id_holds_a_gt_is_taken() {
    // Ruling C3: a sketch id may hold `>`. The line `a>b` is a line of the sketch, not a piece: an override naming it is
    // taken, and applies. Mitred, c reaches the corner's outer side, y = −20 (Auto alone would butt it on a>b at y = 20).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [600, 0]], "ids": ["a>b"]}));
    run(&mut s, "sketch.line", json!({"points": [[600, 0], [600, 400]], "ids": ["c"]}));
    run(&mut s, "sketch.finish", json!({}));
    let corners = json!([{"members": ["a>b", "c"], "corner": "mitre"}]);
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "auto", "corners": corners, "names": {"a>b": "A", "c": "C"}}));
    let bs = boxes(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    assert!(near3(bs["C"].0, [580.0, -20.0, -20.0]) && near3(bs["A"].1, [620.0, 20.0, 20.0]), "{bs:?}");
}

#[test]
fn a_stale_piece_name_is_ignored() {
    let mut s = Session::default();
    x_brace_frame(&mut s);
    // A `names` key for no member (a piece of a crossing there isn't) is no error, and names nothing.
    let names = json!({"d1": "D1", "d2": "D2", "d2>gone": "Gone", "d1>d2": "Nor this"});
    let fid = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "names": names}))["feature"].clone();
    let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert!(made.len() == 7 && !made.iter().any(|n| n == "Gone" || n == "Nor this"), "{made:?}");
    // The crossing taken away leaves d2>d1's pin behind: the frame still builds, and still edits.
    delete_line(&mut s, "Frame", "d1");
    assert!(pinned_names(&s, &fid).contains_key("d2>d1"));
    assert_eq!(s.model.result(fid.as_u64().unwrap()).and_then(|r| r.error.clone()), None);
    let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    assert!(made.len() == 5 && made.contains(&"D2".to_string()), "{made:?}");
    run(&mut s, "timeline.edit", json!({"feature": fid, "set": {"flip": true}}));
    assert_eq!(bodies(&mut s).len(), 5);
}

#[test]
fn remove_tube_on_a_piece_removes_its_line() {
    // Its later piece, or both its pieces at once (the line goes once), takes out d2. The piece's pinned name goes with
    // it, and also when the tube that cut it, d1, goes: then d2 is whole again.
    for (members, line, kept) in [(vec!["d2>d1"], "d2", "D1"), (vec!["d2", "d2>d1"], "d2", "D1"), (vec!["d1"], "d1", "D2")] {
        let mut s = Session::default();
        let fid = x_brace_member(&mut s);
        let members: Vec<Value> = members.iter().map(|m| json!({"feature": fid, "line": m})).collect();
        remove(&mut s, json!(members));
        let solidcraft_doc::FeatureKind::FrameMember { lines, names, .. } = member_kind(&s, &fid) else { panic!() };
        assert!(lines.len() == 5 && !lines.iter().any(|l| l == line), "{lines:?}");
        assert!(!names.contains_key(line) && !names.contains_key("d2>d1") && names.len() == 5, "{names:?}");
        let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
        assert!(made.len() == 5 && made.contains(&kept.to_string()) && !made.iter().any(|n| n.contains('·')), "{made:?}");
    }
}

#[test]
fn remove_tube_mends_an_auto_frame_whose_line_is_gone() {
    // A listed line deleted from the sketch fails the frame, which can't be laid out to find its pieces: its tube can
    // still be taken out, by its line, and the frame builds again.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let lines: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).collect();
    let fid = run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "lines": lines}))["feature"].clone();
    let right = line_at_x(&s, "Frame", 600.0);
    delete_line(&mut s, "Frame", &right);
    let error = |s: &Session| s.model.result(fid.as_u64().unwrap()).and_then(|r| r.error.clone());
    assert!(error(&s).is_some());
    remove(&mut s, json!([{"feature": fid, "line": right}]));
    assert_eq!(error(&s), None);
    assert_eq!(bodies(&mut s).len(), 3);
}

#[test]
fn a_face_fitted_to_a_later_piece_is_named_with_its_id() {
    // As in `a_crossing_tubes_far_end_override_still_applies`: c cuts a at (300, 0), and b's start is fitted to a's
    // later piece `a>c`, which runs through at (600, 0). b's fitted faces carry the piece's id, not its line's.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "L"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [600, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[600, 0], [600, 400]], "ids": ["b"]}));
    run(&mut s, "sketch.line", json!({"points": [[300, -400], [300, 400]], "ids": ["c"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "auto", "names": {"a": "a", "b": "b", "c": "c"}}));
    let (_, faces) = built(&mut s);
    let fitted: Vec<&String> = faces["b"].iter().filter(|n| n.contains("corner_start:")).collect();
    assert!(!fitted.is_empty() && fitted.iter().all(|n| n.contains("corner_start:a>c:face:")), "{:?}", faces["b"]);
}

#[test]
fn auto_overrides_at_three_ends() {
    // a (600) along X, b (400) down Y to the origin, c (283) out at 45°: a and b, laid first, make the corner
    // (a butt at 90°), and c is fitted to both.
    let build = |flip: Option<bool>| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Y"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [600, 0]], "ids": ["a"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 400], [0, 0]], "ids": ["b"]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [200, 200]], "ids": ["c"]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut p = json!({"sketch": "Y", "profile": tube(), "corner": "auto", "names": {"a": "a", "b": "b", "c": "c"}});
        if let Some(flip) = flip {
            p["corners"] = json!([{"members": ["c", "a", "b"], "corner": "auto", "flip": flip}]);
        }
        run(&mut s, "frame.member", p);
        boxes(&mut s)
    };
    // c: fitted to x ≤ 20 and y ≤ 20 near the origin (its edges meet them 48.3 along it); its far end is square
    // at (200, 200), its corners ±20·(−0.707, 0.707) off it: x and y up to 214.142.
    let h = 20.0 * std::f64::consts::FRAC_1_SQRT_2;
    let c = ([20.0, 20.0, -20.0], [200.0 + h, 200.0 + h, 20.0]);
    for flip in [None, Some(false)] {
        let bs = build(flip);
        // Unflipped, the longer a runs through to b's outer face (x = −20); b is fitted to a's near face (y = 20).
        assert!(near3(bs["a"].0, [-20.0, -20.0, -20.0]) && near3(bs["a"].1, [600.0, 20.0, 20.0]), "{flip:?} {bs:?}");
        assert!(near3(bs["b"].0, [-20.0, 20.0, -20.0]) && near3(bs["b"].1, [20.0, 400.0, 20.0]), "{flip:?} {bs:?}");
        assert!(near3(bs["c"].0, c.0) && near3(bs["c"].1, c.1), "{flip:?} {bs:?}");
    }
    // Flipped, the roles swap: b runs through to a's outer face (y = −20), a is fitted to b's near face (x = 20).
    let bs = build(Some(true));
    assert!(near3(bs["b"].0, [-20.0, -20.0, -20.0]) && near3(bs["b"].1, [20.0, 400.0, 20.0]), "{bs:?}");
    assert!(near3(bs["a"].0, [20.0, -20.0, -20.0]) && near3(bs["a"].1, [600.0, 20.0, 20.0]), "{bs:?}");
    assert!(near3(bs["c"].0, c.0) && near3(bs["c"].1, c.1), "{bs:?}");
}

/// Each body's volume and box, and its sorted face names, by body name.
type Built = (std::collections::BTreeMap<String, (f64, [f64; 3], [f64; 3])>, std::collections::BTreeMap<String, Vec<String>>);

fn built(s: &mut Session) -> Built {
    let faces = s
        .model
        .state()
        .bodies
        .iter()
        .map(|b| {
            let mut n = solidcraft_doc::naming::face_names(b).to_vec();
            n.sort();
            (b.name.clone(), n)
        })
        .collect();
    (bodies(s).into_iter().map(|b| (b.0, (b.1, b.2, b.3))).collect(), faces)
}

#[test]
fn auto_is_the_same_whatever_the_pick_order() {
    let build = |reverse: bool| {
        let mut s = Session::default();
        cross_member_frame(&mut s);
        let mut lines: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).collect();
        assert_eq!(lines.len(), 5);
        if reverse {
            lines.reverse();
        }
        let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "auto", "lines": lines, "names": names}));
        built(&mut s)
    };
    let ((drawn, drawn_faces), (picked, picked_faces)) = (build(false), build(true));
    assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
    for (line, (v, lo, hi)) in &drawn {
        let (pv, plo, phi) = picked[line];
        assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{line}: drawn {drawn:?} picked {picked:?}");
    }
    assert_eq!(drawn_faces, picked_faces);
}

#[test]
fn a_non_auto_frame_is_unchanged() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre"}));
    assert_stage_one_mitre_rectangle(&mut s);
}

/// Stage 1's Mitre rectangle, exactly. Recorded on `frame-auto-joints` at 4afc6a0c2, before Auto was built: there the
/// non-Auto path was `frame-corner-cuts`' (3681bf74a) unchanged (`git diff 3681bf74a 4afc6a0c2` changes only override
/// parsing for 3+ members). Each member runs out to the rectangle's outer corners, 20 past its line's ends, and keeps
/// A × its line: A·600 = 252493.80, A·400 = 168329.20.
fn assert_stage_one_mitre_rectangle(s: &mut Session) {
    let (got, faces) = built(s);
    let want = [
        ("Rect Tube 40x40x3", 252_493.801_996_584_55, [-20.0, -20.0, -20.0], [620.0, 20.0, 20.0]),
        ("Rect Tube 40x40x3 (2)", 168_329.201_030_185_65, [580.0, -20.0, -20.0], [620.0, 420.0, 20.0]),
        ("Rect Tube 40x40x3 (3)", 252_493.801_996_583_7, [-20.0, 380.0, -20.0], [620.0, 420.0, 20.0]),
        ("Rect Tube 40x40x3 (4)", 168_329.201_030_186_87, [-20.0, -20.0, -20.0], [20.0, 420.0, 20.0]),
    ];
    assert_eq!(got.len(), 4, "{got:?}");
    // Its two mitred ends named for the ends they finish, and the tube's own faces.
    let mut names: Vec<String> = ["corner_end", "corner_start"].iter().map(|n| format!("F2:{n}")).collect();
    names.extend([0, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17].iter().map(|k| format!("F2:face:{k}")));
    names.sort();
    for (name, v, lo, hi) in want {
        let (gv, glo, ghi) = got[name];
        assert!(rel(gv, v) < 1e-8 && near3(glo, lo) && near3(ghi, hi), "{name}: {:?}", got[name]);
        assert_eq!(faces[name], names, "{name}");
    }
}

#[test]
fn an_auto_override_on_a_mitre_frame_builds_as_mitre() {
    // Only an Auto frame builds by Auto's rules: on a Mitre frame an Auto override is the frame's own Mitre.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "corners": [{"members": [bottom, left], "corner": "auto"}]}),
    );
    assert_stage_one_mitre_rectangle(&mut s);
}

#[test]
fn an_auto_override_on_a_butt_frame_keeps_its_flip() {
    // On a Butt frame, an Auto override with Flip at the origin is a flipped Butt there, built as stage 1 builds a
    // Butt override with Flip.
    let build = |kind: &str| {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
        run(
            &mut s,
            "frame.member",
            json!({"sketch": "Frame", "profile": tube(), "corner": "butt",
                   "corners": [{"members": [bottom, left], "corner": kind, "flip": true}]}),
        );
        let (b, l) = bottom_and_left(&mut s);
        (b, l, built(&mut s))
    };
    let (bottom, left, auto) = build("auto");
    // Flipped at the origin: the left runs through to the bottom's outer face (y = −20), the bottom stops on the
    // left's near face (x = 20). The other corners are the frame's unflipped Butt: the bottom runs out to x = 620,
    // the left stops on the top's near face, y = 380.
    assert!((left.2[1] + 20.0).abs() < 1e-6 && (left.3[1] - 380.0).abs() < 1e-6, "{left:?}");
    assert!((bottom.2[0] - 20.0).abs() < 1e-6 && (bottom.3[0] - 620.0).abs() < 1e-6, "{bottom:?}");
    // And exactly as stage 1's Butt override with Flip: volumes, boxes and face names.
    let (_, _, butt) = build("butt");
    assert_eq!(auto.1, butt.1);
    for (name, (v, lo, hi)) in &butt.0 {
        let (av, alo, ahi) = auto.0[name];
        assert!(rel(av, *v) < 1e-12 && near3(alo, *lo) && near3(ahi, *hi), "{name}: auto {:?} butt {:?}", auto.0, butt.0);
    }
}

#[test]
fn a_three_tube_override_on_a_mitre_frame_still_fails() {
    // A hand-edited design: a Mitre frame whose two-member Auto override is made three members. An Auto override
    // doesn't make the frame Auto, so it is not at a corner of the frame (three tubes never meet at one).
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let l = sketch_lines(&s, "Frame");
    let (a, b, c) = (&l[0].0, &l[1].0, &l[2].0);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "corners": [{"members": [a, b], "corner": "auto"}]}));
    let member = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let json = serde_json::from_str::<Value>(&s.doc.to_json()).unwrap().to_string();
    let from = format!(r#""members":[{{"line":"{a}"}},{{"line":"{b}"}}]"#);
    assert!(json.contains(&from), "{json}");
    let edited = json.replace(&from, &format!(r#""members":[{{"line":"{a}"}},{{"line":"{b}"}},{{"line":"{c}"}}]"#));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&edited).unwrap());
    let e = reopened.model.result(member).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("A corner override is not at a corner of the frame"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Auto lines sections up (#4003 stage 2, Goal 7): a group's sketch normal is up, outer tubes run round the outline

/// A 600 × 400 rectangle on XZ (the front plane: sketch x = +X, sketch y = −Z, normal +Y), as the frame sketch
/// `Frame`: x 0…600, z 0…400, drawn anticlockwise in the sketch, so Auto runs every side the way it is drawn.
fn vertical_rectangle_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XZ", "name": "Frame"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, -400]}));
    run(s, "sketch.finish", json!({}));
}

/// A 40 × 20 flat bar, square cornered: A = 800.
fn flat_bar() -> Value {
    json!({"kind": "bar", "width": 40, "height": 20})
}

/// Every body's (min, max, volume), sorted by box.
fn sorted_boxes(s: &mut Session) -> Vec<([f64; 3], [f64; 3], f64)> {
    let mut bs: Vec<_> = bodies(s).into_iter().map(|b| (b.2, b.3, b.1)).collect();
    // By the box to 1e-3, so rounding never reorders two boxes that share a corner coordinate.
    bs.sort_by_key(|b| b.0.iter().chain(&b.1).map(|v| (v * 1e3).round() as i64).collect::<Vec<_>>());
    bs
}

/// `got` are `want` (min, max, volume), boxes to 1e-6 and volumes to MEASURED.
fn assert_boxes(got: &[([f64; 3], [f64; 3], f64)], want: &[([f64; 3], [f64; 3], f64)]) {
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(near3(g.0, w.0) && near3(g.1, w.1) && rel(g.2, w.2) < MEASURED, "got {g:?} want {w:?}\nall {got:?}");
    }
}

#[test]
fn auto_lines_up_a_vertical_rectangle() {
    let mut s = Session::default();
    vertical_rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": flat_bar(), "corner": "auto"}));
    // Up is the sketch's normal, +Y, for all four: each bar's height (20) lies across the frame's plane, y −10…10,
    // and its width (40) in the plane, ±20 about its line. (By world Z the 600s would lie flat, y −20…20.)
    // Butts at 90°: the 600s run through to the 400s' outer faces, 20 past each end, x −20…620: A·640 = 512000.
    // The 400s are fitted flat to the 600s' near faces, z 20…380: A·360 = 288000.
    let (long, short) = (800.0 * 640.0, 800.0 * 360.0);
    assert_boxes(
        &sorted_boxes(&mut s),
        &[
            ([-20.0, -10.0, -20.0], [620.0, 10.0, 20.0], long),
            ([-20.0, -10.0, 20.0], [20.0, 10.0, 380.0], short),
            ([-20.0, -10.0, 380.0], [620.0, 10.0, 420.0], long),
            ([580.0, -10.0, 20.0], [620.0, 10.0, 380.0], short),
        ],
    );
}

#[test]
fn auto_lines_up_head_to_head_lines() {
    // Two lines drawn away from the corner at the origin on XZ: along −X (600) and along +Z (400, sketch y −400).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "L"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [-600, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -400]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": flat_bar(), "corner": "auto", "anchor": "bottom_left"}));
    // Up is +Y for both, and the bottom-left anchor (−20, −10) puts each section's y 10 above its line: both sit on
    // the +Y side of the frame's plane, y 0…20. (By world Z, the −X line's x would be −Y and its bar y −40…0.)
    // In the plane each section's x is y × its run: Z along −X, X along +Z, so the 600 occupies z 0…40 and the 400
    // x 0…40. The 600 runs through to the 400's far face, x 40 (A·640); the 400 is fitted to the 600's top, z 40
    // (A·360).
    assert_boxes(
        &sorted_boxes(&mut s),
        &[([-600.0, 0.0, 0.0], [40.0, 20.0, 40.0], 800.0 * 640.0), ([0.0, 0.0, 40.0], [40.0, 20.0, 400.0], 800.0 * 360.0)],
    );
}

/// A 600 × 400 rectangle on XY drawn clockwise seen from +Z, as the frame sketch `Frame`, with Auto members of
/// `profile` anchored bottom left. Each side runs against the anticlockwise outline, so Auto turns its section round:
/// then x = up × run points into the rectangle for every side, and the bottom-left anchor (−20, −20) puts each 40
/// tube wholly inside it, 0…40 in from its line and z 0…40.
fn clockwise_auto_frame(profile: Value) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 400], [600, 400], [600, 0], [0, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": profile, "corner": "auto", "anchor": "bottom_left"}));
    s
}

#[test]
fn auto_runs_outer_tubes_round_the_outline() {
    let mut s = clockwise_auto_frame(tube());
    // The 600s run through to the 400s' outer faces, which lie on the lines x = 0 and 600: A·600. The 400s are
    // fitted to the 600s' inner faces, y 40 and 360: A·320.
    let (long, short) = (tube_area() * 600.0, tube_area() * 320.0);
    assert_boxes(
        &sorted_boxes(&mut s),
        &[
            ([0.0, 0.0, 0.0], [600.0, 40.0, 40.0], long),
            ([0.0, 40.0, 0.0], [40.0, 360.0, 40.0], short),
            ([0.0, 360.0, 0.0], [600.0, 400.0, 40.0], long),
            ([560.0, 40.0, 0.0], [600.0, 360.0, 40.0], short),
        ],
    );
}

#[test]
fn auto_lines_up_a_3d_chain_in_its_plane() {
    // A post and a rail of a 3D sketch, in the plane y = 0: their chain's normal pointing up is +Y (no Z part).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tower"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1000], [600, 0, 1000]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Tower", "profile": {"kind": "bar", "width": 60, "height": 10}, "corner": "auto"}));
    // Both bars' height (10) along Y, y −5…5, their width (60) in the plane: the post's along X, the rail's along Z.
    // (By world Z the rail would lie flat, y −30…30.) Their chained corner is mitred on the plane through
    // (0, 0, 1000) square to (1, 0, 1): the post reaches z 1030 at x −30, the rail x −30 at z 1030. A mitre through
    // the centreline keeps A × the line: 600·1000 and 600·600.
    assert_boxes(
        &sorted_boxes(&mut s),
        &[([-30.0, -5.0, 0.0], [30.0, 5.0, 1030.0], 600_000.0), ([-30.0, -5.0, 970.0], [600.0, 5.0, 1030.0], 360_000.0)],
    );
}

#[test]
fn an_end_cap_sits_on_a_tube_auto_runs_against_its_line() {
    // The bottom, drawn (600, 0) → (0, 0), runs through and lies inside the rectangle, y 0…40, z 0…40; its ends are
    // cut square at x = 600 (its drawn start) and x = 0 (its drawn end), the 400s' outer faces. End Cap measures
    // the member from its drawn start, so both are open ends, and each cap covers the same 40 × 40, 6 thick past
    // its end: x 600…606 and −6…0.
    let mut s = clockwise_auto_frame(square_tube());
    let bottom = bodies(&mut s).into_iter().find(|b| b.3[0] - b.2[0] > 500.0 && b.3[1] < 100.0).unwrap();
    let start = face_pick(&s, &bottom.0, [600.0, 1.5, 20.0], [1.0, 0.0, 0.0]);
    let end = face_pick(&s, &bottom.0, [0.0, 1.5, 20.0], [-1.0, 0.0, 0.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [start, end], "thickness": 6}));
    let mut caps: Vec<Measured> = bodies(&mut s).into_iter().filter(|b| b.0.contains("End Cap")).collect();
    caps.sort_by(|a, b| a.2[0].total_cmp(&b.2[0]));
    assert_eq!(caps.len(), 2, "{caps:?}");
    assert!(near3(caps[0].2, [-6.0, 0.0, 0.0]) && near3(caps[0].3, [0.0, 40.0, 40.0]), "{caps:?}");
    assert!(near3(caps[1].2, [600.0, 0.0, 0.0]) && near3(caps[1].3, [606.0, 40.0, 40.0]), "{caps:?}");
}

/// A face of a body, as a point on its plane and its outward normal.
type Pick = ([f64; 3], [f64; 3]);

/// An open U on XY: the bottom (0,0)→(600,0), the right drawn down (600,400)→(600,0), the top (600,400)→(0,400), in
/// Auto members of `member` (frame.member's profile and anchor), after `setup`. Open, nothing is outer and the right
/// runs as drawn. Closed by the left (0,400)→(0,0), the outline runs up the right side, so Auto runs the right against
/// its line. Gives the right tube and the names of its faces at `open`, then once the U is closed, at `closed`.
fn names_across_closing(setup: impl Fn(&mut Session), member: Value, open: &[Pick], closed: &[Pick]) -> [(Measured, Vec<String>); 2] {
    let mut s = Session::default();
    setup(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    for points in [[[0, 0], [600, 0]], [[600, 400], [600, 0]], [[600, 400], [0, 400]]] {
        run(&mut s, "sketch.line", json!({"points": points}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let mut p = json!({"sketch": "Frame", "corner": "auto"});
    for (k, v) in member.as_object().unwrap() {
        p[k] = v.clone();
    }
    run(&mut s, "frame.member", p);
    let named = |s: &mut Session, picks: &[Pick]| {
        let right = bodies(s).into_iter().find(|b| b.3[1] - b.2[1] > 300.0 && b.2[0] > 500.0).unwrap();
        let names = solidcraft_doc::naming::face_names(s.model.state().body(&right.0).unwrap()).to_vec();
        let picked = picks.iter().map(|(at, n)| names[face_pick(s, &right.0, *at, *n)["index"].as_u64().unwrap() as usize].clone()).collect();
        (right, picked)
    };
    let before = named(&mut s, open);
    run(&mut s, "sketch.edit", json!({"sketch": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 400], [0, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    assert_eq!(bodies(&mut s).len(), 4);
    [before, named(&mut s, closed)]
}

/// The walls of a 40 × 40 box tube with 3 mm walls whose box starts at x0 and z0: facing +X, −X, +Z, −Z outside, then
/// the bore's walls behind them.
fn box_walls(x0: f64, z0: f64) -> [Pick; 8] {
    let (mx, mz) = (x0 + 20.0, z0 + 20.0);
    [
        ([x0 + 40.0, 200.0, mz], [1.0, 0.0, 0.0]),
        ([x0, 200.0, mz], [-1.0, 0.0, 0.0]),
        ([mx, 200.0, z0 + 40.0], [0.0, 0.0, 1.0]),
        ([mx, 200.0, z0], [0.0, 0.0, -1.0]),
        ([x0 + 37.0, 200.0, mz], [-1.0, 0.0, 0.0]),
        ([x0 + 3.0, 200.0, mz], [1.0, 0.0, 0.0]),
        ([mx, 200.0, z0 + 37.0], [0.0, 0.0, -1.0]),
        ([mx, 200.0, z0 + 3.0], [0.0, 0.0, 1.0]),
    ]
}

/// The same tube before and after: the same box and volume.
fn assert_same_solid(before: &Measured, after: &Measured) {
    assert!(near3(before.2, after.2) && near3(before.3, after.3) && rel(after.1, before.1) < 1e-9, "{before:?} {after:?}");
}

#[test]
fn closing_an_outline_keeps_a_tubes_face_names() {
    // Each wall of the right tube keeps its name: the wall at the same place in its section, outer wall, bore wall,
    // top and bottom alike. Centred, the tube is the same solid either way round, x 580…620, z −20…20.
    let walls = box_walls(580.0, -20.0);
    let [(open, before), (closed, after)] = names_across_closing(|_| {}, json!({"profile": tube(), "anchor": "center"}), &walls, &walls);
    assert_same_solid(&open, &closed);
    assert_eq!(before, after);
    // Anchored bottom left (−20, −20), its section's x is up × run: +X running as drawn (−Y), the tube outside the U at
    // x 600…640; −X running up (+Y), inside the closed outline at x 560…600; z 0…40 both ways. Built from its section
    // mirrored, it is the open tube moved 40 along −X: each wall keeps the name of the wall it was moved from.
    let [(open, before), (closed, after)] =
        names_across_closing(|_| {}, json!({"profile": tube(), "anchor": "bottom_left"}), &box_walls(600.0, 0.0), &box_walls(560.0, 0.0));
    let at = |b: &Measured| [b.2[0], b.3[0], b.2[2], b.3[2]];
    assert!(at(&open) == [600.0, 640.0, 0.0, 40.0] && at(&closed) == [560.0, 600.0, 0.0, 40.0], "{open:?} {closed:?}");
    assert_eq!(before, after);
}

#[test]
fn a_sketch_section_off_its_origin_keeps_its_face_names() {
    // A square tube drawn in its own sketch about (100, 50), not about the sketch's origin: 40 × 40 with 3 mm walls
    // and square corners, placed by its centre. It is the same solid either way round, x 580…620, z −20…20, and each
    // wall keeps its name.
    let section = |s: &mut Session| {
        run(s, "sketch.create", json!({"plane": "XY", "name": "Tube section"}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [80, 30], "p1": [120, 70]}));
        run(s, "sketch.rectangle.two_point", json!({"p0": [83, 33], "p1": [117, 67]}));
        run(s, "sketch.finish", json!({}));
    };
    let walls = box_walls(580.0, -20.0);
    let [(open, before), (closed, after)] =
        names_across_closing(section, json!({"profile_sketch": "Tube section", "anchor": "center"}), &walls, &walls);
    assert_same_solid(&open, &closed);
    assert_eq!(before, after);
}

#[test]
fn an_angle_section_keeps_its_face_names() {
    // A 60 × 40 × 5 angle, centred: its 60 leg along the section's x at its bottom (z −20…−15), its 40 leg up its
    // left side. Open, the right runs −Y and the section's x is +X: the 40 leg at x 570…575. Run the other way the
    // section is mirrored and the 40 leg is at x 625…630. Each face keeps the name of the face swept from the same
    // edge of the section: the 60 leg's outer (z −20) and inner (z −15) faces, and the 40 leg's outer face (x 570
    // facing −X, then x 630 facing +X) and its inner face (x 575, then 625).
    let open = [
        ([600.0, 200.0, -20.0], [0.0, 0.0, -1.0]),
        ([600.0, 200.0, -15.0], [0.0, 0.0, 1.0]),
        ([570.0, 200.0, 0.0], [-1.0, 0.0, 0.0]),
        ([575.0, 200.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let closed = [
        ([600.0, 200.0, -20.0], [0.0, 0.0, -1.0]),
        ([600.0, 200.0, -15.0], [0.0, 0.0, 1.0]),
        ([630.0, 200.0, 0.0], [1.0, 0.0, 0.0]),
        ([625.0, 200.0, 0.0], [-1.0, 0.0, 0.0]),
    ];
    let angle = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    let [(_, before), (_, after)] = names_across_closing(|_| {}, json!({"profile": angle, "anchor": "center"}), &open, &closed);
    assert_eq!(before, after);
}

#[test]
fn mitre_frames_keep_todays_placement() {
    // Only Auto lines sections up: the vertical rectangle under Mitre places each bar as stage 1 does, by world Z.
    let mut s = Session::default();
    vertical_rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": flat_bar(), "corner": "mitre"}));
    // The 600s lie flat: width along Y (y −20…20), height in the plane (z ±10). The 400s stand on edge: width along X
    // (x ±20), height along Y (y −10…10). Each mitre plane bisects its corner, through the corner point, so the
    // 600s' outer faces (z −10, 410) reach x −10 and 610, and the 400s' (x −20, 620) reach z −20 and 420. A plane
    // through the centreline keeps A × the line: A·600 = 480000, A·400 = 320000.
    assert_boxes(
        &sorted_boxes(&mut s),
        &[
            ([-20.0, -10.0, -20.0], [20.0, 10.0, 420.0], 320_000.0),
            ([-10.0, -20.0, -10.0], [610.0, 20.0, 10.0], 480_000.0),
            ([-10.0, -20.0, 390.0], [610.0, 20.0, 410.0], 480_000.0),
            ([580.0, -10.0, -20.0], [620.0, 10.0, 420.0], 320_000.0),
        ],
    );
}

// ---------------------------------------------------------------------------------------------
// Fitting to earlier Frame Members and their copies (#4003 Goal 6)

/// Frame Member `A`: the 600 × 400 rectangle `Frame` in 40 × 40 tube, its bodies `A`, `A (2)`…. Corner None
/// leaves each tube square, from its line's start to its end: the bottom is x 0…600, y −20…20, z −20…20, the top
/// y 380…420.
fn earlier_frame(s: &mut Session) {
    rectangle_frame(s);
    run(s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "name": "A", "body_name": "A"}));
}

/// A `size` square tube, 2 thick.
fn square_of(size: f64) -> Value {
    json!({"kind": "rect_tube", "width": size, "height": size, "wall": 2})
}

/// Frame Member `B` (bodies `B`, `B (2)`…) under Auto: `lines` in a sketch on XY raised `z`, in `size` square tube.
fn later_frame(s: &mut Session, z: f64, lines: &[[[f64; 2]; 2]], size: f64, fit_ends: bool) {
    let mut sketch = json!({"plane": "XY", "name": "Later"});
    if z != 0.0 {
        sketch["offset"] = json!(z);
    }
    run(s, "sketch.create", sketch);
    for l in lines {
        run(s, "sketch.line", json!({"points": l}));
    }
    run(s, "sketch.finish", json!({}));
    run(s, "frame.member", json!({"sketch": "Later", "profile": square_of(size), "corner": "auto", "fit_ends": fit_ends, "body_name": "B"}));
}

/// The bodies whose names start with `prefix`.
fn named(s: &mut Session, prefix: &str) -> Vec<Measured> {
    bodies(s).into_iter().filter(|b| b.0.starts_with(prefix)).collect()
}

/// The one body of B, as B's line (300, 0)…(300, 400) at `z` builds it: its bbox y runs `lo`…`hi`.
fn assert_b_runs(s: &mut Session, lo: f64, hi: f64) {
    let bs = named(s, "B");
    assert_eq!(bs.len(), 1, "{bs:?}");
    let b = &bs[0];
    assert!((b.2[1] - lo).abs() < 1e-6 && (b.3[1] - hi).abs() < 1e-6, "B runs y {}…{}, not {lo}…{hi}: {b:?}", b.2[1], b.3[1]);
}

#[test]
fn fit_ends_fits_a_later_frame_to_an_earlier_one() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, 0.0], [300.0, 400.0]]], 25.0, true);
    // B's ends lie on A's bottom tube (y −20…20) and top tube (y 380…420): each is fitted flat to the tube's near
    // face, so B runs y 20…380.
    assert_b_runs(&mut s, 20.0, 380.0);
}

#[test]
fn without_fit_ends_the_later_frame_overlaps() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, 0.0], [300.0, 400.0]]], 25.0, false);
    // Square line ends: y 0…400, overlapping A's tubes.
    assert_b_runs(&mut s, 0.0, 400.0);
}

#[test]
fn two_braces_meeting_inside_an_earlier_tube_are_fitted_not_mitred() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    // Two braces from (300, 0), inside A's bottom tube, out to (150, 200) and (450, 200): a corner of B at 106°
    // that Auto would butt, but it lies inside an earlier tube, so it isn't jointed. Both ends are fitted flat to
    // the tube's near face, y = 20; jointed, the through brace would run on to its corner cut, below y = 0.
    later_frame(&mut s, 0.0, &[[[300.0, 0.0], [150.0, 200.0]], [[300.0, 0.0], [450.0, 200.0]]], 20.0, true);
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 2, "{bs:?}");
    for b in &bs {
        assert!((b.2[1] - 20.0).abs() < 1e-6, "{b:?}");
    }
}

#[test]
fn a_moved_earlier_frame_is_fitted_where_it_is() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    let a: Vec<String> = named(&mut s, "A").into_iter().map(|b| b.0).collect();
    run(&mut s, "solid.move", json!({"bodies": a, "translate": [0, 0, 100]}));
    // A's tubes now lie at z 80…120: B, 100 up, ends on them and is fitted to them, y 20…380.
    later_frame(&mut s, 100.0, &[[[300.0, 0.0], [300.0, 400.0]]], 25.0, true);
    assert_b_runs(&mut s, 20.0, 380.0);
}

#[test]
fn a_mirrored_earlier_frame_is_fitted() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    let a: Vec<String> = named(&mut s, "A").into_iter().map(|b| b.0).collect();
    run(&mut s, "solid.mirror", json!({"bodies": a, "plane": "YZ"}));
    // The copies lie at x −600…0: B at x = −300 ends on the mirrored bottom and top tubes and is fitted to their
    // faces, y 20…380 (A's own tubes are 300 away).
    later_frame(&mut s, 0.0, &[[[-300.0, 0.0], [-300.0, 400.0]]], 25.0, true);
    assert_b_runs(&mut s, 20.0, 380.0);
}

#[test]
fn a_patterned_earlier_frame_is_fitted() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["A"], "dir1": [0, 0, 1], "count1": 2, "spacing1": 300}));
    // The second instance's tubes lie at z 280…320: B, 300 up, is fitted to them, y 20…380.
    later_frame(&mut s, 300.0, &[[[300.0, 0.0], [300.0, 400.0]]], 25.0, true);
    assert_b_runs(&mut s, 20.0, 380.0);
}

#[test]
fn an_end_on_the_second_diagonal_of_an_earlier_x_brace_is_fitted_to_it() {
    // A: an X brace of solid 40 × 40 bar centred on its lines, (0, 0)…(400, 400) then (0, 400)…(400, 0), crossing at
    // (200, 200), where each diagonal's middle lies inside the other. B's line runs square off the second diagonal
    // from (300, 100) on it: its end is fitted to that diagonal's side, so B and A (2) don't overlap. The same for
    // A's copies mirrored across YZ, which B meets at (−300, 100): B overlaps no tube of A or of its copies.
    for mirrored in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "X"}));
        run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 400]]}));
        run(&mut s, "sketch.line", json!({"points": [[0, 400], [400, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        let bar = json!({"kind": "bar", "width": 40, "height": 40});
        run(&mut s, "frame.member", json!({"sketch": "X", "profile": bar, "anchor": "center", "name": "A", "body_name": "A"}));
        let a: Vec<String> = named(&mut s, "A").into_iter().map(|b| b.0).collect();
        if mirrored {
            run(&mut s, "solid.mirror", json!({"bodies": a, "plane": "YZ"}));
        }
        let x = if mirrored { -1.0 } else { 1.0 };
        later_frame(&mut s, 0.0, &[[[300.0 * x, 100.0], [450.0 * x, 250.0]]], 25.0, true);
        for other in bodies(&mut s).into_iter().map(|b| b.0).filter(|n| n != "B") {
            let i = run(&mut s, "inspect.interference", json!({"bodies": ["B", other]}));
            let o: f64 = i["interferences"].as_array().map_or(0.0, |all| all.iter().map(|x| f(&x["volume_mm3"])).sum());
            assert!(o < 1e-3, "mirrored {mirrored}: B overlaps {other} by {o} mm³");
        }
    }
}

/// B ([`later_frame`], one 25 tube) is fitted to the earlier body `to`: its start is cut flat on it, so its box runs
/// `lo`…`hi` (z ±12.5), its fitted faces are named for `to`, and it overlaps no other body.
fn assert_b_fitted_to(s: &mut Session, to: &str, lo: [f64; 2], hi: [f64; 2]) {
    let b = measured(s, "B");
    assert!(near3(b.2, [lo[0], lo[1], -12.5]) && near3(b.3, [hi[0], hi[1], 12.5]), "B is not cut on {to}: {b:?}");
    let faces = built(s).1;
    let mark = format!("corner_start:{to}:face:");
    assert!(faces["B"].iter().any(|n| n.contains(&mark)), "B is not fitted to {to}: {:?}", faces["B"]);
    for other in bodies(s).into_iter().map(|b| b.0).filter(|n| n != "B") {
        let i = run(s, "inspect.interference", json!({"bodies": ["B", other]}));
        let o: f64 = i["interferences"].as_array().map_or(0.0, |all| all.iter().map(|x| f(&x["volume_mm3"])).sum());
        assert!(o < 1e-3, "B overlaps {other} by {o} mm³");
    }
}

#[test]
fn a_later_end_fits_to_an_earlier_frames_piece() {
    // A: the Auto X brace (x_brace_member), d1 whole, d2 cut at (300, 200) into D2 and D2 · D1, which runs on to
    // (600, 0) (y = 400 − 2x/3). B's line runs square off it from (450, 100) on it to (530, 220), along
    // (2, 3)/√13. Its start is fitted to D2 · D1's near face, 20 along: the end's centre (450, 100) + 20·(2, 3)/√13,
    // its corners ±12.5·(3, −2)/√13 across. So B's box is x 450 + (40 − 37.5)/√13 … 530 + 37.5/√13,
    // y 100 + (60 − 25)/√13 … 220 + 25/√13 (its far end square), against x 450 − 37.5/√13 unfitted.
    let mut s = Session::default();
    x_brace_member(&mut s);
    later_frame(&mut s, 0.0, &[[[450.0, 100.0], [530.0, 220.0]]], 25.0, true);
    let r = 13f64.sqrt();
    assert_b_fitted_to(&mut s, "D2 · D1", [450.0 + 2.5 / r, 100.0 + 35.0 / r], [530.0 + 37.5 / r, 220.0 + 25.0 / r]);
}

#[test]
fn a_mirrored_earlier_pieces_tool_is_placed_right() {
    // The X brace mirrored across YZ: D2 · D1's copy runs (−300, 200)…(−600, 0), its material's middle near
    // (−450, 100). B mirrors the last test's line, (−450, 100)…(−530, 220): it is fitted to that copy, its box the
    // last one mirrored, x −530 − 37.5/√13 … −450 − 2.5/√13.
    let mut s = Session::default();
    x_brace_member(&mut s);
    let a: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    run(&mut s, "solid.mirror", json!({"bodies": a, "plane": "YZ"}));
    let copy = bodies(&mut s)
        .into_iter()
        .find(|b| (-500.0..-400.0).contains(&b.4[0]) && (50.0..150.0).contains(&b.4[1]))
        .unwrap_or_else(|| panic!("no copy of D2 · D1: {:?}", bodies(&mut s)))
        .0;
    later_frame(&mut s, 0.0, &[[[-450.0, 100.0], [-530.0, 220.0]]], 25.0, true);
    let r = 13f64.sqrt();
    assert_b_fitted_to(&mut s, &copy, [-530.0 - 37.5 / r, 100.0 + 35.0 / r], [-450.0 - 2.5 / r, 220.0 + 25.0 / r]);
}

#[test]
fn a_copy_of_a_crossing_tube_is_not_taken_for_a_short_piece_inside_it() {
    // A plus, v (200, −100)…(200, 25) drawn first, h (0, 0)…(400, 0): v's piece past h runs 0…25 along its line, so
    // the points a quarter, half and three quarters along it (y 6.25, 12.5, 18.75) lie in h's wall too. h's mirrored
    // copy is h's, not that piece's: B, square off it from (−100, 0), is fitted to its near face y = 20.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[200, -100], [200, 25]], "ids": ["v"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Plus", "profile": tube(), "corner": "auto", "names": {"h": "H", "v": "V"}}));
    let a: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    run(&mut s, "solid.mirror", json!({"bodies": a, "plane": "YZ"}));
    let copy = bodies(&mut s)
        .into_iter()
        .find(|b| near3(b.2, [-400.0, -20.0, -20.0]) && near3(b.3, [0.0, 20.0, 20.0]))
        .unwrap_or_else(|| panic!("no copy of H: {:?}", bodies(&mut s)))
        .0;
    later_frame(&mut s, 0.0, &[[[-100.0, 0.0], [-100.0, 200.0]]], 25.0, true);
    assert_b_fitted_to(&mut s, &copy, [-112.5, 20.0], [-87.5, 200.0]);
}

#[test]
fn a_pasted_frame_with_a_dropped_piece_is_matched_by_shape_not_position() {
    // The plus with v's piece past h (y 0…10) wholly inside h: no body, so a pasted copy (bodies named by position)
    // has one body fewer than members and a body's place in the list is not its member's. w crosses h too; B, from
    // (300, -60) along +x, lands in the copy's w tube and is still fitted to its face x = 320.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[100, -100], [100, 10]], "ids": ["v"]}));
    run(&mut s, "sketch.line", json!({"points": [[300, -100], [300, 90]], "ids": ["w"]}));
    run(&mut s, "sketch.finish", json!({}));
    let fid = run(&mut s, "frame.member", json!({"sketch": "Plus", "profile": tube(), "corner": "auto"}))["feature"].clone();
    run(&mut s, "timeline.copy", json!({"features": [fid]}));
    let copy = run(&mut s, "timeline.paste", json!({}))["pasted"][0]["id"].as_u64().unwrap();
    assert!(!s.doc.feature(copy).unwrap().body_names.is_empty(), "a copy names its bodies by position");
    run(&mut s, "timeline.delete", json!({"feature": fid}));
    let w = bodies(&mut s)
        .into_iter()
        .find(|b| near3(b.2, [280.0, -100.0, -20.0]) && near3(b.3, [320.0, -20.0, 20.0]))
        .unwrap_or_else(|| panic!("no W: {:?}", bodies(&mut s)))
        .0;
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Later"}));
    run(&mut s, "sketch.line", json!({"points": [[300, -60], [500, -60]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Later", "profile": square_of(25.0), "corner": "auto", "fit_ends": true, "body_name": "B"}));
    let b = measured(&mut s, "B");
    assert!(near3(b.2, [320.0, -72.5, -12.5]) && near3(b.3, [500.0, -47.5, 12.5]), "B is not cut on {w}: {b:?}");
}

#[test]
fn a_later_end_fits_to_an_earlier_frames_whole_diagonal() {
    // The same, B landing on d1, which the crossing leaves whole (y = 2x/3): from (450, 300) on it square off to
    // (530, 180), along (2, −3)/√13. Fitted 20 along to D1's near face, B's box is x 450 + 2.5/√13 … 530 + 37.5/√13,
    // y 180 − 25/√13 … 300 − 35/√13.
    let mut s = Session::default();
    x_brace_member(&mut s);
    later_frame(&mut s, 0.0, &[[[450.0, 300.0], [530.0, 180.0]]], 25.0, true);
    let r = 13f64.sqrt();
    assert_b_fitted_to(&mut s, "D1", [450.0 + 2.5 / r, 180.0 - 25.0 / r], [530.0 + 37.5 / r, 300.0 - 35.0 / r]);
}

#[test]
fn earlier_tubes_are_never_changed() {
    let mut s = Session::default();
    earlier_frame(&mut s);
    let before = named(&mut s, "A");
    later_frame(&mut s, 0.0, &[[[300.0, 0.0], [300.0, 400.0]], [[0.0, 200.0], [600.0, 200.0]]], 25.0, true);
    assert_eq!(named(&mut s, "A"), before);
}

#[test]
fn a_later_group_in_another_plane_is_fitted_where_it_shares_a_point() {
    // A: a side frame, the 600 × 400 rectangle on XZ under Auto, a corner of it at the origin. B: on XY, an L from
    // the origin out to (0, −400) and on to (600, −400). B's end at the origin lies in A's corner: it is fitted to
    // A's tubes there, whose faces lie at y = ±20 (A is laid in XZ), so B's first tube runs y −400…−20 (from its
    // line's far end, where B's own corner is jointed; its 40 × 40 section reaches y −420 round that corner).
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Side"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Side", "profile": tube(), "corner": "auto", "name": "A", "body_name": "A"}));
    let before = named(&mut s, "A");
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Floor"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, -400]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, -400], [600, -400]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Floor", "profile": tube(), "corner": "auto", "fit_ends": true, "body_name": "B"}));
    let bs = named(&mut s, "B");
    let side = bs.iter().find(|b| b.3[0] < 30.0).unwrap_or_else(|| panic!("{bs:?}"));
    assert!((side.3[1] + 20.0).abs() < 1e-6, "B's end at the origin stops on A's face y = −20: {side:?}");
    assert_eq!(named(&mut s, "A"), before);
}

#[test]
fn a_mitre_frame_fits_its_free_ends_and_none_never_does() {
    // Fit Ends on a frame that isn't Auto fits the ends at no corner (B's line has two), and leaves its corners as
    // stage 1 builds them; a None frame never fits. So B runs y 20…380 under Mitre, 0…400 under None.
    for (corner, lo, hi) in [("mitre", 20.0, 380.0), ("none", 0.0, 400.0)] {
        let mut s = Session::default();
        earlier_frame(&mut s);
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Later"}));
        run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 400]]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "frame.member", json!({"sketch": "Later", "profile": square_of(25.0), "corner": corner, "fit_ends": true, "body_name": "B"}));
        assert_b_runs(&mut s, lo, hi);
    }
}

/// The overlap (mm³) between the bodies whose names start with `prefix`.
fn overlap(s: &mut Session, prefix: &str) -> f64 {
    let names: Vec<String> = named(s, prefix).into_iter().map(|b| b.0).collect();
    let i = run(s, "inspect.interference", json!({"bodies": names}));
    i["interferences"].as_array().map_or(0.0, |all| all.iter().map(|x| f(&x["volume_mm3"])).sum())
}

#[test]
fn ends_at_a_free_corner_are_fitted_to_each_other_too() {
    // An upright (300, 0)…(300, 300) and a 45° diagonal (300, 0)…(500, 200), 20 mm, leave one node inside A's
    // bottom tube: 135° from in line, Auto would mitre them, but the node is Free. Fitted to A alone, both stop at
    // y = 20, where the upright spans x 290…310 and the diagonal x 305.9…334.1 (its centre 320, half-width
    // 10 / cos 45°): they overlap up to y ≈ 24. The later laid of the two is also fitted to the earlier one.
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, 0.0], [300.0, 300.0]], [[300.0, 0.0], [500.0, 200.0]]], 20.0, true);
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 2, "{bs:?}");
    for b in &bs {
        assert!(b.2[1] >= 20.0 - 1e-6, "{b:?}");
    }
    let o = overlap(&mut s, "B");
    assert!(o < 1e-3, "the braces overlap by {o} mm³: {bs:?}");
}

/// Construction points named after each `(name, point)`, for pairs to join.
fn points(s: &mut Session, at: &[(&str, [f64; 3])]) {
    for (name, p) in at {
        run(s, "construct.point.vertex", json!({"point": {"point": p}, "name": name}));
    }
}

/// A pair from construction point `a` to `b`, as `pairs` takes it.
fn pair(a: &str, b: &str) -> Value {
    json!({"a": {"construct_point": a}, "b": {"construct_point": b}})
}

#[test]
fn pair_braces_meeting_inside_an_earlier_tube_are_fitted_not_chained() {
    // Ruling A15: the braces of `ends_at_a_free_corner_are_fitted_to_each_other_too` as point pairs, tubes in no
    // sketch plane. Their node (300, 0, 0) lies inside A's bottom tube: the chain corner there is Free, not mitred
    // inside the tube, and both ends are fitted to A and to each other.
    let mut s = Session::default();
    earlier_frame(&mut s);
    points(&mut s, &[("N", [300.0, 0.0, 0.0]), ("U", [300.0, 300.0, 0.0]), ("D", [500.0, 200.0, 0.0])]);
    let pairs = json!([pair("N", "U"), pair("N", "D")]);
    run(&mut s, "frame.member", json!({"pairs": pairs, "profile": square_of(20.0), "corner": "auto", "fit_ends": true, "body_name": "B"}));
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 2, "{bs:?}");
    for b in &bs {
        assert!(b.2[1] >= 20.0 - 1e-6, "{b:?}");
    }
    let o = overlap(&mut s, "B");
    assert!(o < 1e-3, "the braces overlap by {o} mm³: {bs:?}");
}

#[test]
fn pairs_ending_on_their_own_frames_tube_are_fitted_to_it_not_chained() {
    // Ruling A15 (finding 6): the same two pair braces in the rectangle's own Frame Member, under Auto. Their node is
    // on the bottom tube's middle, inside it: Free, so each brace is fitted to the bottom tube (laid first) and the
    // later one to the earlier, not mitred inside the tube. No tube of the frame overlaps another.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    points(&mut s, &[("N", [300.0, 0.0, 0.0]), ("U", [300.0, 300.0, 0.0]), ("D", [500.0, 200.0, 0.0])]);
    let pairs = json!([pair("N", "U"), pair("N", "D")]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "pairs": pairs, "profile": tube(), "corner": "auto", "fit_ends": true, "body_name": "B"}));
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 6, "{bs:?}");
    let braces: Vec<&Measured> = bs.iter().filter(|b| b.3[1] < 350.0 && b.2[0] > 250.0 && b.3[0] < 560.0).collect();
    assert_eq!(braces.len(), 2, "{bs:?}");
    for b in braces {
        assert!(b.2[1] >= 20.0 - 1e-6, "{b:?}");
    }
    let o = overlap(&mut s, "B");
    assert!(o < 1e-3, "the frame's tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn a_mitre_frame_fits_its_free_ends_to_its_own_tubes_in_drawing_order() {
    // Ruling A14: with no earlier Frame Member, the Mitre rectangle's cross-member (300, 0)…(300, 400) has two free
    // ends on the rails' middles. Each is fitted to the rail laid before it, in drawing order, not pick order: the
    // cross-member runs y 20…380 whichever order the lines are picked in, and every body comes out the same.
    let build = |reverse: bool| {
        let mut s = Session::default();
        cross_member_frame(&mut s);
        let mut lines: Vec<String> = sketch_lines(&s, "Frame").into_iter().map(|l| l.0).collect();
        if reverse {
            lines.reverse();
        }
        let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
        run(
            &mut s,
            "frame.member",
            json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "fit_ends": true, "lines": lines, "names": names}),
        );
        built(&mut s)
    };
    let ((drawn, drawn_faces), (picked, picked_faces)) = (build(false), build(true));
    assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
    for (line, (v, lo, hi)) in &drawn {
        let (pv, plo, phi) = picked[line];
        assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{line}: drawn {drawn:?} picked {picked:?}");
    }
    assert_eq!(drawn_faces, picked_faces);
    let cross = drawn.values().find(|b| b.1[0] > 250.0 && b.2[0] < 350.0).unwrap_or_else(|| panic!("{drawn:?}"));
    assert!(near3(cross.1, [280.0, 20.0, -20.0]) && near3(cross.2, [320.0, 380.0, 20.0]), "{cross:?}");
    assert!(rel(cross.0, tube_area() * 360.0) < MEASURED, "{cross:?} vs {}", tube_area() * 360.0);
}

#[test]
fn a_corner_on_an_earlier_frames_corner_is_free() {
    // B's corner at (0, 0) is a corner of A, where A's two tubes meet: on both tubes' end or mitre faces, inside
    // neither. It still lies in A, so it is Free: the diagonal (0, 0)…(300, 200) is fitted to A's two tubes, which
    // fill x < 20 and y < 20 round there (min x and y 20). Under Mitre A fills the whole corner square, so B's
    // other tube, (0, 0)…(0, −300), stops on A's outer face y = −20.
    for corner in ["mitre", "none"] {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": corner, "name": "A", "body_name": "A"}));
        later_frame(&mut s, 0.0, &[[[0.0, 0.0], [300.0, 200.0]], [[0.0, 0.0], [0.0, -300.0]]], 20.0, true);
        let bs = named(&mut s, "B");
        let diagonal = bs.iter().find(|b| b.3[0] > 200.0).unwrap_or_else(|| panic!("{bs:?}"));
        assert!(diagonal.2[0] >= 20.0 - 1e-6 && diagonal.2[1] >= 20.0 - 1e-6, "{corner}: {diagonal:?}");
        if corner == "mitre" {
            let down = bs.iter().find(|b| b.3[0] < 20.0).unwrap_or_else(|| panic!("{bs:?}"));
            assert!((down.3[1] + 20.0).abs() < 1e-6, "{down:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tubes that cross or clip in 3D are split or notched (#4003)

#[test]
fn a_pair_brace_ending_on_a_mitre_rails_middle_is_fitted_to_it() {
    // Ruling O1: a point-pair post (300, 0, 0)…(300, 0, 400) stands on the bottom rail's middle, Fit Ends off. Its
    // foot is fitted to the rail (filled, squared: z ≤ 20 there), so it runs z 20…400, A·380; the rail and the other
    // sides are the plain Mitre rectangle's.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    points(&mut s, &[("N", [300.0, 0.0, 0.0]), ("U", [300.0, 0.0, 400.0])]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "pairs": [pair("N", "U")], "profile": tube(), "corner": "mitre", "body_name": "R"}));
    let bs = bodies(&mut s);
    let post = bs.iter().find(|b| b.3[2] > 100.0).unwrap_or_else(|| panic!("{bs:?}")).clone();
    assert_tube(&post, [280.0, -20.0, 20.0], [320.0, 20.0, 400.0], 380.0);
    assert_same_tubes(&volumes_and_boxes(&mut s, &[post.0.as_str()]), &plain_rectangle(tube(), "mitre"), "sides");
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the frame overlaps by {o} mm³: {bs:?}");
}

#[test]
fn a_sketch_post_ending_on_a_butt_sides_middle_is_fitted_to_it() {
    // Ruling O1: the post (300, 0)…(300, 200) of a Butt rectangle ends on the bottom side's middle, Fit Ends off: it
    // is fitted flat to the side's face y = 20, running y 20…200, A·180; the sides are the plain Butt rectangle's.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 200]], "ids": ["p"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "names": {"p": "P"}}));
    let bs = bodies(&mut s);
    assert_tube(body_named(&bs, "P"), [280.0, 20.0, -20.0], [320.0, 200.0, 20.0], 180.0);
    assert_same_tubes(&volumes_and_boxes(&mut s, &["P"]), &plain_rectangle(tube(), "butt"), "sides");
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the frame overlaps by {o} mm³: {bs:?}");
}

/// Rail `pair1` (0, 0, 0)…(600, 0, 0) and brace `pair2` (300, −200, `z`)…(300, 200, `z`) in 40 × 40 × 3 tube, under
/// `corner`, named `T` and `T (2)`, with `more` added to the command.
fn rail_and_brace(z: f64, corner: &str, more: Value) -> Session {
    let mut s = Session::default();
    points(&mut s, &[("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [300.0, -200.0, z]), ("B1", [300.0, 200.0, z])]);
    let mut p = json!({"pairs": [pair("R0", "R1"), pair("B0", "B1")], "profile": tube(), "corner": corner, "body_name": "T"});
    for (k, v) in more.as_object().unwrap() {
        p[k] = v.clone();
    }
    run(&mut s, "frame.member", p);
    s
}

/// The body named `name`.
fn body_named<'a>(bs: &'a [Measured], name: &str) -> &'a Measured {
    bs.iter().find(|b| b.0 == name).unwrap_or_else(|| panic!("{name} in {bs:?}"))
}

/// `b` is a whole 40 tube from `lo` to `hi`, `length` long.
fn assert_tube(b: &Measured, lo: [f64; 3], hi: [f64; 3], length: f64) {
    assert!(near3(b.2, lo) && near3(b.3, hi), "{b:?}: not {lo:?}…{hi:?}");
    assert!(rel(b.1, tube_area() * length) < MEASURED, "{b:?} vs {}", tube_area() * length);
}

#[test]
fn a_point_pair_brace_crossing_a_rail_is_split() {
    // Tubes in no sketch plane are laid longest first: the rail (600) before the brace (400), which gives way. Cut by
    // the rail's filled, squared tool (y and z within ±20 over x 0…600), the brace leaves y −200…−20 and 20…200,
    // A·180 each. The piece from its drawn start (y = −200) keeps its name; the other is named after it and the rail.
    let mut s = rail_and_brace(0.0, "auto", json!({}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 3, "{bs:?}");
    assert_tube(body_named(&bs, "T"), [0.0, -20.0, -20.0], [600.0, 20.0, 20.0], 600.0);
    assert_tube(body_named(&bs, "T (2)"), [280.0, -200.0, -20.0], [320.0, -20.0, 20.0], 180.0);
    assert_tube(body_named(&bs, "T (2) · T"), [280.0, 20.0, -20.0], [320.0, 200.0, 20.0], 180.0);
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn a_piece_name_pinned_to_another_piece_is_not_taken() {
    // Two braces across the rail, at x = 200 (pair2) and 400 (pair3), each split as in
    // `a_point_pair_brace_crossing_a_rail_is_split`. pair3's later piece is pinned to `T (2) · T`, the name pair2's
    // would be given: the pin holds, and pair2's piece takes the next free name.
    let mut s = Session::default();
    let at = [("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [200.0, -200.0, 0.0]), ("B1", [200.0, 200.0, 0.0])];
    points(&mut s, &at);
    points(&mut s, &[("C0", [400.0, -200.0, 0.0]), ("C1", [400.0, 200.0, 0.0])]);
    let pairs = json!([pair("R0", "R1"), pair("B0", "B1"), pair("C0", "C1")]);
    let names = json!({"pair3>pair1": "T (2) · T"});
    run(&mut s, "frame.member", json!({"pairs": pairs, "profile": tube(), "corner": "auto", "body_name": "T", "names": names}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 5, "{bs:?}");
    assert_tube(body_named(&bs, "T (2) · T"), [380.0, 20.0, -20.0], [420.0, 200.0, 20.0], 180.0);
    assert_tube(body_named(&bs, "T (2) · T (2)"), [180.0, 20.0, -20.0], [220.0, 200.0, 20.0], 180.0);
}

#[test]
fn of_two_pieces_pinned_to_one_name_the_first_keeps_it() {
    // Both braces' later pieces pinned to `Brace`: pair2's, named first, takes it; pair3's is named as if unpinned.
    let mut s = Session::default();
    let at = [("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [200.0, -200.0, 0.0]), ("B1", [200.0, 200.0, 0.0])];
    points(&mut s, &at);
    points(&mut s, &[("C0", [400.0, -200.0, 0.0]), ("C1", [400.0, 200.0, 0.0])]);
    let pairs = json!([pair("R0", "R1"), pair("B0", "B1"), pair("C0", "C1")]);
    let names = json!({"pair2>pair1": "Brace", "pair3>pair1": "Brace"});
    run(&mut s, "frame.member", json!({"pairs": pairs, "profile": tube(), "corner": "auto", "body_name": "T", "names": names}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 5, "{bs:?}");
    assert_tube(body_named(&bs, "Brace"), [180.0, 20.0, -20.0], [220.0, 200.0, 20.0], 180.0);
    assert_tube(body_named(&bs, "T (3) · T"), [380.0, 20.0, -20.0], [420.0, 200.0, 20.0], 180.0);
}

#[test]
fn remove_tubes_reply_names_the_tubes_it_changed() {
    // A mitred rectangle loses its bottom tube: the side tubes that were mitred to it are changed (square ends now), the
    // top is not, and nothing is created.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre"}));
    let fid = first_frame(&s);
    let (before, bottom) = (boxes(&mut s), line_at_y(&s, "Frame", 0.0));
    let r = remove(&mut s, json!([{"feature": fid, "line": bottom}]));
    let after = boxes(&mut s);
    let moved: Vec<&String> =
        after.iter().filter(|(n, (lo, hi))| !before.get(*n).is_some_and(|b| near3(b.0, *lo) && near3(b.1, *hi))).map(|b| b.0).collect();
    assert_eq!((after.len(), moved.len()), (3, 2), "{before:?} {after:?}");
    assert_eq!(r["created_bodies"], json!([]), "{r}");
    let mut changed: Vec<String> = serde_json::from_value(r["changed_bodies"].clone()).unwrap();
    changed.sort();
    assert_eq!(changed.iter().collect::<Vec<_>>(), moved, "{r}");
}

#[test]
fn a_clipping_tube_is_notched() {
    // The brace runs 30 above the rail: the centrelines miss by 30, less than the section's 40. The brace keeps one
    // body, its name and its box (z 10…50), losing what lies in the rail's squared tool, y −20…20 and z 10…20. Of its
    // section (40 square, outer radius 6; bore 34 square, radius 3) that is the bottom 10: the outer 40 × 10 less two
    // corners 36 − 9π, less the bore's 34 × 7 less two corners 9 − 9π/4, 150.4115, over 40 of its length: 6016.46.
    let mut s = rail_and_brace(30.0, "auto", json!({}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    assert_tube(body_named(&bs, "T"), [0.0, -20.0, -20.0], [600.0, 20.0, 20.0], 600.0);
    let brace = body_named(&bs, "T (2)");
    assert!(near3(brace.2, [280.0, -200.0, 10.0]) && near3(brace.3, [320.0, 200.0, 50.0]), "{brace:?}");
    let strip = (400.0 - 2.0 * (36.0 - 9.0 * PI)) - (34.0 * 7.0 - 2.0 * (9.0 - 9.0 * PI / 4.0));
    let want = tube_area() * 400.0 - 40.0 * strip;
    assert!(rel(brace.1, want) < MEASURED, "{brace:?} vs {want}");
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {bs:?}");
}

/// The ids of 3D sketch `name`'s wires, in drawing order.
fn wire_ids(s: &Session, name: &str) -> Vec<String> {
    let id = s.doc.find_feature(name).unwrap().id;
    s.doc.sketch(id).unwrap().wires.iter().map(|w| w.id.clone()).collect()
}

#[test]
fn a_3d_sketch_line_crossing_a_sketch_tube_is_split() {
    // A 3D sketch's line along X (600) and its 3D line up Z through it (400), both in no sketch plane: the post is laid
    // second and gives way, cut by the rail's tool (y and z within ±20) into z −200…−20 and 20…200, A·180 each.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Space"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [600, 0]]}));
    run(&mut s, "sketch.line3d", json!({"points": [[300, 0, -200], [300, 0, 200]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Space", "profile": tube(), "corner": "auto", "body_name": "P"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 3, "{bs:?}");
    assert_tube(body_named(&bs, "P"), [0.0, -20.0, -20.0], [600.0, 20.0, 20.0], 600.0);
    assert_tube(body_named(&bs, "P (2)"), [280.0, -20.0, -200.0], [320.0, 20.0, -20.0], 180.0);
    assert_tube(body_named(&bs, "P (2) · P"), [280.0, -20.0, 20.0], [320.0, 20.0, 200.0], 180.0);
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn fit_ends_notches_against_an_earlier_frame() {
    // A's bottom tube (a None frame: square ends, x 0…600, y and z within ±20) and B's line (300, −100)…(300, 100) in
    // 25 square tube. B's ends are 80 from A, beyond their end zones, so Fit Ends leaves them; the overlap mid-span is
    // cut by A's tool: B keeps y −100…−20 (its name) and 20…100 (named after it and A's bottom tube), 80 of its 200
    // each, prismatic, cut square. With Fit Ends off B stays whole, overlapping A.
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, -100.0], [300.0, 100.0]]], 25.0, false);
    let whole = named(&mut s, "B");
    assert_eq!(whole.len(), 1, "{whole:?}");
    assert!((whole[0].2[1] + 100.0).abs() < 1e-6 && (whole[0].3[1] - 100.0).abs() < 1e-6, "{whole:?}");
    assert!(overlap_all(&mut s) > 1.0, "{whole:?}");
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, -100.0], [300.0, 100.0]]], 25.0, true);
    let bottom = bodies(&mut s).into_iter().find(|b| b.0.starts_with('A') && b.3[1] < 21.0).unwrap();
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 2, "{bs:?}");
    let first = body_named(&bs, "B");
    let later = body_named(&bs, &format!("B · {}", bottom.0));
    assert!((first.2[1] + 100.0).abs() < 1e-6 && (first.3[1] + 20.0).abs() < 1e-6, "{first:?}");
    assert!((later.2[1] - 20.0).abs() < 1e-6 && (later.3[1] - 100.0).abs() < 1e-6, "{later:?}");
    for b in [first, later] {
        assert!(rel(b.1, whole[0].1 * 0.4) < 1e-6, "{b:?} vs {}", whole[0].1 * 0.4);
    }
    // A's own corners overlap (a None frame): only B against A's bottom tube counts.
    let i = run(&mut s, "inspect.interference", json!({"bodies": [first.0, later.0, bottom.0]}));
    let o: f64 = i["interferences"].as_array().map_or(0.0, |all| all.iter().map(|x| f(&x["volume_mm3"])).sum());
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn a_none_frame_keeps_overlaps() {
    let mut s = rail_and_brace(0.0, "none", json!({}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    assert_tube(body_named(&bs, "T (2)"), [280.0, -200.0, -20.0], [320.0, 200.0, 20.0], 400.0);
    assert!(overlap_all(&mut s) > 1.0, "{bs:?}");
}

#[test]
fn an_overlap_override_flips_who_gives_way() {
    let flip = json!({"corners": [{"members": ["pair1", "pair2"], "corner": "auto", "flip": true}]});
    assert_the_rail_gives_way(flip);
    // Under Mitre too: the override names no corner, and is no error.
    let mut s = rail_and_brace(0.0, "mitre", json!({"corners": [{"members": ["pair1", "pair2"], "corner": "mitre", "flip": true}]}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 3, "{bs:?}");
    assert_tube(body_named(&bs, "T (2)"), [280.0, -200.0, -20.0], [320.0, 200.0, 20.0], 400.0);
}

#[test]
fn an_override_order_picks_who_gives_way_at_an_overlap() {
    // The order alone, no Flip: the brace, first, stays whole.
    assert_the_rail_gives_way(json!({"corners": [{"members": ["pair2", "pair1"], "kind": "auto"}]}));
}

fn assert_the_rail_gives_way(corners: Value) {
    // The rail and brace of `a_point_pair_brace_crossing_a_rail_is_split`, their set flipped: the rail gives way,
    // cut by the brace's tool (x 280…320) into x 0…280 and 320…600, A·280 each; the brace stays whole.
    let mut s = rail_and_brace(0.0, "auto", corners);
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 3, "{bs:?}");
    assert_tube(body_named(&bs, "T"), [0.0, -20.0, -20.0], [280.0, 20.0, 20.0], 280.0);
    assert_tube(body_named(&bs, "T · T (2)"), [320.0, -20.0, -20.0], [600.0, 20.0, 20.0], 280.0);
    assert_tube(body_named(&bs, "T (2)"), [280.0, -200.0, -20.0], [320.0, 200.0, 20.0], 400.0);
}

#[test]
fn an_overlap_override_on_tubes_that_no_longer_overlap_fails() {
    // The brace 60 above the rail misses it: an override of the two names no corner, crossing or overlap.
    let mut s = Session::default();
    points(&mut s, &[("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [300.0, -200.0, 60.0]), ("B1", [300.0, 200.0, 60.0])]);
    let p = json!({"pairs": [pair("R0", "R1"), pair("B0", "B1")], "profile": tube(), "corner": "auto",
                   "corners": [{"members": ["pair1", "pair2"], "corner": "auto", "flip": true}]});
    let e = s.execute("frame.member", &p).map(|_| String::new()).unwrap_or_else(|e| e.to_string());
    let member = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).map(|f| f.id);
    let e = member.and_then(|m| s.model.result(m)).and_then(|r| r.error.clone()).unwrap_or(e);
    assert!(e.contains("A corner override is not at a corner of the frame"), "{e}");
}

#[test]
fn overlaps_are_the_same_whatever_the_pick_order() {
    // A 3D sketch's rail (0, 0, 0)…(600, 0, 0), a brace clipping it 30 above at x = 300 and one crossing it at x = 100:
    // the braces tie (400 each) and are laid in drawing order, never the pick order, after the rail.
    let build = |reverse: bool| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Space"}));
        run(&mut s, "sketch.options", json!({"sketch_3d": true}));
        run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [600, 0, 0]]}));
        run(&mut s, "sketch.line3d", json!({"points": [[300, -200, 30], [300, 200, 30]]}));
        run(&mut s, "sketch.line3d", json!({"points": [[100, -200, 0], [100, 200, 0]]}));
        run(&mut s, "sketch.finish", json!({}));
        let mut lines = wire_ids(&s, "Space");
        if reverse {
            lines.reverse();
        }
        let names: serde_json::Map<String, Value> = lines.iter().map(|l| (l.clone(), json!(l))).collect();
        run(&mut s, "frame.member", json!({"sketch": "Space", "profile": tube(), "corner": "auto", "lines": lines, "names": names}));
        (built(&mut s), overlap_all(&mut s))
    };
    let (((drawn, drawn_faces), o), ((picked, picked_faces), _)) = (build(false), build(true));
    assert_eq!(drawn.len(), 4, "{drawn:?}");
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {drawn:?}");
    assert_eq!(drawn.keys().collect::<Vec<_>>(), picked.keys().collect::<Vec<_>>());
    for (name, (v, lo, hi)) in &drawn {
        let (pv, plo, phi) = picked[name];
        assert!(rel(pv, *v) < 1e-9 && near3(plo, *lo) && near3(phi, *hi), "{name}: drawn {drawn:?} picked {picked:?}");
    }
    assert_eq!(drawn_faces, picked_faces);
}

/// Each body as `name volume min max`, the numbers exactly as `{:?}` gives them, sorted.
fn exactly(s: &mut Session) -> Vec<String> {
    let mut out: Vec<String> = bodies(s).into_iter().map(|b| format!("{} {:?} {:?} {:?}", b.0, b.1, b.2, b.3)).collect();
    out.sort();
    out
}

/// An [`exactly`] line split into its name, its volume and its bounding box's corners: kernel updates move a volume's
/// last digits (a few parts in 1e9, #18310) and a corner's (about 1e-13 mm) without changing the shape, while the
/// names and the face names stay exact.
fn volume_apart(line: &str) -> (String, f64, Vec<f64>) {
    let Some(at) = line.find(" [") else { return (line.to_string(), 0.0, vec![]) };
    let (head, rest) = line.split_at(at);
    let corners = rest.split(['[', ']', ',']).filter_map(|v| v.trim().parse::<f64>().ok()).collect();
    match head.rsplit_once(' ').map(|(name, v)| (name, v.parse::<f64>())) {
        Some((name, Ok(v))) => (name.to_string(), v, corners),
        _ => (head.to_string(), 0.0, corners),
    }
}

/// Every body's name and sorted face names ([`built`]), as one FNV-1a digest and the number of faces: exact, and
/// short enough to record.
fn face_digest(s: &mut Session) -> String {
    let (_, faces) = built(s);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (body, names) in &faces {
        for part in std::iter::once(body).chain(names) {
            for b in part.bytes().chain([0]) {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }
    format!("{h:016x} {}", faces.values().map(Vec::len).sum::<usize>())
}

#[test]
fn frames_without_overlaps_are_unchanged() {
    // Built before overlaps were resolved (frame-crossings c522549e9), and recorded: an Auto X brace, a Mitre
    // cross-member with Fit Ends, two pairs mitred at a point, a later frame fitted to an earlier one, a None X
    // brace (which keeps its overlaps) and an Auto 3D sketch. Their face names too (a fitted end cut again would keep
    // its volume and box but rename its faces), as digests ([`face_digest`]) recorded at c522549e9.
    let mut designs: Vec<(&str, Session)> = Vec::new();
    let mut s = Session::default();
    x_brace_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "auto", "names": {"d1": "D1", "d2": "D2"}}));
    designs.push(("auto x brace", s));
    let mut s = Session::default();
    cross_member_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "fit_ends": true}));
    designs.push(("mitre cross-member", s));
    let mut s = Session::default();
    box_frame(&mut s);
    run(&mut s, "frame.member", json!({"profile": tube(), "pairs": [top_rail(), upright()], "corner": "mitre"}));
    designs.push(("mitred pairs", s));
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, 0.0], [300.0, 400.0]]], 25.0, true);
    designs.push(("fitted to an earlier frame", s));
    let mut s = Session::default();
    x_brace_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "none"}));
    designs.push(("none x brace", s));
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tower"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 1500], [600, 0, 1500]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Tower", "profile": tube(), "corner": "auto"}));
    designs.push(("auto 3d sketch", s));
    let got: Vec<(&str, Vec<String>, String)> = designs.into_iter().map(|(what, mut s)| (what, exactly(&mut s), face_digest(&mut s))).collect();
    let faces = [
        ("auto x brace", "244e5f6024b03c47 74"),
        ("mitre cross-member", "9f808806e6399040 90"),
        ("mitred pairs", "68889f4fe43671b7 42"),
        ("fitted to an earlier frame", "90810e5c064aefa0 90"),
        ("none x brace", "582487ac99a6847a 108"),
        ("auto 3d sketch", "d5435c6b5cc36dc5 36"),
    ];
    let want: &[(&str, &[&str])] = &[
        (
            "auto x brace",
            &[
                "D1 279714.87382276944 [19.999999999999993, 19.999999999999964, -20.0] [580.0000000000001, 380.00000000000006, 20.0]",
                "D2 130237.43691138434 [19.999999999999993, 200.00000000000003, -20.0] [299.99999999999994, 380.0, 20.0]",
                "D2 · D1 130237.43691138568 [300.0, 20.0, -20.0] [580.0, 199.99999999999997, 20.0]",
                "Rect Tube 40x40x3 (2) 159840.00000000076 [580.0, 20.0, -20.0] [620.0, 380.0, 20.0]",
                "Rect Tube 40x40x3 (3) 284160.0 [-19.999999999999773, 380.0, -20.0] [620.0, 420.0, 20.0]",
                "Rect Tube 40x40x3 (4) 159839.99999999913 [-20.0, 20.0, -20.0] [20.0, 380.0, 20.0]",
                "Rect Tube 40x40x3 284160.0 [-20.0, -20.0, -20.0] [620.0, 20.0, 20.0]",
            ],
        ),
        (
            "mitre cross-member",
            &[
                "Rect Tube 40x40x3 (2) 168329.20103018565 [580.0, -20.000000000000455, -20.0] [620.0, 419.9999999999998, 20.0]",
                "Rect Tube 40x40x3 (3) 252493.8019965837 [-20.000000000000227, 380.0, -20.0] [619.9999999999989, 420.0, 20.0]",
                "Rect Tube 40x40x3 (4) 168329.20103018687 [-20.0, -20.0, -20.0] [20.0, 420.0, 20.0]",
                "Rect Tube 40x40x3 (5) 151496.2808549591 [280.0, 20.0, -20.0] [320.0, 379.99999999999994, 20.0]",
                "Rect Tube 40x40x3 252493.80199658455 [-19.999999999999773, -20.0, -20.0] [620.0, 20.0, 20.0]",
            ],
        ),
        (
            "mitred pairs",
            &[
                "Box 96000000.0 [0.0, 0.0, 0.0] [600.0, 400.0, 400.0]",
                "Rect Tube 40x40x3 (2) 168329.2006366101 [580.0, -20.0, 0.0] [620.0, 20.0, 420.0000000000002]",
                "Rect Tube 40x40x3 252493.8017773667 [0.0, -20.0, 380.0] [619.9999999999995, 20.0, 420.0]",
            ],
        ),
        (
            "fitted to an earlier frame",
            &[
                "A (2) 168329.20145972772 [580.0, 0.0, -20.0] [620.0, 400.0, 20.0]",
                "A (3) 252493.8012062296 [0.0, 380.0, -20.0] [600.0, 420.0, 20.0]",
                "A (4) 168329.20145972774 [-20.0, 0.0, -20.0] [20.0, 400.0, 20.0]",
                "A 252493.80120622925 [0.0, -20.0, -20.0] [600.0, 20.0, 20.0]",
                "B 62531.68038144873 [287.5, 20.0, -12.5] [312.5, 380.0, 12.5]",
            ],
        ),
        (
            "none x brace",
            &[
                "Rect Tube 40x40x3 (2) 168329.20145972772 [580.0, 0.0, -20.0] [620.0, 400.0, 20.0]",
                "Rect Tube 40x40x3 (3) 252493.8012062296 [0.0, 380.0, -20.0] [600.0, 420.0, 20.0]",
                "Rect Tube 40x40x3 (4) 168329.20145972774 [-20.0, 0.0, -20.0] [20.0, 400.0, 20.0]",
                "Rect Tube 40x40x3 (5) 303459.78309394524 [-11.094003924504584, -16.641005886756876, -20.0] [611.0940039245046, 416.6410058867569, 20.0]",
                "Rect Tube 40x40x3 (6) 303459.783093863 [-11.094003924504582, -16.641005886756886, -20.00000000000002] [611.0940039245046, 416.6410058867569, 20.0]",
                "Rect Tube 40x40x3 252493.80120622925 [0.0, -20.0, -20.0] [600.0, 20.0, 20.0]",
            ],
        ),
        (
            "auto 3d sketch",
            &[
                "Rect Tube 40x40x3 (2) 252493.80179954885 [-19.999999999999773, -20.0, 1480.0] [600.0, 20.0, 1520.0]",
                "Rect Tube 40x40x3 631234.5013588069 [-20.0, -20.0, 0.0] [20.0, 20.0, 1520.0]",
            ],
        ),
    ];
    assert_eq!(got.len(), want.len(), "{got:#?}");
    for (((what, g, digest), (w_what, w)), (f_what, f)) in got.iter().zip(want).zip(faces) {
        assert_eq!((what, what), (w_what, &f_what));
        let (g, w): (Vec<_>, Vec<_>) = (g.iter().map(|l| volume_apart(l)).collect(), w.iter().map(|l| volume_apart(l)).collect());
        assert_eq!(g.iter().map(|l| &l.0).collect::<Vec<_>>(), w.iter().map(|l| &l.0).collect::<Vec<_>>(), "{what}");
        for (a, b) in g.iter().zip(&w) {
            assert!(rel(a.1, b.1) < 1e-8, "{what}: {} volume {} vs {}", a.0, a.1, b.1);
            assert!(a.2.len() == b.2.len() && a.2.iter().zip(&b.2).all(|(x, y)| (x - y).abs() < 1e-9), "{what}: {} box {:?} vs {:?}", a.0, a.2, b.2);
        }
        assert_eq!(digest, f, "{what}: face names");
    }
}

/// The id of the session's first Frame Member.
fn first_frame(s: &Session) -> u64 {
    s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id
}

/// The member of Frame Member `fid` whose body is `body`, as the context menu and End Cap find it.
fn member_of(s: &Session, fid: u64, body: &str) -> Option<String> {
    solidcraft_doc::frame::member_of_body(&s.doc, &s.model.state(), s.doc.feature(fid).unwrap(), body).unwrap()
}

#[test]
fn removing_an_overlap_piece_removes_its_line() {
    // The brace split by the rail (`a_point_pair_brace_crossing_a_rail_is_split`): its later piece `T (2) · T` is the
    // brace's piece past the rail, `pair2>pair1`. Remove Tube on it takes the whole brace out, and the rail stays whole.
    let mut s = rail_and_brace(0.0, "auto", json!({}));
    let fid = first_frame(&s);
    assert_eq!(member_of(&s, fid, "T (2) · T").as_deref(), Some("pair2>pair1"));
    remove(&mut s, json!([{"feature": fid, "line": "pair2>pair1"}]));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 1, "{bs:?}");
    assert_tube(body_named(&bs, "T"), [0.0, -20.0, -20.0], [600.0, 20.0, 20.0], 600.0);
    // The same for a sketch's lines: the 3D sketch's post split by its rail.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Space"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [600, 0]]}));
    run(&mut s, "sketch.line3d", json!({"points": [[300, 0, -200], [300, 0, 200]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Space", "profile": tube(), "corner": "auto", "body_name": "P"}));
    let fid = first_frame(&s);
    let piece = member_of(&s, fid, "P (2) · P").unwrap();
    let post = member_of(&s, fid, "P (2)").unwrap();
    assert!(piece.starts_with(&format!("{post}>")), "{piece} {post}");
    remove(&mut s, json!([{"feature": fid, "line": piece}]));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 1, "{bs:?}");
    assert_tube(body_named(&bs, "P"), [0.0, -20.0, -20.0], [600.0, 20.0, 20.0], 600.0);
}

#[test]
fn an_end_cap_on_an_overlap_pieces_cut_end_is_refused() {
    // The brace's later piece `T (2) · T` runs from its cut at the rail's face y = 20 to its line's end y = 200: End Cap
    // goes on that end (40 × 40, 6 past it) and refuses its cut end, as it does the first piece's cut end y = −20.
    let mut s = rail_and_brace(0.0, "auto", json!({}));
    let top = face_pick(&s, "T (2) · T", [318.5, 200.0, 0.0], [0.0, 1.0, 0.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let cap = measured(&mut s, "T (2) · T End Cap");
    assert!(near3(cap.2, [280.0, 200.0, -20.0]) && near3(cap.3, [320.0, 206.0, 20.0]), "{cap:?}");
    for (body, y, n) in [("T (2) · T", 20.0, -1.0), ("T (2)", -20.0, 1.0)] {
        let cut = face_pick(&s, body, [318.5, y, 0.0], [0.0, n, 0.0]);
        let e = s.execute("frame.end_cap", &json!({"faces": [cut], "thickness": 6})).unwrap_err().to_string();
        assert!(e.contains("not an open end"), "{body}: {e}");
    }
}

#[test]
fn an_overlap_piece_rename_is_kept() {
    // Renamed, the brace's later piece is pinned under its piece id `pair2>pair1`, not by position: it keeps its name
    // when the rail it was named after is renamed, and through an edit of the feature.
    let mut s = rail_and_brace(0.0, "auto", json!({}));
    let fid = first_frame(&s);
    run(&mut s, "body.rename", json!({"body": "T (2) · T", "name": "Upper brace"}));
    let names = pinned_names(&s, &json!(fid));
    assert_eq!(names.get("pair2>pair1").map(String::as_str), Some("Upper brace"), "{names:?}");
    assert!(s.doc.feature(fid).unwrap().body_names.is_empty());
    run(&mut s, "body.rename", json!({"body": "T", "name": "Rail"}));
    let solidcraft_doc::FeatureKind::FrameMember { pairs, .. } = member_kind(&s, &json!(fid)) else { panic!() };
    let bigger = json!({"kind": "rect_tube", "width": 50, "height": 50, "wall": 3});
    let params = json!({"pairs": pairs, "profile": bigger, "corner": "auto", "body_name": "T"});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": params}));
    let mut made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    made.sort();
    assert_eq!(made, ["Rail", "T (2)", "Upper brace"]);
    assert_eq!(member_of(&s, fid, "Upper brace").as_deref(), Some("pair2>pair1"));
}

#[test]
fn an_overlap_piece_cut_by_an_earlier_tube_keeps_its_pin_when_that_tube_is_renamed() {
    // B's line, split by A's bottom tube under Fit Ends (`fit_ends_notches_against_an_earlier_frame`): its later piece's
    // id names that tube by its feature and member (ruling O4), not its body name, so renaming the tube keeps the
    // piece's id and the name pinned under it.
    let mut s = Session::default();
    earlier_frame(&mut s);
    later_frame(&mut s, 0.0, &[[[300.0, -100.0], [300.0, 100.0]]], 25.0, true);
    let a = first_frame(&s);
    let b = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let bottom = bodies(&mut s).into_iter().find(|x| x.0.starts_with('A') && x.3[1] < 21.0).unwrap().0;
    let piece = format!("B · {bottom}");
    let id = member_of(&s, b, &piece).unwrap();
    assert!(id.contains(&format!(">{a}:")), "{id}");
    run(&mut s, "body.rename", json!({"body": piece, "name": "Upper B"}));
    run(&mut s, "body.rename", json!({"body": bottom, "name": "Base"}));
    let made: Vec<String> = bodies(&mut s).into_iter().map(|x| x.0).collect();
    assert!(made.contains(&"Upper B".to_string()) && !made.contains(&"B · Base".to_string()), "{made:?}");
    assert_eq!(pinned_names(&s, &json!(b)).get(&id).map(String::as_str), Some("Upper B"));
    assert_eq!(member_of(&s, b, "Upper B"), Some(id));
}

#[test]
fn an_overlap_piece_with_a_numbered_name_is_found() {
    // A body already called `T (2) · T`: the brace's later piece is numbered `T (2) · T (2)`. It is still the piece
    // `pair2>pair1` (ruling O5), and Remove Tube on it takes the brace out.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [2000, 2000, 2000], "length": 10, "width": 10, "height": 10, "body_name": "T (2) · T"}));
    points(&mut s, &[("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [300.0, -200.0, 0.0]), ("B1", [300.0, 200.0, 0.0])]);
    run(&mut s, "frame.member", json!({"pairs": [pair("R0", "R1"), pair("B0", "B1")], "profile": tube(), "corner": "auto", "body_name": "T"}));
    let fid = first_frame(&s);
    assert!(bodies(&mut s).iter().any(|b| b.0 == "T (2) · T (2)"));
    assert_eq!(member_of(&s, fid, "T (2) · T (2)").as_deref(), Some("pair2>pair1"));
    remove(&mut s, json!([{"feature": fid, "line": "pair2>pair1"}]));
    let mut made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    made.sort();
    assert_eq!(made, ["T", "T (2) · T"]);
}

#[test]
fn an_overlap_piece_of_a_line_with_no_pin_is_found() {
    // The brace's pin taken away (a frame pinned before its brace was): it is named `T (2)` from the base name, and its
    // later piece `T (2) · T` is still the piece `pair2>pair1`.
    let mut s = rail_and_brace(0.0, "auto", json!({}));
    let fid = first_frame(&s);
    change_pins(&mut s, &json!(fid), |names| {
        names.remove("pair2");
    });
    s.refresh();
    assert!(bodies(&mut s).iter().any(|b| b.0 == "T (2) · T"));
    assert_eq!(member_of(&s, fid, "T (2) · T").as_deref(), Some("pair2>pair1"));
}

#[test]
fn fit_ends_fits_a_later_frame_to_an_earlier_short_end_piece() {
    // The rail at y = −120 cuts the brace (300, −200)…(300, 200) in its first quarter: its short piece `T (2)` runs
    // y −200…−140 and holds none of the brace's quarter points. A later Fit Ends tube y = −170, x 200…400, 25 square,
    // crosses it mid-span and is split by its tool, x 200…280 and 320…400, overlapping nothing.
    let mut s = Session::default();
    points(&mut s, &[("R0", [0.0, -120.0, 0.0]), ("R1", [600.0, -120.0, 0.0]), ("B0", [300.0, -200.0, 0.0]), ("B1", [300.0, 200.0, 0.0])]);
    run(&mut s, "frame.member", json!({"pairs": [pair("R0", "R1"), pair("B0", "B1")], "profile": tube(), "corner": "auto", "body_name": "T"}));
    let short = measured(&mut s, "T (2)");
    assert!((short.2[1] + 200.0).abs() < 1e-6 && (short.3[1] + 140.0).abs() < 1e-6, "{short:?}");
    later_frame(&mut s, 0.0, &[[[200.0, -170.0], [400.0, -170.0]]], 25.0, true);
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 2, "{bs:?}");
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn an_auto_stem_ending_on_a_pair_tubes_middle_leaves_it_whole() {
    // Ruling O2: a sketch line (300, 200)…(300, 0) ends on the middle of a point-pair rail (0, 0, 0)…(600, 0, 0). Auto's
    // plan doesn't fit that end, so the two touch end on middle as they always did: the rail, laid after the sketch's
    // tube, is not notched by it and stays one whole A·600 tube.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(&mut s, "sketch.line", json!({"points": [[300, 200], [300, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    points(&mut s, &[("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0])]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "pairs": [pair("R0", "R1")], "profile": tube(), "corner": "auto", "body_name": "T"}));
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 2, "{bs:?}");
    let rail = bs.iter().find(|b| b.3[0] - b.2[0] > 500.0).unwrap_or_else(|| panic!("no rail in {bs:?}"));
    assert_tube(rail, [0.0, -20.0, -20.0], [600.0, 20.0, 20.0], 600.0);
}

#[test]
fn fit_ends_fits_a_later_frame_to_an_earlier_frames_overlap_pieces() {
    // The brace split by the rail (`a_point_pair_brace_crossing_a_rail_is_split`): `T (2)` y −200…−20 and `T (2) · T`
    // y 20…200, x 280…320. A later Fit Ends frame's lines y = ∓110, x 200…400, in 25 square tube, cross one piece each
    // mid-span: each is split by that piece's tool, x 200…280 and 320…400, 80 of its 200 each, and overlaps nothing.
    let mut s = rail_and_brace(0.0, "auto", json!({}));
    later_frame(&mut s, 0.0, &[[[200.0, -110.0], [400.0, -110.0]], [[200.0, 110.0], [400.0, 110.0]]], 25.0, true);
    let bs = named(&mut s, "B");
    assert_eq!(bs.len(), 4, "{bs:?}");
    for b in &bs {
        let (lo, hi) = (b.2[0], b.3[0]);
        let cut = ((lo - 200.0).abs() < 1e-6 && (hi - 280.0).abs() < 1e-6) || ((lo - 320.0).abs() < 1e-6 && (hi - 400.0).abs() < 1e-6);
        assert!(cut && rel(b.1, bs[0].1) < 1e-6, "{b:?}: {bs:?}");
    }
    assert!(bs.iter().any(|b| b.0.ends_with(" · T (2) · T")), "{bs:?}");
    let o = overlap_all(&mut s);
    assert!(o < 1e-3, "the tubes overlap by {o} mm³: {bs:?}");
}

#[test]
fn a_corner_override_at_the_far_end_of_a_crossed_line_is_kept() {
    // Overrides name lines (crossings, "Decided after the builder check" 2). An open U on a Mitre frame, l (0, 400)…(0, 0),
    // b (0, 0)…(600, 0) and r (600, 0)…(600, 400), and a rail x (200, −300)…(200, 400), the longest, laid first: b is
    // cut at x = 200, and its far piece `b>x` meets r. A Butt override of b and r butts that corner: b (600, by its
    // line) runs through to r's outer face, x = 620, and R stops on its top face, y = 20. Mitred, R reaches y = −20.
    let build = |corners: Value| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Lines"}));
        for (id, points) in [("l", [[0, 400], [0, 0]]), ("b", [[0, 0], [600, 0]]), ("r", [[600, 0], [600, 400]]), ("x", [[200, -300], [200, 400]])] {
            run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        let names = json!({"l": "L", "b": "B", "r": "R", "x": "X"});
        run(&mut s, "frame.member", json!({"sketch": "Lines", "profile": square_tube(), "corner": "mitre", "names": names, "corners": corners}));
        s
    };
    let mut s = build(json!([]));
    assert!((measured(&mut s, "R").2[1] + 20.0).abs() < 1e-6, "{:?}", bodies(&mut s));
    let mut s = build(json!([{"members": ["b", "r"], "corner": "butt"}]));
    let (r, far) = (measured(&mut s, "R"), measured(&mut s, "B · X"));
    assert!(near3(r.2, [580.0, 20.0, -20.0]) && near3(r.3, [620.0, 400.0, 20.0]), "{r:?}");
    assert!((far.3[0] - 620.0).abs() < 1e-6, "{far:?}");
}

#[test]
fn an_overlap_pieces_id_names_its_cutters_line() {
    // Crossings rule 5, for overlaps too: rail r (0, 0)…(600, 0) is cut at x = 100 by c (100, −400)…(100, 400), longer
    // and laid first, and a pair brace P (300, −200, 0)…(300, 200, 0) crosses r's piece past c. P's later piece is
    // `pair1>r`, named `P · R`, after r's line, not its piece: a line d (200, −400)…(200, 400) crossing r too, so that
    // P is now cut by r's piece past d, leaves that id and name as they were.
    for with_d in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Lines"}));
        let mut lines = vec![("r", [[0, 0], [600, 0]]), ("c", [[100, -400], [100, 400]])];
        if with_d {
            lines.push(("d", [[200, -400], [200, 400]]));
        }
        for (id, points) in lines {
            run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
        }
        run(&mut s, "sketch.finish", json!({}));
        points(&mut s, &[("P0", [300.0, -200.0, 0.0]), ("P1", [300.0, 200.0, 0.0])]);
        let names = json!({"r": "R", "c": "C", "d": "D", "pair1": "P"});
        run(
            &mut s,
            "frame.member",
            json!({"sketch": "Lines", "pairs": [pair("P0", "P1")], "profile": square_tube(), "corner": "mitre", "names": names}),
        );
        let fid = first_frame(&s);
        let made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
        assert!(made.contains(&"P · R".to_string()), "{with_d}: {made:?}");
        assert_eq!(member_of(&s, fid, "P · R").as_deref(), Some("pair1>r"), "{with_d}");
    }
}

#[test]
fn a_tube_cut_in_three_by_one_other_keeps_its_pins() {
    // An earlier I-beam rail along X (100 high in Z, flanges 50 wide in Y) and a later Fit Ends brace, 10 square,
    // upright at y = 15, through both flanges beside the web: three pieces, z −100…−50, −42…42 and 50…100. Its two later
    // pieces have one id, `pair1>…`; the second is passed over, not the whole frame unpinned (#4003 final review I2).
    let mut s = Session::default();
    points(&mut s, &[("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [300.0, 15.0, -100.0]), ("B1", [300.0, 15.0, 100.0])]);
    let beam = json!({"kind": "i_beam", "height": 100, "width": 50, "web": 6, "flange": 8, "root_radius": 0});
    run(&mut s, "frame.member", json!({"pairs": [pair("R0", "R1")], "profile": beam, "body_name": "Rail"}));
    run(
        &mut s,
        "frame.member",
        json!({"pairs": [pair("B0", "B1")], "profile": square_of(10.0), "corner": "auto", "fit_ends": true, "body_name": "B"}),
    );
    let mut made: Vec<String> = named(&mut s, "B").into_iter().map(|b| b.0).collect();
    made.sort();
    assert_eq!(made, ["B", "B · Rail", "B · Rail (2)"]);
    let b = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let pins = crate::cmd::browser::member_pins(&s, b).expect("pinned");
    assert_eq!(pins.get("pair1").map(String::as_str), Some("B"), "{pins:?}");
    assert_eq!(pins.values().filter(|n| n.starts_with("B · Rail")).count(), 1, "{pins:?}");
}

#[test]
fn member_pins_pass_over_a_body_they_cant_place() {
    // The brace split by the rail, its pieces pinned. Pins that no longer agree with the bodies made (the brace's
    // changed, not yet built): `Brace` is found for no member and keeps its name unpinned, and the rest stay pinned.
    let names = json!({"names": {"pair1": "Rail", "pair2": "Brace", "pair2>pair1": "Upper"}});
    let mut s = rail_and_brace(0.0, "auto", names);
    let fid = first_frame(&s);
    change_pins(&mut s, &json!(fid), |names| {
        names.insert("pair2".into(), "Other".into());
    });
    let pins = crate::cmd::browser::member_pins(&s, fid).expect("pinned");
    let want: std::collections::BTreeMap<String, String> =
        [("pair1", "Rail"), ("pair2", "Other"), ("pair2>pair1", "Upper")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(pins, want);
}

#[test]
fn member_pins_pair_by_name_when_a_piece_evens_the_count() {
    // Ruling O5 (M1): a stub s (300, −10)…(300, 200) crosses the rail a (0, 0)…(600, 0) inside its tube, so its first
    // piece is dropped (C4) and `s>a` kept, and a pair brace (450, ∓200, 0) crossing a is split by it: four members
    // (a, s, s>a, pair1) and four bodies, one of them the overlap piece `pair1>a`. Pins pair them by name, never by
    // position.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Lines"}));
    for (id, points) in [("a", [[0, 0], [600, 0]]), ("s", [[300, -10], [300, 200]])] {
        run(&mut s, "sketch.line", json!({"points": points, "ids": [id]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    points(&mut s, &[("P0", [450.0, -200.0, 0.0]), ("P1", [450.0, 200.0, 0.0])]);
    let names = json!({"a": "A", "s": "S", "pair1": "P"});
    run(&mut s, "frame.member", json!({"sketch": "Lines", "pairs": [pair("P0", "P1")], "profile": square_tube(), "corner": "auto", "names": names}));
    let fid = first_frame(&s);
    let mut made: Vec<String> = bodies(&mut s).into_iter().map(|b| b.0).collect();
    made.sort();
    assert_eq!(made, ["A", "P", "P · A", "S · A"]);
    let lines = solidcraft_doc::frame::member_line_ids(&s.model.state(), &s.doc.feature(fid).unwrap().kind).unwrap();
    assert_eq!(lines, ["a", "s", "s>a", "pair1"]);
    let pins = crate::cmd::browser::member_pins(&s, fid).expect("pinned");
    for (id, name) in [("a", "A"), ("s>a", "S · A"), ("pair1", "P"), ("pair1>a", "P · A")] {
        assert_eq!(pins.get(id).map(String::as_str), Some(name), "{pins:?}");
    }
}

#[test]
fn a_join_after_a_frame_leaves_its_members_found() {
    // Two None frames named `T`: A's crossing pairs `T` and `T (2)`, then B's pairs `T (3)` and `T (4)`, unpinned (a
    // frame saved before names were pinned). A later join takes `T (2)` into `T`, freeing its name: B's bodies are still
    // its members in their order, as named when B was built (#4003 final review (a)).
    let mut s = Session::default();
    points(&mut s, &[("A0", [0.0, 0.0, 0.0]), ("A1", [600.0, 0.0, 0.0]), ("A2", [300.0, -200.0, 0.0]), ("A3", [300.0, 200.0, 0.0])]);
    run(&mut s, "frame.member", json!({"pairs": [pair("A0", "A1"), pair("A2", "A3")], "profile": tube(), "body_name": "T"}));
    points(&mut s, &[("B0", [0.0, 1000.0, 0.0]), ("B1", [600.0, 1000.0, 0.0]), ("B2", [0.0, 1200.0, 0.0]), ("B3", [600.0, 1200.0, 0.0])]);
    run(&mut s, "frame.member", json!({"pairs": [pair("B0", "B1"), pair("B2", "B3")], "profile": tube(), "body_name": "T"}));
    let b = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    change_pins(&mut s, &json!(b), std::collections::BTreeMap::clear);
    s.refresh();
    run(&mut s, "solid.combine", json!({"target": "T", "tools": ["T (2)"], "operation": "join"}));
    let made: Vec<String> = bodies(&mut s).into_iter().map(|x| x.0).collect();
    assert!(!made.contains(&"T (2)".to_string()) && made.contains(&"T (3)".to_string()), "{made:?}");
    assert_eq!(member_of(&s, b, "T (3)").as_deref(), Some("pair1"));
    assert_eq!(member_of(&s, b, "T (4)").as_deref(), Some("pair2"));
}

/// A Locost-sized design (about 60 tubes over four Frame Members, and point-pair braces crossing them, all Auto with
/// fit_ends), timed through a full evaluation, which it prints (no time is asserted). Run on demand: `cargo test ...
/// locost_sized_design_timing -- --ignored --nocapture`.
#[test]
#[ignore = "timing check, run on demand"]
fn locost_sized_design_timing() {
    let mut s = Session::default();
    let frame = |s: &mut Session, name: &str, plane: &str, offset: f64| {
        let mut sk = json!({"plane": plane, "name": name});
        if offset != 0.0 {
            sk["offset"] = json!(offset);
        }
        run(s, "sketch.create", sk);
        run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [1000, 600]}));
        for x in [200, 400, 600, 800] {
            run(s, "sketch.line", json!({"points": [[x, 0], [x, 600]]}));
        }
        run(s, "sketch.finish", json!({}));
        run(
            s,
            "frame.member",
            json!({"sketch": name, "profile": json!({"kind": "rect_tube", "width": 25, "height": 25, "wall": 2}), "corner": "auto", "fit_ends": true, "body_name": name}),
        );
    };
    frame(&mut s, "Left", "XZ", 0.0);
    frame(&mut s, "Right", "XZ", 400.0);
    frame(&mut s, "Floor", "XY", 0.0);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Braces", "offset": 25}));
    let zigzag: Vec<Value> = (0..=36).map(|i| json!([i as f64 * 1000.0 / 36.0, if i % 2 == 0 { 0 } else { 400 }])).collect();
    run(&mut s, "sketch.line", json!({"points": zigzag}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Braces", "profile": json!({"kind": "rect_tube", "width": 20, "height": 20, "wall": 2}), "corner": "auto", "fit_ends": true, "body_name": "Brace"}),
    );
    // Two X braces: each pair of diagonals crosses, so Auto cuts and fits pieces.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Crossed", "offset": 50}));
    for ((x0, x1), (y0, y1)) in [((0, 400), (0, 600)), ((600, 1000), (0, 600))] {
        run(&mut s, "sketch.line", json!({"points": [[x0, y0], [x1, y1]]}));
        run(&mut s, "sketch.line", json!({"points": [[x0, y1], [x1, y0]]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Crossed", "profile": json!({"kind": "rect_tube", "width": 20, "height": 20, "wall": 2}), "corner": "auto", "fit_ends": true, "body_name": "Cross"}),
    );
    // Point-pair braces in the floor's plane, in no sketch plane: two along X crossing the floor's four cross-members,
    // and one along Y crossing both of them and the floor's sides, so overlaps split them against earlier frames and
    // each other.
    let ends = [("P0", [100.0, 150.0, 0.0]), ("P1", [900.0, 150.0, 0.0]), ("P2", [100.0, 450.0, 0.0]), ("P3", [900.0, 450.0, 0.0])];
    points(&mut s, &ends);
    points(&mut s, &[("P4", [500.0, -100.0, 0.0]), ("P5", [500.0, 700.0, 0.0])]);
    let pairs = [pair("P0", "P1"), pair("P2", "P3"), pair("P4", "P5")];
    let profile = json!({"kind": "rect_tube", "width": 20, "height": 20, "wall": 2});
    run(&mut s, "frame.member", json!({"pairs": pairs, "profile": profile, "corner": "auto", "fit_ends": true, "body_name": "Pair"}));
    let json_doc = s.doc.to_json();
    let start = std::time::Instant::now();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&json_doc).unwrap());
    let n = bodies(&mut reopened).len();
    println!("LOCOST tubes={n} full evaluation={:.2?}", start.elapsed());
    assert!(n >= 50, "{n}");
}

// ---------------------------------------------------------------------------------------------
// The weld gap (#17854)

/// A 45 × 45 solid bar, 2025 mm² of section.
fn bar45() -> Value {
    json!({"kind": "bar", "width": 45, "height": 45})
}

/// The rectangle frame in [`bar45`] finished `corner`, with `more` added to the command: each body's volume by name.
fn bar_rectangle(corner: &str, more: Value) -> std::collections::BTreeMap<String, f64> {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let mut p = json!({"sketch": "Frame", "profile": bar45(), "corner": corner});
    for (k, v) in more.as_object().unwrap() {
        p[k] = v.clone();
    }
    run(&mut s, "frame.member", p);
    bodies(&mut s).into_iter().map(|b| (b.0, b.1)).collect()
}

/// The volumes of a rectangle's bars, largest first.
fn sorted_volumes(v: &std::collections::BTreeMap<String, f64>) -> Vec<f64> {
    let mut v: Vec<f64> = v.values().copied().collect();
    v.sort_by(|a, b| b.total_cmp(a));
    v
}

#[test]
fn a_mitred_rectangle_with_a_gap() {
    // Each mitre plane moves 1.5 back square to itself: 1.5√2 along each bar at each end.
    let got = sorted_volumes(&bar_rectangle("mitre", json!({"gap": 3})));
    let r2 = 2f64.sqrt();
    let want = [600.0, 600.0, 400.0, 400.0].map(|l| 2025.0 * (l - 3.0 * r2));
    assert_eq!(got.len(), 4, "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(rel(*g, w) < 1e-6, "{got:?} vs {want:?}");
    }
}

#[test]
fn a_butted_rectangle_with_a_gap() {
    // The 600s run through, unchanged; each 400 stops 2 short of them at both ends.
    let (plain, gapped) = (bar_rectangle("butt", json!({})), bar_rectangle("butt", json!({"gap": 2})));
    assert_eq!(plain.keys().collect::<Vec<_>>(), gapped.keys().collect::<Vec<_>>());
    let mut short = 0;
    for (name, v) in &plain {
        let g = gapped[name];
        if *v > 2025.0 * 500.0 {
            assert!(rel(g, *v) < 1e-9, "{name}: {g} vs {v}");
        } else {
            short += 1;
            assert!(rel(g, v - 2025.0 * 4.0) < 1e-6, "{name}: {g} vs {}", v - 2025.0 * 4.0);
        }
    }
    assert_eq!(short, 2, "{plain:?}");
}

#[test]
fn an_auto_angled_butt_sets_only_the_short_member_back() {
    // `auto_vee(160)` in [`bar45`]: the table's angled butt (20° from in line). The long 400 is cut square to its line
    // at the corner (x = 0) and keeps that cut whatever the gap: A·400. The short 300 sits on the same plane, which
    // the gap moves g back along −X, away from the long bar: x = −g meets its line (at 20° from −X) g / cos 20° from
    // the corner, so it keeps A·(300 − g / cos 20°). A wrong sign would make it longer, into the long bar.
    let vee = |gap: f64| {
        let t = 160f64.to_radians();
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
        run(&mut s, "sketch.line", json!({"points": [[400, 0], [0, 0], [300.0 * t.cos(), 300.0 * t.sin()]]}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "frame.member", json!({"sketch": "V", "profile": bar45(), "corner": "auto", "gap": gap}));
        let (long, short) = long_and_short(&mut s);
        (long.1, short.1)
    };
    let a = 2025.0;
    let (plain, gapped) = (vee(0.0), vee(2.0));
    assert!(rel(plain.0, a * 400.0) < 1e-6 && rel(plain.1, a * 300.0) < 1e-6, "{plain:?}");
    let short = a * (300.0 - 2.0 / 20f64.to_radians().cos());
    assert!(rel(gapped.0, plain.0) < 1e-9, "the long bar: {gapped:?} vs {plain:?}");
    assert!(rel(gapped.1, short) < 1e-6, "the short bar: {} vs {short}", gapped.1);
}

#[test]
fn an_override_gap_beats_the_frames() {
    // Gap 1 on the frame, 3 at the corner (0, 0): the bottom and left bars lose (3 + 1)/√2 over their two ends, the
    // top and right ones 2/√2.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    let (top, right) = (line_at_y(&s, "Frame", 400.0), line_at_x(&s, "Frame", 600.0));
    let mut names = serde_json::Map::new();
    for (line, name) in [(&bottom, "B"), (&left, "L"), (&top, "T"), (&right, "R")] {
        names.insert(line.clone(), json!(name));
    }
    let corners = json!([{"members": [bottom, left], "kind": "mitre", "gap": 3}]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": bar45(), "corner": "mitre", "gap": 1, "corners": corners, "names": names}));
    let r2 = 2f64.sqrt();
    for (name, l, ends) in [("B", 600.0, 4.0), ("L", 400.0, 4.0), ("T", 600.0, 2.0), ("R", 400.0, 2.0)] {
        let v = measured(&mut s, name).1;
        assert!(rel(v, 2025.0 * (l - ends / r2)) < 1e-6, "{name}: {v} vs {}", 2025.0 * (l - ends / r2));
    }
}

/// The least distance between bodies `a` and `b`, measured on their exact faces.
fn distance(s: &mut Session, a: &str, b: &str) -> f64 {
    let r = run(s, "inspect.clearance", json!({"bodies": [a], "against": [b], "clearance": 50}));
    assert_eq!(r["overlaps"].as_array().map(Vec::len), Some(0), "{r}");
    f(&r["near"][0]["distance_mm"])
}

#[test]
fn a_coped_t_with_a_gap_stands_off() {
    // v ends on h's middle, coped to h's round tube grown by 2: its whole end stands 2 off h.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Tee"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[200, 0], [200, 100]], "ids": ["v"]}));
    run(&mut s, "sketch.finish", json!({}));
    let profile = json!({"kind": "round_tube", "diameter": 40, "wall": 3});
    let p = json!({"sketch": "Tee", "profile": profile, "corner": "butt", "coped": true, "gap": 2, "names": {"h": "H", "v": "V"}});
    run(&mut s, "frame.member", p);
    let d = distance(&mut s, "V", "H");
    assert!((d - 2.0).abs() < 1e-6, "{d}");
}

#[test]
fn crossing_pieces_stand_off_by_the_gap() {
    // v crosses h, cut into `V` and `V · H`, each fitted to h's squared tool grown by 2: flat, 22 from h's line.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[200, -100], [200, 100]], "ids": ["v"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Plus", "profile": tube(), "corner": "butt", "gap": 2, "names": {"h": "H", "v": "V"}}));
    for name in ["V", "V · H"] {
        let b = measured(&mut s, name);
        let near_h = b.2[1].abs().min(b.3[1].abs());
        assert!((near_h - 22.0).abs() < 1e-6, "{name}: {b:?}");
        let d = distance(&mut s, name, "H");
        assert!((d - 2.0).abs() < 1e-6, "{name}: {d}");
    }
    // An overlap is notched, never welded: the brace 30 above the rail builds the same at gap 2 as at gap 0.
    let notched = |more: Value| {
        let mut s = rail_and_brace(30.0, "butt", more);
        bodies(&mut s).into_iter().map(|b| (b.0, b.1)).collect::<Vec<_>>()
    };
    let (plain, gapped) = (notched(json!({})), notched(json!({"gap": 2})));
    assert_eq!(plain.len(), 2, "{plain:?}");
    assert_eq!(gapped.len(), 2, "{gapped:?}");
    for ((n, v), (gn, g)) in plain.iter().zip(&gapped) {
        assert!(n == gn && rel(*g, *v) < 1e-9, "{plain:?} vs {gapped:?}");
    }
}

#[test]
fn the_later_frame_stands_off_an_earlier_one_by_its_gap() {
    // A post across the earlier rectangle's middle, fitted to its bottom and top tubes: with gap 2 it stops 2 short of
    // each, 22 from their lines.
    let mut s = Session::default();
    earlier_frame(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Later"}));
    run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 400]]}));
    run(&mut s, "sketch.finish", json!({}));
    let p = json!({"sketch": "Later", "profile": square_of(25.0), "corner": "auto", "fit_ends": true, "gap": 2, "body_name": "B"});
    run(&mut s, "frame.member", p);
    let b = measured(&mut s, "B");
    assert!((b.2[1] - 22.0).abs() < 1e-6 && (b.3[1] - 378.0).abs() < 1e-6, "{b:?}");
}

#[test]
fn an_overlap_override_with_a_gap_is_refused() {
    let mut s = Session::default();
    points(&mut s, &[("R0", [0.0, 0.0, 0.0]), ("R1", [600.0, 0.0, 0.0]), ("B0", [300.0, -200.0, 30.0]), ("B1", [300.0, 200.0, 30.0])]);
    let corners = json!([{"members": ["pair1", "pair2"], "kind": "auto", "gap": 2}]);
    let p = json!({"pairs": [pair("R0", "R1"), pair("B0", "B1")], "profile": tube(), "corner": "butt", "corners": corners});
    let e = s.execute("frame.member", &p).unwrap_err().to_string();
    assert!(e.contains("gap") && e.contains("overlap"), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_gap_out_of_range_is_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    for gap in [-1.0, 1001.0] {
        let p = json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "gap": gap});
        let e = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(e.contains("`gap`") && e.contains("0…1000 mm"), "{gap}: {e}");
        let corners = json!([{"members": [bottom, left], "kind": "mitre", "gap": gap}]);
        let p = json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "corners": corners});
        let e = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(e.contains("gap") && e.contains("0…1000 mm"), "{gap}: {e}");
    }
    assert!(bodies(&mut s).is_empty());
    // A design file is hostile: a saved gap out of range fails the feature with the reason.
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "mitre", "gap": "1 mm"}));
    let member = s.doc.features.iter().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).unwrap().id;
    let json_doc = s.doc.to_json();
    assert!(json_doc.contains(r#""gap": "1 mm""#), "{json_doc}");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&json_doc.replace(r#""gap": "1 mm""#, r#""gap": "-1 mm""#)).unwrap());
    let e = reopened.model.result(member).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("gap") && e.contains("0…1000 mm"), "{e}");
}

#[test]
fn a_bare_gap_is_in_the_design_units() {
    // In an inch design a bare gap is inches, checked as such: 0.1 (2.54 mm) builds and is saved `0.1 in`; 40 is
    // 1016 mm, refused by the command before anything is added, the frame's gap and an override's alike.
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 0], [0, 0], [0, 8]], "ids": ["a", "b"]}));
    run(&mut s, "sketch.finish", json!({}));
    let bar = json!({"kind": "bar", "width": 1, "height": 1});
    let features = s.doc.features.len();
    let corners = json!([{"members": ["a", "b"], "kind": "mitre", "gap": 40}]);
    for p in [
        json!({"sketch": "V", "profile": bar, "corner": "mitre", "gap": 40}),
        json!({"sketch": "V", "profile": bar, "corner": "mitre", "corners": corners}),
    ] {
        let e = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(e.contains("frame.member") && e.contains("0…1000 mm, not 1016 mm"), "{p}: {e}");
        assert_eq!(s.doc.features.len(), features, "{p}: nothing added");
    }
    let corners = json!([{"members": ["a", "b"], "kind": "mitre", "gap": 0.2}]);
    run(&mut s, "frame.member", json!({"sketch": "V", "profile": bar, "corner": "mitre", "gap": 0.1, "corners": corners}));
    let solidcraft_doc::FeatureKind::FrameMember { gap, corners, .. } = &s.doc.features.last().unwrap().kind else { panic!() };
    assert_eq!((gap.as_deref(), corners[0].gap.as_deref()), (Some("0.1 in"), Some("0.2 in")));
    assert_eq!(bodies(&mut s).len(), 2);
}

#[test]
fn a_gap_longer_than_a_member_names_it() {
    // A 30 mm stub butting a 400: cut 40 back from the 400's near face, nothing of it is left.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "V"}));
    run(&mut s, "sketch.line", json!({"points": [[400, 0], [0, 0], [0, 30]]}));
    run(&mut s, "sketch.finish", json!({}));
    let stub = line_off_x_axis(&s, "V");
    let e = s.execute("frame.member", &json!({"sketch": "V", "profile": tube(), "corner": "butt", "gap": 40})).unwrap_err().to_string();
    assert!(e.contains(&format!("the member on line `{stub}` was cut away completely")), "{e}");
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_coped_gap_beyond_the_corner_radius_never_panics() {
    // Outer radius 1, gap 5: the cope's outline grows past its corner radii. It builds, each butting tube 5 off.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let profile = json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 1, "inner_radius": 0.5});
    let r = s.execute("frame.member", &json!({"sketch": "Frame", "profile": profile, "corner": "butt", "coped": true, "gap": 5}));
    let r = r.unwrap_or_else(|e| panic!("{e}"));
    let (bottom, left) = bottom_and_left(&mut s);
    assert_eq!(bodies(&mut s).len(), 4, "{r}");
    // The nearest points lie on the grown radii, where the distance solver settles to about 1e-6.
    let d = distance(&mut s, &left.0, &bottom.0);
    assert!((d - 5.0).abs() < 1e-5, "{d}");
    // A channel's root radii are inside corners: a gap of 20 can't grow round them, and says so.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let channel = json!({"kind": "channel", "height": 60, "width": 45, "web": 5, "flange": 6});
    let r = s.execute("frame.member", &json!({"sketch": "Frame", "profile": channel, "corner": "butt", "coped": true, "gap": 20}));
    if let Err(e) = r {
        assert!(e.to_string().contains("weld gap of 20 mm"), "{e}");
    }
}

#[test]
fn a_crossing_overrides_gap_stands_its_pieces_off() {
    // h is crossed by v and by w; only the crossing of h and v is overridden, with gap 3 on a frame with none: v's
    // pieces stop 23 from h's line, w's 20.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Plus"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [400, 0]], "ids": ["h"]}));
    run(&mut s, "sketch.line", json!({"points": [[100, -100], [100, 100]], "ids": ["v"]}));
    run(&mut s, "sketch.line", json!({"points": [[300, -100], [300, 100]], "ids": ["w"]}));
    run(&mut s, "sketch.finish", json!({}));
    let corners = json!([{"members": ["h", "v"], "kind": "auto", "gap": 3}]);
    let names = json!({"h": "H", "v": "V", "w": "W"});
    run(&mut s, "frame.member", json!({"sketch": "Plus", "profile": tube(), "corner": "butt", "corners": corners, "names": names}));
    for (name, off) in [("V", 23.0), ("V · H", 23.0), ("W", 20.0), ("W · H", 20.0)] {
        let b = measured(&mut s, name);
        let near_h = b.2[1].abs().min(b.3[1].abs());
        assert!((near_h - off).abs() < 1e-6, "{name}: {b:?}");
    }
}

#[test]
fn a_crowded_overrides_gap_stands_its_fitted_member_off() {
    // Three tubes end at (300, 0) on an Auto frame with no gap; the override of all three, gap 3, makes the corner of
    // the two along X (in line, both square) and fits the upright 3 off their top faces, at y = 23.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "T"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [300, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[300, 0], [600, 0]], "ids": ["b"]}));
    run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 300]], "ids": ["up"]}));
    run(&mut s, "sketch.finish", json!({}));
    let corners = json!([{"members": ["a", "b", "up"], "kind": "auto", "gap": 3}]);
    run(&mut s, "frame.member", json!({"sketch": "T", "profile": tube(), "corner": "auto", "corners": corners, "names": {"up": "Up"}}));
    let up = measured(&mut s, "Up");
    assert!((up.2[1] - 23.0).abs() < 1e-6, "{up:?}");
}

// ---------------------------------------------------------------------------------------------
// frame.multi_trim (#17854)

/// The 600 × 400 rectangle in [`bar45`] finished `corner`, `more` added, its bars named B (y 0), L (x 0), T and R:
/// the session and the ids of the bottom and left lines.
fn named_bar_rectangle(corner: &str, more: Value) -> (Session, String, String) {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    let (top, right) = (line_at_y(&s, "Frame", 400.0), line_at_x(&s, "Frame", 600.0));
    let names = json!({bottom.clone(): "B", left.clone(): "L", top: "T", right: "R"});
    let mut p = json!({"sketch": "Frame", "profile": bar45(), "corner": corner, "names": names, "name": "Rect"});
    for (k, v) in more.as_object().unwrap() {
        p[k] = v.clone();
    }
    run(&mut s, "frame.member", p);
    (s, bottom, left)
}

/// The volumes of bars B, L, T and R.
fn blrt(s: &mut Session) -> [f64; 4] {
    ["B", "L", "T", "R"].map(|n| measured(s, n).1)
}

/// The timeline's Frame Member entries.
fn frame_entries(s: &Session) -> usize {
    s.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMember { .. })).count()
}

#[test]
fn multi_trim_sets_a_corner() {
    // Hand values. The Auto rectangle butts its square corners, the 600s running through: B and T 2025 × (600 + 45),
    // L and R 2025 × (400 − 45). The override mitres the corner (0, 0) with gap 2: there B's and L's centre lines
    // end at the corner point (a mitred end's volume is its centre line's length × the section), each set back 1·√2
    // (the plane moved 1 square to itself, at 45° to the bar). Their far ends stay butts: B runs 22.5 past x 600, L
    // stops 22.5 short of y 400. T and R are not touched.
    let (mut s, bottom, left) = named_bar_rectangle("auto", json!({}));
    let plain = blrt(&mut s);
    let (a, r2) = (2025.0, 2f64.sqrt());
    for (k, want) in [a * 645.0, a * 355.0, a * 645.0, a * 355.0].into_iter().enumerate() {
        assert!(rel(plain[k], want) < 1e-9, "{k}: {plain:?}");
    }
    let r = run(&mut s, "frame.multi_trim", json!({"feature": "Rect", "members": [bottom, left], "kind": "mitre", "gap": "2 mm"}));
    assert_eq!(r["name"], json!("Rect"), "{r}");
    let saved = saved_corners(&s);
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(saved[0].lines().collect::<Vec<_>>(), [bottom.as_str(), left.as_str()]);
    assert_eq!((saved[0].kind, saved[0].gap.as_deref()), (solidcraft_doc::frame::CornerType::Mitre, Some("2 mm")));
    assert_eq!(frame_entries(&s), 1);
    let got = blrt(&mut s);
    let want = [a * (600.0 + 22.5 - r2), a * (400.0 - 22.5 - r2), plain[2], plain[3]];
    assert!(got.iter().zip(want).all(|(g, w)| rel(*g, w) < 1e-6), "{got:?} vs {want:?}");
}

#[test]
fn multi_trim_replaces_and_removes() {
    let (mut s, bottom, left) = named_bar_rectangle("auto", json!({}));
    let plain = blrt(&mut s);
    run(&mut s, "frame.multi_trim", json!({"feature": "Rect", "members": [bottom, left], "kind": "mitre", "gap": 2}));
    // The same members the other way round: the override is replaced, in the order given.
    run(&mut s, "frame.multi_trim", json!({"feature": "Rect", "members": [left, bottom], "kind": "butt"}));
    let saved = saved_corners(&s);
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(saved[0].lines().collect::<Vec<_>>(), [left.as_str(), bottom.as_str()]);
    assert_eq!((saved[0].kind, saved[0].gap.clone()), (solidcraft_doc::frame::CornerType::Butt, None));
    // `remove` with anything to set is refused, and changes nothing.
    for extra in [json!({"kind": "auto"}), json!({"flip": false}), json!({"gap": 1})] {
        let mut p = json!({"feature": "Rect", "members": [bottom, left], "remove": true});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let e = s.execute("frame.multi_trim", &p).unwrap_err().to_string();
        assert!(e.contains("`remove`"), "{p}: {e}");
        assert_eq!(saved_corners(&s), saved);
    }
    // Removed by its members in any order: the corner is Auto's again.
    run(&mut s, "frame.multi_trim", json!({"feature": "Rect", "members": [bottom, left], "remove": true}));
    assert!(saved_corners(&s).is_empty());
    let got = blrt(&mut s);
    assert!(got.iter().zip(plain).all(|(g, p)| rel(*g, p) < 1e-9), "{got:?} vs {plain:?}");
    let e = s.execute("frame.multi_trim", &json!({"feature": "Rect", "members": [bottom, left], "remove": true})).unwrap_err().to_string();
    assert!(e.contains("no override"), "{e}");
    assert_eq!(frame_entries(&s), 1);
}

#[test]
fn multi_trim_flips_from_todays_order() {
    // Today's rules run the longer l1 through; Flip runs l2 through, saved as the order l2, l1.
    let mut s = Session::default();
    l_corner(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "butt", "name": "Corner"}));
    run(&mut s, "frame.multi_trim", json!({"feature": "Corner", "members": ["l1", "l2"], "kind": "butt", "flip": true}));
    assert_eq!(saved_corners(&s)[0].lines().collect::<Vec<_>>(), ["l2", "l1"]);
    run(&mut s, "frame.multi_trim", json!({"feature": "Corner", "members": ["l2", "l1"], "kind": "butt", "flip": false}));
    assert_eq!(saved_corners(&s)[0].lines().collect::<Vec<_>>(), ["l1", "l2"]);
    // The frame's own name on a member is its own.
    let members = json!([{"feature": "Corner", "line": "l2"}, {"line": "l1"}]);
    run(&mut s, "frame.multi_trim", json!({"feature": "Corner", "members": members, "kind": "butt"}));
    let saved = saved_corners(&s);
    assert_eq!(saved[0].lines().collect::<Vec<_>>(), ["l2", "l1"]);
    assert!(saved[0].members.iter().all(|m| m.feature.is_none()), "{saved:?}");
}

#[test]
fn multi_trim_refuses() {
    let (mut s, bottom, left) = named_bar_rectangle("auto", json!({}));
    let top = line_at_y(&s, "Frame", 400.0);
    let many: Vec<String> = (0..1001).map(|k| format!("x{k}")).collect();
    let trim = |more: Value| {
        let mut p = json!({"feature": "Rect", "members": [bottom, left]});
        p.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
        p
    };
    for (p, why) in [
        (json!({"feature": "Nowhere", "members": [bottom, left]}), "Nowhere"),
        (json!({"feature": "Frame", "members": [bottom, left]}), "is not a Frame Member"),
        (trim(json!({"members": [bottom, top]})), "not at a corner of the frame"),
        (trim(json!({"members": [bottom]})), "`members`"),
        (trim(json!({"members": many})), "`members`"),
        (trim(json!({"members": [bottom, 3]})), "`members`"),
        (trim(json!({"members": [bottom, bottom]})), "listed twice"),
        (trim(json!({"members": [{"feature": "Other", "line": bottom}, left]})), "not supported yet: a corner with a member of another frame"),
        (trim(json!({"kind": "weld"})), "`kind` must be auto, none, mitre or butt"),
        (trim(json!({"kind": "coped_butt"})), "`kind` must be auto, none, mitre or butt"),
        (trim(json!({"flip": "yes"})), "`flip` must be true or false"),
        (trim(json!({"remove": 1})), "`remove` must be true or false"),
        (trim(json!({"gap": 1001})), "0…1000 mm"),
        (trim(json!({"gap": -1})), "0…1000 mm"),
        (trim(json!({"gap": "nope"})), "`gap`"),
    ] {
        let e = s.execute("frame.multi_trim", &p).unwrap_err().to_string();
        assert!(e.contains(why), "{p}: {e}");
        assert!(saved_corners(&s).is_empty(), "{p}");
    }
    // A gap on an overlap: refused as evaluation refuses it, the design left as it was.
    let mut s = rail_and_brace(0.0, "auto", json!({"name": "RB"}));
    let before = bodies(&mut s);
    let e = s.execute("frame.multi_trim", &json!({"feature": "RB", "members": ["pair1", "pair2"], "gap": 2})).unwrap_err().to_string();
    assert!(e.contains("gap") && e.contains("overlap"), "{e}");
    assert!(saved_corners(&s).is_empty());
    assert_eq!(bodies(&mut s).len(), before.len());
}

#[test]
fn multi_trim_undoes() {
    // Review Focus 4: undo puts the overrides back exactly as they were.
    let (mut s, bottom, left) = named_bar_rectangle("auto", json!({}));
    let top = line_at_y(&s, "Frame", 400.0);
    let right = line_at_x(&s, "Frame", 600.0);
    run(&mut s, "frame.multi_trim", json!({"feature": "Rect", "members": [top, right], "kind": "butt"}));
    let (saved, volumes) = (saved_corners(&s), blrt(&mut s));
    for p in [
        json!({"feature": "Rect", "members": [bottom, left], "kind": "mitre", "gap": 2}),
        json!({"feature": "Rect", "members": [right, top], "kind": "mitre"}),
        json!({"feature": "Rect", "members": [top, right], "remove": true}),
    ] {
        run(&mut s, "frame.multi_trim", p.clone());
        assert_ne!(saved_corners(&s), saved, "{p}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(saved_corners(&s), saved, "{p}");
        let got = blrt(&mut s);
        assert!(got.iter().zip(volumes).all(|(g, v)| rel(*g, v) < 1e-9), "{p}: {got:?} vs {volumes:?}");
        assert_eq!(frame_entries(&s), 1);
    }
}

#[test]
fn the_gap_follows_its_parameter() {
    // Review Focus 3. A mitred end set back by gap g loses g/√2 of length (half of g square to the 45° plane): the
    // frame's gap g everywhere, and 2g at the corner (0, 0) set with `frame.multi_trim`.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "g", "expression": "2 mm"}));
    rectangle_frame(&mut s);
    let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    let (top, right) = (line_at_y(&s, "Frame", 400.0), line_at_x(&s, "Frame", 600.0));
    let names = json!({bottom.clone(): "B", left.clone(): "L", top: "T", right: "R"});
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": bar45(), "corner": "mitre", "gap": "g", "names": names, "name": "Rect"}));
    run(&mut s, "frame.multi_trim", json!({"feature": "Rect", "members": [bottom, left], "kind": "mitre", "gap": "2 * g"}));
    let r2 = 2f64.sqrt();
    for g in [2.0, 4.0] {
        run(&mut s, "parameters.change", json!({"name": "g", "expression": format!("{g} mm")}));
        let got = blrt(&mut s);
        for (k, (l, ends)) in [(600.0, 3.0 * g), (400.0, 3.0 * g), (600.0, 2.0 * g), (400.0, 2.0 * g)].into_iter().enumerate() {
            assert!(rel(got[k], 2025.0 * (l - ends / r2)) < 1e-6, "g {g}, {k}: {got:?}");
        }
    }
}

#[test]
fn a_stale_override_can_be_removed() {
    // Review Focus 5: a sketch edit takes away a line an override names; the frame fails as before, never panics,
    // and `remove` takes the stale override away.
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "U"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [300, 0]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [0, 200]], "ids": ["b"]}));
    run(&mut s, "sketch.line", json!({"points": [[300, 0], [300, 200]], "ids": ["c"]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "U", "profile": tube(), "corner": "butt", "name": "U frame"}));
    run(&mut s, "frame.multi_trim", json!({"feature": "U frame", "members": ["b", "a"], "kind": "butt"}));
    delete_line(&mut s, "U", "b");
    let fid = first_frame(&s);
    let e = s.model.result(fid).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("not at a corner of the frame"), "{e}");
    run(&mut s, "frame.multi_trim", json!({"feature": "U frame", "members": ["a", "b"], "remove": true}));
    assert!(saved_corners(&s).is_empty());
    assert!(s.model.result(fid).and_then(|r| r.error.clone()).is_none());
    assert_eq!(bodies(&mut s).len(), 2);
}

// Corner entries through the other commands (#17854).

#[test]
fn flips_of_several_corners_are_ordered_in_one_go() {
    // A Butt rectangle runs its 600s through; Flip at two corners runs the 400s through there instead.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (bottom, left) = (line_at_y(&s, "Frame", 0.0), line_at_x(&s, "Frame", 0.0));
    let (top, right) = (line_at_y(&s, "Frame", 400.0), line_at_x(&s, "Frame", 600.0));
    let corners = json!([
        {"members": [bottom, left], "kind": "butt", "flip": true},
        {"members": [top, right], "kind": "butt"},
        {"members": [right, bottom], "kind": "butt", "flip": true},
    ]);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": "butt", "corners": corners}));
    let orders: Vec<Vec<String>> = saved_corners(&s).iter().map(|c| c.lines().map(str::to_string).collect()).collect();
    assert_eq!(orders, [vec![left.clone(), bottom.clone()], vec![top, right.clone()], vec![right, bottom]]);
}

#[test]
fn timeline_edit_saves_flip_as_an_order() {
    let mut s = Session::default();
    l_corner(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "butt", "name": "Corner"}));
    let set = json!({"corners": [{"members": ["l1", "l2"], "kind": "butt", "flip": true}]});
    run(&mut s, "timeline.edit", json!({"feature": "Corner", "set": set}));
    let saved = saved_corners(&s);
    assert_eq!(saved[0].lines().collect::<Vec<_>>(), ["l2", "l1"]);
    assert!(!serde_json::to_string(&saved).unwrap().contains("flip"));
    let set = json!({"corners": [{"members": ["l2", "l1"], "kind": "butt", "flip": false}]});
    run(&mut s, "timeline.edit", json!({"feature": "Corner", "set": set}));
    assert_eq!(saved_corners(&s)[0].lines().collect::<Vec<_>>(), ["l1", "l2"]);
    let set = json!({"corners": [{"members": ["l1", "l2"], "kind": "butt", "flip": 1}]});
    let e = s.execute("timeline.edit", &json!({"feature": "Corner", "set": set})).unwrap_err().to_string();
    assert!(e.contains("`flip` must be true or false"), "{e}");
}

#[test]
fn a_corner_member_named_by_its_frame_is_its_own() {
    let mut s = Session::default();
    l_corner(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "L", "profile": tube(), "corner": "butt", "name": "Corner"}));
    // timeline.edit: the frame's own name is dropped, another frame's refused.
    let own = json!({"corners": [{"members": [{"feature": "Corner", "line": "l2"}, "l1"], "kind": "butt"}]});
    run(&mut s, "timeline.edit", json!({"feature": "Corner", "set": own}));
    let saved = saved_corners(&s);
    assert_eq!(saved[0].lines().collect::<Vec<_>>(), ["l2", "l1"]);
    assert!(saved[0].members.iter().all(|m| m.feature.is_none()), "{saved:?}");
    let other = json!({"corners": [{"members": [{"feature": "Other", "line": "l2"}, "l1"], "kind": "butt"}]});
    let e = s.execute("timeline.edit", &json!({"feature": "Corner", "set": other})).unwrap_err().to_string();
    assert!(e.contains("not supported yet: a corner with a member of another frame"), "{e}");
    assert_eq!(saved_corners(&s), saved);
    // timeline.redefine: the old name stripped.
    let corners = json!([{"members": ["l1", {"feature": "Corner", "line": "l2"}], "kind": "butt"}]);
    let params = json!({"sketch": "L", "profile": tube(), "corner": "butt", "corners": corners});
    run(&mut s, "timeline.redefine", json!({"feature": "Corner", "command": "frame.member", "params": params}));
    let saved = saved_corners(&s);
    assert_eq!(saved[0].lines().collect::<Vec<_>>(), ["l1", "l2"]);
    assert!(saved[0].members.iter().all(|m| m.feature.is_none()), "{saved:?}");
    // A design file naming another frame's member fails the feature with the same words.
    let fid = first_frame(&s);
    let json_doc = s.doc.to_json();
    let edited = json_doc.replacen(r#""line": "l2""#, r#""feature": "Other", "line": "l2""#, 1);
    assert_ne!(edited, json_doc);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&edited).unwrap());
    let e = reopened.model.result(fid).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(e.contains("not supported yet: a corner with a member of another frame"), "{e}");
}

#[test]
fn multi_trim_refuses_a_crossing_piece_as_frame_member_does() {
    // An override names lines and pairs, never a piece of a line cut where tubes cross (`d2>d1` under Auto, and under
    // Butt, which cuts crossings too): refused with `frame.member`'s words, the design unchanged.
    for corner in ["auto", "butt"] {
        let mut s = Session::default();
        x_brace_frame(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "corner": corner, "name": "X"}));
        let before = bodies(&mut s);
        assert!(before.len() >= 6, "{corner}: {before:?}");
        let e = s.execute("frame.multi_trim", &json!({"feature": "X", "members": ["d2>d1", "d1"], "kind": "auto"})).unwrap_err().to_string();
        assert!(e.contains("`d2>d1` is a piece of a tube cut where tubes cross"), "{corner}: {e}");
        assert!(saved_corners(&s).is_empty(), "{corner}");
        assert_eq!(bodies(&mut s).len(), before.len(), "{corner}");
    }
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

#[test]
fn an_end_cap_goes_on_a_butted_through_end() {
    for corner in ["butt", "auto"] {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": corner}));
        let (bottom, _) = bottom_and_left(&mut s);
        // The bottom member runs through: its start is cut square at x = -20, not at the line's end.
        let start = face_pick(&s, &bottom.0, [-20.0, -18.5, 0.0], [-1.0, 0.0, 0.0]);
        run(&mut s, "frame.end_cap", json!({"faces": [start], "thickness": 6}));
        let cap = measured(&mut s, &format!("{} End Cap", bottom.0));
        assert!((cap.3[0] + 20.0).abs() < 1e-6 && (cap.2[0] + 26.0).abs() < 1e-6, "{corner}: {cap:?}");
    }
}

#[test]
fn an_inward_end_cap_measures_a_member_as_cut() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "butt"}));
    let (bottom, left) = bottom_and_left(&mut s);
    // The 600 member is built 640 long (20 past each end): 620 fits, though its line is 600.
    let start = face_pick(&s, &bottom.0, [-20.0, -18.5, 0.0], [-1.0, 0.0, 0.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [start], "thickness": 620, "direction": "inward"}));
    run(&mut s, "edit.undo", json!({}));
    // 640 does not fit.
    let e = s.execute("frame.end_cap", &json!({"faces": [start], "thickness": 640, "direction": "inward"})).unwrap_err().to_string();
    assert!(e.contains("needs a longer member"), "{e}");
    // The 400 member's end is butted against the bottom member: refused at the face, whatever
    // the thickness.
    let end = face_pick(&s, &left.0, [-18.5, 380.0, 0.0], [0.0, 1.0, 0.0]);
    let e = s.execute("frame.end_cap", &json!({"faces": [end], "thickness": 6, "direction": "inward"})).unwrap_err().to_string();
    assert!(e.contains("not an open end"), "{e}");
}

#[test]
fn an_end_cap_on_a_butted_or_coped_end_is_refused() {
    // Auto's U is fitted, flat on T's near face at y = 380 in square tube.
    for (corner, at) in [("butt", 380.0), ("coped_butt", 20.0), ("auto", 380.0)] {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": corner}));
        let (_, left) = bottom_and_left(&mut s);
        // U's butted end faces up at y = 380; its cope face, flat on T's near face, down at y = 20.
        let normal = if at > 200.0 { [0.0, 1.0, 0.0] } else { [0.0, -1.0, 0.0] };
        let face = face_pick(&s, &left.0, [-18.5, at, 0.0], normal);
        for direction in ["outward", "inward"] {
            let e = s.execute("frame.end_cap", &json!({"faces": [face], "thickness": 6, "direction": direction})).unwrap_err().to_string();
            assert!(e.contains("not an open end"), "{corner} {direction}: {e}");
        }
    }
}

#[test]
fn an_end_cap_on_a_mitred_end_is_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": square_tube(), "corner": "mitre"}));
    let (bottom, _) = bottom_and_left(&mut s);
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let start = face_pick(&s, &bottom.0, [-19.0, -19.0, 0.0], [-h, h, 0.0]);
    let e = s.execute("frame.end_cap", &json!({"faces": [start], "thickness": 6})).unwrap_err().to_string();
    assert!(e.contains("not square"), "{e}");
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

/// A post in the component `Part`, made while `Part` is active, and its side picked through
/// the occurrence.
fn post_in_a_component() -> (Session, Value) {
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
    (s, face)
}

#[test]
fn rotate_member_refuses_a_member_in_another_occurrence() {
    let (mut s, face) = post_in_a_component();
    let before = s.doc.to_json();
    let e = s.execute("frame.rotate_member", &json!({"face": face, "angle": "30 deg"})).unwrap_err().to_string();
    assert!(e.contains("not supported yet: rotating a member inside a component"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

/// With the top level active, a member inside a component is refused as not supported, not with
/// "Activate the member's component" (activating it can't help); the member names the refusal
/// pins on the Frame Member go back with it.
#[test]
fn rotate_member_refuses_a_component_member_with_the_top_level_active() {
    let (mut s, face) = post_in_a_component();
    run(&mut s, "component.activate", json!({"component": "root"}));
    let before = s.doc.to_json();
    let e = s.execute("frame.rotate_member", &json!({"face": face, "angle": "30 deg"})).unwrap_err().to_string();
    assert!(e.contains("not supported yet: rotating a member inside a component"), "{e}");
    assert_eq!(s.doc.to_json(), before);
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

fn ref_line(s: &mut Session, plane: &str, points: Value) {
    run(s, "sketch.create", json!({"plane": plane, "name": "Ref"}));
    run(s, "sketch.line", json!({"points": points, "ids": ["ref"]}));
    run(s, "sketch.finish", json!({}));
}

/// The brace turned `before` degrees (when given), then its side picked at that turn laid
/// parallel to a line in the XY plane at 45 degrees: the picked side's family (it and its
/// opposite) is perpendicular to the line afterwards.
fn parallel_to_after(before: Option<&str>) {
    let mut s = brace();
    ref_line(&mut s, "XY", json!([[200, -50], [250, 0]]));
    let start: f64 = before.map_or(0.0, |b| b.trim_end_matches(" deg").parse().unwrap());
    if let Some(angle) = before {
        let face = brace_side(&s, 0.0);
        run(&mut s, "frame.rotate_member", json!({"face": face, "angle": angle}));
    }
    let face = brace_side(&s, start);
    run(&mut s, "frame.rotate_member", json!({"face": face, "parallel_to": {"sketch": "Ref", "curve": "ref"}}));
    let step = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMemberTurn { .. })).unwrap();
    let solidcraft_doc::FeatureKind::FrameMemberTurn { angle, .. } = &step.kind else { panic!("not a turn") };
    let deg: f64 = angle.as_ref().trim_end_matches(" deg").parse().unwrap();
    // The nearest turn: within a quarter turn of the previous one.
    assert!(deg - start > -90.0 - 1e-9 && deg - start <= 90.0 + 1e-9, "{angle}");
    // The picked side now faces `deg` round (the turn is absolute); its normal is perpendicular to the line.
    let picked = brace_side(&s, deg);
    let i = picked["index"].as_u64().unwrap() as usize;
    let st = s.model.state();
    let n = st.body("Brace").unwrap().body.planar_face(i).unwrap().normal();
    assert!((n.x + n.y).abs() < 1e-9, "{n:?} after {before:?}");
}

#[test]
fn parallel_to_lines_a_sloped_member_side_up_with_a_line() {
    parallel_to_after(None);
    parallel_to_after(Some("30 deg"));
}

/// A member turned more than a full turn: Parallel To finds the nearest turn to the one it shows,
/// within a quarter turn of its remainder.
#[test]
fn parallel_to_works_from_the_remainder_of_a_full_turn() {
    let mut s = brace();
    ref_line(&mut s, "XY", json!([[200, -50], [250, 0]]));
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "390 deg"}));
    let face = brace_side(&s, 30.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "parallel_to": {"sketch": "Ref", "curve": "ref"}}));
    let step = s.doc.features.iter().rev().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameMemberTurn { .. })).unwrap();
    let solidcraft_doc::FeatureKind::FrameMemberTurn { angle, .. } = &step.kind else { panic!("not a turn") };
    let deg: f64 = angle.as_ref().trim_end_matches(" deg").parse().unwrap();
    assert!(deg - 30.0 > -90.0 - 1e-9 && deg - 30.0 <= 90.0 + 1e-9, "{angle}");
}

#[test]
fn parallel_to_null_falls_back_to_the_angle() {
    let mut s = brace();
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg", "parallel_to": null}));
    let solidcraft_doc::FeatureKind::FrameMemberTurn { angle, .. } = &turn_step(&s).kind else { panic!("not a turn") };
    assert_eq!(angle.as_ref() as &str, "30 deg");
}

#[test]
fn rotate_member_refuses_a_member_outside_the_active_component() {
    let mut s = brace();
    let face = brace_side(&s, 0.0);
    run(&mut s, "component.create", json!({"name": "Other"}));
    let before = s.doc.to_json();
    let e = s.execute("frame.rotate_member", &json!({"face": face, "angle": "30 deg"})).unwrap_err().to_string();
    assert!(e.contains("Activate the member's component"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

/// An edit of a Rotate step can't move it to another member (the dialog opens with the member
/// fixed).
#[test]
fn a_rotate_step_keeps_its_member_through_an_edit() {
    let mut s = brace();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Post path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[-100, 0, 0], [-100, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "frame.member", json!({"sketch": "Post path", "name": "Post member", "body_name": "Post", "profile": square_tube()}));
    let face = brace_side(&s, 0.0);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "30 deg"}));
    let other = face_pick(&s, "Post", [-80.0, 0.0, 150.0], [1.0, 0.0, 0.0]);
    let before = s.doc.to_json();
    let e = s
        .execute("timeline.redefine", &json!({"feature": "Rotate1", "command": "frame.rotate_member", "params": {"face": other, "angle": "30 deg"}}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("A Rotate step keeps its member"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    // `timeline.edit` setting the member's fields straight is refused the same way.
    let post = s.doc.features.iter().find(|f| f.name == "Post member").unwrap().id;
    let e = s.execute("timeline.edit", &json!({"feature": "Rotate1", "set": {"member.feature": post}})).unwrap_err().to_string();
    assert!(e.contains("A Rotate step keeps its member"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

/// The current turn Parallel To adds its delta to is reduced by whole turns first, as the member
/// is built: a huge turn keeps its delta instead of landing half a turn off.
#[test]
fn parallel_to_reduces_a_huge_current_turn_as_the_member_is_built() {
    use crate::cmd::frame_rotate_member::half_turns;
    use std::f64::consts::TAU;
    for r in [0.0, -PI / 6.0, 190f64.to_radians(), 1e300, -1e300, 1e300f64.to_radians()] {
        let h = half_turns(r);
        assert!((-PI..PI).contains(&h), "{r}: {h}");
        let off = (h - r.rem_euclid(TAU)).rem_euclid(TAU);
        assert!(off < 1e-9 || TAU - off < 1e-9, "{r}: {h} is {off} off the built turn");
    }
    assert!((half_turns(-PI / 6.0) + PI / 6.0).abs() < 1e-12, "-30° stays -30°");
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

/// Hostile Rotate Member input is refused, never a panic; a huge finite angle builds the same
/// member as its remainder (a full turn changes nothing).
#[test]
fn rotate_member_hostile_params_never_panic() {
    let mut s = brace();
    let face = brace_side(&s, 0.0);
    let before = s.doc.to_json();
    let mut path_face = face.clone();
    path_face["path"] = json!([1, 2, 3]);
    let mut far_face = face.clone();
    far_face["index"] = json!(999999);
    for p in [
        json!({"face": face, "angle": "nan deg"}),
        json!({"face": far_face, "angle": "30 deg"}),
        json!({"face": path_face, "angle": "30 deg"}),
        json!({"face": face, "parallel_to": {"edge": [1e308, 0, 0]}}),
        json!({"face": face, "angle": "30 deg", "bogus": 1}),
        json!({"face": face}),
    ] {
        assert!(s.execute("frame.rotate_member", &p).is_err(), "{p}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
    // 1e300 deg is finite: accepted, and the member still builds, as the same turn's remainder.
    let huge = brace_turned("1e300 deg");
    let remainder = (1e300_f64.to_radians().rem_euclid(std::f64::consts::TAU)).to_degrees();
    assert!(same_box(huge, brace_turned(&format!("{remainder} deg"))), "{huge:?}");
}

// ---------------------------------------------------------------------------------------------
// Groups: several groups of lines of one profile, each placed and named its own way

/// The rectangle frame's rails (bottom and top) and posts (left and right), by line id.
fn rails_and_posts(s: &Session) -> (Vec<String>, Vec<String>) {
    let rails = vec![line_at_y(s, "Frame", 0.0), line_at_y(s, "Frame", 400.0)];
    let posts = vec![line_at_x(s, "Frame", 0.0), line_at_x(s, "Frame", 600.0)];
    (rails, posts)
}

fn grouped(rails: &[String], posts: &[String]) -> Value {
    json!({"sketch": "Frame", "profile": {"kind": "angle", "width": 60, "height": 40, "thickness": 5}, "groups": [
        {"lines": rails, "anchor": "bottom_left", "body_name": "Rail"},
        {"lines": posts, "anchor": "bottom_left", "angle": "90 deg", "body_name": "Post"},
    ]})
}

#[test]
fn groups_place_and_name_their_members_their_own_way() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    run(&mut s, "frame.member", grouped(&rails, &posts));
    let bs = boxes(&mut s);
    assert_eq!(bs.keys().collect::<Vec<_>>(), ["Post", "Post (2)", "Rail", "Rail (2)"], "{bs:?}");
    // Group placement is a command rule: rails keep their 40 mm leg up, while posts turn their
    // 60 mm leg up. Command cases belong here rather than in the retired kernel-case suite.
    let size = |name: &str| {
        let (lo, hi) = bs[name];
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    };
    for (name, want) in
        [("Rail", [600.0, 60.0, 40.0]), ("Rail (2)", [600.0, 60.0, 40.0]), ("Post", [40.0, 400.0, 60.0]), ("Post (2)", [40.0, 400.0, 60.0])]
    {
        assert!(near3(size(name), want), "{name}: {:?} {bs:?}", size(name));
    }
    assert!(bs.values().all(|(lo, _)| lo[2].abs() < 1e-6), "the bottom-left anchor keeps every heel on z = 0: {bs:?}");
    // The 60x40x5 angle has area 60*5 + 35*5, plus its r=5 root round and minus two r=2.5 toes.
    let area = 475.0 + 12.5 * (1.0 - PI / 4.0);
    let volume = total_volume(&mut s);
    assert!(rel(volume, area * 2000.0) < MEASURED, "{volume} vs {}", area * 2000.0);
}

#[test]
fn rotate_member_uses_its_groups_placement_and_preserves_other_members() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Posts path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    for x in [0, 100, 200] {
        run(&mut s, "sketch.line3d", json!({"points": [[x, 0, 0], [x, 0, 300]]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let sketch = s.doc.find_feature("Posts path").unwrap().id;
    let lines: Vec<String> = s.model.state().sketch(sketch).unwrap().sketch.wires.iter().map(|w| w.id.clone()).collect();
    assert_eq!(lines.len(), 3);
    run(
        &mut s,
        "frame.member",
        json!({
            "sketch": "Posts path", "name": "Posts",
            "profile": {"kind": "rect_tube", "width": 60, "height": 40, "wall": 2, "outer_radius": 0},
            "groups": [
                {"lines": [&lines[0]], "body_name": "Flat"},
                {"lines": [&lines[1], &lines[2]], "anchor": "bottom_left", "angle": "90 deg", "mirror": true, "body_name": "Turned"}
            ]
        }),
    );
    let before = boxes(&mut s);
    let side = face_pick(&s, "Turned", [60.0, 30.0, 150.0], [-1.0, 0.0, 0.0]);
    run(&mut s, "frame.rotate_member", json!({"face": side, "angle": "90 deg"}));
    let check = |s: &mut Session| {
        let after = boxes(s);
        for name in ["Flat", "Turned (2)"] {
            assert!(same_box(after[name], before[name]), "{name} must stay put: {after:?}");
        }
        // The group's corner anchor puts its centre at (80, 30). The extra quarter
        // turn exchanges its 40 x 60 bounds for 60 x 40 about that same centre.
        assert!(same_box(after["Turned"], ([50.0, 10.0, 0.0], [110.0, 50.0, 300.0])), "{after:?}");
        let solidcraft_doc::FeatureKind::FrameMemberTurn { member, .. } = &turn_step(s).kind else { panic!("not a turn") };
        let pivot = solidcraft_doc::end_cap::member_pivot(&s.doc, &s.model.state(), member).unwrap();
        assert!(near3([pivot.x, pivot.y, pivot.z], [80.0, 30.0, 150.0]), "{pivot:?}");
    };
    check(&mut s);
    // End caps use member_section as well: the cap must follow the turned group,
    // rather than either the flat group's section or the original corner anchor.
    let top = face_pick(&s, "Turned", [109.0, 30.0, 300.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 4}));
    let check_cap = |s: &mut Session| {
        let cap = measured(s, "Turned End Cap");
        assert!(same_box((cap.2, cap.3), ([50.0, 10.0, 300.0], [110.0, 50.0, 304.0])), "{cap:?}");
    };
    check_cap(&mut s);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut reopened);
    check_cap(&mut reopened);
}

#[test]
fn end_caps_use_each_member_groups_section_placement() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Posts path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    for x in [0, 100] {
        run(&mut s, "sketch.line3d", json!({"points": [[x, 0, 0], [x, 0, 300]]}));
    }
    run(&mut s, "sketch.finish", json!({}));
    let sketch = s.doc.find_feature("Posts path").unwrap().id;
    let lines: Vec<String> = s.model.state().sketch(sketch).unwrap().sketch.wires.iter().map(|w| w.id.clone()).collect();
    assert_eq!(lines.len(), 2);
    run(
        &mut s,
        "frame.member",
        json!({
            "sketch": "Posts path", "profile": {"kind": "rect_tube", "width": 60, "height": 40, "wall": 2, "outer_radius": 0},
            "groups": [
                {"lines": [&lines[0]], "body_name": "Flat"},
                {"lines": [&lines[1]], "anchor": "bottom_left", "angle": "90 deg", "mirror": true, "body_name": "Turned"}
            ]
        }),
    );
    let flat = face_pick(&s, "Flat", [29.0, 0.0, 300.0], [0.0, 0.0, 1.0]);
    let turned = face_pick(&s, "Turned", [61.0, 30.0, 300.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [flat, turned], "thickness": 4}));
    let check = |s: &mut Session| {
        for (name, lo, hi) in
            [("Flat End Cap", [-30.0, -20.0, 300.0], [30.0, 20.0, 304.0]), ("Turned End Cap", [60.0, 0.0, 300.0], [100.0, 60.0, 304.0])]
        {
            let cap = measured(s, name);
            assert!(near3(cap.2, lo) && near3(cap.3, hi), "{cap:?}");
            assert!(rel(cap.1, 60.0 * 40.0 * 4.0) < 1e-9, "{cap:?}");
        }
    };
    check(&mut s);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut reopened);
}

/// Each group keeps its own located profile point (`locate`), also after saving and reopening.
#[test]
fn each_group_locates_its_members_by_its_own_profile_point() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Rails"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 500], [1000, 500]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({
            "sketch": "Rails", "profile": {"kind": "angle", "width": 60, "height": 40, "thickness": 5},
            "groups": [{"lines": ["l1"], "locate": [0, 0], "body_name": "Heel"}, {"lines": ["l2"], "locate": [10, 5], "body_name": "Offset"}]
        }),
    );
    let check = |s: &mut Session| {
        let bs = boxes(s);
        // [0, 0] puts the angle's heel on its line; [10, 5] moves the profile back 10 across, 5 down.
        assert!(near3(bs["Heel"].0, [0.0, 0.0, 0.0]) && near3(bs["Heel"].1, [1000.0, 60.0, 40.0]), "{bs:?}");
        assert!(near3(bs["Offset"].0, [0.0, 490.0, -5.0]) && near3(bs["Offset"].1, [1000.0, 550.0, 35.0]), "{bs:?}");
    };
    check(&mut s);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut reopened);
}

#[test]
fn one_group_is_saved_as_the_flat_form() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, _) = rails_and_posts(&s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "groups": [{"lines": rails, "mirror": true, "body_name": "Rail"}]}));
    let saved = serde_json::to_value(&s.doc.features.last().unwrap().kind).unwrap();
    assert!(saved.get("groups").is_none(), "{saved}");
    assert_eq!((&saved["lines"], &saved["mirror"], &saved["body_name"]), (&json!(rails), &json!(true), &json!("Rail")));
    assert_eq!(boxes(&mut s).keys().collect::<Vec<_>>(), ["Rail", "Rail (2)"]);
}

#[test]
fn bad_groups_are_refused() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    let with = |groups: Value| json!({"sketch": "Frame", "profile": tube(), "groups": groups});
    for (p, why) in [
        (with(json!([])), "`groups`"),
        (with(json!("all")), "`groups`"),
        (with(json!([1, 2])), "`groups`"),
        (with(json!([{"lines": rails}, {}])), "group 2: `lines`"),
        (with(json!([{"lines": rails}, {"lines": []}])), "group 2: `lines`"),
        (with(json!([{"lines": rails}, {"lines": [&posts[0], &posts[0]]}])), "listed twice"),
        (with(json!([{"lines": rails}, {"lines": [&rails[0]]}])), "in two groups"),
        (with(json!([{"lines": rails}, {"lines": posts, "anchor": "middle"}])), "group 2: `anchor`"),
        (with(json!([{"lines": rails}, {"lines": posts, "angle": "a lot"}])), "angle"),
        (with(json!([{"lines": rails}, {"lines": posts, "mirror": "yes"}])), "group 2: `mirror`"),
        (with(json!([{"lines": rails}, {"lines": posts, "body_name": ""}])), "group 2: `body_name`"),
        (with(json!([{"lines": rails}, {"lines": ["nope"]}])), "nope"),
        (json!({"sketch": "Frame", "profile": tube(), "lines": rails, "groups": [{"lines": posts}]}), "not beside"),
        (json!({"sketch": "Frame", "profile": tube(), "mirror": true, "groups": [{"lines": posts}]}), "not beside"),
        (json!({"sketch": "Frame", "profile": tube(), "locate": [0, 0], "groups": [{"lines": posts}]}), "not beside"),
        (with(json!([{"lines": rails}, {"lines": posts, "locate": [1]}])), "group 2: `locate`"),
        (with(json!([{"lines": rails}, {"lines": posts, "locate": [0, 1e9]}])), "group 2: `locate`"),
    ] {
        let err = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
    }
    assert!(bodies(&mut s).is_empty(), "nothing was made");
}

#[test]
fn groups_refuse_flat_continuous_even_without_parameter_checks() {
    for level in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Warn, crate::schema::ParamCheck::Strict] {
        let mut s = Session { param_check: level, ..Session::default() };
        rectangle_frame(&mut s);
        let (rails, posts) = rails_and_posts(&s);
        let before = s.doc.to_json();
        for continuous in [json!(true), json!(false), json!(0), json!("yes")] {
            let mut p = grouped(&rails, &posts);
            p["continuous"] = continuous;
            let err = s.execute("frame.member", &p).unwrap_err().to_string();
            let placement_error = "give `continuous` in each of `groups`, not beside them";
            assert!(err.contains(placement_error), "{p}: {err}");
            assert!(crate::schema::issues("frame.member", &p).iter().any(|i| i.message == placement_error));
            assert_eq!(s.doc.to_json(), before, "a refused call changes nothing");
        }
        // Continuous is a recognized flat key. With groups, only null counts as absent,
        // consistently in the handler and the contract at every parameter-check level.
        let mut p = grouped(&rails, &posts);
        p["continuous"] = Value::Null;
        run(&mut s, "frame.member", p);
    }
}

#[test]
fn groups_refuse_unsupported_finishing_and_pairs_without_changing_the_design() {
    for level in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Warn, crate::schema::ParamCheck::Strict] {
        let mut s = Session { param_check: level, ..Session::default() };
        rectangle_frame(&mut s);
        let (rails, posts) = rails_and_posts(&s);
        let sketch = s.doc.find_feature("Frame").unwrap().id;
        let points: Vec<String> = s.doc.sketch(sketch).unwrap().points.iter().map(|p| p.id.clone()).collect();
        let pair = json!({"a": {"sketch": sketch, "point": points[0]}, "b": {"sketch": sketch, "point": points[1]}});
        let before = s.doc.to_json();
        let mirrored = |p: &mut Value| p["groups"][1]["mirror"] = json!(true);
        for (extra, mirror, want) in [
            // Finishing without a corner type finishes nothing.
            (json!({"flip": true}), false, "only with a corner type"),
            (json!({"fit_ends": true}), false, "only with a corner type"),
            (json!({"coped": true}), false, "only with a corner type"),
            (json!({"gap": "2 mm"}), false, "only with a corner type"),
            (json!({"pairs": [pair]}), false, "not supported yet: frame member groups with point pairs"),
            // Groups finished together share one section.
            (json!({"corner": "mitre"}), true, "not supported yet: corners between mirrored and unmirrored"),
        ] {
            let mut p = grouped(&rails, &posts);
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            if mirror {
                mirrored(&mut p);
            }
            let err = s.execute("frame.member", &p).unwrap_err().to_string();
            assert!(err.contains(want), "{p}: {err}");
            assert_eq!(s.doc.to_json(), before, "refusing an unsupported combination is atomic");
        }
    }
}

/// Groups with a corner type are one frame: its corners are finished across the groups, and a
/// corner override between a rail and a post sets that one joint's type, order and gap. A square
/// tube looks the same turned a quarter, so the posts' group turn changes nothing: the grouped
/// frame is the flat frame, member for member, named by its groups.
#[test]
fn groups_with_a_corner_type_finish_their_corners_together() {
    let build = |grouped_lines: bool, extra: Value| {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        let (rails, posts) = rails_and_posts(&s);
        let mut p = if grouped_lines {
            json!({"sketch": "Frame", "profile": tube(), "groups": [
                {"lines": rails, "body_name": "Rail"},
                {"lines": posts, "angle": "90 deg", "body_name": "Post"},
            ]})
        } else {
            json!({"sketch": "Frame", "profile": tube()})
        };
        let corner = json!([{"members": [&posts[0], &rails[0]], "kind": "butt", "gap": "3 mm"}]);
        let extra = if extra == json!("override") { json!({"corner": "mitre", "corners": corner}) } else { extra };
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run(&mut s, "frame.member", p);
        let b = bodies(&mut s);
        // (volume, box), by where each member lies: bodies come in group order, flat ones in drawing order.
        let mut got: Vec<(f64, [f64; 3], [f64; 3])> = b.iter().map(|x| (x.1, x.2, x.3)).collect();
        got.sort_by(|a, c| (a.1[0] + a.2[0], a.1[1] + a.2[1]).partial_cmp(&(c.1[0] + c.2[0], c.1[1] + c.2[1])).unwrap());
        (got, b.iter().map(|x| x.0.clone()).collect::<Vec<_>>())
    };
    for extra in [json!({"corner": "mitre"}), json!({"corner": "butt", "gap": "2 mm"}), json!("override")] {
        let (flat, _) = build(false, extra.clone());
        let (joined, names) = build(true, extra.clone());
        for (k, (f, j)) in flat.iter().zip(&joined).enumerate() {
            assert!(rel(j.0, f.0) < 1e-9 && near3(j.1, f.1) && near3(j.2, f.2), "{extra}: member {k}: {j:?} vs flat {f:?}");
        }
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(sorted, ["Post", "Post (2)", "Rail", "Rail (2)"], "{extra}");
    }
    // The override butts the first post against the bottom rail with its own 3 mm gap, unlike a mitre.
    let (mitred, _) = build(true, json!({"corner": "mitre"}));
    let (overridden, _) = build(true, json!("override"));
    assert!(overridden.iter().zip(&mitred).any(|(o, m)| rel(o.0, m.0) > 1e-3), "the override changes that corner: {overridden:?} vs {mitred:?}");
    // Each group keeps its own placement in one frame: the angle rails keep their 40 mm leg up, the
    // posts turned a quarter put their 60 mm leg up, and the mitres shorten nobody's section.
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    let mut p = grouped(&rails, &posts);
    p["corner"] = json!("mitre");
    run(&mut s, "frame.member", p);
    let bs = boxes(&mut s);
    let size = |name: &str| {
        let (lo, hi) = bs[name];
        [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]]
    };
    for (name, across) in [("Rail", [60.0, 40.0]), ("Rail (2)", [60.0, 40.0]), ("Post", [40.0, 60.0]), ("Post (2)", [40.0, 60.0])] {
        let got = size(name);
        let (w, h) = if name.starts_with("Rail") { (got[1], got[2]) } else { (got[0], got[2]) };
        assert!((w - across[0]).abs() < 1e-6 && (h - across[1]).abs() < 1e-6, "{name}: {got:?} {bs:?}");
    }
}

#[test]
fn copied_grouped_members_keep_global_names_and_the_surviving_groups_names() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    let id = run(&mut s, "frame.member", grouped(&rails, &posts))["feature"].as_u64().unwrap();
    // Copies name bodies by position. Group boundaries must not restart that position.
    s.doc_mut().feature_mut(id).unwrap().body_names = ["Copy Rail A", "Copy Rail B", "Copy Post A", "Copy Post B"].map(String::from).to_vec();
    s.refresh();
    let before = boxes(&mut s);
    assert_eq!(before.keys().map(String::as_str).collect::<Vec<_>>(), ["Copy Post A", "Copy Post B", "Copy Rail A", "Copy Rail B"]);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(boxes(&mut reopened), before);
    let mut p = grouped(&rails, &posts);
    p["groups"] = json!([p["groups"][1].clone()]);
    run(&mut reopened, "timeline.redefine", json!({"feature": id, "command": "frame.member", "params": p}));
    let after = boxes(&mut reopened);
    assert_eq!(after.keys().map(String::as_str).collect::<Vec<_>>(), ["Copy Post A", "Copy Post B"]);
    for name in ["Copy Post A", "Copy Post B"] {
        assert!(same_box(after[name], before[name]), "{name}");
    }
}

#[test]
fn group_rotations_keep_their_parameter_names_after_reordering_or_removing_an_earlier_group() {
    fn rotations(s: &Session, id: u64) -> Vec<(String, String)> {
        let f = s.doc.feature(id).unwrap();
        f.kind
            .inputs()
            .into_iter()
            .zip(&f.param_names)
            .filter(|((label, _, _), _)| *label == "Rotation")
            .map(|((_, expression, _), name)| (name.clone(), expression))
            .collect()
    }
    fn value(s: &mut Session, name: &str) -> f64 {
        let list = run(s, "parameters.list", json!({}));
        list["parameters"].as_array().unwrap().iter().find(|p| p["name"] == name).and_then(|p| p["value"].as_f64()).unwrap()
    }
    for remove_earlier in [false, true] {
        let mut s = Session::default();
        rectangle_frame(&mut s);
        let (rails, posts) = rails_and_posts(&s);
        let mut p = grouped(&rails, &posts);
        p["groups"][0]["angle"] = json!("15 deg");
        run(&mut s, "frame.member", p.clone());
        let id = s.doc.features.last().unwrap().id;
        let names = rotations(&s, id);
        assert_eq!(names.len(), 2);
        for (i, name) in ["RailRotation", "PostRotation"].iter().enumerate() {
            run(&mut s, "parameters.rename", json!({"name": names[i].0, "new_name": name}));
        }
        run(&mut s, "parameters.add", json!({"name": "FollowPost", "expression": "PostRotation", "unit": "deg"}));
        // Reopen before the edit, too: the identity is derived from saved groups and line ids.
        s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        let groups = p["groups"].as_array_mut().unwrap();
        if remove_earlier {
            groups.remove(0);
        } else {
            groups.reverse();
            // Line order within the same group does not change its identity either.
            groups[0]["lines"].as_array_mut().unwrap().reverse();
        }
        run(&mut s, "timeline.redefine", json!({"feature": id, "command": "frame.member", "params": p}));
        let expected = if remove_earlier {
            vec![("PostRotation".to_string(), "90 deg".to_string())]
        } else {
            vec![("PostRotation".to_string(), "90 deg".to_string()), ("RailRotation".to_string(), "15 deg".to_string())]
        };
        for _ in 0..2 {
            assert_eq!(rotations(&s, id), expected);
            assert_eq!(value(&mut s, "PostRotation"), 90.0);
            assert_eq!(value(&mut s, "FollowPost"), 90.0, "expressions still read the surviving group's angle");
            if !remove_earlier {
                assert_eq!(value(&mut s, "RailRotation"), 15.0);
            }
            s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        }
        run(&mut s, "parameters.change", json!({"name": "PostRotation", "expression": "75 deg"}));
        assert_eq!(rotations(&s, id)[0], ("PostRotation".to_string(), "75 deg".to_string()));
        assert_eq!(value(&mut s, "FollowPost"), 75.0);
    }
}

#[test]
fn a_flat_member_keeps_its_rotation_name_when_its_line_selection_changes() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "lines": rails, "angle": "15 deg"}));
    let f = s.doc.features.last().unwrap();
    let id = f.id;
    let index = f.kind.inputs().iter().position(|(label, _, _)| *label == "Rotation").unwrap();
    let name = f.param_names[index].clone();
    run(&mut s, "parameters.rename", json!({"name": name, "new_name": "RailRotation"}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": id, "command": "frame.member", "params": {
            "sketch": "Frame", "profile": tube(), "lines": posts, "angle": "30 deg"
        }}),
    );
    let f = s.doc.feature(id).unwrap();
    let inputs = f.kind.inputs();
    let index = inputs.iter().position(|(label, _, _)| *label == "Rotation").unwrap();
    assert_eq!(f.param_names[index], "RailRotation");
    assert_eq!(inputs[index].1, "30 deg");
}

#[test]
fn a_flat_member_keeps_its_named_rotation_and_references_when_switching_path_sketches() {
    fn check(s: &mut Session, id: u64, expression: &str, value: f64) {
        let f = s.doc.feature(id).unwrap();
        let inputs = f.kind.inputs();
        let index = inputs.iter().position(|(label, _, _)| *label == "Rotation").unwrap();
        assert_eq!(f.param_names[index], "MemberRotation");
        assert_eq!(inputs[index].1, expression);
        let list = run(s, "parameters.list", json!({}));
        for name in ["MemberRotation", "FollowRotation"] {
            let row = list["parameters"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap();
            assert_eq!(row["value"].as_f64(), Some(value), "{name}: {row}");
        }
    }
    let mut s = Session::default();
    rectangle_frame(&mut s);
    // Both paths precede the member in the timeline, so the new path is a valid dependency.
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Other frame"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [1000, 0], "p1": [1600, 400]}));
    run(&mut s, "sketch.finish", json!({}));
    let other_path = s.doc.find_feature("Other frame").unwrap().id;
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube(), "angle": "15 deg"}));
    let f = s.doc.features.last().unwrap();
    let id = f.id;
    let index = f.kind.inputs().iter().position(|(label, _, _)| *label == "Rotation").unwrap();
    let name = f.param_names[index].clone();
    run(&mut s, "parameters.rename", json!({"name": name, "new_name": "MemberRotation"}));
    run(&mut s, "parameters.add", json!({"name": "FollowRotation", "expression": "MemberRotation", "unit": "deg"}));
    check(&mut s, id, "15 deg", 15.0);
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut s, id, "15 deg", 15.0);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": id, "command": "frame.member", "params": {
            "sketch": "Other frame", "profile": tube(), "angle": "30 deg"
        }}),
    );
    assert!(matches!(&s.doc.feature(id).unwrap().kind,
        solidcraft_doc::FeatureKind::FrameMember { path_sketch, groups, .. }
            if path_sketch.as_ref().is_some_and(|sketch| sketch.0 == other_path) && groups.is_empty()));
    check(&mut s, id, "30 deg", 30.0);
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut s, id, "30 deg", 30.0);
    run(&mut s, "parameters.change", json!({"name": "MemberRotation", "expression": "75 deg"}));
    check(&mut s, id, "75 deg", 75.0);
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut s, id, "75 deg", 75.0);
}

/// A design's groups survive saving and reopening; an edit keeps every member's name but those of
/// a group given another name, and a member renamed by hand keeps its new name.
#[test]
fn groups_survive_reopening_and_edits_keep_their_names() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    run(&mut s, "frame.member", grouped(&rails, &posts));
    let fid = s.doc.features.last().unwrap().id;
    let before = boxes(&mut s);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(boxes(&mut reopened), before);
    run(&mut s, "body.rename", json!({"body": "Post (2)", "name": "Right post"}));
    // Turn the posts back: same names, the rails untouched.
    let mut p = grouped(&rails, &posts);
    p["groups"][1]["angle"] = json!("0 deg");
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": p.clone()}));
    let after = boxes(&mut s);
    assert_eq!(after.keys().collect::<Vec<_>>(), ["Post", "Rail", "Rail (2)", "Right post"], "{after:?}");
    assert_eq!((after["Rail"], after["Rail (2)"]), (before["Rail"], before["Rail (2)"]));
    assert_ne!(after["Post"], before["Post"], "the post turned");
    // The rails' group renamed: its members take the new name, the posts keep theirs.
    p["groups"][0]["body_name"] = json!("Beam");
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": p}));
    assert_eq!(boxes(&mut s).keys().collect::<Vec<_>>(), ["Beam", "Beam (2)", "Post", "Right post"]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(boxes(&mut s).keys().collect::<Vec<_>>(), ["Post", "Rail", "Rail (2)", "Right post"]);
}

/// A flat feature turned into groups keeps its members' names (no group renamed them).
#[test]
fn a_flat_feature_split_into_groups_keeps_its_names() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    run(&mut s, "frame.member", json!({"sketch": "Frame", "profile": tube()}));
    let fid = s.doc.features.last().unwrap().id;
    let before = boxes(&mut s);
    let p = json!({"sketch": "Frame", "profile": tube(), "groups": [{"lines": rails}, {"lines": posts, "anchor": "bottom"}]});
    run(&mut s, "timeline.redefine", json!({"feature": fid, "command": "frame.member", "params": p}));
    let after = boxes(&mut s);
    assert_eq!(after.keys().collect::<Vec<_>>(), before.keys().collect::<Vec<_>>());
    for line in &rails {
        let name = &s.doc.feature(fid).map(|f| f.kind.clone()).and_then(|k| match k {
            solidcraft_doc::FeatureKind::FrameMember { names, .. } => names.get(line).cloned(),
            _ => None,
        });
        let name = name.as_ref().unwrap();
        assert_eq!(after[name], before[name], "the rail {name} stayed put");
    }
}

// ---------------------------------------------------------------------------------------------
// Trim/Extend (frame.trim)

/// A 45 × 45 bar: its volume is 2025 mm² × its length.
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

/// A trimmed member's end takes an end cap as any end does, and the cap reads the member's
/// trimmed length: an inward cap longer than the trimmed member is refused, though the member
/// as first made was long enough for it.
#[test]
fn an_end_cap_closes_a_trimmed_members_end() {
    let mut s = capped_post(square_tube());
    run(&mut s, "frame.trim", json!({"members": ["Post"], "to": {"plane": {"origin": [0, 0, 250], "normal": [0, 0, 1]}}, "end": "end"}));
    let top = face_pick(&s, "Post", [18.5, 0.0, 250.0], [0.0, 0.0, 1.0]);
    let before = s.doc.to_json();
    let e = s.execute("frame.end_cap", &json!({"faces": [top.clone()], "thickness": 260, "direction": "inward"})).unwrap_err().to_string();
    assert!(e.contains("needs a longer member"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let cap = measured(&mut s, "Post End Cap");
    assert!(rel(cap.1, 40.0 * 40.0 * 6.0) < 1e-9, "{cap:?}");
    assert!(near3(cap.2, [-20.0, -20.0, 250.0]) && near3(cap.3, [20.0, 20.0, 256.0]), "{cap:?}");
}

/// The 40 x 20 section starts at (0, 0), so its centre is (20, 10), off the path.
/// A 90-degree turn keeps that centre: bounds (10, -10)..(30, 30). Trimming at z=180
/// leaves 40*20*180 material; the cap starts there, while the Rotate pivot stays at z=150.
#[test]
fn an_off_centre_rectangular_member_rotates_then_trims_and_caps_after_reopen() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Post path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Post path", "name": "Post member", "body_name": "Post", "anchor": "bottom_left",
            "profile": {"kind": "bar", "width": 40, "height": 20}}),
    );
    let side = face_pick(&s, "Post", [40.0, 10.0, 150.0], [1.0, 0.0, 0.0]);
    run(&mut s, "frame.rotate_member", json!({"face": side, "angle": "90 deg"}));
    run(&mut s, "frame.trim", json!({"members": ["Post"], "to": {"plane": {"origin": [0, 0, 180], "normal": [0, 0, 1]}}, "end": "end"}));
    let top = face_pick(&s, "Post", [20.0, 10.0, 180.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for mut session in [s, reopened] {
        let post = measured(&mut session, "Post");
        assert!(rel(post.1, 40.0 * 20.0 * 180.0) < 1e-9, "{post:?}");
        assert!(near3(post.2, [10.0, -10.0, 0.0]) && near3(post.3, [30.0, 30.0, 180.0]), "{post:?}");
        assert!(near3(post.4, [20.0, 10.0, 90.0]), "{post:?}");
        let cap = measured(&mut session, "Post End Cap");
        assert!(rel(cap.1, 40.0 * 20.0 * 6.0) < 1e-9, "{cap:?}");
        assert!(near3(cap.2, [10.0, -10.0, 180.0]) && near3(cap.3, [30.0, 30.0, 186.0]), "{cap:?}");
        let state = session.model.state();
        let record = state.frame_members.iter().find(|r| r.body == "Post").unwrap();
        assert!(record.ends[0].is_none());
        let end = record.ends[1].as_ref().unwrap();
        assert!((end.origin.z - 180.0).abs() < 1e-6 && end.normal().z > 1.0 - 1e-9, "{end:?}");
        let solidcraft_doc::FeatureKind::FrameMemberTurn { member, .. } = &turn_step(&session).kind else { panic!("not a turn") };
        let pivot = solidcraft_doc::end_cap::member_pivot(&session.doc, &state, member).unwrap();
        assert!(near3([pivot.x, pivot.y, pivot.z], [20.0, 10.0, 150.0]), "{pivot:?}");
    }
}

/// A cornered frame must not be reconstructed as unjointed stock by a later Trim.
#[test]
fn general_trim_refuses_cornered_members_without_losing_their_cuts() {
    for corner in ["auto", "mitre", "butt"] {
        let (mut s, made) = polyline_frame(json!([[0, 0], [300, 0], [300, 300]]), corner);
        made.unwrap();
        let members = bodies(&mut s);
        assert_eq!(members.len(), 2, "{corner}: {members:?}");
        let before = s.doc.to_json();
        let err = s.execute("frame.trim", &json!({"members": [members[0].0], "to": {"plane": "XY"}})).unwrap_err().to_string();
        assert!(err.contains("not supported yet: Trim/Extend") && err.contains("existing cuts must be kept"), "{corner}: {err}");
        assert_eq!(s.doc.to_json(), before, "{corner}: refused trim changed the design");
    }
}

/// The pair runs vertically at x=600. Its centred 40 x 20 bar ends at z=250 after Trim.
#[test]
fn general_trim_keeps_a_pair_only_members_section_after_reopen() {
    let mut s = Session::default();
    box_frame(&mut s);
    let made = run(
        &mut s,
        "frame.member",
        json!({"profile": {"kind": "bar", "width": 40, "height": 20},
        "pairs": [upright()], "anchor": "center", "body_name": "Pair"}),
    );
    let solidcraft_doc::FeatureKind::FrameMember { path_sketch, .. } = member_kind(&s, &made["feature"]) else { panic!("not a frame member") };
    assert!(path_sketch.is_none());
    run(&mut s, "frame.trim", json!({"members": ["Pair"], "to": {"plane": {"origin": [0, 0, 250], "normal": [0, 0, 1]}}, "end": "end"}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for mut session in [s, reopened] {
        let bar = measured(&mut session, "Pair");
        assert!(rel(bar.1, 40.0 * 20.0 * 250.0) < 1e-9, "{bar:?}");
        assert!(near3(bar.2, [580.0, -10.0, 0.0]) && near3(bar.3, [620.0, 10.0, 250.0]), "{bar:?}");
    }
}

/// Moving the occurrence that places a trim's target face trims the member again: the incremental
/// model gives what a model built from scratch does.
#[test]
fn moving_a_trim_targets_occurrence_trims_as_from_scratch() {
    let mut s = Session::default();
    let bar = members_3d(&mut s, "Bar", &[([100.0, 0.0, 0.0], [300.0, 0.0, 0.0])]);
    run(&mut s, "solid.box", json!({"length": 10, "width": 100, "height": 100, "corner": [0, -50, -50], "body_name": "Plate"}));
    let holder = run(&mut s, "component.create", json!({"name": "Holder", "grounded": false}))["component"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": "Holder:1", "translate": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": 0}));
    std::sync::Arc::make_mut(&mut s.doc).body_components.insert("Plate".into(), holder);
    s.refresh();
    // The plate's face at x = 10 in its component, x = 30 where its occurrence places it.
    let face = face_pick(&s, "Plate", [10.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    let face = json!({"body": "Plate", "index": face["index"], "face": [30.0, 0.0, 0.0]});
    run(&mut s, "frame.trim", json!({"members": bar, "to": {"plane": face}}));
    let trimmed = measured(&mut s, "Bar");
    assert!((trimmed.2[0] - 30.0).abs() < 1e-6, "{trimmed:?}");
    run(&mut s, "occurrence.move", json!({"occurrence": "Holder:1", "translate": [50, 0, 0]}));
    let warm = bodies(&mut s);
    let mut cold = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let cold = bodies(&mut cold);
    assert_eq!(warm.len(), cold.len(), "{warm:?} vs {cold:?}");
    for (w, c) in warm.iter().zip(&cold) {
        assert!(w.0 == c.0 && rel(w.1, c.1) < 1e-9 && near3(w.2, c.2) && near3(w.3, c.3), "{w:?} vs {c:?}");
    }
    assert!((measured(&mut s, "Bar").2[0] - trimmed.2[0]).abs() > 1.0, "the trim followed the move");
}

/// A trim pasted with the members and the face it trims to targets their copies: the pasted bar
/// ends on the pasted plate's face, not on the face of the plate it was copied from.
#[test]
fn a_pasted_trim_trims_to_the_pasted_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 100, "height": 100, "corner": [0, -50, -50], "body_name": "Plate", "name": "Plate box"}));
    let bar = members_3d(&mut s, "Bar", &[([100.0, 0.0, 0.0], [300.0, 0.0, 0.0])]);
    let face = face_pick(&s, "Plate", [10.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    run(&mut s, "frame.trim", json!({"members": bar, "to": {"plane": face}, "name": "Butt"}));
    assert!((measured(&mut s, "Bar").2[0] - 10.0).abs() < 1e-6);
    // The member feature made by `members_3d` is Member1.
    run(&mut s, "timeline.copy", json!({"features": ["Plate box", "Bar", "Member1", "Butt"]}));
    let pasted = run(&mut s, "timeline.paste", json!({"translate": [200, 500, 0]}));
    assert!(pasted["errors"].as_array().unwrap().is_empty(), "{pasted}");
    let all = bodies(&mut s);
    let copy =
        all.iter().find(|b| b.0.starts_with("Bar") && b.2[1] > 400.0).cloned().unwrap_or_else(|| panic!("no pasted bar: {all:?} after {pasted}"));
    // The pasted plate's face is at x = 210: the pasted bar, 300 to 500, is extended back to it.
    assert!(near3(copy.2, [210.0, 477.5, -22.5]) && near3(copy.3, [500.0, 522.5, 22.5]), "{copy:?}");
    assert!((measured(&mut s, "Bar").2[0] - 10.0).abs() < 1e-6, "the original keeps its trim");
    // A moved trim to a face the paste leaves behind is refused, as a moved sketch on one is.
    run(&mut s, "timeline.copy", json!({"features": ["Bar", "Member1", "Butt"]}));
    let refused = s.execute("timeline.paste", &json!({"translate": [0, 900, 0]})).unwrap_err().to_string();
    assert!(refused.contains("external exact face attachment"), "{refused}");
}

#[test]
fn trimming_a_turned_group_keeps_its_own_section_and_the_other_group_after_reopen() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Posts"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]}));
    run(&mut s, "sketch.line3d", json!({"points": [[100, 0, 0], [100, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Posts", "profile": {"kind": "bar", "width": 40, "height": 20},
        "groups": [{"lines": ["w1"], "anchor": "center", "body_name": "A"},
                   {"lines": ["w2"], "anchor": "bottom_left", "angle": "90 deg", "body_name": "B"}]}),
    );
    let other = measured(&mut s, "A");
    run(&mut s, "frame.trim", json!({"members": ["B"], "to": {"plane": {"origin": [0, 0, 180], "normal": [0, 0, 1]}}, "end": "end"}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    for mut session in [s, reopened] {
        let b = measured(&mut session, "B");
        // A 90-degree turn of a bottom-left anchored 40x20 section at x=100
        // occupies x=80..100 and y=0..40. Its trimmed length is 180.
        assert!(rel(b.1, 40.0 * 20.0 * 180.0) < 1e-9, "{b:?}");
        assert!(near3(b.2, [80.0, 0.0, 0.0]) && near3(b.3, [100.0, 40.0, 180.0]), "{b:?}");
        let a = measured(&mut session, "A");
        assert!(rel(a.1, other.1) < 1e-9 && near3(a.2, other.2) && near3(a.3, other.3), "{a:?} vs {other:?}");
    }
}

#[test]
fn a_trimmed_member_keeps_its_gusset_identity_after_cached_reopen() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"Post path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d":true}));
    let drawn = run(&mut s, "sketch.line3d", json!({"points":[[0,0,0],[0,0,300]]}));
    let line = drawn["created"]["wires"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let feature =
        run(&mut s, "frame.member", json!({"sketch":"Post path","profile":{"kind":"bar","width":40,"height":40},"body_name":"Post"}))["feature"]
            .as_u64()
            .unwrap();
    run(&mut s, "solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"}));
    run(&mut s, "frame.trim", json!({"members":["Post"],"to":{"plane":{"origin":[0,0,200],"normal":[0,0,1]}},"end":"end"}));
    let saved = s.model.saved(&s.doc).unwrap();
    let mut cached = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    cached.model = solidcraft_doc::Model::adopt(&cached.doc, &saved).unwrap();
    for mut session in [s, cached] {
        let before = measured(&mut session, "Post");
        assert!(rel(before.1, 40.0 * 40.0 * 200.0) < 1e-9, "{before:?}");
        let params = gusset_args(&session);
        let id = run(&mut session, "frame.gusset", params)["feature"].as_u64().unwrap();
        let solidcraft_doc::FeatureKind::FrameGusset { supports, .. } = &session.doc.feature(id).unwrap().kind else { panic!("not a gusset") };
        let member = supports[0].member.as_ref().unwrap();
        assert_eq!(member.feature.0, feature);
        assert_eq!(member.line, line);
        assert!(rel(gusset_volume(&session), 80.0 * 100.0 / 2.0 * 6.0) < 1e-7);
        let after = measured(&mut session, "Post");
        assert!(rel(after.1, before.1) < 1e-9 && near3(after.2, before.2) && near3(after.3, before.3), "{after:?} vs {before:?}");
    }
}

/// One 100 mm radius quarter bend between a 200 mm and a 300 mm straight.
fn chassis_path(s: &mut Session) -> Vec<String> {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Chassis"}));
    let mut ids = Vec::new();
    for (cmd, p) in [
        ("sketch.line", json!({"points": [[100, -200], [100, 0]]})),
        ("sketch.arc.three_point", json!({"start": [100, 0], "through": [100.0 / 2f64.sqrt(), 100.0 / 2f64.sqrt()], "end": [0, 100]})),
        // Deliberately drawn backwards: selection order, rather than drawing direction,
        // decides how the segments connect.
        ("sketch.line", json!({"points": [[-300, 100], [0, 100]]})),
    ] {
        ids.extend(run(s, cmd, p)["created"]["curves"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()));
    }
    run(s, "sketch.finish", json!({}));
    ids
}

#[test]
fn a_continuous_straight_member_rotates_about_its_profile_centre_and_keeps_its_cap() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Post path"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 0, 0], [0, 0, 300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Post path", "lines": ["w1"], "continuous": true, "name": "Post member", "body_name": "Post",
            "profile": square_tube(), "anchor": "bottom_left"}),
    );
    let flat = measured(&mut s, "Post");
    let c = flat.4;
    let face = face_pick(&s, "Post", [c[0] + 20.0, c[1], c[2]], [1.0, 0.0, 0.0]);
    run(&mut s, "frame.rotate_member", json!({"face": face, "angle": "45 deg"}));
    let solidcraft_doc::FeatureKind::FrameMemberTurn { member, .. } = &turn_step(&s).kind else { panic!("missing turn") };
    assert_eq!(member.line, "$path");
    let turned = measured(&mut s, "Post");
    assert!(near3(turned.4, c), "the off-axis profile centre must stay put: {turned:?} vs {flat:?}");
    assert!(rel(turned.3[0] - turned.2[0], 40.0 * 2f64.sqrt()) < 1e-9, "{turned:?}");
    assert!(rel(turned.1, flat.1) < 1e-9, "{turned:?} vs {flat:?}");
    let top = face_pick(&s, "Post", [c[0] + 18.5 / 2f64.sqrt(), c[1] + 18.5 / 2f64.sqrt(), 300.0], [0.0, 0.0, 1.0]);
    run(&mut s, "frame.end_cap", json!({"faces": [top], "thickness": 6}));
    let cap = measured(&mut s, "Post End Cap");
    assert!(rel(cap.1, 40.0 * 40.0 * 6.0) < 1e-9, "{cap:?}");
    assert!(rel(cap.3[0] - cap.2[0], 40.0 * 2f64.sqrt()) < 1e-9, "the cap must use the turned section: {cap:?}");
    assert!(near3(cap.4, [c[0], c[1], 303.0]), "{cap:?}");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let again = measured(&mut reopened, "Post End Cap");
    assert!(same_box((again.2, again.3), (cap.2, cap.3)) && near3(again.4, cap.4), "{again:?} vs {cap:?}");
}

#[test]
fn rotate_member_refuses_a_curved_path_without_changing_it() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Chassis", "lines": ids, "continuous": true,
            "profile": {"kind": "bar", "width": 20, "height": 10}, "body_name": "Hoop"}),
    );
    let side = face_pick(&s, "Hoop", [110.0, -100.0, 0.0], [1.0, 0.0, 0.0]);
    let before = s.doc.to_json();
    let e = s.execute("frame.rotate_member", &json!({"face": side, "angle": "45 deg"})).unwrap_err().to_string();
    assert!(e.contains("rotation on curved frame members"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn explicit_arcs_make_round_and_rectangular_members_without_changing_all_lines() {
    for profile in [
        json!({"kind": "round_tube", "diameter": 20, "wall": 2}),
        json!({"kind": "rect_tube", "width": 20, "height": 10, "wall": 2, "outer_radius": 0}),
    ] {
        let mut s = Session::default();
        let ids = chassis_path(&mut s);
        run(&mut s, "frame.member", json!({"sketch": "Chassis", "lines": [ids[1]], "profile": profile}));
        assert_eq!(bodies(&mut s).len(), 1);
        run(&mut s, "edit.undo", json!({}));
        run(&mut s, "frame.member", json!({"sketch": "Chassis", "profile": profile}));
        assert_eq!(bodies(&mut s).len(), 2, "all lines still selects only straight lines");
    }
}

#[test]
fn continuous_member_keeps_one_identity_through_reversal_edit_reopen_and_undo() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    let profile = json!({"kind": "round_tube", "diameter": 20, "wall": 2});
    let params = json!({"sketch": "Chassis", "lines": ids, "continuous": true, "profile": profile, "body_name": "Hoop"});
    let fid = run(&mut s, "frame.member", params)["feature"].clone();
    let bs = bodies(&mut s);
    assert_eq!(bs.len(), 1);
    assert_eq!(bs[0].0, "Hoop");
    run(&mut s, "edit.undo", json!({}));
    assert!(bodies(&mut s).is_empty());
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "body.rename", json!({"body": "Hoop", "name": "Main hoop"}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": fid, "command": "frame.member", "params": {
            "sketch": "Chassis", "lines": ids, "continuous": true,
            "profile": {"kind": "round_tube", "diameter": 20, "wall": 3}, "body_name": "Hoop"
        }}),
    );
    let bs = bodies(&mut s);
    assert_eq!(bs[0].0, "Main hoop");
    let saved = s.model.saved(&s.doc).unwrap();
    let member = saved.records["model_bodies"].as_array().unwrap().iter().find(|b| b["name"] == "Main hoop").unwrap();
    assert_eq!(member["frame_member_line"].as_str(), Some("$path"));
    let kind = &s.doc.feature(fid.as_u64().unwrap()).unwrap().kind;
    assert_eq!(solidcraft_doc::frame::member_line_ids(&s.model.state(), kind).unwrap(), ["$path"]);
    let solidcraft_doc::FeatureKind::FrameMember { names, .. } = kind else { panic!("not a member") };
    assert_eq!(names.get("$path").map(String::as_str), Some("Main hoop"));
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(bodies(&mut reopened).into_iter().map(|b| b.0).collect::<Vec<_>>(), ["Main hoop"]);
    run(&mut s, "edit.undo", json!({}));
    // Reverse the traversal, including the bend, without changing the member identity.
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": fid, "command": "frame.member", "params": {
            "sketch": "Chassis", "lines": ids.iter().rev().collect::<Vec<_>>(), "continuous": true, "profile": profile, "body_name": "Hoop"
        }}),
    );
    assert_eq!(bodies(&mut s)[0].0, "Main hoop");
    // An explicit change of the base name still renames the physical path member.
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": fid, "command": "frame.member", "params": {
            "sketch": "Chassis", "lines": ids, "continuous": true, "profile": profile, "body_name": "Rear hoop"
        }}),
    );
    assert_eq!(bodies(&mut s)[0].0, "Rear hoop");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(bodies(&mut s)[0].0, "Main hoop");
}

#[test]
fn invalid_continuous_path_selection_is_atomic() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    let original = s.doc.to_json();
    for (extra, why) in [
        (json!({"continuous": true}), "explicit"),
        (json!({"continuous": 1, "lines": ids}), "continuous"),
        (json!({"continuous": true, "lines": [ids[0], ids[2]]}), "pick the curves in order along the path"),
        (json!({"continuous": true, "lines": [ids[0], ids[2], ids[1]]}), "pick the curves in order along the path"),
        (json!({"continuous": true, "lines": [ids[0], ids[1], ids[1]]}), "twice"),
    ] {
        let mut p = json!({"sketch": "Chassis", "profile": {"kind": "round_tube", "diameter": 20, "wall": 2}});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let err = s.execute("frame.member", &p).unwrap_err().to_string();
        assert!(err.contains(why), "{p}: {err}");
        assert_eq!(s.doc.to_json(), original);
        assert!(bodies(&mut s).is_empty());
    }
    rectangle_frame(&mut s);
    let err = s.execute("frame.member", &json!({"sketch": "Frame", "lines": ["l1", "l2"], "continuous": true, "profile": tube()})).unwrap_err();
    assert!(err.to_string().contains("tangent"), "{err}");
}

#[test]
fn curved_member_follows_radius_parameters_and_a_different_component_frame() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "R", "expression": "100 mm"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Bend"}));
    let arc = run(
        &mut s,
        "sketch.arc.three_point",
        json!({
            "start": ["R", 0], "through": ["R * 0.7071067811865476", "R * 0.7071067811865476"], "end": [0, "R"]
        }),
    )["created"]["curves"][0]
        .clone();
    run(&mut s, "sketch.finish", json!({}));
    let c = run(&mut s, "component.create", json!({"name": "Bent tube", "grounded": false}))["component"].as_u64().unwrap();
    let o = s.doc.occurrence_of(c).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence": o, "translate": [0, 0, 100], "capture": true}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Bend", "lines": [arc], "continuous": true,
        "profile": {"kind": "round_tube", "diameter": 20, "wall": 2}}),
    );
    let b = bodies(&mut s).remove(0);
    run(&mut s, "parameters.change", json!({"name": "R", "expression": "150 mm"}));
    let after = bodies(&mut s).remove(0);
    assert_eq!(after.0, b.0);
    // A quarter bend of radius R is R*pi/2 long; the 20 x 2 tube's section is pi*(10^2 - 8^2) = 36*pi.
    assert!(rel(b.1, 36.0 * PI * 50.0 * PI) < 1e-6, "{b:?}");
    assert!(rel(after.1, 36.0 * PI * 75.0 * PI) < 1e-6, "{after:?}");
}

fn measured_frames(s: &mut Session) -> serde_json::Map<String, Value> {
    let m = run(s, "inspect.measure", json!({}));
    m["bodies"].as_array().unwrap().iter().map(|b| (b["name"].as_str().unwrap().to_string(), b.clone())).collect()
}

fn v3(v: &Value) -> [f64; 3] {
    [f(&v[0]), f(&v[1]), f(&v[2])]
}

#[test]
fn a_continuous_round_member_traversed_backwards_has_its_closed_form_mass_properties() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    let lines: Vec<&String> = ids.iter().rev().collect();
    let profile = json!({"kind": "round_tube", "diameter": 20, "wall": 2});
    run(&mut s, "frame.member", json!({"sketch": "Chassis", "lines": lines, "continuous": true, "profile": profile, "body_name": "Main hoop"}));
    // r_o = 10, r_i = 8, R = 100: A = pi*(r_o^2 - r_i^2) = 36*pi along L = 200 + R*pi/2 + 300.
    let (area, length) = (36.0 * PI, 500.0 + 50.0 * PI);
    let m = measured_frames(&mut s);
    assert_eq!(m.len(), 1, "the bend stays one physical member: {m:?}");
    let b = &m["Main hoop"];
    assert!(rel(f(&b["volume_mm3"]), area * length) < 1e-6, "{b}");
    assert!(rel(f(&b["area_mm2"]), 2.0 * PI * 18.0 * length + 2.0 * area) < 1e-6, "{b}");
    // First moments of each straight and of the quarter torus, whose section has I/A = (r_o^2 + r_i^2)/4 = 41.
    assert!(near3(v3(&b["center_of_mass"]), [(-15000.0 + 41.0) / length, (20000.0 + 41.0) / length, 0.0]), "{b}");
    assert!(near3(v3(&b["bbox"]["min"]), [-300.0, -200.0, -10.0]) && near3(v3(&b["bbox"]["max"]), [110.0, 110.0, 10.0]), "{b}");
}

#[test]
fn a_continuous_rectangular_member_is_its_section_area_times_the_developed_centreline() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    let profile = json!({"kind": "rect_tube", "width": 20, "height": 10, "wall": 2, "outer_radius": 0});
    run(&mut s, "frame.member", json!({"sketch": "Chassis", "lines": ids, "continuous": true, "profile": profile, "body_name": "Hoop"}));
    // A centred 20 x 10 x 2 section, 200 - 16*6 = 104 mm^2, carried round the bend.
    let m = measured_frames(&mut s);
    assert_eq!(m.len(), 1, "{m:?}");
    assert!(rel(f(&m["Hoop"]["volume_mm3"]), 104.0 * (500.0 + 50.0 * PI)) < 1e-6, "{m:?}");
}

#[test]
fn two_compound_spatial_bends_carry_a_rectangular_section_without_restarting_its_up_direction() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Chassis"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    let h = 100.0 / 2f64.sqrt();
    run(&mut s, "sketch.line3d", json!({"points": [[100, -200, 0], [100, 0, 0]]}));
    run(&mut s, "sketch.arc3d", json!({"start": [100, 0, 0], "through": [h, h, 0], "end": [0, 100, 0]}));
    run(&mut s, "sketch.line3d", json!({"points": [[0, 100, 0], [-200, 100, 0]]}));
    run(&mut s, "sketch.arc3d", json!({"start": [-200, 100, 0], "through": [-200.0 - h, 100, 100.0 - h], "end": [-300, 100, 100]}));
    run(&mut s, "sketch.line3d", json!({"points": [[-300, 100, 100], [-300, 100, 400]]}));
    run(&mut s, "sketch.finish", json!({}));
    let profile = json!({"kind": "bar", "width": 20, "height": 10});
    let lines = ["w1", "w2", "w3", "w4", "w5"];
    run(&mut s, "frame.member", json!({"sketch": "Chassis", "lines": lines, "continuous": true, "profile": profile, "body_name": "Hoop"}));
    // Two R100 quarter bends, in XY and then XZ, with 700 mm of straight stock: V = 20*10*(700 + 100*pi).
    let m = measured_frames(&mut s);
    let b = &m["Hoop"];
    assert!(rel(f(&b["volume_mm3"]), 200.0 * (700.0 + 100.0 * PI)) < 1e-6, "{b}");
    // The 10 mm height is carried to X at the vertical end; restarting world up would swap it with the width.
    assert!(near3(v3(&b["bbox"]["min"]), [-305.0, -200.0, -5.0]) && near3(v3(&b["bbox"]["max"]), [110.0, 110.0, 400.0]), "{b}");
}

#[test]
fn a_tube_wider_than_its_bend_radius_is_refused() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    let before = s.doc.to_json();
    // The tube radius 125 exceeds the centreline bend radius 100: the swept section would cross the bend axis.
    let profile = json!({"kind": "round_tube", "diameter": 250, "wall": 2});
    let p = json!({"sketch": "Chassis", "lines": ids, "continuous": true, "profile": profile, "body_name": "Hoop"});
    let err = s.execute("frame.member", &p).unwrap_err().to_string();
    assert!(err.contains("bend radius is too small"), "{err}");
    assert_eq!(s.doc.to_json(), before);
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_nearly_straight_spline_is_not_accepted_as_straight_tube_stock() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Nearly straight"}));
    run(&mut s, "sketch.options", json!({"sketch_3d": true}));
    // The middle defining point lies 0.00001 mm off the endpoints' line: every display sample is
    // within 0.001 mm of a line, but the curve is not one.
    run(&mut s, "sketch.spline3d", json!({"points": [[0, 0, 0], [100, 1e-5, 0], [200, 0, 0]]}));
    run(&mut s, "sketch.finish", json!({}));
    let before = s.doc.to_json();
    let p = json!({"sketch": "Nearly straight", "lines": ["w1"], "profile": {"kind": "round_tube", "diameter": 40, "wall": 2}});
    let err = s.execute("frame.member", &p).unwrap_err().to_string();
    assert!(err.contains("straight line or an exact circular arc"), "{err}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn continuous_stock_refuses_unsupported_corner_and_fitting_options_atomically() {
    let mut s = Session::default();
    box_frame(&mut s);
    let ids = chassis_path(&mut s);
    let before = s.doc.to_json();
    let before_bodies = bodies(&mut s);
    for extra in [
        json!({"corner": "auto"}),
        json!({"corner": "mitre"}),
        json!({"corner": "butt"}),
        json!({"corners": [{"members": [ids[0], ids[1]], "kind": "none"}]}),
        json!({"fit_ends": true}),
        json!({"coped": true}),
        json!({"flip": true}),
        json!({"gap": "0 mm"}),
        json!({"pairs": [upright()]}),
    ] {
        let mut params = json!({"sketch": "Chassis", "lines": ids, "continuous": true,
            "profile": {"kind": "round_tube", "diameter": 20, "wall": 2}});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let error = s.execute("frame.member", &params).unwrap_err().to_string();
        assert!(error.contains("not supported yet"), "{params}: {error}");
        assert_eq!(s.doc.to_json(), before, "{params}");
        assert_eq!(bodies(&mut s), before_bodies, "{params}");
    }
    run(
        &mut s,
        "frame.member",
        json!({"sketch": "Chassis", "lines": ids, "continuous": true,
        "profile": {"kind": "round_tube", "diameter": 20, "wall": 2}}),
    );
    assert_eq!(member_boxes(&mut s).len(), 1, "plain continuous None stock still builds");
}

#[test]
fn separate_curves_and_point_pairs_keep_none_member_identity_and_analytic_volumes() {
    let mut s = Session::default();
    box_frame(&mut s);
    let curves = chassis_path(&mut s);
    let reply = run(
        &mut s,
        "frame.member",
        json!({"sketch": "Chassis", "lines": [curves[1]], "pairs": [upright()],
        "profile": {"kind": "round_tube", "diameter": 20, "wall": 2}, "body_name": "Stock"}),
    );
    let feature = s.doc.feature(reply["feature"].as_u64().unwrap()).unwrap();
    let state = s.model.state();
    let (ids, later) = solidcraft_doc::frame::member_parts(&state, &feature.kind).unwrap();
    assert_eq!(ids, [curves[1].clone(), "pair1".to_string()]);
    assert_eq!(later, [None, None]);
    for (name, id) in [("Stock", &ids[0]), ("Stock (2)", &ids[1])] {
        assert_eq!(solidcraft_doc::frame::member_of_body(&s.doc, &state, feature, name).unwrap().as_ref(), Some(id));
    }
    let measured = measured_frames(&mut s);
    // A = pi*(10^2 - 8^2) = 36*pi; the R100 quarter arc is 50*pi long, the pair 400.
    assert!(rel(f(&measured["Stock"]["volume_mm3"]), 36.0 * PI * 50.0 * PI) < 1e-6);
    assert!(rel(f(&measured["Stock (2)"]["volume_mm3"]), 36.0 * PI * 400.0) < 1e-6);
}

#[test]
fn frame_member_refusals_keep_their_geometry_data_and_session_failure_code() {
    use solidcraft_kernel::services::frame_member::{self as stock, Failure, Refusal};
    use solidcraft_kernel::services::{Error, FailureCode, session};

    let expected_code = FailureCode { call: "frame_member", kind: FailureCode::OPS, operation: "FrameMember" };
    let origin = solidcore_record::Vec3::ZERO;
    let error = stock::line(origin, origin).unwrap_err();
    assert!(matches!(&error, Failure::Geometry(Refusal::Line)), "{error:?}");
    assert_eq!(error.to_string(), "the line has no length or has invalid coordinates");
    assert_eq!(session::take_failure_code(), Some(expected_code));

    let first = stock::line(origin, [10.0, 0.0, 0.0].into()).unwrap();
    let second = stock::line([20.0, 0.0, 0.0].into(), [30.0, 0.0, 0.0].into()).unwrap();
    assert_eq!(session::take_failure_code(), None, "a successful query clears the previous refusal");
    let mut path = [first, second];
    let before = path;
    let error = stock::orient(&mut path).unwrap_err();
    let Failure::Geometry(reason) = &error else { panic!("geometry refusal was reclassified: {error:?}") };
    assert_eq!(*reason, Refusal::Connection(1));
    assert_eq!(path, before, "a refused traversal leaves its input unchanged");
    let typed = Error::FrameMember(reason.clone());
    assert_eq!(typed.name(), "FrameMember");
    assert_eq!(typed.to_string(), "curve 1 is not connected end to end in path order; pick the curves in order along the path");
    assert_eq!(error.to_string(), typed.to_string(), "the public error and session diagnostic agree");
    assert_eq!(session::take_failure_code(), Some(expected_code), "the code contains no curve index or model data");

    stock::line(origin, [10.0, 0.0, 0.0].into()).unwrap();
    assert_eq!(session::take_failure_code(), None);
}

#[test]
fn removing_a_continuous_tube_uses_its_physical_member_identity_and_undo_restores_it() {
    let mut s = Session::default();
    let ids = chassis_path(&mut s);
    let feature = run(
        &mut s,
        "frame.member",
        json!({"sketch": "Chassis", "lines": ids, "continuous": true,
        "profile": {"kind": "round_tube", "diameter": 20, "wall": 2}, "body_name": "Hoop"}),
    )["feature"]
        .as_u64()
        .unwrap();
    let before = s.doc.to_json();
    remove(&mut s, json!([{"feature": feature, "line": "$path"}]));
    assert!(s.doc.feature(feature).is_none());
    assert!(bodies(&mut s).is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    let frame = s.doc.feature(feature).unwrap();
    let state = s.model.state();
    assert_eq!(solidcraft_doc::frame::member_of_body(&s.doc, &state, frame, "Hoop").unwrap().as_deref(), Some("$path"));
}

#[test]
fn continuous_members_refuse_placement_groups_without_losing_group_settings() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let (rails, posts) = rails_and_posts(&s);
    let params = grouped(&rails, &posts);
    let feature = run(&mut s, "frame.member", params.clone())["feature"].as_u64().unwrap();
    let before = s.doc.to_json();
    let mut unsupported = params;
    unsupported["continuous"] = json!(true);
    let error = s.execute("frame.member", &unsupported).unwrap_err().to_string();
    assert!(error.contains("continuous") && error.contains("groups"), "{error}");
    assert_eq!(s.doc.to_json(), before);
    // Saved documents must be refused too, even without command parameter validation.
    let solidcraft_doc::FeatureKind::FrameMember { continuous, .. } = &mut s.doc_mut().feature_mut(feature).unwrap().kind else {
        panic!("not a member")
    };
    *continuous = true;
    s.refresh();
    let error = s.model.result(feature).unwrap().error.as_ref().unwrap().to_string();
    assert!(error.contains("continuous") && error.contains("groups"), "{error}");
}

/// Make perpendicular finite support faces with room for an 80 × 100 mm plate.
fn gusset_joint() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":300,"name":"Post feature","body_name":"Post"}));
    run(&mut s, "solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"name":"Rail feature","body_name":"Rail"}));
    s
}
/// Find a known definition face by its plane and outward normal for an exact command pick.
fn gusset_pick(s: &Session, body: &str, point: [f64; 3], normal: [f64; 3]) -> Value {
    let st = s.model.state();
    let b = st.body(body).unwrap();
    let at = solidcore_record::Vec3::new(point[0], point[1], point[2]);
    let n = solidcore_record::Vec3::new(normal[0], normal[1], normal[2]);
    let i = (0..b.body.face_count())
        .find(|i| b.body.planar_face(*i).is_some_and(|pl| pl.normal().dot(n) > 0.999 && (pl.origin - at).dot(n).abs() < 1e-6))
        .unwrap();
    json!({"body":body,"index":i,"face":point})
}
/// Pick the post side first and rail top second so the joint direction is predictable.
fn gusset_args(s: &Session) -> Value {
    json!({"supports":[gusset_pick(s,"Post",[20.,0.,100.],[1.,0.,0.]),gusset_pick(s,"Rail",[100.,0.,40.],[0.,0.,1.])],"leg1":80,"leg2":100,"thickness":6,"body_name":"Gusset"})
}
fn gusset_volume(s: &Session) -> f64 {
    s.model.state().body("Gusset").unwrap().mesh().measure().volume
}

#[test]
/// Compare each outline with the area of a triangle less its clipped tips and weld relief.
fn gusset_triangle_and_clipped_relief_have_analytic_volumes() {
    for (r, c) in [(0., 0.), (10., 0.), (0., 10.), (10., 10.)] {
        let mut s = gusset_joint();
        let mut p = gusset_args(&s);
        p["relief"] = json!(r);
        p["clip"] = json!(c);
        run(&mut s, "frame.gusset", p);
        let expected = (80. * 100. / 2. - r * r / 2. - c * c / 2. * 180. / 16400f64.sqrt()) * 6.;
        assert!(rel(gusset_volume(&s), expected) < 1e-7, "{r} {c}: {} vs {expected}", gusset_volume(&s));
        assert_eq!(s.model.state().bodies.len(), 3);
    }
}
#[test]
/// Name an unnamed plate `Gusset`, then `Gusset (2)`, so a cut list of a frame's gussets reads.
fn gusset_without_a_body_name_is_named_gusset() {
    let mut s = gusset_joint();
    for expected in ["Gusset", "Gusset (2)"] {
        let mut p = gusset_args(&s);
        p.as_object_mut().unwrap().remove("body_name");
        let r = run(&mut s, "frame.gusset", p);
        assert_eq!(r["created_bodies"], json!([expected]), "{r}");
    }
}
#[test]
/// Check all three placements along the ordered cross product, with a nonzero joint offset.
fn gusset_thickness_placement_and_offset_have_predictable_bounds() {
    for (placement, lo, hi) in [("center", -8., -2.), ("positive", -11., -5.), ("negative", -5., 1.)] {
        let mut s = gusset_joint();
        let mut p = gusset_args(&s);
        p["placement"] = json!(placement);
        p["offset"] = json!(5);
        run(&mut s, "frame.gusset", p);
        let b = bodies(&mut s).into_iter().find(|b| b.0 == "Gusset").unwrap();
        assert!(near3(b.2, [20., lo, 40.]), "{placement}: {b:?}");
        assert!(near3(b.3, [120., hi, 120.]), "{placement}: {b:?}");
    }
}
#[test]
/// Keep exact face identity through a support edit, rename, undo and save/reopen.
fn gusset_follows_support_edits_and_rename_then_undo_reopen() {
    let mut s = gusset_joint();
    let p = gusset_args(&s);
    run(&mut s, "frame.gusset", p);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.model.state().body("Gusset").is_none());
    run(&mut s, "edit.redo", json!({}));
    assert!((gusset_volume(&s) - 24000.).abs() < 1e-5);
    run(&mut s, "body.rename", json!({"body":"Post","name":"Column"}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Post feature","command":"solid.box","params":{"corner":[-20,-20,0],"length":60,"width":40,"height":300,"body_name":"Column"}}),
    );
    let b = bodies(&mut s).into_iter().find(|b| b.0 == "Gusset").unwrap();
    assert!(near3(b.2, [40., -3., 40.]));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(rel(gusset_volume(&reopened), 24000.) < 1e-7);
}
#[test]
/// A rejected support or dimension leaves no feature or changed member names behind.
fn gusset_refuses_bad_geometry_atomically() {
    let mut s = gusset_joint();
    let good = gusset_args(&s);
    for (key, value) in [
        ("leg1", json!(-1)),
        ("leg2", json!(400)),
        ("thickness", json!(41)),
        ("offset", json!(100)),
        ("relief", json!(80)),
        ("clip", json!(80)),
        ("placement", json!("elsewhere")),
        ("leg1", json!("1e99 mm")),
        ("unsupported", json!(true)),
    ] {
        let mut p = good.clone();
        p[key] = value;
        let before = s.doc.to_json();
        assert!(s.execute("frame.gusset", &p).is_err(), "{p}");
        assert_eq!(s.doc.to_json(), before, "{key}");
    }
    let mut p = good;
    p["supports"] = json!([p["supports"][0], p["supports"][0]]);
    assert!(s.execute("frame.gusset", &p).is_err());
}
#[test]
/// Contact must cover the whole strip: a cut between valid endpoints still breaks support.
fn gusset_contact_detects_a_hole_between_its_endpoints() {
    let mut s = gusset_joint();
    run(&mut s, "solid.hole", json!({"position":[70,0,40],"direction":[0,0,-1],"diameter":10,"depth":40,"body":"Rail"}));
    let p = gusset_args(&s);
    let before = s.doc.to_json();
    let e = s.execute("frame.gusset", &p).unwrap_err().to_string();
    assert!(e.contains("contact"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
/// Follow a resized tube while refusing a support whose path-line identity was replaced.
fn gusset_tracks_frame_member_profile_edits() {
    let mut s = Session::default();
    for (name, points) in [("Post", json!([[0, 0, 0], [0, 0, 300]])), ("Rail", json!([[20, 0, 20], [320, 0, 20]]))] {
        run(&mut s, "sketch.create", json!({"plane":"XY","name":format!("{name} path")}));
        run(&mut s, "sketch.options", json!({"sketch_3d":true}));
        run(&mut s, "sketch.line3d", json!({"points":points}));
        run(&mut s, "sketch.finish", json!({}));
        run(
            &mut s,
            "frame.member",
            json!({"sketch":format!("{name} path"),"name":format!("{name} member"),"body_name":name,"profile":{"kind":"rect_tube","width":40,"height":40,"wall":3,"outer_radius":0}}),
        );
    }
    let p = gusset_args(&s);
    run(&mut s, "frame.gusset", p);
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature":"Post member","command":"frame.member","params":{"sketch":"Post path","body_name":"Post","profile":{"kind":"rect_tube","width":60,"height":40,"wall":3,"outer_radius":0}}}),
    );
    let b = bodies(&mut s).into_iter().find(|b| b.0 == "Gusset").unwrap();
    assert!(near3(b.2, [30., -3., 40.]), "{b:?}");
    assert!(rel(gusset_volume(&s), 24000.) < 1e-7);
    // A saved support identity must not be silently replaced by another sketch line.
    let mut broken = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let gusset = broken.features.iter_mut().find(|f| matches!(f.kind, solidcraft_doc::FeatureKind::FrameGusset { .. })).unwrap();
    if let solidcraft_doc::FeatureKind::FrameGusset { supports, .. } = &mut gusset.kind {
        supports[0].member.as_mut().unwrap().line = "a different line".into();
    }
    let broken = Session::new(broken);
    assert!(broken.model.state().body("Gusset").is_none(), "a lost line must not be adopted by proximity");
}
#[test]
/// The triangular area scales by the sine of the angle between the two support legs.
fn gusset_nonorthogonal_faces_use_the_actual_included_angle() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner":[-40,-20,-100],"length":40,"width":40,"height":400,"body_name":"Post"}));
    run(&mut s, "solid.box", json!({"corner":[-20,-20,-40],"length":320,"width":40,"height":40,"body_name":"Rail"}));
    run(&mut s, "solid.move", json!({"bodies":["Rail"],"axis":[0,1,0],"angle":"30 deg"}));
    let c = 30f64.to_radians().cos();
    let p = json!({"supports":[gusset_pick(&s,"Post",[0.,0.,100.],[1.,0.,0.]),gusset_pick(&s,"Rail",[100.*c,0.,-50.],[0.5,0.,c])],"leg1":80,"leg2":100,"thickness":6,"body_name":"Gusset"});
    run(&mut s, "frame.gusset", p);
    assert!(rel(gusset_volume(&s), 24000. * c) < 1e-7);
}
#[test]
/// Internal anchors move rigidly and remap to the copied part, whose thickness can vary independently.
fn gusset_component_motion_and_independent_copy_keep_attachments() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Part","grounded":false}))["component"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":300,"body_name":"Post"}));
    run(&mut s, "solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"}));
    let p = gusset_args(&s);
    let fid = run(&mut s, "frame.gusset", p)["feature"].as_u64().unwrap();
    let occurrence = s.doc.occurrence_of(part).unwrap().id;
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[50,20,0],"axis":[0,0,1],"angle":"90 deg","capture":true}));
    let world = s.world_state().unwrap();
    let b = world.bodies.iter().find(|b| b.feature == fid).unwrap().mesh().bounds();
    assert!(near3([b.min.x, b.min.y, b.min.z], [47., 40., 40.]), "{b:?}");
    let copied = run(&mut s, "component.paste_new", json!({"component":part,"translate":[500,0,0]}))["component"].as_u64().unwrap();
    let f = s.doc.features.iter().find(|f| f.component == copied && matches!(f.kind, solidcraft_doc::FeatureKind::FrameGusset { .. })).unwrap();
    let parameter = f.param_names[2].clone();
    run(&mut s, "parameters.change", json!({"name":parameter,"expression":"8 mm"}));
    let original = run(&mut s, "component.properties", json!({"component":part}))["total"]["volume_mm3"].as_f64().unwrap();
    let copy = run(&mut s, "component.properties", json!({"component":copied}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((original - 984000.).abs() < 1e-4, "{original}");
    assert!((copy - 992000.).abs() < 1e-4, "{copy}");
}

/// Recorded feature names from timeline.inspect replay with the same support identities.
#[test]
fn gusset_recorded_type_name_replays() {
    let mut source = gusset_joint();
    let p = gusset_args(&source);
    let id = run(&mut source, "frame.gusset", p)["feature"].as_u64().unwrap();
    let inspected = run(&mut source, "timeline.inspect", json!({"feature": id}));
    assert_eq!(inspected["type_name"], "GussetFeature");
    let mut script = run(&mut source, "timeline.export_script", json!({}))["script"].clone();
    let recorded = script["commands"].as_array_mut().unwrap().iter_mut().find(|step| step["params"]["feature"]["id"] == id).unwrap();
    recorded["params"]["feature"]["type"] = inspected["type_name"].clone();

    let mut replay = Session::default();
    replay.run_script(&script).unwrap();
    assert_eq!(replay.doc.feature(id).unwrap().kind, source.doc.feature(id).unwrap().kind);
    assert!(replay.model.result(id).unwrap().error.is_none());
    assert!(rel(gusset_volume(&replay), 24000.) < 1e-7);
}

/// A root gusset follows both independent live occurrences, and cannot survive a lost joint.
#[test]
fn gusset_rebuilds_when_either_live_support_moves() {
    let mut s = Session::default();
    let post = run(&mut s, "component.create", json!({"name":"Upright","grounded":false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":300,"body_name":"Post"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    let rail = run(&mut s, "component.create", json!({"name":"Cross member","grounded":false}))["occurrence"].as_u64().unwrap();
    run(&mut s, "solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    let p = gusset_args(&s);
    let fid = run(&mut s, "frame.gusset", p)["feature"].as_u64().unwrap();
    let origin = |s: &Session| {
        let min = s.model.state().body("Gusset").unwrap().mesh().bounds().min;
        [min.x, min.y, min.z]
    };
    assert!(near3(origin(&s), [20., -3., 40.]));
    run(&mut s, "occurrence.move", json!({"occurrence":rail,"translate":[0,0,5],"capture":true}));
    assert!(near3(origin(&s), [20., -3., 45.]));
    run(&mut s, "occurrence.move", json!({"occurrence":post,"translate":[10,0,0],"capture":true}));
    assert!(near3(origin(&s), [30., -3., 45.]));
    run(&mut s, "occurrence.move", json!({"occurrence":rail,"translate":[0,100,0],"capture":true}));
    assert!(s.model.result(fid).unwrap().error.as_deref().is_some_and(|e| e.contains("no common joint span")));
    assert!(s.model.state().body("Gusset").is_none());
    run(&mut s, "edit.undo", json!({}));
    assert!(near3(origin(&s), [30., -3., 45.]));
}

/// A part's captured gusset stays rigid; copying it must disclose its external rail support.
#[test]
fn gusset_external_support_is_reported_and_refused_across_designs() {
    let mut source = Session::default();
    run(&mut source, "solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"}));
    let part = run(&mut source, "component.create", json!({"name":"Upright","grounded":false}));
    let occurrence = part["occurrence"].as_u64().unwrap();
    run(&mut source, "solid.box", json!({"corner":[-20,-20,0],"length":40,"width":40,"height":300,"body_name":"Post"}));
    let p = gusset_args(&source);
    run(&mut source, "frame.gusset", p);
    run(&mut source, "occurrence.move", json!({"occurrence":occurrence,"translate":[0,100,0],"capture":true}));
    assert!(rel(gusset_volume(&source), 24000.) < 1e-7);

    let mut target = Session {
        foreign_copy: Some(std::sync::Arc::new(crate::ForeignCopy::of(&source, Vec::new(), vec![vec![occurrence]]).unwrap())),
        ..Session::default()
    };
    let before = target.doc.to_json();
    let error = target.execute("component.paste_from_design", &json!({})).unwrap_err().to_string();
    assert!(error.contains("exact face attachment to Rail"), "{error}");
    assert_eq!(target.doc.to_json(), before);

    let copied = run(&mut source, "component.paste_new", json!({"component":part["component"]}));
    assert!(
        copied["notices"].as_array().unwrap().iter().any(|n| n.as_str().is_some_and(|n| n.contains("external exact face attachment to Rail"))),
        "{copied}"
    );
}

/// An attachment reads the copied member's line without converting its positional names.
#[test]
fn gusset_preserves_copied_member_names_and_refuses_a_reused_name() {
    use solidcraft_doc::{FeatureKind, Model};
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Joint"}))["component"].as_u64().unwrap();
    run(&mut s, "sketch.create", json!({"plane":"XZ","name":"Posts"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[0,-300]]}));
    run(&mut s, "sketch.line", json!({"points":[[-80,0],[-80,-300]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "frame.member",
        json!({"sketch":"Posts","body_name":"Post","profile":{"kind":"rect_tube","width":40,"height":40,"wall":3,"outer_radius":0}}),
    );
    run(&mut s, "solid.box", json!({"corner":[20,-20,0],"length":300,"width":40,"height":40,"body_name":"Rail"}));
    run(&mut s, "component.activate", json!({"component":"root"}));
    let copy = run(&mut s, "component.paste_new", json!({"component":part}))["component"].as_u64().unwrap();
    let member = s.doc.features.iter().find(|f| f.component == copy && matches!(f.kind, FeatureKind::FrameMember { .. })).unwrap().clone();
    let FeatureKind::FrameMember { path_sketch, .. } = &member.kind else { panic!("not a member") };
    let sketch = s.doc.feature(path_sketch.as_ref().unwrap().0).unwrap().name.clone();
    let first_line = line_at_x(&s, &sketch, 0.);
    let [first, second] = member.body_names.as_slice() else { panic!("copy must have two positional names") };
    let rail = s.model.state().bodies.iter().find(|b| b.component == copy && b.feature != member.id).unwrap().name.clone();

    // Opening an already evaluated design must retain the same support provenance.
    let saved = s.model.saved(&s.doc).unwrap();
    s.model = Model::adopt(&s.doc, &saved).unwrap();
    run(&mut s, "component.activate", json!({"component":copy}));
    let p = json!({"supports":[gusset_pick(&s,first,[20.,0.,100.],[1.,0.,0.]),gusset_pick(&s,&rail,[100.,0.,40.],[0.,0.,1.])],"leg1":80,"leg2":100,"thickness":6,"body_name":"Gusset"});
    let gusset = run(&mut s, "frame.gusset", p)["feature"].as_u64().unwrap();
    assert_eq!(s.doc.feature(member.id), Some(&member), "attaching a gusset must not edit its support feature");
    assert!(rel(gusset_volume(&s), 24000.) < 1e-7);

    delete_line(&mut s, &sketch, &first_line);
    let state = s.model.state();
    assert!(state.body(second).is_none(), "the copy still assigns names by position");
    let surviving = state.body(first).unwrap().mesh().bounds();
    assert!(near3([surviving.min.x, surviving.min.y, surviving.min.z], [-100., -20., 0.]), "{surviving:?}");
    assert!(state.body("Gusset").is_none());
    assert!(s.model.result(gusset).unwrap().error.as_deref().is_some_and(|e| e.contains("member identity changed")));
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
    let state = s.world_state().unwrap();
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
