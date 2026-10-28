//! Profile library tests. Expected values are worked out by hand: rectangles and circles, and
//! composite rectangles by the parallel-axis theorem; each fillet adds or removes a spandrel of
//! (1 − π/4) r².
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use std::f64::consts::PI;

const SPANDREL: f64 = 1.0 - PI / 4.0;

fn close(got: f64, want: f64, what: &str) {
    let tol = 1e-9 * (1.0 + want.abs());
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want}");
}

fn profile(json: serde_json::Value) -> Profile {
    serde_json::from_value(json).unwrap()
}

fn sec(p: &Profile) -> Section {
    let r = p.region().unwrap();
    assert!(r.outer.is_closed(1e-9), "{} outline is closed", p.kind());
    assert!(r.outer.is_ccw(), "{} outline runs counter-clockwise", p.kind());
    for h in &r.holes {
        assert!(h.is_closed(1e-9) && !h.is_ccw(), "{} holes are closed and clockwise", p.kind());
    }
    // The exact boundary integral agrees with the region's own area.
    let s = section(&r);
    close(s.area, r.area(), "area against Region2::area");
    s
}

/// Area, centroid (in the profile's centred frame) and centroidal Ixx, Iyy of rectangles
/// given as (x0, y0, x1, y1), combined.
fn rects(parts: &[(f64, f64, f64, f64)]) -> (f64, f64, f64, f64, f64) {
    let a: f64 = parts.iter().map(|&(x0, y0, x1, y1)| (x1 - x0) * (y1 - y0)).sum();
    let cx = parts.iter().map(|&(x0, y0, x1, y1)| (x1 - x0) * (y1 - y0) * (x0 + x1) / 2.0).sum::<f64>() / a;
    let cy = parts.iter().map(|&(x0, y0, x1, y1)| (x1 - x0) * (y1 - y0) * (y0 + y1) / 2.0).sum::<f64>() / a;
    let ixx = parts
        .iter()
        .map(|&(x0, y0, x1, y1)| {
            let (b, h) = (x1 - x0, y1 - y0);
            b * h * h * h / 12.0 + b * h * ((y0 + y1) / 2.0 - cy).powi(2)
        })
        .sum();
    let iyy = parts
        .iter()
        .map(|&(x0, y0, x1, y1)| {
            let (b, h) = (x1 - x0, y1 - y0);
            h * b * b * b / 12.0 + b * h * ((x0 + x1) / 2.0 - cx).powi(2)
        })
        .sum();
    (a, cx, cy, ixx, iyy)
}

fn check_rects(s: &Section, parts: &[(f64, f64, f64, f64)], what: &str) {
    let (a, cx, cy, ixx, iyy) = rects(parts);
    close(s.area, a, &format!("{what} area"));
    close(s.centroid[0], cx, &format!("{what} centroid x"));
    close(s.centroid[1], cy, &format!("{what} centroid y"));
    close(s.ixx, ixx, &format!("{what} Ixx"));
    close(s.iyy, iyy, &format!("{what} Iyy"));
}

#[test]
fn bar_is_a_rectangle() {
    let s = sec(&profile(serde_json::json!({"kind": "bar", "width": 40, "height": 10})));
    close(s.area, 400.0, "area");
    close(s.ixx, 40.0 * 1000.0 / 12.0, "Ixx = b h³ / 12");
    close(s.iyy, 10.0 * 64_000.0 / 12.0, "Iyy = h b³ / 12");
    close(s.ixy, 0.0, "Ixy");
    close(s.width, 40.0, "width");
    close(s.height, 10.0, "height");
}

#[test]
fn rounded_bar_loses_four_spandrels() {
    let s = sec(&profile(serde_json::json!({"kind": "bar", "width": 30, "height": 20, "corner_radius": 4})));
    close(s.area, 600.0 - 4.0 * SPANDREL * 16.0, "area");
    // Fully rounded: a 20 × 20 bar with radius 10 is a circle.
    let s = sec(&profile(serde_json::json!({"kind": "bar", "width": 20, "height": 20, "corner_radius": 10})));
    close(s.area, PI * 100.0, "fully rounded area");
    close(s.ixx, PI * 10f64.powi(4) / 4.0, "fully rounded Ixx");
}

