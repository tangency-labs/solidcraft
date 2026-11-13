//! One open profile swept along two rails (`sweep::loft_rails::sweep_two_rails`): at each
//! station of the rails' common parameter the profile is placed with its ends on the two rails,
//! the chord between them its across direction and size. A line between two parallel line rails
//! sweeps a planar strip exactly; diverging line rails scale the profile in closed form (Scale)
//! or only across (Stretch); a curved pair keeps the profile's ends on the rails within the
//! certified bound; a closed profile is refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::query::api_queries::curve_closest_point;
use solidcore::sweep::loft_rails::{sweep_two_rails, RailLoftOptions};
use solidcore::sweep::stations::RailFit;
use solidcore::topology::{Brep, EdgeId};
use solidcore::{Kernel, SolidError};

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn line(a: Point3, b: Point3) -> Curve {
    Curve::from(create_line_segment(a, b).expect("line"))
}

/// The cubic Bezier on `points`.
fn cubic(points: [Point3; 4]) -> Curve {
    Curve::from(
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 3,
            control_points: &points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[4, 4],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .expect("cubic"),
    )
}

/// A bump across x from 0 to 10, rising to z = 1.5 (the cubic on (0, 0, 0), (3, 0, 2),
/// (7, 0, 2), (10, 0, 0)).
const BUMP: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [3.0, 0.0, 2.0],
    [7.0, 0.0, 2.0],
    [10.0, 0.0, 0.0],
];

fn bump() -> Curve {
    cubic(BUMP.map(|[x, y, z]| p(x, y, z)))
}

fn opts(tolerance: f64, fit: RailFit) -> RailLoftOptions {
    let mut o = RailLoftOptions::new(tolerance);
    o.fit = fit;
    o
}

/// The edge of `b` between the points `a` and `z` (either way round).
fn edge_between(b: &Brep, a: Point3, z: Point3) -> EdgeId {
    let at = |v, q: Point3| (b.vertex(v).expect("vertex").point - q).length() < 1e-9;
    *b.edge_list()
        .iter()
        .find(|&&e| {
            let [s, t] = b.edge(e).expect("edge").vertices;
            (at(s, a) && at(t, z)) || (at(s, z) && at(t, a))
        })
        .unwrap_or_else(|| panic!("no edge from {a:?} to {z:?}"))
}

/// Every point of `want` lies on edge `e`'s curve, to `tol`.
fn on_edge(k: &Kernel, b: &Brep, e: EdgeId, want: &[Point3], tol: f64) {
    let c = b.edge(e).expect("edge").curve.as_ref().expect("curve");
    for q in want {
        let d = curve_closest_point(k, c, *q)
            .expect("closest")
            .closest
            .distance;
        assert!(d <= tol, "{q:?} is {d} off the edge");
    }
}

/// The bump at the cubic's parameter `u`, its x scaled by `kx`, its z by `kz`, at y = `y`.
fn bump_at(u: f64, kx: f64, kz: f64, y: f64) -> Point3 {
    let b = [
        (1.0 - u).powi(3),
        3.0 * u * (1.0 - u).powi(2),
        3.0 * u * u * (1.0 - u),
        u.powi(3),
    ];
    let mut q = p(0.0, y, 0.0);
    for (w, c) in b.iter().zip(BUMP) {
        q.x += w * c[0] * kx;
        q.z += w * c[2] * kz;
    }
    q
}

#[test]
fn a_line_between_parallel_line_rails_sweeps_a_planar_strip_exactly() {
    let mut k = Kernel::default();
    let profile = line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0));
    let r1 = line(p(0.0, 0.0, 0.0), p(0.0, 20.0, 0.0));
    let r2 = line(p(10.0, 0.0, 0.0), p(10.0, 20.0, 0.0));
    let made = sweep_two_rails(&mut k, &[&profile], [&r1, &r2], &opts(1e-6, RailFit::Scale))
        .expect("swept");
    assert_eq!(made.bounds.len(), 2);
    for b in &made.bounds {
        assert!(*b <= 1e-9, "the strip is exact: {b}");
    }
    let b = &made.skin.brep;
    for &v in b.vertex_list() {
        assert!(b.vertex(v).expect("vertex").point.z.abs() <= 1e-12);
    }
    let far = edge_between(b, p(0.0, 20.0, 0.0), p(10.0, 20.0, 0.0));
    on_edge(&k, b, far, &[p(2.5, 20.0, 0.0), p(7.5, 20.0, 0.0)], 1e-9);
}

