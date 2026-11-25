//! Stretch (#20512): sizes derived by hand from the blocks built here.

use std::f64::consts::PI;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::{Body, FaceSurface, extrude, face_surface, measure, stretch};

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// A 40 x 20 x 10 mm block from the origin, with a hole of `hole` mm diameter straight down
/// through it at (`x`, 10).
fn block_with_hole(hole: f64, x: f64) -> Body {
    let outer = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 20.0), Vec2::new(0.0, 20.0)]);
    let holes = if hole > 0.0 { vec![Loop2::circle(Vec2::new(x, 10.0), hole / 2.0)] } else { vec![] };
    extrude(&Plane::XY, &[Region2 { outer, holes }], 0.0, 10.0).unwrap().pop().unwrap()
}

/// Diameters of the body's cylindrical faces whose axis is along `axis`.
fn round_walls(b: &Body, axis: Vec3) -> Vec<f64> {
    (0..b.face_count())
        .filter_map(|f| match face_surface(b, f) {
            Some(FaceSurface::Cylinder { axis: a, radius, .. }) if axis.dot(a).abs() > 1.0 - 1e-9 => Some(2.0 * radius),
            _ => None,
        })
        .collect()
}

#[test]
fn a_block_stretched_along_x_grows_by_the_distance_and_its_hole_stays_round() {
    let b = block_with_hole(6.0, 10.0);
    let before = measure(&b).unwrap();
    // Cut at x = 30, clear of the hole (7…13): the right part moves 15 mm along +X.
    let s = stretch(&b, Vec3::new(30.0, 0.0, 0.0), Vec3::X, 15.0).unwrap();
    let m = measure(&s).unwrap();
    assert!((m.bbox.min.x - 0.0).abs() < 1e-6 && (m.bbox.max.x - 55.0).abs() < 1e-6, "{:?}", m.bbox);
    assert!((m.bbox.max.y - 20.0).abs() < 1e-6 && (m.bbox.max.z - 10.0).abs() < 1e-6, "{:?}", m.bbox);
    // The volume grows by the section (20 x 10) times 15.
    assert!(rel(m.volume, before.volume + 20.0 * 10.0 * 15.0) < 1e-6, "{} vs {}", m.volume, before.volume + 3000.0);
    assert!(rel(before.volume, 40.0 * 20.0 * 10.0 - PI * 9.0 * 10.0) < 1e-6);
    assert_eq!(m.shells, 1, "one solid");
    // The joins leave no seams: six sides and the hole's wall, as before.
    assert_eq!(m.merged.faces, before.merged.faces, "{:?} vs {:?}", m.merged, before.merged);
    // The hole keeps its 6 mm diameter.
    let walls = round_walls(&s, Vec3::Z);
    assert!(!walls.is_empty() && walls.iter().all(|d| (d - 6.0).abs() < 1e-6), "{walls:?}");
}

#[test]
fn a_stretch_toward_minus_x_keeps_the_far_side_and_moves_the_near_one() {
    let b = block_with_hole(0.0, 0.0);
    let s = stretch(&b, Vec3::new(10.0, 0.0, 0.0), -Vec3::X, 5.0).unwrap();
    let m = measure(&s).unwrap();
    assert!((m.bbox.min.x + 5.0).abs() < 1e-6 && (m.bbox.max.x - 40.0).abs() < 1e-6, "{:?}", m.bbox);
    assert!(rel(m.volume, 45.0 * 20.0 * 10.0) < 1e-9, "{}", m.volume);
}

#[test]
fn a_cut_through_a_hole_across_the_direction_is_refused_and_names_it() {
    // Stretching along Z through the plate's middle: the hole runs along Z, so this is fine.
    let b = block_with_hole(6.0, 10.0);
    let s = stretch(&b, Vec3::new(0.0, 0.0, 5.0), Vec3::Z, 4.0).unwrap();
    let m = measure(&s).unwrap();
    assert!((m.bbox.max.z - 14.0).abs() < 1e-6, "{:?}", m.bbox);
    assert!(round_walls(&s, Vec3::Z).iter().all(|d| (d - 6.0).abs() < 1e-6));
    // Along X at x = 10 the plane crosses the hole's round wall, which runs along Z, not X.
    let err = stretch(&b, Vec3::new(10.0, 0.0, 0.0), Vec3::X, 5.0).unwrap_err().to_string();
    assert!(err.contains("round face across the direction"), "{err}");
}

