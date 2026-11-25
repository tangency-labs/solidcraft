//! An edge's precise properties hold the accuracy they are asked for in every component,
//! against closed forms (#2887).
//!
//! `curve_properties` (`SmEdge::ComputePreciseProperties`) integrates 27 components along
//! the edge: its length, the area and volume of the tube of the cross-section radius
//! about it, and their eight moment vectors. Each step's error is held to
//! `accuracy * scale` per component. Only the length had a scale of its size; the 26
//! others had `BIG_DOUBLE`, so their steps were the length's and their accuracy whatever
//! that gave: an exact quarter circle's volume second moment missed 1e-9 by 2.3e-8 of
//! its value. Each component now has a scale of its own size.
//!
//! Each component is checked against its size: the length `L`, the tube's area
//! `2 pi r L` and volume `pi r^2 L`, and a moment of power `n` that measure times
//! `D^n`, where `D` bounds the distance of every point of the edge from the origin of
//! computation. A moment through zero (a centred static moment) is held to that size,
//! not to its own value.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::mass::integrator::properties::{curve_properties, EdgeProperties};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::Kernel;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// The moment groups' powers of length, in the order of `PreciseProperties::moments`:
/// area static, second, products, axial; then the same of the volume.
const POWERS: [i32; 8] = [1, 2, 2, 2, 1, 2, 2, 2];

/// The quarter circle of `radius` about `center` in the plane `z = center.z`, from
/// `+x` to `+y`: one exact rational quadratic span on `[0, 1]`.
fn quarter(center: Vec3, radius: f64) -> Curve {
    let w = FRAC_PI_4.cos();
    let (cx, cy, cz) = (center.x, center.y, center.z);
    let cpw = [
        CPoint::new(cx + radius, cy, cz, 1.0),
        CPoint::new((cx + radius) * w, (cy + radius) * w, cz * w, w),
        CPoint::new(cx, cy + radius, cz, 1.0),
    ];
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    Curve::from(BSplineCurve::from_data(3, 2, 5, 2, &cpw, &knots).unwrap())
}

/// The segment from `a` to `b` on `[0, 1]`.
fn segment(a: Vec3, b: Vec3) -> Curve {
    let cpw = [
        CPoint::new(a.x, a.y, a.z, 1.0),
        CPoint::new(b.x, b.y, b.z, 1.0),
    ];
    let knots = [0.0, 0.0, 1.0, 1.0];
    Curve::from(BSplineCurve::from_data(3, 1, 3, 1, &cpw, &knots).unwrap())
}

/// The line integrals `[1, x, y, z, xx, yy, zz, yz, zx, xy]` over the edge, `(x, y, z)`
/// the point less the origin of computation.
type LineIntegrals = [f64; 10];

/// [`LineIntegrals`] of [`quarter`]: with `(a, b, c) = center - origin`, `R` the radius
/// and `h = pi / 2`, the point is `(a + R cos t, b + R sin t, c)` and `ds = R dt`.
fn quarter_integrals(center: Vec3, radius: f64, origin: Vec3) -> LineIntegrals {
    let (a, b, c) = (
        center.x - origin.x,
        center.y - origin.y,
        center.z - origin.z,
    );
    let (r, h) = (radius, FRAC_PI_2);
    let x = r * (a * h + r);
    let y = r * (b * h + r);
    [
        r * h,
        x,
        y,
        r * c * h,
        r * (a * a * h + 2.0 * a * r + r * r * FRAC_PI_4),
        r * (b * b * h + 2.0 * b * r + r * r * FRAC_PI_4),
        r * c * c * h,
        c * y,
        c * x,
        r * (a * b * h + a * r + b * r + r * r / 2.0),
    ]
}

