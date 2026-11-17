//! Regression for a closed C0-split spline whose outward offset loses a piece.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
use solidcore::curve::{
    composite_curve::{CompositeCurve, OffsetCornerType, OffsetParams},
    curve::Curve,
};
use solidcore::geom::{geom_utility::p2::ContinuityType, tol::ANG_TOL_DEG};
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::{
    math::{Point3, Vec3},
    Kernel,
};

fn source() -> BSplineCurve {
    source_at(0.0)
}
fn source_at(dx: f64) -> BSplineCurve {
    let points: [[f64; 2]; 20] = [
        [1.7719, 7.0417],
        [-0.4928, 8.4677],
        [-1.0873, 10.4847],
        [-0.6096, 12.8662],
        [-0.6096, 12.8662],
        [-0.1354, 15.2442],
        [1.4428, 16.8507],
        [3.4315, 18.1388],
        [3.4315, 18.1388],
        [2.3522, 19.6038],
        [1.0535, 20.1735],
        [-0.5813, 19.8904],
        [-0.5813, 19.8904],
        [-3.6847, 19.3561],
        [-5.8468, 15.9377],
        [-5.2523, 12.5301],
        [-5.2523, 12.5301],
        [-4.6472, 9.0339],
        [-1.5756, 6.5427],
        [1.7719, 7.0417],
    ];
    let points: Vec<_> = points
        .iter()
        .map(|p| Point3::new(p[0] + dx, p[1], 0.))
        .collect();
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[4, 4, 4, 4, 4, 4],
        knots: &[0., 1. / 6., 1. / 3., 0.5, 2. / 3., 1.],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap()
}
fn unmatched(k: &Kernel, pieces: &[solidcore::curve::composite_curve::OffsetPiece]) -> usize {
    let ends: Vec<_> = pieces
        .iter()
        .flat_map(|piece| {
            let iv = piece.curve.natural_interval();
            [
                piece.curve.evaluate_point(k, iv.min()).unwrap(),
                piece.curve.evaluate_point(k, iv.max()).unwrap(),
            ]
        })
        .collect();
    ends.iter()
        .filter(|point| {
            ends.iter()
                .filter(|other| point.distance(**other) < 1e-5)
                .count()
                < 2
        })
        .count()
}
fn run(angle: f64, trim: bool) -> (usize, usize) {
    let k = Kernel::default();
    let parts = source()
        .subdivide_at_discontinuity(&k, ContinuityType::C0, angle)
        .unwrap();
    let composite = CompositeCurve::with_curves(
        3,
        parts.into_iter().map(Curve::from).collect(),
        true,
        None,
        None,
    )
    .unwrap();
    let p = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: trim,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result =
        CompositeCurve::create_offsets_of_many_curves(&k, &[&composite], &[2], &p).unwrap();
    (result.offsets.len(), unmatched(&k, &result.offsets))
}
#[test]
fn closed_spline_offset_stays_closed_after_trimming() {
    let trimmed = run(ANG_TOL_DEG, true);
    assert!(trimmed.0 > 0);
    assert_eq!(
        trimmed.1, 0,
        "trimmed offset must remain closed: {trimmed:?}"
    );
}
#[test]
fn stricter_geometric_subdivision_keeps_this_result_closed() {
    let result = run(0.001, true);
    assert!(result.0 > 0);
    assert_eq!(result.1, 0, "{result:?}");
}
#[test]
fn inspect_offset_at_hidden_corner() {
    let k = Kernel::default();
    let parts = source()
        .subdivide_at_discontinuity(&k, ContinuityType::C0, ANG_TOL_DEG)
        .unwrap();
    let base = Curve::from(parts[1].clone());
    let interval = base.natural_interval();
    let curve = Curve::from(
        solidcore::curve::offset_curve::OffsetCurve::new(
            3,
            base,
            interval,
            Vec3::new(0., 0., 1.),
            1.,
            false,
        )
        .unwrap(),
    );
    let knot = 2. / 3.;
    let left = curve.evaluate(&k, knot, 0, true, true).unwrap()[0];
    let right = curve.evaluate(&k, knot, 0, false, true).unwrap()[0];
    eprintln!(
        "knot left={left:?}, right={right:?}, gap={}",
        left.distance(right)
    );
    assert!(left.distance(right) > 1e-5);
}