#[test]
fn a_shrink_takes_a_slice_out_and_one_into_a_detail_says_how_far_it_can_go() {
    let b = block_with_hole(6.0, 10.0);
    // Clear of the hole: x 20…30 is plain block.
    let s = stretch(&b, Vec3::new(20.0, 0.0, 0.0), Vec3::X, -10.0).unwrap();
    let m = measure(&s).unwrap();
    assert!((m.bbox.max.x - 30.0).abs() < 1e-6, "{:?}", m.bbox);
    assert!(rel(m.volume, 30.0 * 20.0 * 10.0 - PI * 9.0 * 10.0) < 1e-6, "{}", m.volume);
    // From x = 30 toward -X the block is plain down to the hole (x 7…13): 17 mm, not 20.
    let err = stretch(&b, Vec3::new(30.0, 0.0, 0.0), -Vec3::X, -20.0).unwrap_err().to_string();
    assert!(err.contains("The most it can shrink here is 16.9"), "{err}");
    // More than lies in front of the plane.
    let err = stretch(&b, Vec3::new(30.0, 0.0, 0.0), Vec3::X, -10.0).unwrap_err().to_string();
    assert!(err.contains("takes out everything in front of the cut plane"), "{err}");
}

#[test]
fn hostile_input_is_refused_not_a_crash() {
    let b = block_with_hole(0.0, 0.0);
    for (at, n, d) in [
        (Vec3::new(50.0, 0.0, 0.0), Vec3::X, 5.0),
        (Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, 5.0),
        (Vec3::new(f64::NAN, 0.0, 0.0), Vec3::X, 5.0),
        (Vec3::new(10.0, 0.0, 0.0), Vec3::X, f64::INFINITY),
    ] {
        assert!(stretch(&b, at, n, d).is_err(), "{at:?} {n:?} {d}");
    }
    // Zero distance changes nothing.
    let same = stretch(&b, Vec3::new(10.0, 0.0, 0.0), Vec3::X, 0.0).unwrap();
    assert!(rel(measure(&same).unwrap().volume, 8000.0) < 1e-12);
}

#[test]
fn a_long_stretch_is_one_valid_solid_whatever_the_distance() {
    // A long stretch lengthens the band behind the cut many times over: its edges' tolerances
    // must stay within the body's (an 80 mm stretch of a 112 mm enclosure was refused, #20512).
    let outer = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(112.0, 0.0), Vec2::new(112.0, 60.0), Vec2::new(0.0, 60.0)]);
    let inner = Loop2::polygon(&[Vec2::new(3.0, 3.0), Vec2::new(3.0, 57.0), Vec2::new(109.0, 57.0), Vec2::new(109.0, 3.0)]);
    let wall = extrude(&Plane::XY, &[Region2 { outer: outer.clone(), holes: vec![inner] }], 0.0, 40.0).unwrap().pop().unwrap();
    let floor = extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 3.0).unwrap().pop().unwrap();
    let enclosure = crate::union_all(&[&wall, &floor]).unwrap();
    let before = measure(&enclosure).unwrap().volume;
    for distance in [80.0, 500.0] {
        let s = stretch(&enclosure, Vec3::new(56.0, 0.0, 0.0), Vec3::X, distance).unwrap_or_else(|e| panic!("{distance}: {e}"));
        let m = measure(&s).unwrap();
        assert!((m.bbox.max.x - (112.0 + distance)).abs() < 1e-6, "{distance}: {:?}", m.bbox);
        // The section at the cut: the floor (112 wide is along X; across it 60 x 3) and the two
        // side walls above it (3 x 37 each).
        let section = 60.0 * 3.0 + 2.0 * 3.0 * 37.0;
        assert!(rel(m.volume, before + section * distance) < 1e-6, "{distance}: {} vs {}", m.volume, before + section * distance);
        assert_eq!(m.shells, 1);
    }
}

#[test]
fn a_thread_axis_moves_or_grows_with_the_stretch() {
    use crate::stretch_axis;
    let x = Vec3::new(1.0, 0.0, 0.0);
    let z = Vec3::new(0.0, 0.0, 1.0);
    let cut = Vec3::new(50.0, 0.0, 0.0);
    // Upright at x = 80, beyond the cut along X: it moves 15 with the part.
    let (p, s, e) = stretch_axis(Vec3::new(80.0, 30.0, 0.0), z, 0.0, 10.0, cut, x, 15.0);
    assert_eq!((p, s, e), (Vec3::new(95.0, 30.0, 0.0), 0.0, 10.0));
    // Upright at x = 20, before the cut: it stays.
    let (p, s, e) = stretch_axis(Vec3::new(20.0, 30.0, 0.0), z, 0.0, 10.0, cut, x, 15.0);
    assert_eq!((p, s, e), (Vec3::new(20.0, 30.0, 0.0), 0.0, 10.0));
    // Along X from x = 10 to x = 90, across the cut: its far end moves, so it is 15 longer.
    let (p, s, e) = stretch_axis(Vec3::new(10.0, 30.0, 5.0), x, 0.0, 80.0, cut, x, 15.0);
    assert_eq!((p, s, e), (Vec3::new(10.0, 30.0, 5.0), 0.0, 95.0));
    // The same axis pointing the other way: its start is the end beyond the cut.
    let (p, s, e) = stretch_axis(Vec3::new(90.0, 30.0, 5.0), -x, -10.0, 70.0, cut, x, 15.0);
    assert_eq!((p, s, e), (Vec3::new(90.0, 30.0, 5.0), -25.0, 70.0));
}
