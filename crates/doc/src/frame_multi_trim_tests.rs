use std::f64::consts::PI;

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_kernel as kernel;

use super::*;
use crate::frame::{self, Profile};
use crate::lineage::{Traced, numbered_tool};

/// How close a measured volume comes: the kernel measures curved faces through their facets.
const MEASURED: f64 = 1e-3;

fn rel(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs()
}

fn section(json: serde_json::Value) -> Vec<Region2> {
    vec![serde_json::from_value::<Profile>(json).unwrap().region().unwrap()]
}

/// A 40 × 40 × 3 rectangular tube (outer radius 6, inner 3).
fn box_tube() -> Vec<Region2> {
    section(serde_json::json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}))
}

/// A Ø40 × 3 round tube.
fn round_tube() -> Vec<Region2> {
    section(serde_json::json!({"kind": "round_tube", "diameter": 40, "wall": 3}))
}

/// The section area of [`box_tube`].
fn tube_area() -> f64 {
    (1600.0 - (4.0 - PI) * 36.0) - (34.0 * 34.0 - (4.0 - PI) * 9.0)
}

/// The plane square to the line from `a` towards `b`, centred on `a`, as a member's section is placed.
fn square_to(a: Vec3, b: Vec3) -> Plane {
    let (x, y) = frame::member_axes(b - a, 0.0, frame::UpAxis::Z).unwrap();
    Plane::new(a, x, y).unwrap()
}

/// A tube of `regions` on the line from `a` to `b`, built from `start` to `end` along it.
fn member(regions: &[Region2], a: Vec3, b: Vec3, start: f64, end: f64) -> Traced {
    let mut bodies = kernel::extrude(&square_to(a, b), regions, start, end).unwrap();
    assert_eq!(bodies.len(), 1);
    numbered_tool(bodies.pop().unwrap())
}

fn volume(t: &Traced) -> f64 {
    kernel::measure_mass(&t.body).unwrap().volume
}

#[test]
fn a_box_tube_squares_to_a_plain_rectangle() {
    let out = squared_outline(&box_tube());
    assert_eq!(out.len(), 1);
    assert!(out[0].holes.is_empty(), "{out:?}");
    let corners: Vec<Vec2> = out[0]
        .outer
        .segs
        .iter()
        .map(|s| match *s {
            Seg2::Line { a, .. } => a,
            _ => panic!("not a line: {s:?}"),
        })
        .collect();
    assert_eq!(corners.len(), 4, "{out:?}");
    for want in [Vec2::new(-20.0, -20.0), Vec2::new(20.0, -20.0), Vec2::new(20.0, 20.0), Vec2::new(-20.0, 20.0)] {
        assert!(corners.iter().any(|c| (c.x - want.x).abs() < 1e-9 && (c.y - want.y).abs() < 1e-9), "{want:?} in {corners:?}");
    }
    // Closed: each side starts where the one before ends.
    let segs = &out[0].outer.segs;
    for (k, s) in segs.iter().enumerate() {
        assert!(s.end().dist(segs[(k + 1) % segs.len()].start()) < 1e-9, "{segs:?}");
    }
}

#[test]
fn a_round_tube_keeps_its_circle() {
    let tube = round_tube();
    let out = squared_outline(&tube);
    assert_eq!(out, vec![Region2 { outer: tube[0].outer.clone(), holes: Vec::new() }]);
}

/// A 40 × 40 tube along +X from the origin, built out to x = 320, and its end at x = 300.
fn box_member() -> (Traced, Vec3, Vec3, Vec3) {
    let m = member(&box_tube(), Vec3::ZERO, Vec3::X * 300.0, 0.0, 320.0);
    // In the wall, halfway along.
    (m, Vec3::X * 300.0, Vec3::X, Vec3::new(150.0, 18.5, 0.0))
}

/// A 40 × 40 tool tube along +Y at x = `x`, from y = -200 to 200.
fn box_tool_at(x: f64) -> Traced {
    let (a, b) = (Vec3::new(x, -200.0, 0.0), Vec3::new(x, 200.0, 0.0));
    fit_tool(&square_to(a, b), &box_tube(), 0.0, 400.0, &[], 0.0).unwrap()
}

