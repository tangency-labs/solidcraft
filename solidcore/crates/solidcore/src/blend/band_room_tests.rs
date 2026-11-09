//! The band's room against closed forms: a box edge, a notch, a wedge closing to a point,
//! faces closing at both ends, a leaning end, and a radius law refused where it runs out.

use super::*;
use crate::boolean::api_brep::boolean_difference;
use crate::curve::curve::Curve;
use crate::primitive::api_curves::create_line_segment;
use crate::primitive::api_primitives::create_box;
use crate::query::api_queries::edge_closest_point;
use crate::sweep::api_brep::create_taper_extrude;

/// The prism of the polygon `pts` (in xy, counter-clockwise) from z = 0 to `h`.
fn prism(k: &mut Kernel, pts: &[(f64, f64)], h: f64) -> Brep {
    let n = pts.len();
    let curves: Vec<Curve> = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            Curve::from(
                create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, h, 0.0, 3, false).unwrap()
}

/// The edge of `b` through `p`.
fn edge_at(k: &Kernel, b: &Brep, p: Point3) -> EdgeId {
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| edge_closest_point(k, b, e, p).unwrap().closest.distance < 1e-9)
        .unwrap()
}

/// Whether the edge `e` of `b` starts at `p`.
fn starts_at(b: &Brep, e: EdgeId, p: Point3) -> bool {
    let s = edge::start_end_vertices(b, e).unwrap().0.unwrap();
    (b.vertex(s).unwrap().point - p).length() < 1e-9
}

/// `room` holds `exact` and is within `spread` of it at both ends.
fn holds(room: Room, exact: f64, spread: f64) {
    assert!(
        room.lower <= exact && exact <= room.upper,
        "{room:?} vs {exact}"
    );
    assert!(
        exact - room.lower <= spread && room.upper - exact <= spread,
        "{room:?} vs {exact}"
    );
}

/// The 30 x 20 x 10 box's top front edge: at right angles (tan 45 = 1) its band reaches
/// as far along each face as its radius, so the room is the narrower face, the front's 10,
/// all along and at both (square) ends.
#[test]
fn a_box_edge_has_the_room_of_its_narrower_face() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let e = edge_at(&k, &b, Point3::new(15.0, 0.0, 10.0));
    let r = band_room(&k, &mut b, e, &[0.1, 0.5, 0.9]).unwrap();
    for room in &r.stations {
        holds(*room, 10.0, 1e-11);
    }
    for end in [r.at_start, r.at_end] {
        let EndRoom::Square(room) = end else {
            panic!("{end:?}")
        };
        holds(room, 10.0, 1e-11);
    }
    assert!(band_room(&k, &mut b, e, &[0.0]).is_err());
    assert!(band_room(&k, &mut b, e, &[1.0]).is_err());
}

/// A notch 5 deep and 10 wide cut into the top face from its back, x 10 to 20: across it
/// the top face reaches only 5 from the front edge, beside it 20 (the front's 10 holds).
#[test]
fn a_notch_narrows_the_room_across_it() {
    let mut k = Kernel::default();
    let block = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 30.0, 20.0, 10.0).unwrap();
    let notch = create_box(&mut k, Point3::new(10.0, 5.0, 5.0), 10.0, 20.0, 10.0).unwrap();
    let mut b = boolean_difference(&mut k, block, notch).unwrap();
    let e = edge_at(&k, &b, Point3::new(15.0, 0.0, 10.0));
    let r = band_room(&k, &mut b, e, &[0.1, 0.5]).unwrap();
    holds(r.stations[0], 10.0, 1e-11);
    holds(r.stations[1], 5.0, 1e-11);
}