/// [`LineIntegrals`] of [`segment`]: `p(t) = p0 + t d`, `ds = |d| dt`, and the
/// integral over `[0, 1]` of `(p0_i + t d_i)(p0_j + t d_j)` is
/// `p0_i p0_j + (p0_i d_j + d_i p0_j) / 2 + d_i d_j / 3`.
fn segment_integrals(a: Vec3, b: Vec3, origin: Vec3) -> LineIntegrals {
    let p = [a.x - origin.x, a.y - origin.y, a.z - origin.z];
    let d = [b.x - a.x, b.y - a.y, b.z - a.z];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let first = |i: usize| l * (p[i] + d[i] / 2.0);
    let second = |i: usize, j: usize| {
        l * (p[i] * p[j] + (p[i] * d[j] + d[i] * p[j]) / 2.0 + d[i] * d[j] / 3.0)
    };
    [
        l,
        first(0),
        first(1),
        first(2),
        second(0, 0),
        second(1, 1),
        second(2, 2),
        second(1, 2),
        second(2, 0),
        second(0, 1),
    ]
}

/// The 27 components in `curve_properties`' order from the line integrals and the
/// cross-section radius `r`: the length, the tube's area and volume, then their
/// moment vectors.
fn components(li: &LineIntegrals, r: f64) -> [f64; 27] {
    let [l, x, y, z, xx, yy, zz, yz, zx, xy] = *li;
    let mut out = [0.0; 27];
    out[0] = l;
    for (k, measure) in [2.0 * PI * r, PI * r * r].into_iter().enumerate() {
        out[1 + k] = measure * l;
        let group = [
            [x, y, z],
            [xx, yy, zz],
            [yz, zx, xy],
            [yy + zz, xx + zz, xx + yy],
        ];
        for (g, v) in group.iter().enumerate() {
            for (c, value) in v.iter().enumerate() {
                out[3 + 12 * k + 3 * g + c] = measure * value;
            }
        }
    }
    out
}

/// The size each component is held to: the length, the tube's area and volume, and a
/// moment of power `n` its measure times `reach^n`, `reach` bounding the distance of
/// the edge's points from the origin.
fn sizes(length: f64, r: f64, reach: f64) -> [f64; 27] {
    let mut out = [0.0; 27];
    out[0] = length;
    let measures = [2.0 * PI * r * length, PI * r * r * length];
    out[1] = measures[0];
    out[2] = measures[1];
    for (m, &power) in POWERS.iter().enumerate() {
        let measure = measures[m / 4];
        for c in 0..3 {
            out[3 + 3 * m + c] = measure * reach.powi(power);
        }
    }
    out
}

fn flatten(p: &EdgeProperties) -> [f64; 27] {
    let mut out = [0.0; 27];
    out[0] = p.length;
    out[1] = p.properties.area;
    out[2] = p.properties.volume;
    for (m, v) in p.properties.moments.iter().enumerate() {
        out[3 + 3 * m] = v.x;
        out[4 + 3 * m] = v.y;
        out[5 + 3 * m] = v.z;
    }
    out
}

/// Every component of `curve`'s properties at each accuracy `10^-e`, `e` in `exponents`,
/// within the accuracy of its size.
fn holds(
    curve: &Curve,
    origin: Vec3,
    r: f64,
    exact: &[f64; 27],
    size: &[f64; 27],
    exponents: std::ops::RangeInclusive<i32>,
) {
    let k = Kernel::default();
    let interval = Extent1d::from_min_max(0.0, 1.0);
    for e in exponents {
        let accuracy = 10f64.powi(-e);
        let got = curve_properties(&k, curve, &interval, accuracy, origin, r)
            .unwrap_or_else(|err| panic!("at accuracy {accuracy:e}: {err:?}"));
        let got = flatten(&got);
        for i in 0..27 {
            let off = (got[i] - exact[i]).abs();
            assert!(
                off <= accuracy * size[i],
                "at accuracy {accuracy:e}: component {i} is {} where the closed form is {}, \
                 off by {:e} of its size {}",
                got[i],
                exact[i],
                off / size[i],
                size[i]
            );
        }
    }
}