#[test]
fn a_box_end_fits_flat_on_a_box_side() {
    let (m, end, toward, middle) = box_member();
    let tool = box_tool_at(300.0);
    let fitted = fit_end(&m, end, toward, middle, &[&tool], "m").unwrap();
    let b = fitted.body.exact_bounds().unwrap();
    // Flat on the tool's side: no wedge left in its corner radii.
    assert!((b.max.x - 280.0).abs() < 1e-6, "{b:?}");
    assert!(rel(volume(&fitted), tube_area() * 280.0) < MEASURED, "{} vs {}", volume(&fitted), tube_area() * 280.0);
}

#[test]
fn a_round_end_is_fishmouthed() {
    // Stage 1's 600 × 400 rectangle in round tube: the left member, built out across the bottom
    // and top members, fitted to both.
    let tube = round_tube();
    let left = member(&tube, Vec3::ZERO, Vec3::Y * 400.0, -20.0, 420.0);
    let bottom = fit_tool(&square_to(Vec3::ZERO, Vec3::X * 600.0), &tube, -20.0, 620.0, &[], 0.0).unwrap();
    let (tl, tr) = (Vec3::new(0.0, 400.0, 0.0), Vec3::new(600.0, 400.0, 0.0));
    let top = fit_tool(&square_to(tr, tl), &tube, -20.0, 620.0, &[], 0.0).unwrap();
    let middle = Vec3::new(18.5, 200.0, 0.0);
    let fitted = fit_end(&left, Vec3::ZERO, -Vec3::Y, middle, &[&bottom, &top], "left").unwrap();
    let fitted = fit_end(&fitted, tl, Vec3::Y, middle, &[&bottom, &top], "left").unwrap();
    let b = fitted.body.exact_bounds().unwrap();
    // The tips reach the cross tubes' centrelines at their sides.
    assert!(b.min.y.abs() < 1e-3 && (b.max.y - 400.0).abs() < 1e-3, "{b:?}");
    // A·360 + 2∫(20 − √(400 − z²)) dA over the annulus.
    assert!(rel(volume(&fitted), 129_458.17) < MEASURED, "{} vs 129458.17", volume(&fitted));
}

#[test]
fn a_far_tool_leaves_the_end_alone() {
    let (m, end, toward, middle) = box_member();
    let tool = box_tool_at(800.0);
    let fitted = fit_end(&m, end, toward, middle, &[&tool], "m").unwrap();
    assert!(fitted.body.same(&m.body));
    assert!(rel(volume(&fitted), tube_area() * 320.0) < MEASURED);
}

#[test]
fn a_member_wholly_inside_a_tool_is_cut_away() {
    let (m, end, toward, middle) = box_member();
    let tool = fit_tool(&square_to(Vec3::new(-50.0, 0.0, 0.0), Vec3::new(400.0, 0.0, 0.0)), &box_tube(), 0.0, 450.0, &[], 0.0).unwrap();
    let e = fit_end(&m, end, toward, middle, &[&tool], "m").unwrap_err();
    assert!(e.to_string().contains("the member on line `m` was cut away completely"), "{e}");
}

#[test]
fn an_oblique_tool_crossing_mid_span_leaves_the_member_whole() {
    // A 600 mm member crossed at x = 200 by a brace whose box covers the end, far from the end itself.
    let m = member(&box_tube(), Vec3::ZERO, Vec3::X * 600.0, 0.0, 600.0);
    let (a, b) = (Vec3::new(0.0, -200.0, 0.0), Vec3::new(800.0, 600.0, 0.0));
    let tool = fit_tool(&square_to(a, b), &box_tube(), 0.0, a.dist(b), &[], 0.0).unwrap();
    let tb = tool.body.exact_bounds().unwrap();
    assert!(tb.min.x < 600.0 && tb.max.x > 600.0 && tb.min.y < 0.0 && tb.max.y > 0.0, "the tool's box covers the end: {tb:?}");
    let fitted = fit_end(&m, Vec3::X * 600.0, Vec3::X, Vec3::new(300.0, 18.5, 0.0), &[&tool], "m").unwrap();
    assert!(fitted.body.same(&m.body));
    assert!(rel(volume(&fitted), tube_area() * 600.0) < MEASURED);
}