#[test]
fn round_bar_and_tube() {
    let s = sec(&Profile::RoundBar { diameter: 20.0 });
    close(s.area, PI * 100.0, "bar area");
    close(s.ixx, PI * 20f64.powi(4) / 64.0, "bar Ixx = π d⁴ / 64");
    close(s.iyy, s.ixx, "bar Iyy");
    let s = sec(&Profile::RoundTube { diameter: 42.4, wall: 3.2 });
    let d = 42.4 - 6.4;
    close(s.area, PI / 4.0 * (42.4f64.powi(2) - d * d), "tube area");
    close(s.ixx, PI / 64.0 * (42.4f64.powi(4) - d.powi(4)), "tube Ixx = π (D⁴ − d⁴) / 64");
}

#[test]
fn rect_tube_sharp_and_rounded() {
    let s = sec(&profile(serde_json::json!({"kind": "rect_tube", "width": 60, "height": 40, "wall": 3, "outer_radius": 0})));
    close(s.area, 60.0 * 40.0 - 54.0 * 34.0, "sharp area");
    close(s.ixx, (60.0 * 40f64.powi(3) - 54.0 * 34f64.powi(3)) / 12.0, "sharp Ixx");
    close(s.iyy, (40.0 * 60f64.powi(3) - 34.0 * 54f64.powi(3)) / 12.0, "sharp Iyy");
    // Defaults: outer radius 2 t = 6, inner radius 3.
    let p = profile(serde_json::json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}));
    assert_eq!(p.filled(), Profile::RectTube { width: 40.0, height: 40.0, wall: 3.0, outer_radius: Some(6.0) });
    let s = sec(&p);
    let outer = 1600.0 - 4.0 * SPANDREL * 36.0;
    let inner = 34.0 * 34.0 - 4.0 * SPANDREL * 9.0;
    close(s.area, outer - inner, "rounded area");
    close(s.centroid[0], 0.0, "centroid x");
    close(s.ixy, 0.0, "Ixy");
}

#[test]
fn angle_sharp_matches_two_rectangles() {
    let (a, b, t) = (60.0, 40.0, 5.0);
    let s = sec(&profile(serde_json::json!({"kind": "angle", "width": a, "height": b, "thickness": t, "root_radius": 0, "toe_radius": 0})));
    let (x0, y0) = (-a / 2.0, -b / 2.0);
    check_rects(&s, &[(x0, y0, x0 + a, y0 + t), (x0, y0 + t, x0 + t, y0 + b)], "angle");
    assert!(s.ixy.abs() > 1.0, "an unequal angle has a product of inertia");
}

#[test]
fn angle_root_adds_and_toes_remove_spandrels() {
    let (a, b, t) = (50.0, 50.0, 5.0);
    let p = profile(serde_json::json!({"kind": "angle", "width": a, "height": b, "thickness": t}));
    let s = sec(&p);
    // Defaults: root radius t, toe radius t / 2.
    close(s.area, t * (a + b - t) + SPANDREL * t * t - 2.0 * SPANDREL * (t / 2.0).powi(2), "area");
    // An equal angle is symmetric about its diagonal.
    close(s.centroid[0], s.centroid[1], "centroid on the diagonal");
    close(s.ixx, s.iyy, "Ixx = Iyy");
}

#[test]
fn channel_and_i_beam_sharp_match_rectangles() {
    let (h, b, tw, tf) = (100.0, 50.0, 6.0, 8.0);
    let s = sec(&profile(serde_json::json!({"kind": "channel", "height": h, "width": b, "web": tw, "flange": tf, "root_radius": 0})));
    let (x0, y0) = (-b / 2.0, -h / 2.0);
    let parts = [(x0, y0, x0 + b, y0 + tf), (x0, y0 + tf, x0 + tw, y0 + h - tf), (x0, y0 + h - tf, x0 + b, y0 + h)];
    check_rects(&s, &parts, "channel");
    close(s.ixy, 0.0, "channel Ixy");

    let (h, b, tw, tf) = (100.0, 50.0, 4.5, 7.0);
    let s = sec(&profile(serde_json::json!({"kind": "i_beam", "height": h, "width": b, "web": tw, "flange": tf, "root_radius": 0})));
    close(s.area, 2.0 * b * tf + (h - 2.0 * tf) * tw, "I area");
    close(s.ixx, (b * h.powi(3) - (b - tw) * (h - 2.0 * tf).powi(3)) / 12.0, "I Ixx");
    close(s.iyy, (2.0 * tf * b.powi(3) + (h - 2.0 * tf) * tw.powi(3)) / 12.0, "I Iyy");
}

