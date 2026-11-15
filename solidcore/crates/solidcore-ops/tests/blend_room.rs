//! The band's room through ops (SolidCraft's taper cases): a wedge whose top face closes to a
//! point, from either end; the kernel's refusals as [`Error`]s; a radius law refused where it
//! runs out of room.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // test helpers

mod common;

use solidcore::curve::curve::Curve;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::Vec3;

use common::*;
use solidcore_ops::*;

/// The prism of the polygon `pts` (in xy, counter-clockwise) from z = 0 to `h`.
fn prism(pts: &[(f64, f64)], h: f64) -> Body {
    let pts = pts.to_vec();
    built(move |k| {
        let n = pts.len();
        let mut curves = Vec::new();
        for i in 0..n {
            let (a, c) = (pts[i], pts[(i + 1) % n]);
            curves.push(Curve::from(create_line_segment(
                Vec3::new(a.0, a.1, 0.0),
                Vec3::new(c.0, c.1, 0.0),
            )?));
        }
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        create_taper_extrude(k, &refs, h, 0.0, 3, false)
    })
}

fn wedge() -> Body {
    prism(&[(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)], 15.0)
}

fn holds(r: Room, exact: f64) {
    assert!(
        r.lower <= exact && exact <= r.upper && r.upper - r.lower < 1e-11,
        "{r:?} vs {exact}"
    );
}

/// The wedge's top front edge: the top face 20 (1 - x / 40) wide, the front 15 high, at
/// right angles: the room min(20 (1 - x / 40), 15), square (15) at x = 0, closing at x = 40;
/// the same from either end.
#[test]
fn the_room_along_a_wedge_from_either_end() {
    all_fixes();
    let b = wedge();
    let e = edge_at(&b, [20.0, 0.0, 15.0]);
    let (origin, tip) = (
        vertex_at(&b, [0.0, 0.0, 15.0]),
        vertex_at(&b, [40.0, 0.0, 15.0]),
    );
    let r = band_room(&b, e, origin, &[0.25, 0.5, 0.75]).unwrap();
    assert_eq!(r.at_end, EndRoom::ClosesToPoint);
    let EndRoom::Square(s) = r.at_start else {
        panic!("{r:?}")
    };
    holds(s, 15.0);
    for (room, x) in r.stations.iter().zip([10.0_f64, 20.0, 30.0]) {
        holds(*room, (20.0 * (1.0 - x / 40.0)).min(15.0));
    }
    let back = band_room(&b, e, tip, &[0.25, 0.5, 0.75]).unwrap();
    assert_eq!(back.at_start, EndRoom::ClosesToPoint);
    assert_eq!(back.at_end, r.at_start);
    for (room, x) in back.stations.iter().zip([30.0_f64, 20.0, 10.0]) {
        holds(*room, (20.0 * (1.0 - x / 40.0)).min(15.0));
    }
    assert_eq!(band_room(&b, e, origin, &[0.0]), Err(Error::Argument));
    assert_eq!(band_room(&b, 999, origin, &[0.5]), Err(Error::Argument));
}

/// The wedge's slanted top edge closes at both ends; a trapezoid's top back edge leans out
/// 10 past both its ends (refused at the vertex named as the start).
#[test]
fn rooms_the_kernel_refuses() {
    all_fixes();
    let b = wedge();
    let e = edge_at(&b, [20.0, 10.0, 15.0]);
    let v = vertex_at(&b, [40.0, 0.0, 15.0]);
    assert_eq!(
        band_room(&b, e, v, &[0.5]),
        Err(Error::FacesCloseAtBothEnds)
    );
    let t = prism(&[(0.0, 0.0), (40.0, 0.0), (30.0, 20.0), (10.0, 20.0)], 15.0);
    let e = edge_at(&t, [20.0, 20.0, 15.0]);
    let ends = [
        vertex_at(&t, [10.0, 20.0, 15.0]),
        vertex_at(&t, [30.0, 20.0, 15.0]),
    ];
    match band_room(&t, e, ends[0], &[0.5]) {
        Err(Error::TaperEndUnsupported {
            vertex,
            lean: Some(lean),
        }) => {
            assert!(ends.contains(&vertex), "{vertex}");
            assert!((lean - 10.0).abs() < 1e-12, "{lean}");
        }
        other => panic!("{other:?}"),
    }
}

/// A variable round of 12 all along the wedge's top front edge runs out of room past x = 16
/// (20 (1 - x / 40) < 12): among the stations i / 32 from x = 0 it is refused at the first
/// past it, x = 16.25, the body untouched; one from 9 down to 0 at the point fits the room and
/// builds.
#[test]
fn a_variable_round_is_refused_where_its_room_runs_out() {
    all_fixes();
    let b = wedge();
    let e = edge_at(&b, [20.0, 0.0, 15.0]);
    let origin = vertex_at(&b, [0.0, 0.0, 15.0]);
    let before = b.volume(1e-8).unwrap();
    let r = fillet_variable(&b, e, origin, 1, &[0.0, 0.0, 1.0, 1.0], &[12.0, 12.0]);
    match r {
        Err(Error::RoomRunsOut { at, largest }) => {
            // stations i / 32: the first past x = 16 is i = 13, x = 16.25
            assert_eq!(at, 13.0 / 32.0);
            let room = 20.0 * (1.0 - 16.25 / 40.0);
            assert!(largest <= room && room - largest < 1e-11, "{largest}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(b.volume(1e-8).unwrap(), before);
    fillet_variable(&b, e, origin, 1, &[0.0, 0.0, 1.0, 1.0], &[9.0, 0.0]).unwrap();
    assert!(b.volume(1e-8).unwrap() < before);
}