#[test]
fn an_x_brace_crossing_does_not_cut_the_brace() {
    // The diagonals of a 600 × 400 rectangle: one brace, crossed at its middle by the other, ends at
    // the right side's tube; only that end is fitted.
    let (a, b) = (Vec3::ZERO, Vec3::new(600.0, 400.0, 0.0));
    let len = a.dist(b);
    let brace = member(&box_tube(), a, b, 0.0, len);
    let (c, d) = (Vec3::new(0.0, 400.0, 0.0), Vec3::new(600.0, 0.0, 0.0));
    let other = fit_tool(&square_to(c, d), &box_tube(), 0.0, c.dist(d), &[], 0.0).unwrap();
    let side = fit_tool(&square_to(d, b), &box_tube(), -20.0, 420.0, &[], 0.0).unwrap();
    let toward = (b - a).normalized().unwrap();
    let (x, _) = frame::member_axes(toward, 0.0, frame::UpAxis::Z).unwrap();
    // In the wall at the brace's middle, where the other diagonal crosses it.
    let middle = (a + b) * 0.5 + x * 18.5;
    let fitted = fit_end(&brace, b, toward, middle, &[&other, &side], "brace").unwrap();
    let (got, was) = (fitted.body.exact_bounds().unwrap(), brace.body.exact_bounds().unwrap());
    // The start and the crossing are untouched; the end stops flat on the side's near face.
    assert!(got.min.dist(was.min) < 1e-6, "{got:?} vs {was:?}");
    assert!((got.max.x - 580.0).abs() < 1e-6, "{got:?}");
    assert!(volume(&fitted) > tube_area() * (len - 100.0) && volume(&fitted) < volume(&brace), "{} of {}", volume(&fitted), volume(&brace));
}

#[test]
fn auto_groups_sharing_a_point_fit_across() {
    // The planner's two groups sharing a point: a 400 square on XY and a 600 x 300 rectangle on XZ, meeting only at
    // the origin. One Frame Member takes one 2D sketch, so the build is given the two planes' lines directly.
    let p = Vec3::new;
    let segs = vec![
        ("s1".to_string(), p(0.0, 0.0, 0.0), p(400.0, 0.0, 0.0)),
        ("s2".to_string(), p(400.0, 0.0, 0.0), p(400.0, 400.0, 0.0)),
        ("s3".to_string(), p(400.0, 400.0, 0.0), p(0.0, 400.0, 0.0)),
        ("s4".to_string(), p(0.0, 400.0, 0.0), p(0.0, 0.0, 0.0)),
        ("r1".to_string(), p(0.0, 0.0, 0.0), p(-600.0, 0.0, 0.0)),
        ("r2".to_string(), p(-600.0, 0.0, 0.0), p(-600.0, 0.0, 300.0)),
        ("r3".to_string(), p(-600.0, 0.0, 300.0), p(0.0, 0.0, 300.0)),
        ("r4".to_string(), p(0.0, 0.0, 300.0), p(0.0, 0.0, 0.0)),
    ];
    let mut planes = vec![Some((Vec3::Z, Vec3::X)); 4];
    planes.extend([Some((-Vec3::Y, Vec3::X)); 4]);
    let placed = |m: usize, at: Vec3| {
        let (_, a, b) = &segs[m];
        let (x, y) = frame::member_axes(*b - *a, 0.0, frame::UpAxis::Z).unwrap();
        Plane::new(at, x, y).unwrap()
    };
    let drawn: Vec<usize> = (0..segs.len()).collect();
    let built = {
        let plan = crate::frame_layout::plan(&segs, &planes, &drawn, &|_, _| true, &[]).unwrap();
        crate::eval::frame_member::auto_members(&segs, &box_tube(), (&placed, &|_, _| None), plan, (Some(Vec3::X), &drawn), false, 0.0).unwrap().0
    };
    let bounds = |m: usize| built[m].as_ref().unwrap().body.exact_bounds().unwrap();
    // The rectangle, laid first (its 600 is the longest tube), makes the origin's corner: a butt of r1 (600) and r4
    // (300). r1 runs through to r4's outer face, x = 20; r4 is fitted to r1's top face, z = 20.
    assert!((bounds(4).max.x - 20.0).abs() < 1e-6, "{:?}", bounds(4));
    assert!((bounds(7).min.z - 20.0).abs() < 1e-6, "{:?}", bounds(7));
    // The square's s1 and s4, laid later, end there too and are fitted to the rectangle's tubes: s1 stops on r1's
    // cut end and r4's side (x = 20), s4 on r1's side (y = 20).
    assert!((bounds(0).min.x - 20.0).abs() < 1e-6, "{:?}", bounds(0));
    assert!((bounds(3).min.y - 20.0).abs() < 1e-6, "{:?}", bounds(3));
}