#[test]
fn diverging_line_rails_scale_the_profile_in_closed_form() {
    // the chord grows from 10 to 20: at the far end the bump is twice as wide, and with Scale
    // twice as high, with Stretch as high as it was
    let r1 = line(p(0.0, 0.0, 0.0), p(0.0, 20.0, 0.0));
    let r2 = line(p(10.0, 0.0, 0.0), p(20.0, 20.0, 0.0));
    for (fit, kz) in [(RailFit::Scale, 2.0), (RailFit::Stretch, 1.0)] {
        let mut k = Kernel::default();
        let profile = bump();
        let made = sweep_two_rails(&mut k, &[&profile], [&r1, &r2], &opts(1e-6, fit))
            .unwrap_or_else(|e| panic!("{fit:?}: {e}"));
        for b in &made.bounds {
            assert!(*b <= 1e-6, "{fit:?}: {b}");
        }
        let b = &made.skin.brep;
        let far = edge_between(b, p(0.0, 20.0, 0.0), p(20.0, 20.0, 0.0));
        let want: Vec<Point3> = (0..=10)
            .map(|i| bump_at(f64::from(i) / 10.0, 2.0, kz, 20.0))
            .collect();
        on_edge(&k, b, far, &want, 1e-9);
    }
}

#[test]
fn a_curved_pair_keeps_the_profile_on_the_rails_within_the_bound() {
    let mut k = Kernel::default();
    let profile = bump();
    let r1 = cubic([
        p(0.0, 0.0, 0.0),
        p(0.0, 7.0, 1.0),
        p(-1.0, 14.0, -1.0),
        p(0.0, 20.0, 0.0),
    ]);
    let r2 = cubic([
        p(10.0, 0.0, 0.0),
        p(12.0, 7.0, 0.0),
        p(13.0, 14.0, 1.0),
        p(12.0, 20.0, 0.0),
    ]);
    let tol = 1e-3;
    let made = sweep_two_rails(&mut k, &[&profile], [&r1, &r2], &opts(tol, RailFit::Scale))
        .expect("swept");
    assert_eq!(made.bounds.len(), 2);
    for b in &made.bounds {
        assert!(*b <= tol, "{b}");
    }
    assert!(made.wires >= 3);
    // the far ends of the rails are corners of the skin
    let b = &made.skin.brep;
    edge_between(b, p(0.0, 20.0, 0.0), p(12.0, 20.0, 0.0));
}

#[test]
fn a_closed_profile_is_refused() {
    let mut k = Kernel::default();
    let square = [
        line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)),
        line(p(10.0, 0.0, 0.0), p(10.0, 0.0, 10.0)),
        line(p(10.0, 0.0, 10.0), p(0.0, 0.0, 10.0)),
        line(p(0.0, 0.0, 10.0), p(0.0, 0.0, 0.0)),
    ];
    let refs: Vec<&Curve> = square.iter().collect();
    let r1 = line(p(0.0, 0.0, 0.0), p(0.0, 20.0, 0.0));
    let r2 = line(p(10.0, 0.0, 0.0), p(10.0, 20.0, 0.0));
    assert!(matches!(
        sweep_two_rails(&mut k, &refs, [&r1, &r2], &opts(1e-6, RailFit::Scale)),
        Err(SolidError::Unsupported { .. })
    ));
    // and a profile asked to close
    let profile = line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0));
    let mut closed = opts(1e-6, RailFit::Scale);
    closed.closed = true;
    assert!(matches!(
        sweep_two_rails(&mut k, &[&profile], [&r1, &r2], &closed),
        Err(SolidError::Unsupported { .. })
    ));
}