#[test]
fn i_beam_root_radii_add_four_spandrels() {
    let (h, b, tw, tf, r) = (100.0, 50.0, 4.5, 7.0, 7.0);
    let s = sec(&profile(serde_json::json!({"kind": "i_beam", "height": h, "width": b, "web": tw, "flange": tf, "root_radius": r})));
    close(s.area, 2.0 * b * tf + (h - 2.0 * tf) * tw + 4.0 * SPANDREL * r * r, "area");
    close(s.centroid[0], 0.0, "centroid x");
    close(s.centroid[1], 0.0, "centroid y");
}

#[test]
fn every_kind_example_builds() {
    for k in profile_kinds() {
        assert_eq!(k.example.kind(), k.kind);
        let s = sec(&k.example);
        assert!(s.area > 0.0 && s.ixx > 0.0 && s.iyy > 0.0, "{}: {s:?}", k.kind);
        let (w, h) = k.example.size();
        close(s.width, w, &format!("{} width", k.kind));
        close(s.height, h, &format!("{} height", k.kind));
    }
}

#[test]
fn dimensions_that_do_not_fit_are_refused() {
    let bad = [
        serde_json::json!({"kind": "bar", "width": f64::MAX, "height": 1}),
        serde_json::json!({"kind": "bar", "width": -1, "height": 1}),
        serde_json::json!({"kind": "bar", "width": 0, "height": 1}),
        serde_json::json!({"kind": "bar", "width": 10, "height": 10, "corner_radius": 6}),
        serde_json::json!({"kind": "round_bar", "diameter": 1e300}),
        serde_json::json!({"kind": "rect_tube", "width": 10, "height": 10, "wall": 5}),
        serde_json::json!({"kind": "rect_tube", "width": 10, "height": 10, "wall": 1, "outer_radius": -1}),
        serde_json::json!({"kind": "round_tube", "diameter": 10, "wall": 5}),
        serde_json::json!({"kind": "angle", "width": 10, "height": 10, "thickness": 10}),
        serde_json::json!({"kind": "angle", "width": 10, "height": 10, "thickness": 2, "root_radius": 9}),
        serde_json::json!({"kind": "angle", "width": 10, "height": 10, "thickness": 2, "toe_radius": 3}),
        serde_json::json!({"kind": "channel", "height": 10, "width": 10, "web": 2, "flange": 5}),
        serde_json::json!({"kind": "channel", "height": 10, "width": 10, "web": 10, "flange": 2}),
        serde_json::json!({"kind": "i_beam", "height": 100, "width": 50, "web": 5, "flange": 7, "root_radius": 30}),
    ];
    for j in bad {
        let p: Profile = serde_json::from_value(j.clone()).unwrap();
        assert!(p.region().is_err(), "{j} is refused");
    }
    for j in [
        serde_json::json!({"kind": "bar", "width": 1}),
        serde_json::json!({"kind": "bar", "width": 1, "height": 1, "colour": 2}),
        serde_json::json!({"kind": "hexagon"}),
    ] {
        assert!(serde_json::from_value::<Profile>(j.clone()).is_err(), "{j} is not a profile");
    }
}

#[test]
fn non_finite_dimensions_are_refused() {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Profile::RoundBar { diameter: v }.region().is_err());
        assert!(Profile::RectTube { width: 40.0, height: 40.0, wall: 3.0, outer_radius: Some(v) }.region().is_err());
    }
}

