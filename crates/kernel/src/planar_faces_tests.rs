#![allow(clippy::unwrap_used, clippy::panic)]
//! A wall swept along a spiral has a flat top and bottom that are B-spline faces. They have to be
//! planes for the coplanar handling of a boolean to see them: a join of two such walls ran into
//! the general intersection with faces that coincide, and failed. With planes, the walls are prisms
//! and a join of two is decided in 2D (`prism.rs`).

use std::f64::consts::{FRAC_PI_2, TAU};

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::*;

const WALL: f64 = 12.0;
const HEIGHT: f64 = 60.0;
/// How far the spiral moves out in one turn.
const PITCH: f64 = 22.0;

/// The centreline of the spiral that starts `start` radians round from +X at `radius`, `t` radians along.
fn centre(radius: f64, start: f64, t: f64) -> Vec3 {
    let (r, a) = (radius + PITCH / TAU * t, start + t);
    Vec3::new(r * a.cos(), r * a.sin(), 0.0)
}

/// A wall `WALL` thick and `HEIGHT` tall along that spiral for `turn` radians, in `stations` steps,
/// the profile square to the path.
fn spiral_wall(radius: f64, start: f64, turn: f64, stations: usize) -> Body {
    let path: Vec<Vec3> = (0..=stations).map(|i| centre(radius, start, turn * i as f64 / stations as f64)).collect();
    let tangent = (path[1] - path[0]).normalized().unwrap();
    // x up the wall, y across it: x cross y runs along the path.
    let plane = Plane { origin: path[0], x: Vec3::Z, y: tangent.cross(Vec3::Z) };
    let profile = Region2 {
        outer: Loop2::polygon(&[
            Vec2::new(0.0, -WALL / 2.0),
            Vec2::new(HEIGHT, -WALL / 2.0),
            Vec2::new(HEIGHT, WALL / 2.0),
            Vec2::new(0.0, WALL / 2.0),
        ]),
        holes: vec![],
    };
    crate::sweep_path(&plane, &profile, &path).unwrap()
}

/// Two such walls whose centrelines are 11 mm apart, so that they overlap by 1 mm; the second
/// starts a little further round, so that no end face lies in the other's.
fn two_walls(turn: f64, stations: usize) -> (Body, Body) {
    (spiral_wall(60.0, 0.0, turn, stations), spiral_wall(71.0, 0.26, turn, stations))
}

/// Faces that lie on a `Plane` surface (the face-type counts of `measure` also count flat splines).
fn plane_faces(b: &Body) -> usize {
    b.solid.face_iter().filter(|f| matches!(f.surface(), truck_modeling::Surface::Plane(_))).count()
}

fn volume(b: &Body) -> f64 {
    b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume
}

#[test]
fn the_flat_faces_of_a_swept_wall_become_planes() {
    let wall = spiral_wall(60.0, 0.0, FRAC_PI_2, 8);
    // Two end caps are planes already; the top and bottom are flat B-splines like the two sides.
    assert_eq!(plane_faces(&wall), 2);
    let flat = crate::planar_faces::planarized(&wall).expect("the top and the bottom are flat");
    assert_eq!(plane_faces(&flat), 4);
    assert_eq!(flat.face_count(), wall.face_count());
    assert!(flat.validity().is_empty(), "{:?}", flat.validity());
    let (before, after) = (volume(&wall), volume(&flat));
    assert!((before - after).abs() < 1e-3 * before, "{before} vs {after}");
    // Nothing flat is left to change, and a box has nothing to begin with.
    assert!(crate::planar_faces::planarized(&flat).is_none());
    assert!(crate::planar_faces::planarized(&box_solid(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0)).unwrap()).is_none());
}

#[test]
fn walls_that_share_a_top_and_a_bottom_coincide_only_once_those_are_planes() {
    let (a, b) = two_walls(FRAC_PI_2, 8);
    assert!(crate::coplanar::coincident_planes(&a, &b).is_empty());
    let (pa, pb) = (crate::planar_faces::planarized(&a).unwrap(), crate::planar_faces::planarized(&b).unwrap());
    let planes = crate::coplanar::coincident_planes(&pa, &pb);
    assert_eq!(planes.len(), 2, "{planes:?}");
    let mut heights: Vec<f64> = planes.iter().map(|(n, d, _)| d * n.z).collect();
    heights.sort_by(f64::total_cmp);
    assert!(heights[0].abs() < 1e-6 && (heights[1] - HEIGHT).abs() < 1e-6, "{heights:?}");
}

