#![allow(clippy::unwrap_used, clippy::panic)]
use std::f64::consts::{PI, TAU};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::*;

/// A 40 x 30 x 2 plate on the XY plane with a Ø5 hole at (10, 15) and a 6 mm wide obround slot
/// from (25, 15) to (33, 15) (centre to centre, so 14 mm long overall).
fn plate() -> Body {
    plate_with(true)
}

fn plate_with(round_hole: bool) -> Body {
    let outer = Loop2::polygon(&[Vec2::new(0., 0.), Vec2::new(40., 0.), Vec2::new(40., 30.), Vec2::new(0., 30.)]);
    let hole = Loop2::circle(Vec2::new(10., 15.), 2.5).reversed();
    let (c1, c2) = (Vec2::new(25., 15.), Vec2::new(33., 15.));
    let slot = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(25., 12.), b: Vec2::new(33., 12.) },
            Seg2::Arc { center: c2, radius: 3., start: -PI / 2., sweep: PI },
            Seg2::Line { a: Vec2::new(33., 18.), b: Vec2::new(25., 18.) },
            Seg2::Arc { center: c1, radius: 3., start: PI / 2., sweep: PI },
        ],
    }
    .reversed();
    let holes = if round_hole { vec![hole, slot] } else { vec![slot] };
    let region = Region2 { outer, holes };
    extrude(&Plane::XY, &[region], 0., 2.).unwrap().remove(0)
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, 0.)
}

#[test]
fn a_round_hole_sweeps_the_full_turn_and_a_slot_end_half_of_it() {
    let b = plate();
    let mesh = b.display_mesh(0.01).unwrap();
    let walls = hole_walls(&b, &mesh);
    assert_eq!(walls.len(), 3, "{walls:?}");
    for w in &walls {
        assert!(w.axis.cross(Vec3::Z).len() < 1e-6, "{w:?}");
        let (lo, hi) = if w.axis.z > 0. { (w.start, w.end) } else { (-w.end, -w.start) };
        let base = w.axis_point.z;
        assert!((base + lo).abs() < 1e-6 && (base + hi - 2.).abs() < 1e-6, "the wall runs through the plate: {w:?}");
    }
    let round: Vec<&HoleWall> = walls.iter().filter(|w| (w.radius - 2.5).abs() < 1e-3).collect();
    assert_eq!(round.len(), 1, "{walls:?}");
    let h = round[0];
    assert!((h.sweep - TAU).abs() < 1e-9, "a round hole closes: {h:?}");
    assert_eq!(h.opening, None);
    assert!(flat(h.axis_point).dist(Vec3::new(10., 15., 0.)) < 1e-3, "{h:?}");
    let ends: Vec<&HoleWall> = walls.iter().filter(|w| (w.radius - 3.).abs() < 1e-3).collect();
    assert_eq!(ends.len(), 2, "{walls:?}");
    for e in ends {
        // About half a turn (to the 5° bins), open toward the other end.
        assert!((e.sweep - PI).abs() <= TAU / 72. * 2. + 1e-9, "{e:?}");
        let open = e.opening.unwrap_or_else(|| panic!("a slot end opens: {e:?}"));
        let toward = if flat(e.axis_point).x < 29. { Vec3::X } else { -Vec3::X };
        assert!(open.dot(toward) > 0.99, "{e:?} opens along {open:?}, not {toward:?}");
    }
}

#[test]
fn a_countersink_joins_the_wall_it_opens_out_from() {
    // The same plate, with a Ø8 x 90° countersink on top of the round hole (1.5 mm deep).
    // Cut as one revolved hole: Ø5 through, opening out at 45° from 0.5 mm up.
    let b = plate_with(false);
    let plane = Plane::new(Vec3::new(10., 15., 0.), Vec3::X, Vec3::Z).unwrap();
    let profile = Region2 {
        outer: Loop2::polygon(&[Vec2::new(0., -1.), Vec2::new(2.5, -1.), Vec2::new(2.5, 0.5), Vec2::new(5., 3.), Vec2::new(0., 3.)]),
        holes: vec![],
    };
    let cutter = revolve(&plane, &[profile], Vec2::ZERO, Vec2::new(0., 1.), TAU).unwrap().remove(0);
    let b = boolean(&b, &cutter, BoolOp::Cut).unwrap().unwrap();
    let mesh = b.display_mesh(0.01).unwrap();
    let walls = hole_walls(&b, &mesh);
    let h = walls.iter().find(|w| (w.radius - 2.5).abs() < 1e-3).unwrap_or_else(|| panic!("{walls:?}"));
    let (lo, hi) = if h.axis.z > 0. { (h.start, h.end) } else { (-h.end, -h.start) };
    let base = h.axis_point.z;
    assert!((base + lo).abs() < 1e-6 && (base + hi - 2.).abs() < 1e-6, "the wall reaches the top face through the countersink: {h:?}");
}
