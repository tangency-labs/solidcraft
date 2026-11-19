//! Compatible rational boundary rows blended into a bounded Coons tensor net.
//! Boundary rows are used without fitting; bottom/top controls own tolerated corner joins.
use crate::error::nurbs::NurbsFailure;
use crate::{Result, SolidError, Vec3};

pub mod basis;
pub mod quad_cap;
pub mod rechart;
pub mod regularity;

const OP: &str = "geom::form_fill::coons";
const MAX_POINTS: usize = 1 << 20;
const CORNER_JOIN_MM: f64 = 1e-9;
const CORNER_WEIGHT_ULPS: f64 = 256.0 * f64::EPSILON;

/// Cartesian net plus its separately retained positive weights.
pub struct CoonsNet {
    /// U-varying-fastest Cartesian controls.
    pub points: Vec<Vec3>,
    /// Positive homogeneous weights.
    pub weights: Vec<f64>,
    /// Whether any weight differs from one.
    pub rational: bool,
}

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

fn reserved<T>(count: usize) -> Result<Vec<T>> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| SolidError::nurbs(OP, NurbsFailure::Memory))?;
    Ok(output)
}

fn greville(degree: usize, knots: &[f64]) -> Result<Vec<f64>> {
    let count = basis::validate_space(degree, knots)?;
    if knots.first() != Some(&0.0) || knots.last() != Some(&1.0) {
        return Err(bad(
            "Coons knot domains must be normalized to the unit interval",
        ));
    }
    let mut stations = reserved(count)?;
    for i in 0..count {
        let row = knots
            .get(i + 1..=i + degree)
            .ok_or_else(|| bad("incomplete Coons knot support"))?;
        let value = row.iter().sum::<f64>() / degree as f64;
        if !value.is_finite() {
            return Err(bad("nonfinite Coons station"));
        }
        stations.push(value);
    }
    Ok(stations)
}

fn blend(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let mut output = [0.; 4];
    for (out, (a, b)) in output.iter_mut().zip(a.into_iter().zip(b)) {
        *out = a * (1. - t) + b * t;
    }
    output
}

fn compatible_corner(a: [f64; 4], b: [f64; 4]) -> bool {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    Vec3::new(ax / aw, ay / aw, az / aw).distance(Vec3::new(bx / bw, by / bw, bz / bw))
        <= CORNER_JOIN_MM
        && (aw - bw).abs() <= CORNER_WEIGHT_ULPS * aw.max(bw)
}

