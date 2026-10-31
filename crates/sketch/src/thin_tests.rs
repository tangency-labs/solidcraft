//! Thin walls against areas worked out by hand: a wall along a chain covers its length times
//! the thickness, less what corners overlap and plus what mitres add.

use std::f64::consts::PI;

use solidcraft_geom::{Seg2, Vec2};

use crate::thin_wall_regions;

fn line(a: [f64; 2], b: [f64; 2]) -> Vec<Seg2> {
    vec![Seg2::Line { a: Vec2::new(a[0], a[1]), b: Vec2::new(b[0], b[1]) }]
}

fn area(curves: &[Vec<Seg2>], inside: f64, outside: f64) -> f64 {
    let regions = thin_wall_regions(curves, inside, outside).unwrap_or_else(|e| panic!("wall: {e}"));
    regions.iter().map(|r| r.area()).sum()
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6 * (1.0 + b.abs()), "{a} != {b}");
}

/// A 40 × 20 rectangle, its sides listed in any order and drawn either way.
fn rectangle() -> Vec<Vec<Seg2>> {
    vec![line([0.0, 0.0], [40.0, 0.0]), line([40.0, 20.0], [0.0, 20.0]), line([40.0, 0.0], [40.0, 20.0]), line([0.0, 0.0], [0.0, 20.0])]
}

#[test]
fn a_rectangle_makes_a_ring_on_either_side_or_centred() {
    let regions = thin_wall_regions(&rectangle(), 1.0, 1.0).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 1);
    // The mitred corners keep straight sides: four lines outside, four inside.
    assert_eq!(regions[0].outer.segs.len(), 4);
    assert_eq!(regions[0].holes[0].segs.len(), 4);
    close(area(&rectangle(), 1.0, 1.0), 42.0 * 22.0 - 38.0 * 18.0);
    close(area(&rectangle(), 2.0, 0.0), 40.0 * 20.0 - 36.0 * 16.0);
    close(area(&rectangle(), 0.0, 2.0), 44.0 * 24.0 - 40.0 * 20.0);
    close(area(&rectangle(), 0.5, 1.5), 43.0 * 23.0 - 39.0 * 19.0);
}

#[test]
fn an_open_corner_has_its_inside_where_it_bends() {
    // (0,0) → (10,0) → (10,10) turns left: the inside is toward the triangle it closes.
    let l = vec![line([0.0, 0.0], [10.0, 0.0]), line([10.0, 10.0], [10.0, 0.0])];
    close(area(&l, 1.0, 1.0), 40.0);
    close(area(&l, 2.0, 0.0), 20.0 + 20.0 - 4.0);
    close(area(&l, 0.0, 2.0), 20.0 + 20.0 + 4.0);
    let inside = thin_wall_regions(&l, 2.0, 0.0).unwrap();
    assert!(inside[0].contains(Vec2::new(5.0, 1.0)) && !inside[0].contains(Vec2::new(5.0, -1.0)));
}

/// Every corner of the wall's outlines lies in the closed area `inside`, within rounding.
fn stays_within(regions: &[solidcraft_geom::Region2], inside: impl Fn(Vec2) -> bool) {
    for r in regions {
        for lp in std::iter::once(&r.outer).chain(&r.holes) {
            for p in lp.polyline(1e-4) {
                assert!(inside(p), "the wall reaches out to {p:?}");
            }
        }
    }
}

