//! Profile library tests. Volumes and boxes are worked out by hand: sharp-cornered tubes are two
//! rectangles, and a member is its section run the length of its line.
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

fn refused(s: &mut Session, id: &str, p: Value, why: &str) {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should be refused, got {v}"),
        Err(e) => assert!(e.to_string().contains(why), "{id} {p}: {e} (wanted `{why}`)"),
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// A body's name, volume and bounding box.
type Measured = (String, f64, [f64; 3], [f64; 3]);

fn bodies(s: &mut Session) -> Vec<Measured> {
    let m = run(s, "inspect.measure", json!({}));
    let v3 = |v: &Value| [f(&v[0]), f(&v[1]), f(&v[2])];
    m["bodies"]
        .as_array()
        .map(|a| {
            a.iter().map(|b| (b["name"].as_str().unwrap().to_string(), f(&b["volume_mm3"]), v3(&b["bbox"]["min"]), v3(&b["bbox"]["max"]))).collect()
        })
        .unwrap_or_default()
}

fn total_volume(s: &mut Session) -> f64 {
    bodies(s).iter().map(|b| b.1).sum()
}

fn near3(got: [f64; 3], want: [f64; 3]) -> bool {
    got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6)
}

/// A square tube with sharp corners: `w` across, `t` wall.
fn sharp_tube(w: f64, t: f64) -> Value {
    json!({"kind": "rect_tube", "width": w, "height": w, "wall": t, "outer_radius": 0})
}

fn sharp_area(w: f64, t: f64) -> f64 {
    w * w - (w - 2.0 * t) * (w - 2.0 * t)
}

/// A 600 × 400 rectangle on XY, as the frame sketch `Frame`.
fn rectangle_frame(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Frame"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [600, 400]}));
    run(s, "sketch.finish", json!({}));
}

/// One 1000 mm line along +X, as the frame sketch `Rail`.
fn rail(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Rail"}));
    run(s, "sketch.line", json!({"points": [[0, 0], [1000, 0]]}));
    run(s, "sketch.finish", json!({}));
}

#[test]
fn members_made_from_a_library_profile_follow_its_edits() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    let added = run(&mut s, "frame.library.add", json!({"name": "SHS 40x40x3", "category": ["Steel", "SHS"], "profile": sharp_tube(40.0, 3.0)}));
    assert_eq!(added["profile"]["id"], "lp1", "{added}");
    assert!(rel(f(&added["profile"]["area"]), sharp_area(40.0, 3.0)) < 1e-12, "{added}");
    run(&mut s, "frame.member", json!({"sketch": "Frame", "library": "SHS 40x40x3"}));
    let bs = bodies(&mut s);
    let names: Vec<&str> = bs.iter().map(|b| b.0.as_str()).collect();
    assert_eq!(names, ["SHS 40x40x3", "SHS 40x40x3 (2)", "SHS 40x40x3 (3)", "SHS 40x40x3 (4)"], "members are named after the entry");
    assert!(rel(total_volume(&mut s), sharp_area(40.0, 3.0) * 2000.0) < 1e-9);
    // Editing the entry rebuilds the members: 50 × 50 × 3 now.
    let edited = run(&mut s, "frame.library.edit", json!({"id": "lp1", "profile": sharp_tube(50.0, 3.0)}));
    assert!(edited.get("errors").is_none(), "{edited}");
    assert!(rel(total_volume(&mut s), sharp_area(50.0, 3.0) * 2000.0) < 1e-9, "{}", total_volume(&mut s));
    assert_eq!(bodies(&mut s)[0].0, "SHS 40x40x3", "a member keeps its name through an edit of its profile");
    run(&mut s, "edit.undo", json!({}));
    assert!(rel(total_volume(&mut s), sharp_area(40.0, 3.0) * 2000.0) < 1e-9, "undo brings the old profile back");
    let listed = run(&mut s, "frame.library.list", json!({}));
    assert_eq!(listed["profiles"][0]["used_by"], json!(["Member1"]), "{listed}");
    assert_eq!(listed["profiles"][0]["category"], json!(["Steel", "SHS"]));
}