/// Blend bottom, top, left, right rows running with their normalized parameters.
/// Both knot domains must be [0,1], and every supplied homogeneous control must
/// have a finite Cartesian projection and a weight greater than `f64::MIN_POSITIVE`.
/// Paired corner positions may differ by at most 1e-9 model units, with relative
/// weight disagreement at most `256*f64::EPSILON`. Bottom/top own such corner joins,
/// preserving the existing directed fill policy; larger disagreements are refused.
pub fn coons(degree: [usize; 2], knots: [&[f64]; 2], sides: [&[[f64; 4]]; 4]) -> Result<CoonsNet> {
    let [ku, kv] = knots;
    let [pu, pv] = degree;
    let u = greville(pu, ku)?;
    let v = greville(pv, kv)?;
    let [bottom, top, left, right] = sides;
    if [bottom.len(), top.len()] != [u.len(); 2]
        || [left.len(), right.len()] != [v.len(); 2]
        || sides.iter().any(|row| {
            row.iter().any(|&[x, y, z, w]| {
                ![x, y, z, w].into_iter().all(f64::is_finite)
                    || w <= f64::MIN_POSITIVE
                    || ![x / w, y / w, z / w].into_iter().all(f64::is_finite)
            })
        })
    {
        return Err(bad("Coons rows do not match their knot spaces"));
    }
    let count = u
        .len()
        .checked_mul(v.len())
        .filter(|&n| n <= MAX_POINTS)
        .ok_or_else(|| bad("Coons tensor exceeds its control budget"))?;
    let [p00, p10, p01, p11] = [
        *bottom.first().ok_or_else(|| bad("empty Coons side"))?,
        *bottom.last().ok_or_else(|| bad("empty Coons side"))?,
        *top.first().ok_or_else(|| bad("empty Coons side"))?,
        *top.last().ok_or_else(|| bad("empty Coons side"))?,
    ];
    let corners = [
        (p00, left.first()),
        (p10, right.first()),
        (p01, left.last()),
        (p11, right.last()),
    ];
    if corners.into_iter().any(|(owned, supplied)| {
        supplied.is_none_or(|&supplied| !compatible_corner(owned, supplied))
    }) {
        return Err(bad(
            "Coons corner positions or homogeneous weights disagree",
        ));
    }
    let mut points = reserved(count)?;
    let mut weights = reserved(count)?;
    for (j, &y) in v.iter().enumerate() {
        let d0 = *left.get(j).ok_or_else(|| bad("incomplete Coons side"))?;
        let d1 = *right.get(j).ok_or_else(|| bad("incomplete Coons side"))?;
        for (i, &x) in u.iter().enumerate() {
            let c0 = *bottom.get(i).ok_or_else(|| bad("incomplete Coons side"))?;
            let c1 = *top.get(i).ok_or_else(|| bad("incomplete Coons side"))?;
            let homogeneous = if j == 0 {
                c0
            } else if j + 1 == v.len() {
                c1
            } else if i == 0 {
                d0
            } else if i + 1 == u.len() {
                d1
            } else {
                let a = blend(c0, c1, y);
                let b = blend(d0, d1, x);
                let c = blend(blend(p00, p10, x), blend(p01, p11, x), y);
                let mut output = [0.; 4];
                for (out, ((a, b), c)) in output.iter_mut().zip(a.into_iter().zip(b).zip(c)) {
                    *out = a + b - c;
                }
                output
            };
            let [x, y, z, w] = homogeneous;
            let point = Vec3::new(x / w, y / w, z / w);
            if !homogeneous.iter().all(|x| x.is_finite())
                || w <= f64::MIN_POSITIVE
                || !point.to_array().into_iter().all(f64::is_finite)
            {
                return Err(bad("a rational Coons patch needs positive finite weights"));
            }
            points.push(point);
            weights.push(w);
        }
    }
    let rational = weights.iter().any(|&w| w != 1.);
    Ok(CoonsNet {
        points,
        weights,
        rational,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coons_preserves_directed_rational_quarter_cylinder_rows() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let bottom = [[1., 0., 0., 1.], [w, w, 0., w], [0., 1., 0., 1.]];
        let top = bottom.map(|[x, y, z, w]| [x, y, z + 2. * w, w]);
        let left = [bottom[0], top[0]];
        let right = [bottom[2], top[2]];
        let u = [0., 0., 0., 1., 1., 1.];
        let v = [0., 0., 1., 1.];
        let net = coons([2, 1], [&u, &v], [&bottom, &top, &left, &right]).unwrap();
        assert_eq!(net.weights, [1., w, 1., 1., w, 1.]);
        assert_eq!(
            net.points,
            [
                Vec3::X,
                Vec3::new(1., 1., 0.),
                Vec3::Y,
                Vec3::new(1., 0., 2.),
                Vec3::new(1., 1., 2.),
                Vec3::new(0., 1., 2.)
            ]
        );
        assert!(net.rational);
    }

    #[test]
    fn coons_has_exact_bilinear_reproduction_and_checked_input_budgets() {
        let knots = [0., 0., 0., 0., 1., 1., 1., 1.];
        let t = [0., 1. / 3., 2. / 3., 1.];
        let bottom = t.map(|x| [x, 0., 0., 1.]);
        let top = t.map(|x| [x, 1., x, 1.]);
        let left = t.map(|y| [0., y, 0., 1.]);
        let right = t.map(|y| [1., y, y, 1.]);
        let net = coons([3, 3], [&knots, &knots], [&bottom, &top, &left, &right]).unwrap();
        for (point, (y, x)) in net.points.iter().zip(
            t.into_iter()
                .flat_map(|y| t.into_iter().map(move |x| (y, x))),
        ) {
            assert!((point.x - x).abs() < 1e-15);
            assert!((point.y - y).abs() < 1e-15);
            assert!((point.z - x * y).abs() < 1e-15);
        }
        assert!(!net.rational);
        assert!(coons(
            [usize::MAX, 3],
            [&knots, &knots],
            [&bottom, &top, &left, &right]
        )
        .is_err());
        assert!(coons(
            [3, 3],
            [&knots, &knots],
            [&bottom[..3], &top, &left, &right]
        )
        .is_err());
        let mut bad = bottom;
        bad[0][3] = -1.;
        assert!(coons([3, 3], [&knots, &knots], [&bad, &top, &left, &right]).is_err());
    }

    #[test]
    fn coons_refuses_nonunit_domains_and_every_invalid_overwritten_endpoint() {
        let knots = [0., 0., 1., 1.];
        let bottom = [[0., 0., 0., 1.], [1., 0., 0., 1.]];
        let top = [[0., 1., 0., 1.], [1., 1., 0., 1.]];
        let left = [bottom[0], top[0]];
        let right = [bottom[1], top[1]];
        assert!(coons(
            [1, 1],
            [&[2., 2., 3., 3.], &knots],
            [&bottom, &top, &left, &right]
        )
        .is_err());
        for endpoint in 0..2 {
            for weight in [0., -1., f64::NAN, f64::INFINITY] {
                let mut invalid = left;
                invalid[endpoint][3] = weight;
                assert!(
                    coons([1, 1], [&knots, &knots], [&bottom, &top, &invalid, &right]).is_err()
                );
                let mut invalid = right;
                invalid[endpoint][3] = weight;
                assert!(coons([1, 1], [&knots, &knots], [&bottom, &top, &left, &invalid]).is_err());
            }
        }
    }

    #[test]
    fn coons_qualifies_tolerated_corner_joins_and_preserves_their_existing_owner() {
        let knots = [0., 0., 1., 1.];
        let bottom = [[0., 0., 0., 1.], [1., 0., 0., 1.]];
        let top = [[0., 1., 0., 1.], [1., 1., 0., 1.]];
        let mut left = [bottom[0], top[0]];
        let right = [bottom[1], top[1]];
        left[0][0] = 1e-10;
        left[0][3] = 1. + 128. * f64::EPSILON;
        let accepted = coons([1, 1], [&knots, &knots], [&bottom, &top, &left, &right]).unwrap();
        assert_eq!(accepted.points[0], Vec3::ZERO);
        assert_eq!(accepted.weights[0], 1.);
        left[0][0] = 1e-3;
        assert!(coons([1, 1], [&knots, &knots], [&bottom, &top, &left, &right]).is_err());
        left[0][0] = 0.;
        left[0][3] = 1. + 512. * f64::EPSILON;
        assert!(coons([1, 1], [&knots, &knots], [&bottom, &top, &left, &right]).is_err());
    }
}
