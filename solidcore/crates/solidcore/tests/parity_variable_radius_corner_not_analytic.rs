//! C++ parity: `SmFilletCorner::MakeAnalyticCornerSurface` returns `FALSE` (no analytic
//! corner) when a fillet edge's solver is `SM_FS_VARIABLE_RADIUS`; `GetSolverType` is
//! virtual, so it is the concrete solver's type. The port asked the base solver of the
//! session's store, which is always `SM_FS_UNKNOWN`, so a variable-radius corner whose
//! curves fit a sphere (a constant law, or any corner small against the fit's tolerance)
//! was built as a sphere where the C++ builds the Coons patch.
//!
//! Here the three edges at a box corner rounded by a constant variable-radius law, all
//! upstream fixes off: the corner face is a B-spline patch, not a sphere.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_standard_solver::p1::{FilletLaw, FilletLawKind};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::face::p2 as face;
use solidcore::topology::{edge, EdgeId};
use solidcore::{Brep, Kernel};

/// The edges of `b` with an end at `p`.
fn edges_at(k: &Kernel, b: &Brep, p: Point3, tol: f64) -> Vec<EdgeId> {
    b.edges()
        .ids()
        .filter(|&e| {
            let ivl = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let s = c.evaluate_point(k, ivl.min()).unwrap();
            let t = c.evaluate_point(k, ivl.max()).unwrap();
            s.distance(p) < tol || t.distance(p) < tol
        })
        .collect()
}

/// A box of side `size` with the three edges at its origin corner rounded by the constant
/// variable-radius law `radius`, with the default kernel.
fn round_corner(size: f64, radius: f64) -> Brep {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), size, size, size).unwrap();
    let edges = edges_at(&k, &b, Point3::new(0.0, 0.0, 0.0), 1e-3 * size);
    assert_eq!(edges.len(), 3);
    let law = FilletLaw::new(FilletLawKind::Linear {
        start_radius: radius,
        end_radius: radius,
    });
    {
        let mut session = FilletSession::new(&mut k, &mut b).unwrap();
        session
            .prepare_variable_radius_edges(&mut k, &edges, radius, law, 1, 1)
            .unwrap();
        session.do_fillet(&mut k).unwrap();
    }
    b
}

/// The variable-radius corner is the Coons patch (a B-spline face), not an analytic sphere,
/// at both sizes: `MakeAnalyticCornerSurface` asks the concrete solver's `GetSolverType`.
#[test]
fn variable_radius_corner_is_not_analytic() {
    for (size, radius) in [(10.0, 1.0), (0.01, 1e-3)] {
        let b = round_corner(size, radius);
        let kinds: Vec<_> = b
            .faces()
            .ids()
            .filter_map(|f| face::surface(&b, f).unwrap())
            .filter(|s| !matches!(s.kind(), SurfaceKind::Plane(_)))
            .map(|s| matches!(s.kind(), SurfaceKind::Sphere(_)))
            .collect();
        assert_eq!(kinds.len(), 4, "size {size}: three rounds and the corner");
        assert!(
            !kinds.iter().any(|&sphere| sphere),
            "size {size}: no sphere corner"
        );
    }
}
