//! WR-382 through public wire construction and whole-body checking (#1954).
//!
//! The audit pinned only the curve helper. Here the body checker must reject a
//! regular wire whose minimum curvature radius is below tolerance, while accepting
//! a healthy wire. Only `bspline_check_min_radius` differs between the two kernels.
#![allow(clippy::unwrap_used)]

use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::topology::brep::new_brep;
use solidcore::topology::brep::p5::create_wire_edge_from_curve;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel, Vec3};

fn wire(k: &mut Kernel, width: f64) -> Brep {
    // C(t) = (width*(t-1/2), (t-1/2)^2, 0), 0 <= t <= 1.
    // Speed at the vertex is width, not zero; rho_min = width^2/2.
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 2,
        control_points: &[
            Vec3::new(-width / 2.0, 0.25, 0.0),
            Vec3::new(0.0, -0.25, 0.0),
            Vec3::new(width / 2.0, 0.25, 0.0),
        ],
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[3, 3],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let mut b = new_brep(k, 1.0, 1e6).unwrap();
    create_wire_edge_from_curve(
        k,
        &mut b,
        Curve::from(curve),
        Extent1d::from_min_max(0.0, 1.0),
    )
    .unwrap();
    assert_eq!(b.edge_list().len(), 1);
    assert_eq!(b.vertex_list().len(), 2);
    b
}

#[test]
fn whole_body_check_rejects_only_the_under_radius_wire_with_wr382() {
    for width in [0.001, 0.02] {
        let mut k = Kernel::default();
        let mut upstream_body = wire(&mut k, width);
        let upstream = check_brep(&mut k, &mut upstream_body, CheckLevel::Level2).unwrap();
        assert!(upstream.is_valid(), "{upstream:?}");
        assert_eq!(upstream.assert_valid, Some(true));
        assert_eq!(upstream.failure_count(), 0);

        let mut fixed_body = wire(&mut k, width);
        k.fixes.bspline_check_min_radius = true;
        let fixed = check_brep(&mut k, &mut fixed_body, CheckLevel::Level2).unwrap();
        assert!(fixed.pointer_reports.is_empty(), "{fixed:?}");
        if width == 0.001 {
            assert!(!fixed.is_valid(), "{fixed:?}");
            assert_eq!(fixed.assert_valid, Some(false));
            let failures: Vec<_> = fixed.failures().collect();
            assert_eq!(failures.len(), 1, "{fixed:?}");
            let report = failures.first().unwrap();
            assert_eq!(report.reporting_type, BSplineCurve::TYPE);
            assert_eq!(report.test_index, 14);
            let radius = width * width / 2.0;
            assert!((report.value - radius).abs() <= 1e-9 * radius, "{report:?}");
            assert!((report.owner_param.x - 0.5).abs() <= 1e-9, "{report:?}");
            assert!(report.value < report.tol3d);
        } else {
            assert!(fixed.is_valid(), "{fixed:?}");
            assert_eq!(fixed.failure_count(), 0);
        }
    }
}