/// The plan area of the union of the walls (each given as `(radius, start)`), by sampling every
/// 0.4 mm: a point is in a wall if it is within half its thickness of the wall's centreline and
/// not beyond either end (flat ends).
fn union_plan_area(walls: &[(f64, f64)], turn: f64) -> f64 {
    let lines: Vec<Vec<Vec3>> = walls.iter().map(|(r, s)| (0..=400).map(|i| centre(*r, *s, turn * i as f64 / 400.0)).collect()).collect();
    let inside = |line: &[Vec3], p: Vec3| -> bool {
        let mut best = (f64::INFINITY, 0.0, 0);
        for (j, w) in line.windows(2).enumerate() {
            let d = w[1] - w[0];
            let s = ((p - w[0]).dot(d) / d.dot(d)).clamp(0.0, 1.0);
            let dist = (p - (w[0] + d * s)).len();
            if dist < best.0 {
                best = (dist, (p - w[0]).dot(d) / d.dot(d), j);
            }
        }
        let beyond = (best.2 == 0 && best.1 < 0.0) || (best.2 + 2 == line.len() && best.1 > 1.0);
        best.0 <= WALL / 2.0 && !beyond
    };
    let h = 0.4;
    let n = (2.0 * 100.0 / h) as usize;
    let mut cells = 0usize;
    for i in 0..n {
        for j in 0..n {
            let p = Vec3::new(-100.0 + (i as f64 + 0.5) * h, -100.0 + (j as f64 + 0.5) * h, 0.0);
            if lines.iter().any(|l| inside(l, p)) {
                cells += 1;
            }
        }
    }
    cells as f64 * h * h
}

/// The faces of `b` that are neither planes nor cylinders (the face types `measure` reports).
fn free_form_faces(b: &Body) -> usize {
    crate::measure(b).unwrap().merged.face_types.get("other").copied().unwrap_or(0)
}

fn join_checked(a: &Body, b: &Body, walls: &[(f64, f64)], turn: f64) {
    let joined = boolean(a, b, BoolOp::Union).unwrap().unwrap();
    assert_eq!(joined.solid.boundaries().len(), 1);
    assert!(joined.validity().is_empty(), "{:?}", joined.validity());
    // Decided in 2D: the walls come back as arcs, not as splines.
    assert_eq!(free_form_faces(&joined), 0);
    let want = HEIGHT * union_plan_area(walls, turn);
    let got = joined.mesh(&crate::MeshTolerance::chord(1e-2)).unwrap().measure().volume;
    assert!((got - want).abs() < 5e-3 * want, "{got} vs {want}");
}

/// The general intersection fails on these two walls (`no result`: their tops and bottoms coincide,
/// and as B-splines the coincident-face handling does not see them). With flat faces made planes
/// they are prisms, and the join is decided in 2D, with the walls fitted by arcs within a micron.
#[test]
fn a_join_of_swept_walls_is_decided_in_2d() {
    let turn = 0.6;
    let (a, b) = two_walls(turn, 16);
    join_checked(&a, &b, &[(60.0, 0.0), (71.0, 0.26)], turn);
}

/// Two interleaved spirals, 0.75 turn, as in the snail: the second is the first turned half a turn.
#[test]
fn interleaved_spiral_walls_join() {
    let turn = 0.75 * TAU;
    let half = std::f64::consts::PI;
    let (a, b) = (spiral_wall(60.0, 0.0, turn, 36), spiral_wall(60.0, half, turn, 36));
    join_checked(&a, &b, &[(60.0, 0.0), (60.0, half)], turn);
}

/// An extrusion of a spline outline has plane caps already: a cut of it by a cylinder keeps the
/// exact spline wall, whatever the 2D route does for swept walls.
#[test]
fn a_cut_of_an_extruded_spline_keeps_its_spline_wall() {
    let p = Vec2::new;
    let line = |a: Vec2, b: Vec2| solidcraft_geom::Seg2::Line { a, b };
    let outline = Loop2 {
        segs: vec![
            line(p(0.0, 0.0), p(40.0, 0.0)),
            line(p(40.0, 0.0), p(40.0, 20.0)),
            solidcraft_geom::Seg2::Cubic { p0: p(40.0, 20.0), p1: p(30.0, 35.0), p2: p(10.0, 5.0), p3: p(0.0, 20.0) },
            line(p(0.0, 20.0), p(0.0, 0.0)),
        ],
    };
    let plate = crate::extrude(&Plane::XY, &[Region2 { outer: outline, holes: vec![] }], 0.0, 10.0).unwrap().remove(0);
    assert_eq!(free_form_faces(&plate), 1);
    let hole = crate::cylinder(Vec3::new(20.0, 10.0, -1.0), Vec3::Z, 3.0, 12.0).unwrap();
    let cut = boolean(&plate, &hole, BoolOp::Cut).unwrap().unwrap();
    assert_eq!(free_form_faces(&cut), 1);
    assert!(cut.face_count() < 12, "{}", cut.face_count());
}
