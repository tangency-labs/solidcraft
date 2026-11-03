//! WR-573 (tangency-labs/solidcore#1501) at operation level: the offset of a sheet on a
//! cubic B-spline wave with simple inner knots cannot be exported as B-splines.
//!
//! The wave (6 control points, knots 0 0 0 0 1/3 2/3 1 1 1 1) is swept 10 along +y
//! (`create_linear_sweep`) and offset by 0.5 (`SmApiOffsetBrepFull`). The moved face lies on
//! an `OffsetSurface`, so a B-spline writer approximates it with
//! `SmSurface::ApproximateSurface`: `SmBrepData`'s `bWriteAsBSplines` writer with the B-rep's
//! approximation tolerance, 3 subdivisions and no copy, the flat export (#1441) with 5
//! subdivisions and B-splines copied. The starter is the root B-spline of the swept surface,
//! with simple inner knots; its cubic Bezier patches run past its control points, and
//! upstream returns success with no surface. `Kernel::fixes.approximate_surface_bezier_starter`
//! (the fork's tangency-labs/smlib@eae96cb) raises the starter's inner knots to full
//! multiplicity first.
//!
//! Reference: the pinned C++ (v1.0.1) through `SmApiCreateLinearSweep`,
//! `SmApiOffsetBrepFull` and `ApproximateSurface` gives success with no surface for both
//! calls; with eae96cb's `SmSurface.cpp` linked in, both give a 33 x 4 surface with an
//! achieved tolerance of 1.1245747026548022e-05.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use solidcore::curve::curve::Curve;
use solidcore::geom::tol::{brep_approx_tol_3d, SmallTopoType};
use solidcore::geom::vector3d::Vec3;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::brep_data_write::bspline_surface_part;
use solidcore::{Brep, Kernel};

/// The achieved tolerance of the pinned C++ with eae96cb's fix, for both calls.
const CPP_FIXED_ACHIEVED_TOL: f64 = 1.124_574_702_654_802_2e-5;

/// (subdivisions, copy B-splines): `SmBrepData`'s writer, then the flat export.
const CALLS: [(usize, bool); 2] = [(3, false), (5, true)];

/// #1501's sheet offset by 0.5: one face, on an offset surface with no B-spline part.
fn offset_wave(k: &mut Kernel) -> Brep {
    let pts = [
        (0.0, 0.0),
        (2.0, 0.4),
        (4.0, -0.4),
        (6.0, 0.4),
        (8.0, -0.4),
        (10.0, 0.0),
    ]
    .map(|(x, z)| Vec3::new(x, 0.0, z));
    let wave = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &pts,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[4, 1, 1, 4],
        knots: &[0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let wave = Curve::from(wave);
    let mut sheet =
        sweeps::create_linear_sweep(k, &[Some(&wave)], Vec3::new(0.0, 1.0, 0.0), 10.0, false)
            .unwrap();
    let moved = offset_fixture::offset_full(k, &mut sheet, 0.5).unwrap();
    assert_eq!(moved.face_list().len(), 1);
    let face = moved.face(moved.face_list()[0]).unwrap();
    let surface = face.surface.as_ref().unwrap();
    assert!(
        matches!(surface.kind(), SurfaceKind::OffsetSurface(_)),
        "the moved face is not on an offset surface"
    );
    assert!(bspline_surface_part(surface).unwrap().is_none());
    moved
}

/// The defect WR-573 is about, pinned while the fix is opt-in: without the flag the
/// writer's call succeeds with nothing to write. This test goes away when the flag becomes
/// the default.
#[test]
fn without_the_flag_the_writer_succeeds_with_nothing_wr573() {
    let mut k = Kernel::default();
    assert!(!k.fixes.approximate_surface_bezier_starter);
    let moved = offset_wave(&mut k);
    let face = moved.face(moved.face_list()[0]).unwrap();
    let surface = face.surface.as_ref().unwrap();
    let tol = brep_approx_tol_3d(&k, &moved, SmallTopoType::NormalSize);
    for (subdivisions, copy) in CALLS {
        // as the pinned C++: success, no surface, nothing tested
        let r = surface
            .approximate_surface(&k, tol, subdivisions, copy)
            .unwrap();
        assert!(r.surface.is_none(), "({subdivisions}, {copy})");
        assert_eq!(r.achieved_tol, -1.0, "({subdivisions}, {copy})");
    }
}

#[test]
fn the_flag_approximates_the_offset_face() {
    let mut k = Kernel::default();
    k.fixes.approximate_surface_bezier_starter = true;
    let moved = offset_wave(&mut k);
    let face = moved.face(moved.face_list()[0]).unwrap();
    let surface = face.surface.as_ref().unwrap();
    let tol = brep_approx_tol_3d(&k, &moved, SmallTopoType::NormalSize);
    for (subdivisions, copy) in CALLS {
        let r = surface
            .approximate_surface(&k, tol, subdivisions, copy)
            .unwrap();
        let nurbs = r.surface.expect("an approximating B-spline surface");
        let SurfaceKind::BSpline(b) = nurbs.kind() else {
            panic!("({subdivisions}, {copy}): not a B-spline");
        };
        assert_eq!(b.number_control_points(SurfParamType::U), 33);
        assert_eq!(b.number_control_points(SurfParamType::V), 4);
        // The C++ with the fix gives the same. It is above `tol` (5e-6): upstream accepts a
        // pass when its last row of patches is within `tol`, and the achieved tolerance is
        // the largest deviation over the whole pass (as the record says for its 1e-3 case).
        assert!(
            (r.achieved_tol - CPP_FIXED_ACHIEVED_TOL).abs() <= 1e-9 * CPP_FIXED_ACHIEVED_TOL,
            "({subdivisions}, {copy}): {}",
            r.achieved_tol
        );
        // over the face's domain the NURBS stays near the offset surface (upstream samples
        // other points than this grid, hence the extra `tol`)
        let bound = r.achieved_tol + tol;
        let domain = face.uv_domain;
        for i in 0..=20 {
            for j in 0..=20 {
                let uv = domain.evaluate(f64::from(i) / 20.0, f64::from(j) / 20.0);
                let a = surface.evaluate_point(&k, uv).unwrap();
                let p = nurbs.evaluate_point(&k, uv).unwrap();
                let gap = a.distance(p);
                assert!(gap <= bound, "({i}, {j}): gap {gap} > {bound}");
                assert!((-bound..=10.0 + bound).contains(&p.y), "({i}, {j}): {p:?}");
            }
        }
    }
}