#[test]
fn placing_moves_the_anchor_onto_the_point() {
    let p = profile(serde_json::json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5}));
    let r = p.region().unwrap();
    let s0 = section(&r);
    let anchor = anchor_point(std::slice::from_ref(&r), Anchor::BottomLeft);
    close(anchor.x, -30.0, "bottom left x");
    close(anchor.y, -20.0, "bottom left y");
    let at = Vec2::new(100.0, 50.0);
    let moved = placed(&r, anchor, at, PI / 2.0);
    let s1 = section(&moved);
    close(s1.area, s0.area, "area is kept");
    // Turned a quarter: the heel stays on `at`, the 60 mm leg now runs up and the 40 mm one left.
    close(s1.width, 40.0, "turned width");
    close(s1.height, 60.0, "turned height");
    let c = at + Vec2::new(-(s0.centroid[1] + 20.0), s0.centroid[0] + 30.0);
    close(s1.centroid[0], c.x, "centroid x");
    close(s1.centroid[1], c.y, "centroid y");
    close(s1.ixx, s0.iyy, "Ixx after a quarter turn");
    let centre = anchor_point(std::slice::from_ref(&r), Anchor::Centroid);
    let s2 = section(&placed(&r, centre, Vec2::ZERO, 0.0));
    close(s2.centroid[0], 0.0, "centroid anchor x");
    close(s2.centroid[1], 0.0, "centroid anchor y");
}

#[test]
fn member_axes_are_square_to_the_line_and_keep_up() {
    for d in [Vec3::X, Vec3::new(0.0, -3.0, 0.0), Vec3::new(1.0, 2.0, 3.0), Vec3::Z, Vec3::new(0.0, 0.0, -2.0), Vec3::new(1e-12, 0.0, 1.0)] {
        for angle in [0.0, 0.3, -PI / 2.0] {
            let (x, y) = member_axes(d, angle).unwrap();
            let n = d.normalized().unwrap();
            close(x.len(), 1.0, "|x|");
            close(y.len(), 1.0, "|y|");
            close(x.dot(y), 0.0, "x ⟂ y");
            close(x.cross(y).dist(n), 0.0, "x × y runs along the line");
        }
    }
    // Unturned: a horizontal member's y is world up; a vertical one's x is world X.
    let (_, y) = member_axes(Vec3::new(1.0, 1.0, 0.0), 0.0).unwrap();
    close(y.dist(Vec3::Z), 0.0, "horizontal y");
    let (x, _) = member_axes(Vec3::Z, 0.0).unwrap();
    close(x.dist(Vec3::X), 0.0, "vertical x");
    let (x, _) = member_axes(Vec3::new(0.0, 0.0, -1.0), 0.0).unwrap();
    close(x.dist(Vec3::X), 0.0, "downward x");
    // A quarter turn takes x to where y was.
    let (_, y0) = member_axes(Vec3::X, 0.0).unwrap();
    let (x1, _) = member_axes(Vec3::X, PI / 2.0).unwrap();
    close(x1.dist(y0), 0.0, "quarter turn");
    assert!(member_axes(Vec3::ZERO, 0.0).is_none());
}

#[test]
fn mirroring_keeps_the_section_and_flips_x() {
    let p = profile(serde_json::json!({"kind": "angle", "width": 60, "height": 40, "thickness": 5}));
    let r = p.region().unwrap();
    let m = mirrored(&r);
    assert!(m.outer.is_ccw() && m.outer.is_closed(1e-9), "the outline still runs counter-clockwise");
    let (a, b) = (section(&r), section(&m));
    close(b.area, a.area, "area");
    close(b.centroid[0], -a.centroid[0], "centroid x");
    close(b.centroid[1], a.centroid[1], "centroid y");
    close(b.ixy, -a.ixy, "Ixy changes sign");
    let tube = mirrored(&profile(serde_json::json!({"kind": "rect_tube", "width": 40, "height": 20, "wall": 2})).region().unwrap());
    assert!(tube.holes.iter().all(|h| !h.is_ccw()), "holes still run clockwise");
}

#[test]
fn anchors_of_an_off_centre_region() {
    // A sketch profile need not be centred: a 10 × 20 box from (5, 5).
    let r = Region2 {
        outer: Loop2::polygon(&[Vec2::new(5.0, 5.0), Vec2::new(15.0, 5.0), Vec2::new(15.0, 25.0), Vec2::new(5.0, 25.0)]),
        holes: Vec::new(),
    };
    let at = |a| anchor_point(std::slice::from_ref(&r), a);
    assert_eq!(at(Anchor::Origin), Vec2::ZERO);
    assert_eq!(at(Anchor::Center), Vec2::new(10.0, 15.0));
    assert_eq!(at(Anchor::BottomLeft), Vec2::new(5.0, 5.0));
    assert_eq!(at(Anchor::TopRight), Vec2::new(15.0, 25.0));
    assert_eq!(at(Anchor::Left), Vec2::new(5.0, 15.0));
    let c = at(Anchor::Centroid);
    close(c.x, 10.0, "centroid x");
    close(c.y, 15.0, "centroid y");
}

