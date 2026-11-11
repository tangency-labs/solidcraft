//! WR-450 (#387; the fork's tangency-labs/smlib@7af1987, was 276b509) through a boolean
//! (#1954): a box minus a cylinder given as two rational quadratic half-cylinder faces of
//! four spans each. Splitting a face copies its surface for the new face with
//! `SmBSplineSurface::CopyAndAddAnalytics`; upstream's copy is the recognised analytic
//! cylinder in its own parameterization, so the kept UV trims point off their edges and
//! the edges grow their tolerances to cover it. With
//! `Kernel::fixes.analytic_copy_keeps_parameterization` the copy stays exact where the
//! analytic form differs, and the result is valid.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::boolean::api_brep::boolean_difference;
use solidcore::curve::curve::Curve;
use solidcore::geom::assert_array::labels;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::p1::BSplineCurveForm;
use solidcore::primitive::api_curves::{create_canonical_curve, CanonicalCurveParams};
use solidcore::primitive::api_primitives::create_box;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::edge;
use solidcore::{Brep, Kernel};

/// #387's half circle (centre (0, 50), r 40) as SolidCraft's record has it: a rational
/// quadratic in four equal spans, weights 1, .75, .5, .5, .75, 1 (the same control points
/// as `nurbs::b_spline_surface::p2`'s unit test).
const HALF: [(f64, f64); 6] = [
    (-32.714_727_726_319_09, 26.983_775_509_593_045),
    (-25.042_652_896_183_43, 16.078_866_267_486_678),
    (6.658_860_627_247_413, 5.777_160_028_477_432),
    (39.373_588_353_566_504, 28.793_384_518_884_38),
    (40.386_802_556_454_75, 62.111_315_248_300_59),
    (32.714_727_726_319_09, 73.016_224_490_406_96),
];

/// One half circle at height `z`; `other` turns it a half turn about the centre, so the
/// two make the whole circle.
fn half_circle(z: f64, other: bool) -> Curve {
    let points: Vec<Point3> = HALF
        .iter()
        .map(|&(x, y)| {
            if other {
                Point3::new(-x, 100.0 - y, z)
            } else {
                Point3::new(x, y, z)
            }
        })
        .collect();
    Curve::from(
        create_canonical_curve(&CanonicalCurveParams {
            points: &points,
            knots: &[0.0, 0.25, 0.5, 0.75, 1.0],
            knot_multiplicities: &[3, 1, 1, 1, 3],
            weights: Some(&[1.0, 0.75, 0.5, 0.5, 0.75, 1.0]),
            degree: 2,
            form: BSplineCurveForm::Unspecified,
        })
        .unwrap(),
    )
}

/// The z cylinder at (0, 50), r 40, z -60 to 60: a capped degree-1 skin between the two
/// circles, so its two side faces are rational B-spline surfaces in the curves' own
/// parameterization, as the record's faces are.
fn rational_cylinder(k: &mut Kernel) -> Brep {
    let curves = [
        half_circle(-60.0, false),
        half_circle(-60.0, true),
        half_circle(60.0, false),
        half_circle(60.0, true),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_skin_primitive(k, &[2, 2], &refs, 1, true)
        .unwrap()
        .brep
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The box (-14, 0, -14)-(14, 20, 14) less the disc y >= 50 - sqrt(1600 - x^2), over
/// its 28 of z: 28 * 20 * 28 - 28 * (integral of sqrt(1600 - x^2) - 30 over -14..14).
fn closed_form() -> f64 {
    let circle = 14.0 * 1404.0_f64.sqrt() + 1600.0 * 0.35_f64.asin();
    28.0 * 20.0 * 28.0 - 28.0 * (circle - 30.0 * 28.0)
}

#[test]
fn wr_450_box_minus_rational_cylinder_is_valid_only_with_the_fix() {
    let want = closed_form();
    for fix in [false, true] {
        let mut k = Kernel::default();
        k.fixes.analytic_copy_keeps_parameterization = fix;
        let cylinder = rational_cylinder(&mut k);
        let block = create_box(&mut k, Point3::new(-14.0, 0.0, -14.0), 28.0, 20.0, 28.0).unwrap();
        let mut r = boolean_difference(&mut k, block, cylinder).unwrap();

        // the same body either way: six faces and the closed-form volume
        assert_eq!(r.face_list().len(), 6, "fix {fix}");
        let got = volume(&k, &mut r);
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "fix {fix}: volume {got}, closed form {want}"
        );

        let worst_edge = r
            .edge_list()
            .iter()
            .map(|&e| edge::tolerance(&r, e).unwrap())
            .fold(0.0, f64::max);
        let check = check_brep(&mut k, &mut r, CheckLevel::Level2).unwrap();
        let failures: Vec<_> = check
            .failures()
            .map(|f| (f.reporting_type, f.test_index))
            .collect();
        eprintln!(
            "fix {fix}: volume {got}, worst edge tolerance {worst_edge}, body {}, {:?}",
            r.tolerance(),
            check.lines()
        );
        if fix {
            // the split faces keep the exact surface, so the trims lie on their edges
            assert!(worst_edge <= r.tolerance(), "{worst_edge}");
            assert!(check.is_valid(), "{:?}", check.lines());
            assert_eq!(check.assert_valid, Some(true));
        } else {
            // upstream: the copies' other parameterization puts the trims up to 0.78 off
            // their edges, which take tolerances to cover it
            assert!(worst_edge > 0.5, "{worst_edge}");
            assert!(!check.is_valid());
            assert_eq!(check.assert_valid, None, "ValidatePointers fails");
            assert_eq!(
                failures,
                [
                    (labels::BREP_TYPE, 59),
                    (labels::BREP_TYPE, 82),
                    (labels::BREP_TYPE, 82)
                ],
                "{:?}",
                check.lines()
            );
        }
    }
}