/// The wedge (0, 0), (40, 0), (0, 20) 15 high: along its top front edge the top face is
/// 20 (1 - x / 40) wide and closes to a point at x = 40, the front 15 high; square at
/// x = 0, where the room is the front's 15.
#[test]
fn a_wedge_closes_to_a_point_at_one_end() {
    let mut k = Kernel::default();
    let mut b = prism(&mut k, &[(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)], 15.0);
    let e = edge_at(&k, &b, Point3::new(20.0, 0.0, 15.0));
    let from_origin = starts_at(&b, e, Point3::new(0.0, 0.0, 15.0));
    let x = |u: f64| {
        if from_origin {
            40.0 * u
        } else {
            40.0 * (1.0 - u)
        }
    };
    let stations = [0.125, 0.25, 0.5, 0.75, 0.875];
    let r = band_room(&k, &mut b, e, &stations).unwrap();
    for (&u, room) in stations.iter().zip(&r.stations) {
        holds(*room, (20.0 * (1.0 - x(u) / 40.0)).min(15.0), 1e-11);
    }
    let (square, closed) = if from_origin {
        (r.at_start, r.at_end)
    } else {
        (r.at_end, r.at_start)
    };
    assert_eq!(closed, EndRoom::ClosesToPoint);
    let EndRoom::Square(room) = square else {
        panic!("{square:?}")
    };
    holds(room, 15.0, 1e-11);
}

/// The wedge's slanted top edge: the top face closes to a point at both its ends.
#[test]
fn faces_closing_at_both_ends_are_refused() {
    let mut k = Kernel::default();
    let mut b = prism(&mut k, &[(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)], 15.0);
    let e = edge_at(&k, &b, Point3::new(20.0, 10.0, 15.0));
    let err = band_room(&k, &mut b, e, &[0.5]).unwrap_err();
    assert_eq!(
        err,
        SolidError::RoundRefused {
            operation: OP,
            code: crate::error::SM_ERR,
            refusal: RoundRefusal::FacesCloseAtBothEnds { edge: e },
        }
    );
}

/// The trapezoid (0, 0), (40, 0), (30, 20), (10, 20) 15 high: its top back edge's top face
/// widens past both its ends, the sides leaning out 10 along the edge: refused at its start.
#[test]
fn an_end_leaning_away_is_refused_naming_its_vertex_and_lean() {
    let mut k = Kernel::default();
    let mut b = prism(
        &mut k,
        &[(0.0, 0.0), (40.0, 0.0), (30.0, 20.0), (10.0, 20.0)],
        15.0,
    );
    let e = edge_at(&k, &b, Point3::new(20.0, 20.0, 15.0));
    let start = edge::start_end_vertices(&b, e).unwrap().0.unwrap();
    let err = band_room(&k, &mut b, e, &[0.5]).unwrap_err();
    let SolidError::RoundRefused {
        refusal: RoundRefusal::TaperEndUnsupported { vertex, lean },
        ..
    } = err
    else {
        panic!("{err:?}")
    };
    assert_eq!(vertex, start);
    assert!((lean.unwrap() - 10.0).abs() < 1e-12, "{lean:?}");
}

/// A radius law on the wedge's top front edge: 12 all along runs out where the top face is
/// narrower than 12, past x = 16, and is refused at the first station there (x = 20, room
/// 10), naming the room; one following the room (15 at the square end down to 0 at the
/// point, the room 20 (1 - x / 40) past x = 10 being above 15 (1 - x / 40)) fits.
#[test]
fn a_radius_law_is_refused_where_the_room_runs_out() {
    let mut k = Kernel::default();
    let mut b = prism(&mut k, &[(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)], 15.0);
    let e = edge_at(&k, &b, Point3::new(20.0, 0.0, 15.0));
    let from_origin = starts_at(&b, e, Point3::new(0.0, 0.0, 15.0));
    let reversed = !from_origin;
    // from x = 0 to x = 40
    let stations = if from_origin {
        [0.0, 0.25, 0.5, 0.75, 1.0]
    } else {
        [1.0, 0.75, 0.5, 0.25, 0.0]
    };
    let constant = FilletLaw::of_radii(1, &[0.0, 0.0, 1.0, 1.0], &[12.0, 12.0], reversed).unwrap();
    let err = law_fits_band(&k, &mut b, e, &stations, &constant).unwrap_err();
    let SolidError::RoundRefused {
        refusal: RoundRefusal::RoomRunsOut { edge, at, largest },
        ..
    } = err
    else {
        panic!("{err:?}")
    };
    assert_eq!(edge, e);
    assert_eq!(at, 0.5);
    assert!(
        (largest - 10.0).abs() < 1e-11 && largest <= 10.0,
        "{largest}"
    );
    let follows = FilletLaw::of_radii(1, &[0.0, 0.0, 1.0, 1.0], &[15.0, 0.0], reversed).unwrap();
    law_fits_band(&k, &mut b, e, &stations, &follows).unwrap();
}
