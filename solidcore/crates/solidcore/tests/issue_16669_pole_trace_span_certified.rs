//! #16669: the surface/surface trace accepted a span near a sphere's pole whose UV image
//! missed its 3D span by up to 0.66 (the UV Hermite's second control point at u = −18.5).
//! The five clamped samples of `TestSpanAccuracy` could not see it. Each traced span on the
//! ball is now certified before it is accepted.
//!
//! #17110: the same traces' UV trims zigzagged in u next to the pole (a 2D cusp,
//! `SmBSplineCurve` test 13) where the 3D curve runs one way round the axis; the image
//! certificate cannot see it, as a step in u there moves the image by only `ρ Δu`. The
//! trims' spans are now each monotone in u wherever the 3D span is monotone in longitude
//! (the trace's points are not moved, so a joint keeps any turn they have).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    missing_docs
)]

use solidcore::curve::curve::p2::CurveKind;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::intersect::surface_intersector::sphere_span::{
    sphere_bezier_image, sphere_bezier_sweep, SpanImage, SpanSweep,
};
use solidcore::math::det;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::surface::cone::Cone;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surface::p2::TsectCurveType;
use solidcore::surface::surface::Surface;
use solidcore::{Kernel, Point2, Vec3};

/// The curve's knots and its control polygon with every interior knot at full
/// multiplicity: span `j` is the cubic Bezier `pts[3j..=3j+3]` over `[knots[j], knots[j+1]]`.
fn bezier_spans(c: &BSplineCurve) -> (Vec<f64>, Vec<Vec3>) {
    let mut c = c.copy();
    let (knots, mults) = c.knots(None).unwrap();
    assert_eq!(c.degree().unwrap(), 3, "a traced curve is a cubic");
    let mut extra = Vec::new();
    for j in 1..knots.len() - 1 {
        for _ in mults[j]..3 {
            extra.push(knots[j]);
        }
    }
    if !extra.is_empty() {
        c.insert_knots(&extra).unwrap();
    }
    let (pts, w) = c.control_polygon().unwrap();
    assert!(w.is_empty(), "a traced curve is not rational");
    (knots, pts)
}

/// The r 6 ball about (0, 50 − 1e-6, 20), its seam turned by `turn` about z, against the
/// r 50 cylinder about the x axis: the intersection passes the ball's pole 1e-6 away and
/// crosses the seam δ/sin(turn) from it. Every span of every traced curve's UV image on
/// the ball is certified within the trace's tolerance of the 3D curve's span.
///
/// PROVEN for the Bezier control points as given: `sphere_bezier_image` is an
/// outward-rounded interval bound, not samples, and it bounds the image the evaluator gives,
/// with UV points clamped to the ball's domain. ARGUED: the control points come from knot
/// insertion in floating point, whose rounding (about 1e-16 relative) is negligible
/// against the tolerance of 0.017.
#[test]
fn every_traced_span_near_a_sphere_pole_maps_onto_its_3d_span() {
    let k = &Kernel::default();
    for turn in [0.01_f64, 0.03] {
        let (sn, cs) = det::sin_cos(turn);
        let ball = Surface::from(
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(
                    Vec3::new(0.0, 50.0 - 1e-6, 20.0),
                    Vec3::new(cs, sn, 0.0),
                    Vec3::new(-sn, cs, 0.0),
                ),
                6.0,
            )
            .unwrap(),
        );
        let wall = Surface::from(
            Cone::create_canonical_cylinder(
                k,
                &Axis2Placement::new(
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                ),
                50.0,
            )
            .unwrap(),
        );
        let bd = ball.natural_uv_domain_with_nurb(k).unwrap();
        let wd = wall.natural_uv_domain_with_nurb(k).unwrap();
        // the trace's default tolerance: (|P(min) P(mid)| + |P(mid) P(max)|) / 1000 on the
        // first surface's domain
        let p_min = ball.evaluate_point(k, bd.min()).unwrap();
        let p_mid = ball.evaluate_point(k, bd.evaluate(0.5, 0.5)).unwrap();
        let p_max = ball.evaluate_point(k, bd.max()).unwrap();
        let tol = (p_min.distance(p_mid) + p_mid.distance(p_max)) / 1000.0;

        let (out, _) = ball.global_surface_intersect_with_partial(
            k,
            &bd,
            &wall,
            &wd,
            [true, true],
            None,
            None,
        );
        assert!(!out.curves_3d.is_empty(), "turn {turn}: the surfaces meet");
        let mut spans = 0;
        for (i, (c3, uv)) in out
            .curves_3d
            .iter()
            .zip(&out.surface1_uv_curves)
            .enumerate()
        {
            // a touching point is a degenerate curve: no span to map
            if out.curve_types[i] == TsectCurveType::Touching {
                continue;
            }
            let uv = uv.as_ref().expect("a UV curve on the ball");
            let (CurveKind::BSpline(bu), CurveKind::BSpline(b3)) = (uv.kind(), c3.kind()) else {
                panic!("turn {turn} curve {i}: traced curves are B-splines");
            };
            let (ku, pu) = bezier_spans(bu);
            let (k3, p3) = bezier_spans(b3);
            assert_eq!(
                ku, k3,
                "turn {turn} curve {i}: the UV and 3D curves share knots"
            );
            for j in 0..ku.len() - 1 {
                let q = [pu[3 * j], pu[3 * j + 1], pu[3 * j + 2], pu[3 * j + 3]];
                let h = [p3[3 * j], p3[3 * j + 1], p3[3 * j + 2], p3[3 * j + 3]];
                let image = sphere_bezier_image(&ball, q, h, tol);
                assert!(
                    matches!(image, SpanImage::Certified(b) if b <= tol),
                    "turn {turn} curve {i} span {j} over [{}, {}]: {image:?} (tolerance {tol}), \
                     UV control points {:?}",
                    ku[j],
                    ku[j + 1],
                    q.iter().map(|p| Point2::new(p.x, p.y)).collect::<Vec<_>>(),
                );
                spans += 1;
            }
        }
        assert!(spans > 0, "turn {turn}: some spans were checked");
    }
}

