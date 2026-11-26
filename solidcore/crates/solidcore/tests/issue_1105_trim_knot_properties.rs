//! Analytic Rust-helper regression for integration across narrow UV trim spans.
//! This calls `boundary_properties` directly; it is not a native C++ face comparison.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use solidcore::{
    curve::curve::Curve,
    geom::{extent2d::Extent2d, extent3d::Extent3d},
    mass::integrator::properties::boundary_properties,
    math::{Point2, Vec2, Vec3},
    nurbs::{b_spline_curve::p1::BSplineCurveForm, b_spline_surface_n_lib::p2::OrientType},
    primitive::api_curves::{create_canonical_curve, create_line_segment, CanonicalCurveParams},
    surface::{plane::Plane, surface::Surface},
    topology::face::p2::{PPF_ALL, PPF_AREA},
    Kernel,
};
#[test]
fn narrow_trim_feature_is_integrated_by_default() {
    let domain = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.1));
    let surface = Surface::from(Plane::new(
        Vec3::ZERO,
        Vec3::X,
        Vec3::Y,
        Vec2::new(1.0, 1.0),
        &domain,
    ));
    // Rectangle area 0.1 plus a triangular feature of area 0.005. Its UV
    // support is confined to t = 0.61..0.62 on the first trim.
    let pts = [
        Vec3::new(1.0, 0.1, 0.0),
        Vec3::new(0.39, 0.1, 0.0),
        Vec3::new(0.385, 1.1, 0.0),
        Vec3::new(0.38, 0.1, 0.0),
        Vec3::new(0.0, 0.1, 0.0),
    ];
    let trims = [
        Curve::from(
            create_canonical_curve(&CanonicalCurveParams {
                points: &pts,
                knots: &[0.0, 0.61, 0.615, 0.62, 1.0],
                knot_multiplicities: &[2, 1, 1, 1, 2],
                weights: None,
                degree: 1,
                form: BSplineCurveForm::Unspecified,
            })
            .unwrap(),
        ),
        Curve::from(create_line_segment(pts[4], Vec3::ZERO).unwrap()),
        Curve::from(create_line_segment(Vec3::ZERO, Vec3::X).unwrap()),
        Curve::from(create_line_segment(Vec3::X, pts[0]).unwrap()),
    ];
    let bbox = Extent3d::from_points(Vec3::ZERO, Vec3::new(1.0, 1.1, 0.0));
    let k = Kernel::default();
    for accuracy in [1e-10, 1e-4] {
        for flags in [PPF_AREA, PPF_ALL] {
            let properties = boundary_properties(
                &k,
                &surface,
                &trims,
                &domain,
                &bbox,
                OrientType::Same,
                accuracy,
                Vec3::ZERO,
                1.0,
                0.0,
                flags,
            )
            .unwrap();
            assert!(
                (properties.area - 0.105).abs() <= 1e-9 * 0.105,
                "accuracy={accuracy}, flags={flags}, properties={properties:?}"
            );
        }
    }
}