#[test]
fn a_library_profiles_parameters_drive_its_members_and_follow_a_rename() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "t", "expression": "3 mm"}));
    rail(&mut s);
    run(
        &mut s,
        "frame.library.add",
        json!({"name": "SHS", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": "t", "outer_radius": 0}}),
    );
    run(&mut s, "frame.member", json!({"sketch": "Rail", "library": "SHS"}));
    assert!(rel(total_volume(&mut s), sharp_area(40.0, 3.0) * 1000.0) < 1e-9);
    run(&mut s, "parameters.change", json!({"name": "t", "expression": "5 mm"}));
    assert!(rel(total_volume(&mut s), sharp_area(40.0, 5.0) * 1000.0) < 1e-9, "{}", total_volume(&mut s));
    run(&mut s, "parameters.rename", json!({"name": "t", "new_name": "wall_t"}));
    let listed = run(&mut s, "frame.library.list", json!({}));
    assert_eq!(listed["profiles"][0]["section"]["profile"]["wall"], "wall_t", "{listed}");
    assert!(rel(total_volume(&mut s), sharp_area(40.0, 5.0) * 1000.0) < 1e-9);
    // The parameter is in use by the library profile.
    assert!(s.execute("parameters.delete", &json!({"name": "wall_t"})).is_err(), "a parameter a library profile uses can't be deleted");
}

#[test]
fn placement_points_put_the_section_on_the_line() {
    // A 60 × 40 × 5 angle along +X: its x is world +Y, its y world +Z. The heel is at (−30, −20) of
    // its bounding box's centre, the inside of the heel at (−25, −15), the x leg's toe at
    // (30, −17.5).
    let angle = json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5});
    let place = |member: Value| {
        let mut s = Session::default();
        rail(&mut s);
        run(&mut s, "frame.library.add", json!({"name": "L60", "profile": angle, "default_point": "heel_inside"}));
        let mut p = json!({"sketch": "Rail"});
        for (k, v) in member.as_object().unwrap() {
            p[k] = v.clone();
        }
        run(&mut s, "frame.member", p);
        bodies(&mut s).remove(0)
    };
    // The entry's default point: the heel 5 mm out from the line each way.
    let b = place(json!({"library": "L60"}));
    assert!(near3(b.2, [0.0, -5.0, -5.0]) && near3(b.3, [1000.0, 55.0, 35.0]), "{b:?}");
    // A point named on the member wins over the default.
    let b = place(json!({"library": "L60", "point": "toe_x"}));
    assert!(near3(b.2, [0.0, -60.0, -2.5]) && near3(b.3, [1000.0, 0.0, 37.5]), "{b:?}");
    // An anchor given on the member wins over the default too.
    let b = place(json!({"library": "L60", "anchor": "bottom_left"}));
    assert!(near3(b.2, [0.0, 0.0, 0.0]) && near3(b.3, [1000.0, 60.0, 40.0]), "{b:?}");
    // Points work with a built-in profile given inline as well.
    let b = place(json!({"profile": angle, "point": "heel_inside"}));
    assert!(near3(b.2, [0.0, -5.0, -5.0]) && near3(b.3, [1000.0, 55.0, 35.0]), "{b:?}");
    // Mirrored, the heel stays the heel: the inside of the heel moves to x = +25.
    let b = place(json!({"library": "L60", "mirror": true}));
    assert!(near3(b.2, [0.0, -55.0, -5.0]) && near3(b.3, [1000.0, 5.0, 35.0]), "{b:?}");
}

#[test]
fn a_point_the_profile_lacks_is_refused_with_the_ones_it_has() {
    let mut s = Session::default();
    rail(&mut s);
    refused(
        &mut s,
        "frame.member",
        json!({"sketch": "Rail", "profile": {"kind": "angle", "width": 60, "height": 40, "thickness": 5}, "point": "web_back"}),
        "no placement point `web_back`",
    );
    assert!(bodies(&mut s).is_empty());
}

#[test]
fn a_used_profile_is_not_removed_until_its_members_move() {
    let mut s = Session::default();
    rail(&mut s);
    run(&mut s, "frame.library.add", json!({"name": "SHS 40", "profile": sharp_tube(40.0, 3.0)}));
    run(&mut s, "frame.library.add", json!({"name": "SHS 50", "profile": sharp_tube(50.0, 3.0)}));
    run(&mut s, "frame.member", json!({"sketch": "Rail", "library": "SHS 40", "name": "Top rail"}));
    refused(&mut s, "frame.library.remove", json!({"id": "SHS 40"}), "used by Top rail");
    let moved = run(&mut s, "frame.library.replace", json!({"from": "lp1", "to": "SHS 50"}));
    assert_eq!(moved["features"], json!(["Top rail"]), "{moved}");
    assert!(rel(total_volume(&mut s), sharp_area(50.0, 3.0) * 1000.0) < 1e-9);
    run(&mut s, "frame.library.remove", json!({"id": "SHS 40"}));
    let listed = run(&mut s, "frame.library.list", json!({}));
    assert_eq!(listed["profiles"].as_array().map(Vec::len), Some(1), "{listed}");
    // A new profile never reuses a removed one's id.
    let added = run(&mut s, "frame.library.add", json!({"name": "SHS 60", "profile": sharp_tube(60.0, 3.0)}));
    assert_eq!(added["profile"]["id"], "lp3", "{added}");
}