#[test]
fn default_radii_shrink_to_fit_thick_sections() {
    // Each of these failed with the defaults at their usual size (#4003 review).
    for j in [
        serde_json::json!({"kind": "angle", "width": 20, "height": 20, "thickness": 10}),
        serde_json::json!({"kind": "channel", "height": 50, "width": 10, "web": 6, "flange": 2}),
        serde_json::json!({"kind": "channel", "height": 20, "width": 40, "web": 5, "flange": 8}),
        serde_json::json!({"kind": "i_beam", "height": 100, "width": 10, "web": 6, "flange": 5}),
    ] {
        let p = profile(j.clone());
        let s = sec(&p);
        assert!(s.area > 0.0, "{j}");
    }
    // A 20 × 20 × 10 angle has 10 mm of leg inside: root and toe keep their 2 : 1 ratio.
    let Profile::Angle { root_radius: Some(r1), toe_radius: Some(r2), .. } =
        profile(serde_json::json!({"kind": "angle", "width": 20, "height": 20, "thickness": 10})).filled()
    else {
        panic!("an angle")
    };
    close(r1, 20.0 / 3.0, "root");
    close(r2, 10.0 / 3.0, "toe");
    // One given, the other default takes what is left.
    let Profile::Angle { toe_radius: Some(r2), .. } =
        profile(serde_json::json!({"kind": "angle", "width": 20, "height": 20, "thickness": 10, "root_radius": 8})).filled()
    else {
        panic!("an angle")
    };
    close(r2, 2.0, "toe after an 8 mm root");
    // A channel 20 tall with 8 mm flanges has 4 mm of web inside: two roots of 2.
    let Profile::Channel { root_radius: Some(r1), .. } =
        profile(serde_json::json!({"kind": "channel", "height": 20, "width": 40, "web": 5, "flange": 8})).filled()
    else {
        panic!("a channel")
    };
    close(r1, 2.0, "channel root");
    // Defaults that fit are left as they are.
    assert_eq!(
        profile(serde_json::json!({"kind": "angle", "width": 50, "height": 50, "thickness": 5})).filled(),
        Profile::Angle { width: 50.0, height: 50.0, thickness: 5.0, root_radius: Some(5.0), toe_radius: Some(2.5) }
    );
}

#[test]
fn radii_given_too_large_or_too_small_are_refused_by_name() {
    for (j, name) in [
        (serde_json::json!({"kind": "angle", "width": 20, "height": 20, "thickness": 10, "toe_radius": 11}), "toe_radius"),
        (serde_json::json!({"kind": "angle", "width": 20, "height": 20, "thickness": 10, "root_radius": 9, "toe_radius": 2}), "root_radius"),
        (serde_json::json!({"kind": "channel", "height": 20, "width": 40, "web": 5, "flange": 8, "root_radius": 3}), "root_radius"),
        (serde_json::json!({"kind": "i_beam", "height": 100, "width": 10, "web": 6, "flange": 5, "root_radius": 3}), "root_radius"),
        (serde_json::json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3, "outer_radius": 1e-12}), "outer_radius"),
        (serde_json::json!({"kind": "bar", "width": 40, "height": 10, "corner_radius": 0.001}), "corner_radius"),
    ] {
        let err = profile(j.clone()).region().unwrap_err().to_string();
        assert!(err.contains(name), "{j}: {err}");
    }
    // The value named is the one given, not twice it.
    let err = profile(serde_json::json!({"kind": "rect_tube", "width": 10, "height": 10, "wall": 5})).region().unwrap_err().to_string();
    assert!(err.contains("`wall` (5)") && err.contains("half the smaller side (5)"), "{err}");
    let err = profile(serde_json::json!({"kind": "channel", "height": 10, "width": 10, "web": 2, "flange": 5})).region().unwrap_err().to_string();
    assert!(err.contains("`flange` (5)"), "{err}");
}

