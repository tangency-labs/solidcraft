//! tangency-labs/solidcraft#17633 (WR-717): a blend's crease that runs within the trim's
//! tolerance of a body edge is taken as that edge, and the body edge now moves onto the
//! crease, so the sheet is imprinted and sewn where it lies.
//!
//! Before the fix, the section's end on each end face was drawn the crease's gap g to the
//! old vertex. A 40 x 20 x 10 box's top back edge rounded r 10.01 crosses the bottom plane
//! g = r - sqrt(r^2 - (r - h)^2) = 4.995e-6 inside the bottom back edge, and the body came
//! out 9.990011e-4 above its closed form: L g h / 2 = 40 * 4.995e-6 * 10 / 2 to 2e-10, the
//! triangle of base g and height h that the skewed end face adds along the edge.
//!
//! The expected value is `wr631_cliff_overflow_construction`'s closed form.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

/// The cliff overflow trimmed at its creases, with WR-717's creases along edges.
fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_trace_reaches_rails = true;
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-10, &attrs).unwrap()
}

fn pick(k: &Kernel, b: &Brep, q: Point3) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.expect("an edge")
}

/// The area a round of radius `r` removes between perpendicular faces `w` and `h` wide
/// (`wr631_cliff_overflow_construction`'s closed form).
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

fn assert_valid(k: &mut Kernel, b: &mut Brep, what: &str) {
    assert!(
        is_manifold_solid(b).unwrap(),
        "{what}: not a manifold solid"
    );
    b.use_arena_naming().unwrap();
    let lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(lines.is_empty(), "{what}: {lines:?}");
}

/// The box's round a hair past its bottom: its closed form to 1e-8, and the bottom edge's
/// ends on the crease, not g from it.
#[test]
fn a_round_whose_crease_runs_along_the_bottom_edge_keeps_its_closed_form() {
    for r in [10.01, 10.02] {
        let mut k = kernel();
        let mut b = create_box(&mut k, Point3::ZERO, 40.0, 20.0, 10.0).unwrap();
        let e = pick(&k, &b, Point3::new(20.0, 20.0, 10.0));
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
            .unwrap();
        session
            .do_fillet(&mut k)
            .unwrap_or_else(|e| panic!("r {r}: {e}"));
        drop(session);
        let expected = 8000.0 - 40.0 * removed_area(r, 20.0, 10.0);
        let v = volume(&k, &mut b);
        assert!(
            (v - expected).abs() <= 1e-8 * expected,
            "r {r}: {v} against {expected}"
        );
        // the bottom edge's ends moved onto the crease, g inside the old corner at y = 20
        let g = r - (r * r - (r - 10.0) * (r - 10.0)).sqrt();
        let mut ends = 0;
        for &v in b.vertex_list() {
            let p = solidcore::topology::vertex::point(&b, v).unwrap();
            if p.z.abs() <= 1e-9 && p.y > 19.0 {
                ends += 1;
                assert!(
                    (p.y - (20.0 - g)).abs() <= 1e-9,
                    "r {r}: a bottom corner at {p:?}, not y = {}",
                    20.0 - g
                );
            }
        }
        assert_eq!(ends, 2, "r {r}: the bottom edge's two ends");
        // the box with its back face replaced by the round: bottom, top, front, the two
        // ends and the round, a box's 12 edges and 8 vertices (V - E + F = 2)
        assert_eq!(
            (
                b.face_list().len(),
                b.edge_list().len(),
                b.vertex_list().len()
            ),
            (6, 12, 8),
            "r {r}: faces, edges, vertices"
        );
        assert_valid(&mut k, &mut b, &format!("r {r}"));
    }
}