#[test]
fn splitting_inside_a_source_segment_preserves_original_provenance() {
    let k = Kernel::default();
    let parts = source()
        .subdivide_at_discontinuity(&k, ContinuityType::C0, ANG_TOL_DEG)
        .unwrap();
    assert_eq!(parts.len(), 2);
    let composite = CompositeCurve::with_curves(
        3,
        parts.into_iter().map(Curve::from).collect(),
        true,
        None,
        None,
    )
    .unwrap();
    let params = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result =
        CompositeCurve::create_offsets_of_many_curves(&k, &[&composite], &[2], &params).unwrap();
    let mut seen = [false; 2];
    for piece in result.offsets {
        let attr = piece.attribute.expect("offset map");
        assert_eq!(attr.composite_index, 0);
        assert!(attr.curve < 2, "{attr:?}");
        if attr.corner_offset {
            assert!(attr.other_curve < 2, "{attr:?}");
        } else {
            let origin = attr.originator.expect("source");
            assert_eq!(origin.key.0, attr.curve, "{attr:?}");
            seen[attr.curve as usize] = true;
        }
    }
    assert_eq!(seen, [true, true]);
}

#[test]
fn provenance_indices_are_preserved_across_multiple_composites() {
    let k = Kernel::default();
    let build = |dx| {
        let parts = source_at(dx)
            .subdivide_at_discontinuity(&k, ContinuityType::C0, ANG_TOL_DEG)
            .unwrap();
        CompositeCurve::with_curves(
            3,
            parts.into_iter().map(Curve::from).collect(),
            true,
            None,
            None,
        )
        .unwrap()
    };
    let a = build(0.0);
    let b = build(100.0);
    let params = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result =
        CompositeCurve::create_offsets_of_many_curves(&k, &[&a, &b], &[2, 2], &params).unwrap();
    assert_eq!(unmatched(&k, &result.offsets), 0);
    let mut seen = [[false; 2]; 2];
    for piece in result.offsets {
        let attr = piece.attribute.expect("offset map");
        assert!(attr.composite_index < 2 && attr.curve < 2, "{attr:?}");
        if attr.corner_offset {
            assert!(attr.other_curve < 2, "{attr:?}");
        } else {
            let origin = attr.originator.expect("source");
            assert_eq!(
                origin.key.0,
                attr.composite_index * 2 + attr.curve,
                "{attr:?}"
            );
            seen[attr.composite_index as usize][attr.curve as usize] = true;
        }
    }
    assert_eq!(seen, [[true; 2]; 2]);
}

#[test]
fn closed_reversed_spline_offsets_on_opposite_side() {
    let k = Kernel::default();
    let composite =
        CompositeCurve::with_curves(3, vec![Curve::from(source())], true, Some(&[false]), None)
            .unwrap();
    let params = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result =
        CompositeCurve::create_offsets_of_many_curves(&k, &[&composite], &[1], &params).unwrap();
    assert!(!result.offsets.is_empty());
    assert_eq!(unmatched(&k, &result.offsets), 0);
    for piece in result.offsets {
        assert_eq!(piece.attribute.unwrap().curve, 0);
    }
}

#[test]
fn ninth_shape_preserves_rounded_corners_and_closure() {
    let points: [[f64; 2]; 24] = [
        [-38.1649, -75.406],
        [-38.6462, -67.6893],
        [-34.7749, -63.0236],
        [-27.9666, -60.2257],
        [-27.9666, -60.2257],
        [-25.203, -59.0887],
        [-22.5349, -57.7016],
        [-19.8738, -56.3321],
        [-19.8738, -56.3321],
        [-18.9467, -55.8533],
        [-18.1682, -55.0837],
        [-17.549, -54.6166],
        [-17.549, -54.6166],
        [-21.1123, -56.2051],
        [-24.9341, -57.7585],
        [-28.6107, -59.6025],
        [-28.6107, -59.6025],
        [-30.9815, -60.794],
        [-33.4161, -62.1163],
        [-35.3411, -63.9009],
        [-35.3411, -63.9009],
        [-38.6851, -67.0053],
        [-39.1911, -71.0984],
        [-38.1649, -75.406],
    ];
    let points: Vec<_> = points.iter().map(|p| Point3::new(p[0], p[1], 0.)).collect();
    let spline = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 3,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[4, 4, 4, 4, 4, 4, 4],
        knots: &[0., 1. / 7., 2. / 7., 3. / 7., 4. / 7., 5. / 7., 1.],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let k = Kernel::default();
    let parts = spline
        .subdivide_at_discontinuity(&k, ContinuityType::C0, ANG_TOL_DEG)
        .unwrap();
    let comp = CompositeCurve::with_curves(
        3,
        parts.into_iter().map(Curve::from).collect(),
        true,
        None,
        None,
    )
    .unwrap();
    let params = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result =
        CompositeCurve::create_offsets_of_many_curves(&k, &[&comp], &[2], &params).unwrap();
    assert_eq!(unmatched(&k, &result.offsets), 0);
    assert!(result
        .offsets
        .iter()
        .any(|piece| piece.attribute.is_some_and(|a| a.corner_offset)));
}