#[test]
fn walls_inside_sharp_corners_stay_inside_the_outline() {
    // #7589: at corners sharper than a right angle a band's square end reached past the next
    // side, out of the outline; the two bands now meet in a mitre on the corner's bisector.
    let triangle = vec![line([0.0, 0.0], [20.0, 0.0]), line([20.0, 0.0], [0.0, 10.0]), line([0.0, 10.0], [0.0, 0.0])];
    let regions = thin_wall_regions(&triangle, 1.0, 0.0).unwrap();
    stays_within(&regions, |p| p.x >= -1e-9 && p.y >= -1e-9 && p.x / 20.0 + p.y / 10.0 <= 1.0 + 1e-9);
    // The opening is the triangle shrunk about its incircle's centre.
    let r = 200.0 / (30.0 + 500f64.sqrt());
    close(area(&triangle, 1.0, 0.0), 100.0 * (1.0 - ((r - 1.0) / r).powi(2)));
    assert_eq!((regions[0].outer.segs.len(), regions[0].holes[0].segs.len()), (3, 3));

    // A parallelogram with 80° corners; its opening is a parallelogram 2 mm narrower each way.
    let (c, s) = (80f64.to_radians().cos(), 80f64.to_radians().sin());
    let p = [[0.0, 0.0], [20.0, 0.0], [20.0 + 10.0 * c, 10.0 * s], [10.0 * c, 10.0 * s]];
    let sides: Vec<Vec<Seg2>> = (0..4).map(|k| line(p[k], p[(k + 1) % 4])).collect();
    let regions = thin_wall_regions(&sides, 1.0, 0.0).unwrap();
    stays_within(&regions, |q| q.y >= -1e-9 && q.y <= 10.0 * s + 1e-9);
    let (h1, h2) = (10.0 * s, 20.0 * s);
    close(area(&sides, 1.0, 0.0), (h1 * h2 - (h1 - 2.0) * (h2 - 2.0)) / s);

    // A segment of a circle (chord and arc meeting at 60°): the opening is the segment of the
    // circle 1 mm smaller above the chord 1 mm higher, so the line's and the arc's walls meet
    // where their inner sides cross.
    let a = Vec2::new(75f64.sqrt(), 5.0);
    let cap = vec![
        vec![Seg2::Arc { center: Vec2::ZERO, radius: 10.0, start: PI / 6.0, sweep: 2.0 * PI / 3.0 }],
        vec![Seg2::Line { a: Vec2::new(-a.x, 5.0), b: a }],
    ];
    let regions = thin_wall_regions(&cap, 1.0, 0.0).unwrap();
    stays_within(&regions, |q| q.y >= 5.0 - 1e-9 && q.len() <= 10.0 + 1e-9);
    let segment = |r: f64, d: f64| r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    close(area(&cap, 1.0, 0.0), segment(10.0, 5.0) - segment(9.0, 6.0));
}

#[test]
fn a_straight_wall_takes_its_side_from_the_drawn_direction() {
    let up = thin_wall_regions(&[line([0.0, 0.0], [10.0, 0.0])], 2.0, 0.0).unwrap();
    assert!(up[0].contains(Vec2::new(5.0, 1.0)));
    let down = thin_wall_regions(&[line([10.0, 0.0], [0.0, 0.0])], 2.0, 0.0).unwrap();
    assert!(down[0].contains(Vec2::new(5.0, -1.0)));
    close(area(&[line([0.0, 0.0], [10.0, 0.0])], 1.5, 0.5), 20.0);
}

#[test]
fn a_circle_makes_an_exact_ring() {
    let circle = solidcraft_geom::Loop2::circle(Vec2::new(3.0, 4.0), 10.0).segs;
    let regions = thin_wall_regions(std::slice::from_ref(&circle), 1.0, 1.0).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 1);
    assert!(regions[0].outer.segs.iter().chain(&regions[0].holes[0].segs).all(|s| matches!(s, Seg2::Arc { .. })));
    close(area(std::slice::from_ref(&circle), 1.0, 1.0), PI * (11.0f64.powi(2) - 9.0f64.powi(2)));
    close(area(std::slice::from_ref(&circle), 3.0, 0.0), PI * (100.0 - 49.0));
    // A clockwise circle still has its inside inside.
    let cw: Vec<Seg2> = circle.iter().rev().map(Seg2::reversed).collect();
    close(area(&[cw], 3.0, 0.0), PI * (100.0 - 49.0));
}