/// The issue's case (difftest `review_edge_exact_quarter_circle_1e-09_[0, 1]_0`): the
/// unit quarter circle about the origin, a tube of radius 0.1, moments about
/// (0.37, -0.29, 0.41). Its volume second moment `x^2` missed 1e-9 by 2.3e-8 of its
/// value.
#[test]
fn an_exact_quarter_circle_holds_every_component() {
    let center = Vec3::new(0.0, 0.0, 0.0);
    let origin = Vec3::new(0.37, -0.29, 0.41);
    let r = 0.1;
    let curve = quarter(center, 1.0);
    let exact = components(&quarter_integrals(center, 1.0, origin), r);
    let reach = (center - origin).length() + 1.0;
    let size = sizes(FRAC_PI_2, r, reach);
    holds(&curve, origin, r, &exact, &size, 6..=11);
}

/// A larger arc off the origin of computation, its moments through zero in `y`:
/// radius 7 about (3, -2, 5), moments about (3, 2.5, -1).
#[test]
fn an_arc_off_the_origin_holds_every_component() {
    let center = Vec3::new(3.0, -2.0, 5.0);
    let origin = Vec3::new(3.0, 2.5, -1.0);
    let (radius, r) = (7.0, 0.4);
    let curve = quarter(center, radius);
    let exact = components(&quarter_integrals(center, radius, origin), r);
    let reach = (center - origin).length() + radius;
    let size = sizes(radius * FRAC_PI_2, r, reach);
    holds(&curve, origin, r, &exact, &size, 6..=11);
}

/// The same unit arc with the origin of computation 1e6 away: the moments, of size
/// up to 1e12 times the tube's, are held relative to their size, not the origin's
/// rounding.
#[test]
fn a_far_origin_holds_the_moments_to_their_size() {
    let center = Vec3::new(0.0, 0.0, 0.0);
    let origin = Vec3::new(1e6, -1e6, 1e6);
    let r = 0.1;
    let curve = quarter(center, 1.0);
    let exact = components(&quarter_integrals(center, 1.0, origin), r);
    let reach = (center - origin).length() + 1.0;
    let size = sizes(FRAC_PI_2, r, reach);
    holds(&curve, origin, r, &exact, &size, 6..=10);
}

/// A segment: every integrand is a polynomial of degree at most 2 in the parameter,
/// which each Cash-Karp step integrates exactly.
#[test]
fn a_segment_holds_every_component() {
    let (a, b) = (Vec3::new(-1.0, 2.0, 0.5), Vec3::new(4.0, -3.0, 2.5));
    let origin = Vec3::new(0.25, 0.5, -0.75);
    let r = 0.25;
    let curve = segment(a, b);
    let exact = components(&segment_integrals(a, b, origin), r);
    let reach = (a - origin).length().max((b - origin).length());
    let size = sizes((b - a).length(), r, reach);
    holds(&curve, origin, r, &exact, &size, 6..=11);
}

/// Without a cross-section the tube's area, volume and moments are zero, exactly; the
/// length still holds.
#[test]
fn a_wire_without_a_cross_section_has_zero_tube_properties() {
    let center = Vec3::new(0.0, 0.0, 0.0);
    let origin = Vec3::new(0.37, -0.29, 0.41);
    let curve = quarter(center, 1.0);
    let k = Kernel::default();
    let interval = Extent1d::from_min_max(0.0, 1.0);
    for e in 6..=11 {
        let accuracy = 10f64.powi(-e);
        let got = flatten(&curve_properties(&k, &curve, &interval, accuracy, origin, 0.0).unwrap());
        assert!(((got[0] - FRAC_PI_2) / FRAC_PI_2).abs() <= accuracy);
        assert!(got[1..].iter().all(|v| *v == 0.0), "{got:?}");
    }
}
