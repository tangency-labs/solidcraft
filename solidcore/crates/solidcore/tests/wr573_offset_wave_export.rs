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
//! upstream returns success with no surface. solidcore raises the starter's inner knots to
//! full multiplicity first (WR-573, as the fork's tangency-labs/smlib@eae96cb), and checks
//! every row of patches of a pass against the tolerance (WR-726).
//!
//! Reference: the pinned C++ (v1.0.1) through `SmApiCreateLinearSweep`,
//! `SmApiOffsetBrepFull` and `ApproximateSurface` gives success with no surface for both
//! calls; with eae96cb's `SmSurface.cpp` linked in, both give a 33 x 4 surface with an
//! achieved tolerance of 1.1245747026548022e-05, above the requested 5e-6: its last row of
//! patches is within the tolerance, an earlier one is not (WR-726).
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

/// WR-573: the writer's calls give an approximating B-spline surface. #2507 (WR-726): a
/// pass whose earlier rows of patches miss the tolerance is not accepted, so the pass the
/// C++ with eae96cb accepts (33 x 4 at 2.25 times the tolerance) is subdivided once more,
/// and the result is within the tolerance.
#[test]
fn with_every_row_checked_the_fit_is_within_the_tolerance_2507() {
    let mut k = Kernel::default();
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
        assert_eq!(b.number_control_points(SurfParamType::U), 34);
        assert_eq!(b.number_control_points(SurfParamType::V), 4);
        assert!(
            (0.0..=tol).contains(&r.achieved_tol),
            "({subdivisions}, {copy}): achieved {} > requested {tol}",
            r.achieved_tol
        );
        // pinned (the kernel is deterministic)
        let want = 4.034_260_017_062_413e-6;
        assert!(
            (r.achieved_tol - want).abs() <= 1e-9 * want,
            "({subdivisions}, {copy}): {}",
            r.achieved_tol
        );
        let bound = 2.0 * tol;
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