#[test]
fn a_spec_holds_expressions_and_resolves_them() {
    let spec: ProfileSpec = serde_json::from_value(serde_json::json!({"kind": "rect_tube", "width": 40, "height": "H", "wall": "2 * t"})).unwrap();
    assert_eq!(spec.dims.get("width").map(|e| e.as_str()), Some("40"));
    // Saved as one object of text.
    assert_eq!(serde_json::to_value(&spec).unwrap(), serde_json::json!({"kind": "rect_tube", "width": "40", "height": "H", "wall": "2 * t"}));
    let p = spec
        .resolve(|_, e| match e.as_str() {
            "40" => Ok(40.0),
            "H" => Ok(60.0),
            "2 * t" => Ok(3.0),
            other => Err(crate::DocError::Invalid(other.into())),
        })
        .unwrap();
    assert_eq!(p, Profile::RectTube { width: 40.0, height: 60.0, wall: 3.0, outer_radius: None });
    // A value that is not a finite length is refused, naming the dimension.
    let err = spec.resolve(|name, _| Ok(if name == "wall" { f64::NAN } else { 40.0 })).unwrap_err().to_string();
    assert!(err.contains("wall"), "{err}");
    let mut labels = Vec::new();
    crate::walk::Walk::walk_exprs(&spec, &mut |(label, e, kind, _)| labels.push((label, e.as_str().to_string(), kind)));
    assert_eq!(labels, [("Height", "H".into(), Kind::Length), ("Wall", "2 * t".into(), Kind::Length), ("Width", "40".into(), Kind::Length)]);
    for (j, why) in [
        (serde_json::json!({"width": 40}), "kind"),
        (serde_json::json!({"kind": "hexagon", "width": 40}), "unknown profile kind"),
        (serde_json::json!({"kind": "round_bar"}), "needs `diameter`"),
        (serde_json::json!({"kind": "round_bar", "diameter": 10, "colour": 2}), "no dimension `colour`"),
        (serde_json::json!({"kind": "round_bar", "diameter": true}), "number or an expression"),
        (serde_json::json!({"kind": "round_bar", "diameter": " "}), "number or an expression"),
        (serde_json::json!({"kind": "round_bar", "diameter": "x".repeat(5000)}), "number or an expression"),
    ] {
        let err = serde_json::from_value::<ProfileSpec>(j.clone()).unwrap_err().to_string();
        assert!(err.contains(why), "{j}: {err}");
    }
}

#[test]
fn descriptions_round_to_a_tenth_of_a_micron() {
    let p = Profile::RectTube { width: 25.4, height: 25.4, wall: 0.125 * 25.4, outer_radius: None };
    assert_eq!(p.description(), "Rect Tube 25.4x25.4x3.175");
    assert_eq!(Profile::RoundBar { diameter: 1.0 / 3.0 }.description(), "Round Bar 0.3333");
}

#[test]
fn a_line_a_hair_off_vertical_is_vertical() {
    // Rounding noise must not turn a post a quarter turn (#4003 review).
    let (x0, _) = member_axes(Vec3::Z, 0.0).unwrap();
    for lean in [1e-8, -1e-8, 1e-12] {
        let (x, _) = member_axes(Vec3::new(lean, 0.0, 1.0), 0.0).unwrap();
        close(x.dist(x0), 0.0, "near-vertical x");
        let (x, _) = member_axes(Vec3::new(0.0, lean, 1.0), 0.0).unwrap();
        close(x.dist(x0), 0.0, "near-vertical x");
    }
    // A lean you can see is a lean: its y tips toward up.
    let (_, y) = member_axes(Vec3::new(1e-3, 0.0, 1.0), 0.0).unwrap();
    assert!(y.z > 0.0 && y.x < 0.0, "{y:?}");
}

#[test]
fn gusset_contact_clipping_accounts_for_material_between_endpoints() {
    use solidcraft_geom::Vec2 as V;
    // Unit square in two triangles, clipped to a 0.5 by 0.6 contact rectangle.
    let a = crate::gusset::clipped_area([V::new(0., 0.), V::new(1., 0.), V::new(1., 1.)], 0.2, 0.7, 0.1, 0.7);
    let b = crate::gusset::clipped_area([V::new(0., 0.), V::new(1., 1.), V::new(0., 1.)], 0.2, 0.7, 0.1, 0.7);
    assert!((a + b - 0.3).abs() < 1e-12);
    let reversed = crate::gusset::clipped_area([V::new(1., 1.), V::new(1., 0.), V::new(0., 0.)], 0.2, 0.7, 0.1, 0.7);
    assert!((reversed - a).abs() < 1e-12);
}