#[test]
fn walls_that_cross_or_meet_are_one_region() {
    // Two lines crossing, and four meeting at a point: a plus of four 10 mm arms either way.
    let crossing = vec![line([-10.0, 0.0], [10.0, 0.0]), line([0.0, -10.0], [0.0, 10.0])];
    let meeting: Vec<Vec<Seg2>> = [[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0], [0.0, -10.0]].iter().map(|p| line([0.0, 0.0], *p)).collect();
    for curves in [crossing, meeting] {
        let regions = thin_wall_regions(&curves, 1.0, 1.0).unwrap();
        assert_eq!(regions.len(), 1);
        close(regions[0].area(), 4.0 * 10.0 * 2.0 - 4.0);
    }
    // A ring with a bar across it.
    let mut ring = vec![solidcraft_geom::Loop2::circle(Vec2::ZERO, 10.0).segs];
    ring.push(line([-10.0, 0.0], [10.0, 0.0]));
    let regions = thin_wall_regions(&ring, 1.0, 1.0).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 2);
}

#[test]
fn separate_walls_stay_separate() {
    let regions = thin_wall_regions(&[line([0.0, 0.0], [10.0, 0.0]), line([0.0, 5.0], [10.0, 5.0])], 1.0, 1.0).unwrap();
    assert_eq!(regions.len(), 2);
}

#[test]
fn a_spline_wall_is_its_length_times_its_thickness() {
    // A gentle cubic; its wall's area is its length times the thickness (a centred wall's
    // sides are as much longer on one side as shorter on the other).
    let s = Seg2::Cubic { p0: Vec2::new(0.0, 0.0), p1: Vec2::new(10.0, 8.0), p2: Vec2::new(20.0, -8.0), p3: Vec2::new(30.0, 0.0) };
    // Within the fit's 1e-4 mm along both sides.
    let a = area(&[vec![s]], 0.5, 0.5);
    assert!((a - s.length()).abs() < 2.0 * s.length() * 1e-4, "{a} vs {}", s.length());
    let regions = thin_wall_regions(&[vec![s]], 0.5, 0.5).unwrap();
    for q in [0.25, 0.5, 0.75] {
        let p = s.point_at(q);
        let n = s.derivative(q).normalized().unwrap().perp();
        assert!(regions[0].contains(p + n * 0.4) && regions[0].contains(p - n * 0.4));
        assert!(!regions[0].contains(p + n * 0.6) && !regions[0].contains(p - n * 0.6));
    }
}

#[test]
fn walls_thicker_than_a_curve_bends_or_without_thickness_are_refused() {
    let arc = vec![Seg2::Arc { center: Vec2::ZERO, radius: 2.0, start: 0.0, sweep: PI }];
    assert!(thin_wall_regions(std::slice::from_ref(&arc), 3.0, 0.0).is_err());
    assert!(thin_wall_regions(&[arc], 0.0, 3.0).is_ok());
    let l = [line([0.0, 0.0], [10.0, 0.0])];
    for (a, b) in [(0.0, 0.0), (-1.0, 2.0), (f64::NAN, 1.0), (1.0, f64::INFINITY)] {
        assert!(thin_wall_regions(&l, a, b).is_err());
    }
    assert!(thin_wall_regions(&[], 1.0, 1.0).is_err());
    assert!(thin_wall_regions(&[line([f64::NAN, 0.0], [1.0, 0.0])], 1.0, 1.0).is_err());
}

/// The edges of an `n` × `n` grid of hexagons of side `a` (pointy top), each edge once.
fn honeycomb(n: usize, a: f64) -> Vec<Vec<Seg2>> {
    let key = |p: Vec2| ((p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64);
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let w = a * 3f64.sqrt();
    for i in 0..n {
        for j in 0..n {
            let c = Vec2::new(i as f64 * w + if j % 2 == 1 { w / 2.0 } else { 0.0 }, j as f64 * 1.5 * a);
            let corners: Vec<Vec2> = (0..6).map(|k| c + Vec2::from_angle(PI / 6.0 + k as f64 * PI / 3.0) * a).collect();
            for k in 0..6 {
                let (p, q) = (corners[k], corners[(k + 1) % 6]);
                let edge = if key(p) < key(q) { (key(p), key(q)) } else { (key(q), key(p)) };
                if seen.insert(edge) {
                    out.push(vec![Seg2::Line { a: p, b: q }]);
                }
            }
        }
    }
    out
}

#[test]
fn a_honeycomb_is_one_region_with_a_hole_per_cell() {
    let cells = honeycomb(4, 5.0);
    let regions = thin_wall_regions(&cells, 0.4, 0.4).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 16);
}
