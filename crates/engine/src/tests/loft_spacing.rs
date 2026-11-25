//! Lofted round tubes through sections that are not evenly spaced.
//!
//! A loft's surface used to pass through its sections at uniform parameters, so a section close to
//! its neighbour (a bore that runs 5 mm past the end of a tube whose other sections are 12 mm
//! apart, or 60 mm in a snail) made the surface overshoot between the others, by several times the
//! tube's radius. The overshoot also made booleans with the tube fail for some section counts and
//! not for their neighbours.

use std::f64::consts::PI;

use super::*;

/// `sections` circles of radius `radius` centred on an arc of radius `path` turning `angle`
/// (radians), in planes normal to the arc, lofted into the body `body`; `over` extends the
/// sections past both ends along the tangent.
fn tube(s: &mut Session, body: &str, radius: f64, path: f64, angle: f64, sections: usize, over: f64) {
    let mut stations: Vec<[f64; 4]> = (0..sections)
        .map(|i| {
            let t = angle * i as f64 / (sections - 1) as f64;
            [path * t.cos(), path * t.sin(), -t.sin(), t.cos()]
        })
        .collect();
    if over > 0.0 {
        let (a, b) = (stations[0], stations[sections - 1]);
        stations.insert(0, [a[0] - over * a[2], a[1] - over * a[3], a[2], a[3]]);
        stations.push([b[0] + over * b[2], b[1] + over * b[3], b[2], b[3]]);
    }
    let mut list = Vec::new();
    for (i, [x, y, tx, ty]) in stations.iter().enumerate() {
        let name = format!("{body}{i}");
        run(s, "sketch.create", json!({"plane": {"origin": [x, y, 0], "x_dir": [0, 0, 1], "y_dir": [ty, -tx, 0]}, "name": name}));
        run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": radius}));
        run(s, "sketch.finish", json!({}));
        list.push(json!({"sketch": name}));
    }
    run(s, "solid.loft", json!({"sections": list, "operation": "new", "name": format!("Loft{body}"), "body_name": body}));
}

fn bounds(s: &mut Session, body: &str) -> ([f64; 3], [f64; 3]) {
    let m = run(s, "inspect.measure", json!({}));
    let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] == body).unwrap();
    let get = |k: &str| -> [f64; 3] {
        let v = b["bbox"][k].as_array().unwrap();
        [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()]
    };
    (get("min"), get("max"))
}

#[test]
fn a_bore_with_a_short_overrun_stays_inside_its_tube() {
    // 14 sections 12 mm apart, and one more 5 mm beyond each end: the bore is a tube of radius 18
    // around the arc (z within 18 of the arc's plane), and used to reach z = 88.
    let mut s = Session::default();
    tube(&mut s, "Bore", 18.0, 100.0, PI / 2.0, 14, 5.0);
    let (lo, hi) = bounds(&mut s, "Bore");
    assert!(hi[2] < 18.0 + 0.1 && lo[2] > -18.0 - 0.1, "z from {} to {}", lo[2], hi[2]);
    let want = PI * 18.0 * 18.0 * (100.0 * PI / 2.0 + 10.0);
    let got = volume(&mut s);
    assert!(rel(got, want) < 2e-3, "{got} vs {want}");
}

#[test]
fn a_hollow_elbow_is_cut_for_any_number_of_sections() {
    let (path, outer, inner) = (100.0, 30.0, 18.0);
    // Each of these gave "no result" (the first six) or failed in another way.
    for (degrees, sections) in [(60.0, 5), (60.0, 7), (90.0, 10), (90.0, 14), (120.0, 7), (120.0, 13)] {
        let angle = degrees * PI / 180.0;
        let mut s = Session::default();
        tube(&mut s, "Outer", outer, path, angle, sections, 0.0);
        tube(&mut s, "Inner", inner, path, angle, sections, 5.0);
        run(&mut s, "solid.combine", json!({"target": "Outer", "tools": ["Inner"], "operation": "cut", "name": "Hollow"}));
        assert_eq!(s.model.state().bodies.len(), 1, "{degrees} degrees, {sections} sections");
        let want = PI * (outer * outer - inner * inner) * path * angle;
        let got = volume(&mut s);
        assert!(rel(got, want) < 3e-3, "{degrees} degrees, {sections} sections: {got} vs {want}");
    }
}

/// #15263: both ends of this arch are separate regions of one XY sketch. Measuring the
/// loft used to fail with "no mass", even though all six faces had been built.
#[test]
fn a_loft_returning_to_its_start_plane_has_mass_properties() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Ends"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [30, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane": {"origin": [15, 0, 5], "x_dir": [0, 1, 0], "y_dir": [0, 0, 1]}, "name": "Middle"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.loft",
        json!({"sections": [
            {"sketch": "Ends", "profiles": [{"point": [5, 5]}]},
            {"sketch": "Middle"},
            {"sketch": "Ends", "profiles": [{"point": [25, 5]}]}
        ], "operation": "new", "body_name": "Arch"}),
    );

    let state = s.model.state();
    assert_eq!(state.bodies.len(), 1);
    let body = &state.bodies[0].body;
    assert!(!body.is_surface());
    assert_eq!(body.shell_count(), 1);
    assert_eq!(body.face_count(), 6);

    let faces = run(&mut s, "model.faces", json!({"body": "Arch"}));
    let cap_normals: Vec<_> = faces["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["plane_normal"].as_array())
        .filter(|n| n[2].as_f64().unwrap().abs() > 0.999999)
        .collect();
    assert_eq!(cap_normals.len(), 2, "{faces}");
    assert!(cap_normals.iter().all(|n| n[2].as_f64().unwrap() < -0.999999), "{faces}");

    let measured = run(&mut s, "inspect.measure", json!({"bodies": ["Arch"]}));
    assert_eq!(measured["body_count"], 1, "{measured}");
    let arch = &measured["bodies"][0];
    eprintln!("returning loft mass properties: {arch}");
    assert_eq!(arch["closed"], true, "{measured}");
    for property in ["volume_mm3", "area_mm2", "mass_g"] {
        let value = arch[property].as_f64().unwrap_or_else(|| panic!("missing {property}: {measured}"));
        assert!(value.is_finite() && value > 0.0, "{property}: {measured}");
    }
    // Exact area between the untwisted quadratic arch boundaries, times width 10.
    let volume = arch["volume_mm3"].as_f64().unwrap();
    assert!((volume - 8000.0 / 3.0).abs() < 1e-5, "{measured}");
    assert!(
        arch["center_of_mass"].as_array().is_some_and(|p| p.len() == 3 && p.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))),
        "{measured}"
    );
}