#[test]
fn trimmed_offset_samples_do_not_enter_source_clearance() {
    use solidcore::curve::curve::p1::GlobalPointSolveOptions;
    use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
    use solidcore::surface::surface::p4::NoObject;
    let k = Kernel::default();
    let original = Curve::from(source());
    let parts = source()
        .subdivide_at_discontinuity(&k, ContinuityType::C0, ANG_TOL_DEG)
        .unwrap();
    let comp = CompositeCurve::with_curves(
        3,
        parts.into_iter().map(Curve::from).collect(),
        true,
        None,
        None,
    )
    .unwrap();
    let params = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result =
        CompositeCurve::create_offsets_of_many_curves(&k, &[&comp], &[2], &params).unwrap();
    assert!(!result.offsets.is_empty());
    for piece in result.offsets {
        let interval = piece.curve.natural_interval();
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let point = piece
                .curve
                .evaluate_point(&k, interval.evaluate(fraction))
                .unwrap();
            let distances = original
                .global_point_solve::<NoObject>(
                    &k,
                    &original.natural_interval(),
                    SolverOperation::Minimize,
                    point,
                    1e-7,
                    SolutionRequested::Single,
                    &GlobalPointSolveOptions::default(),
                )
                .unwrap();
            let nearest = distances
                .as_slice()
                .first()
                .expect("closest point")
                .start
                .solution_value;
            assert!(
                nearest >= 1.0 - params.xsect_tol_3d,
                "offset sample {point:?} is only {nearest} from the source"
            );
        }
    }
}

#[test]
fn degree_one_spline_square_offsets_each_corner() {
    let points = [[0., 0.], [10., 0.], [10., 10.], [0., 10.], [0., 0.]];
    let controls: Vec<_> = points.iter().map(|p| Point3::new(p[0], p[1], 0.)).collect();
    let spline = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 1,
        control_points: &controls,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 1, 1, 1, 2],
        knots: &[0., 0.25, 0.5, 0.75, 1.],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })
    .unwrap();
    let k = Kernel::default();
    let comp = CompositeCurve::with_curves(3, vec![Curve::from(spline)], true, None, None).unwrap();
    let p = OffsetParams {
        xsect_tol_3d: 1e-5,
        approx_tol_3d: 2.5e-6,
        plane_normal: Vec3::new(0., 0., 1.),
        corner: OffsetCornerType::FilletCorner,
        trim_results: true,
        offset_distance: 1.,
        match_parameterization: false,
    };
    let result = CompositeCurve::create_offsets_of_many_curves(&k, &[&comp], &[2], &p).unwrap();
    assert_eq!(result.offsets.len(), 8);
    assert_eq!(unmatched(&k, &result.offsets), 0);
    let mut corners = 0;
    for piece in &result.offsets {
        let attr = piece.attribute.expect("offset provenance");
        assert_eq!(attr.curve, 0);
        let interval = piece.curve.natural_interval();
        let mid = piece
            .curve
            .evaluate_point(&k, interval.evaluate(0.5))
            .unwrap();
        if attr.corner_offset {
            corners += 1;
            assert!(
                points[..4]
                    .iter()
                    .any(|q| ((mid.x - q[0]).hypot(mid.y - q[1]) - 1.).abs() < 1e-5),
                "corner midpoint {mid:?}"
            );
        } else {
            assert!(
                [
                    (mid.x + 1.).abs(),
                    (mid.x - 11.).abs(),
                    (mid.y + 1.).abs(),
                    (mid.y - 11.).abs()
                ]
                .into_iter()
                .any(|v| v < 1e-5),
                "edge midpoint {mid:?}"
            );
        }
    }
    assert_eq!(corners, 4);
}