/// #17110: the ball of the test above, its seam turned 0.01, 0.02 and 0.03 rad, against the
/// cylinder. Every span of every traced curve's UV trim on the ball is monotone in u
/// wherever its 3D span is monotone in longitude (`sphere_bezier_sweep` is not
/// `TurnsBack`). Before #17110 the turn 0.01 trace's curve from the seam crossing toward
/// the pole had a span whose u control points ran 0.998, 0.998, 0.715, 1.0.
///
/// PROVEN for the control points as given: `sphere_bezier_sweep` decides on outward-rounded
/// enclosures of the 3D span's longitude rate (a quintic's Bernstein coefficients) and of the
/// UV span's u derivative (a quadratic's), not samples.
#[test]
fn every_traced_trim_near_a_sphere_pole_is_monotone_in_u_where_its_longitude_is() {
    let k = &Kernel::default();
    let mut monotone = 0;
    for turn in [0.01_f64, 0.02, 0.03] {
        let (sn, cs) = det::sin_cos(turn);
        let ball = Surface::from(
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(
                    Vec3::new(0.0, 50.0 - 1e-6, 20.0),
                    Vec3::new(cs, sn, 0.0),
                    Vec3::new(-sn, cs, 0.0),
                ),
                6.0,
            )
            .unwrap(),
        );
        let wall = Surface::from(
            Cone::create_canonical_cylinder(
                k,
                &Axis2Placement::new(
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                ),
                50.0,
            )
            .unwrap(),
        );
        let bd = ball.natural_uv_domain_with_nurb(k).unwrap();
        let wd = wall.natural_uv_domain_with_nurb(k).unwrap();
        let (out, status) = ball.global_surface_intersect_with_partial(
            k,
            &bd,
            &wall,
            &wd,
            [true, true],
            None,
            None,
        );
        status.unwrap();
        assert!(!out.curves_3d.is_empty(), "turn {turn}: the surfaces meet");
        for (i, (c3, uv)) in out
            .curves_3d
            .iter()
            .zip(&out.surface1_uv_curves)
            .enumerate()
        {
            if out.curve_types[i] == TsectCurveType::Touching {
                continue;
            }
            let uv = uv.as_ref().expect("a UV curve on the ball");
            let (CurveKind::BSpline(bu), CurveKind::BSpline(b3)) = (uv.kind(), c3.kind()) else {
                panic!("turn {turn} curve {i}: traced curves are B-splines");
            };
            let (ku, pu) = bezier_spans(bu);
            let (_, p3) = bezier_spans(b3);
            for j in 0..ku.len() - 1 {
                let q = [pu[3 * j], pu[3 * j + 1], pu[3 * j + 2], pu[3 * j + 3]];
                let h = [p3[3 * j], p3[3 * j + 1], p3[3 * j + 2], p3[3 * j + 3]];
                let sweep = sphere_bezier_sweep(&ball, q, h);
                assert_ne!(
                    sweep,
                    SpanSweep::TurnsBack,
                    "turn {turn} curve {i} span {j} over [{}, {}]: UV control points {:?}",
                    ku[j],
                    ku[j + 1],
                    q.iter().map(|p| Point2::new(p.x, p.y)).collect::<Vec<_>>(),
                );
                monotone += usize::from(sweep == SpanSweep::Monotone);
            }
        }
    }
    assert!(monotone > 0, "some spans were constrained");
}