#[test]
fn a_sketch_section_is_copied_into_the_library() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Section"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 20}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 17}));
    let pt = run(&mut s, "sketch.point", json!({"point": [0, 17]}));
    let pid = pt["created"]["points"][0].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    rail(&mut s);
    let added = run(
        &mut s,
        "frame.library.add",
        json!({"name": "Tube 40", "profile_sketch": "Section", "points": [{"name": "inside_top", "sketch_point": pid}], "default_point": "inside_top"}),
    );
    assert_eq!(added["profile"]["section"]["source"], "drawn", "{added}");
    assert_eq!(added["profile"]["named_points"], json!([{"name": "inside_top", "at": [0.0, 17.0]}]), "{added}");
    run(&mut s, "frame.member", json!({"sketch": "Rail", "library": "Tube 40"}));
    let ring = PI * (400.0 - 289.0) * 1000.0;
    let b = bodies(&mut s).remove(0);
    assert!(rel(b.1, ring) < 1e-3, "{b:?} vs {ring}");
    // The inside top of the tube sits on the line: the tube hangs 17 below it and 23 above.
    assert!(near3(b.2, [0.0, -20.0, -37.0]) && near3(b.3, [1000.0, 20.0, 3.0]), "{b:?}");
    // The entry keeps its copy: editing the sketch doesn't change the member.
    run(&mut s, "sketch.edit", json!({"sketch": "Section"}));
    run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    assert!(rel(bodies(&mut s).remove(0).1, b.1) < 1e-12);
}

#[test]
fn the_library_survives_save_reopen_and_undo() {
    let mut s = Session::default();
    rectangle_frame(&mut s);
    run(
        &mut s,
        "frame.library.add",
        json!({"name": "L50", "profile": {"kind": "angle", "width": 50, "height": 50, "thickness": 5}, "default_point": "heel", "stock_length": "6 m", "material": "Steel"}),
    );
    run(&mut s, "frame.member", json!({"sketch": "Frame", "library": "L50", "point": "heel_inside"}));
    let before = bodies(&mut s);
    let json = s.doc.to_json();
    assert!(json.contains("\"frame_profiles\"") && json.contains("\"library_profile\"") && json.contains("\"heel_inside\""), "{json}");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&json).unwrap());
    let after = bodies(&mut reopened);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert!(b.0 == a.0 && rel(a.1, b.1) < 1e-9 && near3(a.2, b.2) && near3(a.3, b.3), "{b:?} vs {a:?}");
    }
    let listed = run(&mut reopened, "frame.library.list", json!({}));
    assert!(rel(f(&listed["profiles"][0]["stock_length_mm"]), 6000.0) < 1e-12, "{listed}");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.frame_profiles.is_empty(), "undo removes the added profile");
}

#[test]
fn the_profile_points_command_lists_generated_points() {
    let mut s = Session::default();
    let r = run(&mut s, "frame.profile.points", json!({"profile": {"kind": "channel", "height": 100, "width": 50, "web": 6, "flange": 8}}));
    let at = |name: &str| r["points"].as_array().unwrap().iter().find(|p| p["name"] == name).map(|p| p["at"].clone());
    assert_eq!(at("origin"), Some(json!([0.0, 0.0])));
    assert_eq!(at("web_inside"), Some(json!([-19.0, 0.0])), "{r}");
    assert_eq!(at("flange_tip_top"), Some(json!([25.0, 46.0])), "{r}");
    assert_eq!(r["points"][0]["name"], "origin", "the points every profile has come first");
}

#[test]
fn bad_library_calls_are_refused_with_the_reason() {
    let mut s = Session::default();
    rail(&mut s);
    run(&mut s, "frame.library.add", json!({"name": "SHS 40", "profile": sharp_tube(40.0, 3.0)}));
    let tube = sharp_tube(40.0, 3.0);
    for (id, p, why) in [
        ("frame.library.add", json!({"name": "shs 40", "profile": tube}), "already has a profile named `SHS 40`"),
        ("frame.library.add", json!({"name": "X"}), "give `profile`"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "profile_sketch": "Rail"}), "not both"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "profiles": "all"}), "`profiles`"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "default_point": "heel"}), "no placement point `heel`"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "stock_length": "-1 m"}), "positive length"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "points": [{"name": "p"}]}), "give `at` or `sketch_point`"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "points": [{"name": "p", "sketch_point": "p1"}]}), "needs `profile_sketch`"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "points": [{"name": "top", "at": [0, 0]}]}), "every profile has already"),
        ("frame.library.add", json!({"name": "X", "profile": {"kind": "rect_tube", "width": 10, "height": 10, "wall": 5}}), "wall"),
        ("frame.library.add", json!({"name": "X", "profile_sketch": "Nowhere"}), "no sketch `Nowhere`"),
        ("frame.library.add", json!({"name": "X", "profile": tube, "colour": "red"}), "colour"),
        ("frame.library.edit", json!({"id": "SHS 99"}), "no library profile `SHS 99` (the library has SHS 40)"),
        ("frame.library.remove", json!({"id": "lp9"}), "no library profile `lp9`"),
        ("frame.library.replace", json!({"from": "SHS 40", "to": "lp1"}), "the same profile"),
        ("frame.profile.points", json!({}), "give `profile`"),
        ("frame.member", json!({"sketch": "Rail", "library": "Nope"}), "no library profile `Nope`"),
        ("frame.member", json!({"sketch": "Rail", "library": "SHS 40", "profile": tube}), "one of them"),
        ("frame.member", json!({"sketch": "Rail", "library": "SHS 40", "point": "  "}), "`point`"),
    ] {
        refused(&mut s, id, p, why);
    }
    assert_eq!(s.doc.frame_profiles.len(), 1, "nothing was added");
    assert!(bodies(&mut s).is_empty(), "no member was made");
}