#[test]
fn an_arc_between_nearly_parallel_lines_keeps_its_arc() {
    // A slot-like end: a half-round arc of radius 5 between two lines 0.001° from parallel. The lines meet about
    // 573 m away; squared, that corner would stand far out of the tube and cut ends nowhere near it. The arc stays,
    // and the tool stays the size of the section.
    let drift = 100.0 * 0.001_f64.to_radians().tan();
    let (p0, p1, p2, p3) = (Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(-100.0, 10.0 + drift));
    let outer = Loop2 {
        segs: vec![
            Seg2::Line { a: p0, b: p1 },
            Seg2::Arc { center: Vec2::new(0.0, 5.0), radius: 5.0, start: -PI / 2.0, sweep: PI },
            Seg2::Line { a: p2, b: p3 },
            Seg2::Line { a: p3, b: p0 },
        ],
    };
    let out = squared_outline(&[Region2 { outer, holes: Vec::new() }]);
    let segs = &out[0].outer.segs;
    assert!(segs.iter().any(|s| matches!(s, Seg2::Arc { .. })), "{segs:?}");
    for s in segs {
        for p in [s.start(), s.end()] {
            assert!(p.x.abs() <= 101.0 && p.y.abs() <= 11.0, "{p:?} in {segs:?}");
        }
    }
}

#[test]
fn a_fitted_ends_clip_faces_are_named_for_the_zone() {
    // A tool 3° off the member's line through its end grooves it far back: the end zone clips the tool, and the clip's
    // face left on the member is named `fit_zone:<k>`, not one of the member's own `face:<k>`.
    let m = member(&box_tube(), Vec3::ZERO, Vec3::X * 600.0, 0.0, 600.0);
    let d = Vec3::new(3.0_f64.to_radians().cos(), 3.0_f64.to_radians().sin(), 0.0);
    let (a, b) = (Vec3::X * 600.0 - d * 1000.0, Vec3::X * 600.0 + d * 200.0);
    let tool = fit_tool(&square_to(a, b), &box_tube(), 0.0, a.dist(b), &[], 0.0).unwrap();
    let fitted = fit_end(&m, Vec3::X * 600.0, Vec3::X, Vec3::new(300.0, 18.5, 0.0), &[&tool], "m").unwrap();
    let lineage = fitted.lineage.as_ref().unwrap();
    // The operands whose faces are named as the zone's, and a face of the fitted member that comes from one.
    let zone: Vec<usize> = (0..lineage.operands.len())
        .filter(
            |&k| matches!(lineage.operands[k].as_ref(), crate::lineage::Operand::Made(r) if r.iter().flatten().any(|n| n.starts_with("fit_zone:"))),
        )
        .collect();
    let from_zone = |o: &kernel::history::FaceOrigin| matches!(o, kernel::history::FaceOrigin::Face { operand, .. } if zone.contains(operand));
    assert!(lineage.history.faces.iter().any(|f| f.origins.iter().any(from_zone)), "{:?}", lineage.operands);
}

#[test]
fn a_grown_tool_stands_off_by_the_gap() {
    // A box tool along +Y at x = 300, cut square at y = 100. Grown by 2 its section reaches 22 from its line and its cut
    // end stands 2 past y = 100; its uncut start stays where it was built.
    let (a, b) = (Vec3::new(300.0, -200.0, 0.0), Vec3::new(300.0, 200.0, 0.0));
    let cut = Cut { origin: Vec3::new(300.0, 100.0, 0.0), normal: -Vec3::Y };
    let bounds = |gap: f64| fit_tool(&square_to(a, b), &box_tube(), 0.0, 400.0, &[cut], gap).unwrap().body.exact_bounds().unwrap();
    let (plain, grown) = (bounds(0.0), bounds(2.0));
    assert!((plain.min.x - 280.0).abs() < 1e-9 && (plain.max.y - 100.0).abs() < 1e-9, "{plain:?}");
    assert!((grown.min.x - 278.0).abs() < 1e-9 && (grown.max.x - 322.0).abs() < 1e-9, "{grown:?}");
    assert!((grown.min.z + 22.0).abs() < 1e-9 && (grown.max.z - 22.0).abs() < 1e-9, "{grown:?}");
    assert!((grown.max.y - 102.0).abs() < 1e-9 && (grown.min.y + 200.0).abs() < 1e-9, "{grown:?}");
    // The real shape grows too, its radii kept: its round corners 6 + 2 out.
    let coped = coped_tool(&square_to(a, b), &box_tube(), 0.0, 400.0, &[cut], 2.0).unwrap();
    assert!(rel(volume(&coped), (44.0 * 44.0 - (4.0 - PI) * 64.0) * 302.0) < MEASURED, "{}", volume(&coped));
}
