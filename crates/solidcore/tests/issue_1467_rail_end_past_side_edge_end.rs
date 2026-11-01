//! tangency-labs/solidcore#1467 (WR-630; the fork's tangency-labs/smlib@3f22e97): a
//! 40 x 20 x 10 box's top edge on a 40 x 10 face, rounded just over 10, built end to end
//! by the cliff overflow.
//!
//! Each rail on the 10 high face lies `r - 10` below the face's bottom edge, so it meets
//! the vertical side edge at each end of the round on the edge's line, `r - 10` past the
//! edge's end (found there by `fillet_trace_reaches_rails`, WR-610). Without
//! `fillet_rail_end_on_continued_surface`, `CalcRailIntEdgeuse` evaluates that end on the
//! face's surface clamped to its domain: it lands on the side edge's bottom vertex, off the
//! fillet by `(r - 10)^2 / 2r` (1.24e-4 at r10.05), under the corner's ten tolerances, so
//! the Nx1 corner takes it as meeting the side edge and builds an end rib off the fillet,
//! which fails (error 3). With the flag the rail end stays on the rail, misses the side
//! edge (`CalcCornerEdgeGeom`'s miss test, part of `fillet_cliff_overflow`), and the
//! overflow removes the round's corner region: the expected volumes are in closed form.
//!
//! Both top edges along x are rounded. On the front one (y = 0) the side edges' UV trims
//! are lines and the run-on finds the rail ends with the flags of #960 alone. On the back
//! one (y = 20, `wr631_cliff_overflow_construction`'s #1467 case) the trims are exact
//! degree-1 B-splines, which the run-on continues only with `fillet_run_on_bspline_trims`
//! (WR-654); without it the rail ends are not found, the corner takes them as misses
//! anyway, and that case never reaches the placement this tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::edge;
use solidcore::Kernel;

/// The flags of the fork's cliff overflow (#960), with or without WR-630's.
fn kernel(rail_end_on_continued_surface: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_run_on_bspline_trims = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_rail_end_on_continued_surface = rail_end_on_continued_surface;
    k
}

/// The area of the section a round of radius `r` removes between perpendicular faces `w`
/// and `h` wide, in closed form (as `wr631_cliff_overflow_construction`).
fn removed_area(r: f64, w: f64, h: f64) -> f64 {
    let a = w.min(r);
    let f = |s: f64| s / 2.0 * (r * r - s * s).max(0.0).sqrt() + r * r / 2.0 * (s / r).asin();
    let u_h = if h >= r {
        0.0
    } else {
        r - (r * r - (r - h) * (r - h)).sqrt()
    };
    let mut area = if h < r { h * u_h.min(a) } else { 0.0 };
    if a > u_h {
        area += r * (a - u_h) - (f(r - u_h) - f(r - a));
    }
    area
}

#[derive(Debug)]
enum Outcome {
    Built(f64),
    Refused(i32, String),
}

/// Rounds the box's top edge along x at `y` by `r`.
fn round(rail_end_on_continued_surface: bool, y: f64, r: f64) -> Outcome {
    let mut k = kernel(rail_end_on_continued_surface);
    let mut b = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
    let e = b
        .edges()
        .ids()
        .find(|&e| {
            let ivl = edge::interval(&b, e).unwrap();
            let c = edge::curve(&b, e).unwrap().unwrap();
            let on = |p: Point3| p.y == y && p.z == 10.0;
            on(c.evaluate_point(&k, ivl.min()).unwrap())
                && on(c.evaluate_point(&k, ivl.max()).unwrap())
        })
        .expect("the top edge");
    let mut session = FilletSession::new(&mut k, &mut b).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
        .unwrap();
    let result = session.do_fillet(&mut k);
    let info = session.error_info().clone();
    drop(session);
    match result {
        Ok(()) => {
            assert!(
                is_manifold_solid(&b).unwrap(),
                "y {y} r {r}: not a manifold solid"
            );
            let store = AttributeStore::default();
            let attrs = PropertyAttributes {
                store: &store,
                key_of: &|_| ObjectKey(0),
            };
            Outcome::Built(brep_volume(&k, &mut b, 1e-6, &attrs).unwrap())
        }
        Err(_) => Outcome::Refused(info.code, info.comment),
    }
}

#[test]
fn a_round_just_past_the_face_keeps_its_rail_ends_on_the_rails() {
    // the fork's test radii (r10.05, r10.07, r12) and r10.06, on both top edges along x
    for y in [0.0, 20.0] {
        for r in [10.05, 10.06, 10.07, 12.0] {
            let expected = 8000.0 - 40.0 * removed_area(r, 20.0, 10.0);
            match round(true, y, r) {
                Outcome::Built(v) => assert!(
                    (v - expected).abs() <= 1e-8 * expected,
                    "y {y} r {r}: volume {v} against {expected}"
                ),
                other @ Outcome::Refused(..) => {
                    panic!("y {y} r {r}: expected a build at {expected}, got {other:?}")
                }
            }
        }
    }
}

#[test]
fn clamped_onto_the_side_edge_s_vertex_the_end_rib_is_off_the_fillet() {
    // without the flag the rail end sits on the side edge's bottom vertex and the corner
    // builds its end rib there, off the fillet: the corner's edges fail (error 3)
    for y in [0.0, 20.0] {
        for r in [10.05, 10.06, 10.07] {
            let Outcome::Refused(code, comment) = round(false, y, r) else {
                panic!("y {y} r {r}: expected the corner to fail without the flag");
            };
            assert_eq!(
                code,
                FilletErrorType::VertexProblem as i32,
                "y {y} r {r}: {comment}"
            );
            assert_eq!(
                comment, "Fillet Corner Error: Corner Edge Computation Failure",
                "y {y} r {r}"
            );
        }
    }
}
