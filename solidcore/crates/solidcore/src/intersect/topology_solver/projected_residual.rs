//! Homogeneous Bernstein certificates for a projected face maximum.
//!
//! Upstream: no direct counterpart; a definition-based enclosure proof.
//! Deviations: at most degree 8 per axis, 4096 subdivision cells, and depth 64;
//! unresolved arithmetic or representation boundaries refuse certification.
//!
//! For homogeneous coordinates `(H,W)`, positive W, and the API's double-cross
//! metric, the bound U is equivalent to the nonnegativity of
//! `U² W² - |N × (H-QW) × N|²`. Bernstein basis functions are nonnegative and
//! sum to one, so nonnegative coefficient lower bounds prove this everywhere.
//! No retention slack or another point's rounding width enters this proof.

use crate::blend::fillet_section::certificate::knot_cuts;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::Nurbs;
use crate::error::Result;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::Vec3;

const OP: &str = "intersect::topology_solver::projected_residual";
const MAX_DEGREE: usize = 8;
const MAX_CELLS: usize = 4096;
const MAX_DEPTH: usize = 64;

// Every operation here is one of `Ivl`'s one-ulp `*_ulp` operations, not its
// `+ - * /` operators: their relative 2.3e-16|x| step and 1e-300 floor on exact
// zeros stop bounds attained exactly (a plane's corner, N = 0) from closing.

/// `a x b`. `projected_metric::projected` uses `Ivl`'s operators (see above).
fn cross(a: [Ivl; 3], b: [Ivl; 3]) -> [Ivl; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [
        ay.mul_ulp(bz).sub_ulp(az.mul_ulp(by)),
        az.mul_ulp(bx).sub_ulp(ax.mul_ulp(bz)),
        ax.mul_ulp(by).sub_ulp(ay.mul_ulp(bx)),
    ]
}

// Convex interpolation `(1-f) a + f b`; the caller proves the exact fraction is
// in [0,1]. A fraction of exactly 0 or 1 is an exact double, so it gives its end
// exactly. Equal operands are not special-cased: an interval that is not a point
// is not one exact double, so `a == b` does not make the result exactly `a`.
fn lerp(a: Ivl, b: Ivl, fraction: Ivl) -> Ivl {
    // (1 - f) x + f y with x, y in one interval and f in [0, 1] is a convex
    // combination, so it lies in that interval: equal ends return it unwidened.
    let convex = fraction.lo >= 0.0 && fraction.hi <= 1.0;
    if fraction == Ivl::from(0.0) || (a == b && convex) {
        a
    } else if fraction == Ivl::from(1.0) {
        b
    } else {
        Ivl::from(1.0)
            .sub_ulp(fraction)
            .mul_ulp(a)
            .add_ulp(fraction.mul_ulp(b))
    }
}

/// Polar de Boor evaluation: p-i arguments a and i arguments b give the ith
/// Bernstein coefficient of the span restricted to [a,b]. Every operation uses
/// defining knots and homogeneous controls, never sampled surface points.
fn blossom(
    knots: &[f64],
    degree: usize,
    span: usize,
    controls: &[[Ivl; 4]],
    domain: [f64; 2],
    index: usize,
) -> Option<[Ivl; 4]> {
    let [a, b] = domain;
    let mut work = controls.to_vec();
    for level in 1..=degree {
        let t = if level <= degree.checked_sub(index)? {
            a
        } else {
            b
        };
        for j in (level..=degree).rev() {
            let knot = span.checked_sub(degree)?.checked_add(j)?;
            let left = *knots.get(knot)?;
            let right = *knots.get(
                knot.checked_add(degree)?
                    .checked_sub(level)?
                    .checked_add(1)?,
            )?;
            if !(left <= t && t <= right && left < right) {
                return None;
            }
            let fraction = if t == left {
                Ivl::from(0.0)
            } else if t == right {
                Ivl::from(1.0)
            } else {
                // Ivl's quotient is the whole line unless right - left excludes 0. The
                // point operands t, left, right are knots and exact doubles, and t lies
                // strictly between left and right here, so neither difference is the
                // `[x, x] - [x, x]` identity.
                let value = Ivl::from(t)
                    .sub_ulp(Ivl::from(left))
                    .div_ulp(Ivl::from(right).sub_ulp(Ivl::from(left)));
                if !value.is_finite() {
                    return None;
                }
                Ivl::new(value.lo.max(0.0), value.hi.min(1.0))
            };
            let before = *work.get(j.checked_sub(1)?)?;
            let after = *work.get(j)?;
            let mut value = [Ivl::from(0.0); 4];
            for ((out, x), y) in value.iter_mut().zip(before).zip(after) {
                *out = lerp(x, y, fraction);
            }
            *work.get_mut(j)? = value;
        }
    }
    work.get(degree).copied()
}

fn patch(net: &Nurbs, u: [f64; 2], v: [f64; 2], ku: usize, kv: usize) -> Option<Vec<[Ivl; 4]>> {
    let mut along_u = Vec::new();
    for j in 0..=net.q {
        let mut controls = Vec::new();
        for i in 0..=net.p {
            let row = ku.checked_sub(net.p)?.checked_add(i)?;
            let column = kv.checked_sub(net.q)?.checked_add(j)?;
            let offset = row
                .checked_mul(net.nv)?
                .checked_add(column)?
                .checked_mul(4)?;
            let slice = net.pw.get(offset..offset.checked_add(4)?)?;
            let [x, y, z, w] = slice else {
                return None;
            };
            controls.push([*x, *y, *z, *w].map(Ivl::from));
        }
        for i in 0..=net.p {
            along_u.push(blossom(&net.u, net.p, ku, &controls, u, i)?);
        }
    }
    let mut result = Vec::new();
    for i in 0..=net.p {
        let mut controls = Vec::new();
        for j in 0..=net.q {
            controls.push(*along_u.get(j.checked_mul(net.p.checked_add(1)?)?.checked_add(i)?)?);
        }
        for j in 0..=net.q {
            result.push(blossom(&net.v, net.q, kv, &controls, v, j)?);
        }
    }
    Some(result)
}

// All n <= 16. Pascal additions are exact integers below 2^14, hence exact f64.
fn choose(n: usize, k: usize) -> Option<f64> {
    if n > 2 * MAX_DEGREE || k > n {
        return None;
    }
    let mut row = vec![1.0];
    for _ in 0..n {
        let mut next = vec![1.0];
        for pair in row.windows(2) {
            let [a, b] = pair else {
                return None;
            };
            next.push(a + b);
        }
        next.push(1.0);
        row = next;
    }
    row.get(k).copied()
}

/// Product identity `B_i^p B_j^p = C(p,i) C(p,j)/C(2p,i+j) B_(i+j)^(2p)`.
fn product(a: &[Ivl], b: &[Ivl], p: usize, q: usize) -> Option<Vec<Ivl>> {
    let mut result = Vec::new();
    for i in 0..=2 * p {
        for j in 0..=2 * q {
            let mut sum = Ivl::from(0.0);
            for ai in i.saturating_sub(p)..=i.min(p) {
                let bi = i.checked_sub(ai)?;
                let wu = Ivl::from(choose(p, ai)?)
                    .mul_ulp(Ivl::from(choose(p, bi)?))
                    .div_ulp(Ivl::from(choose(2 * p, i)?));
                for aj in j.saturating_sub(q)..=j.min(q) {
                    let bj = j.checked_sub(aj)?;
                    let wv = Ivl::from(choose(q, aj)?)
                        .mul_ulp(Ivl::from(choose(q, bj)?))
                        .div_ulp(Ivl::from(choose(2 * q, j)?));
                    let av = *a.get(ai.checked_mul(q.checked_add(1)?)?.checked_add(aj)?)?;
                    let bv = *b.get(bi.checked_mul(q.checked_add(1)?)?.checked_add(bj)?)?;
                    sum = sum.add_ulp(wu.mul_ulp(wv).mul_ulp(av.mul_ulp(bv)));
                }
            }
            result.push(sum);
        }
    }
    Some(result)
}

fn residual(
    controls: &[[Ivl; 4]],
    p: usize,
    q: usize,
    point: Vec3,
    normal: Vec3,
    upper: f64,
) -> Option<Vec<Ivl>> {
    let normal = [normal.x, normal.y, normal.z].map(Ivl::from);
    let query = [point.x, point.y, point.z].map(Ivl::from);
    let mut weights = Vec::new();
    let mut projections = [Vec::new(), Vec::new(), Vec::new()];
    for &[x, y, z, w] in controls {
        if !w.is_finite() || w.lo <= 0.0 {
            return None;
        }
        weights.push(w);
        let mut delta = [Ivl::from(0.0); 3];
        for ((out, coordinate), query) in delta.iter_mut().zip([x, y, z]).zip(query) {
            // A coordinate and q w can be the same point; their difference is
            // then exactly 0 (see `Ivl::sub_ulp`).
            *out = coordinate.sub_ulp(query.mul_ulp(w));
        }
        for (out, coordinate) in projections
            .iter_mut()
            .zip(cross(cross(normal, delta), normal))
        {
            out.push(coordinate);
        }
    }
    let upper_squared = Ivl::from(upper).mul_ulp(Ivl::from(upper));
    let mut result = product(&weights, &weights, p, q)?;
    for value in &mut result {
        *value = value.mul_ulp(upper_squared);
    }
    for coordinate in projections {
        for (value, squared) in result
            .iter_mut()
            .zip(product(&coordinate, &coordinate, p, q)?)
        {
            *value = value.sub_ulp(squared);
        }
    }
    Some(result)
}

#[derive(Clone)]
struct Cell {
    coefficients: Vec<Ivl>,
    depth: usize,
}

fn split(coefficients: &[Ivl], p: usize, q: usize, along_u: bool) -> Option<(Vec<Ivl>, Vec<Ivl>)> {
    let size = p.checked_add(1)?.checked_mul(q.checked_add(1)?)?;
    let mut left = vec![Ivl::from(0.0); size];
    let mut right = left.clone();
    let (degree, lines) = if along_u { (p, q) } else { (q, p) };
    let index = |line: usize, position: usize| -> Option<usize> {
        if along_u {
            position.checked_mul(q.checked_add(1)?)?.checked_add(line)
        } else {
            line.checked_mul(q.checked_add(1)?)?.checked_add(position)
        }
    };
    for line in 0..=lines {
        let mut work = Vec::new();
        for position in 0..=degree {
            work.push(*coefficients.get(index(line, position)?)?);
        }
        *left.get_mut(index(line, 0)?)? = *work.first()?;
        *right.get_mut(index(line, degree)?)? = *work.last()?;
        for level in 1..=degree {
            for position in 0..=degree.checked_sub(level)? {
                let value = lerp(
                    *work.get(position)?,
                    *work.get(position.checked_add(1)?)?,
                    Ivl::from(0.5),
                );
                *work.get_mut(position)? = value;
            }
            *left.get_mut(index(line, level)?)? = *work.first()?;
            *right.get_mut(index(line, degree.checked_sub(level)?)?)? =
                *work.get(degree.checked_sub(level)?)?;
        }
    }
    Some((left, right))
}

/// Certify a whole-domain projected bound, independently of candidate selection.
/// Unsupported degrees, nonpositive weights, uncovered native jumps and exhausted
/// arithmetic/subdivision budgets return false. Cancellation is propagated.
pub(super) fn maximum_is_bounded(
    k: &Kernel,
    net: &Nurbs,
    domain: &Extent2d,
    point: Vec3,
    normal: Vec3,
    upper: f64,
) -> Result<bool> {
    if net.p == 0
        || net.q == 0
        || net.p > MAX_DEGREE
        || net.q > MAX_DEGREE
        || !upper.is_finite()
        || upper < 0.0
        || ![
            domain.min().x,
            domain.min().y,
            domain.max().x,
            domain.max().y,
            point.x,
            point.y,
            point.z,
            normal.x,
            normal.y,
            normal.z,
        ]
        .iter()
        .all(|x| x.is_finite())
        || domain.min().x >= domain.max().x
        || domain.min().y >= domain.max().y
        || net.u.len() != net.nu.saturating_add(net.p).saturating_add(1)
        || net.v.len() != net.nv.saturating_add(net.q).saturating_add(1)
        || net.pw.len() != net.nu.saturating_mul(net.nv).saturating_mul(4)
        || !net
            .u
            .iter()
            .chain(&net.v)
            .chain(&net.pw)
            .all(|x| x.is_finite())
        || net.u.windows(2).any(|x| matches!(x, [a,b] if a>b))
        || net.v.windows(2).any(|x| matches!(x, [a,b] if a>b))
    {
        return Ok(false);
    }
    let jump = |knots: &[f64], degree: usize, count: usize, t: f64| {
        knots.get(count).is_some_and(|end| t < *end)
            && knots.iter().filter(|&&x| x == t).count() > degree
    };
    if jump(&net.u, net.p, net.nu, domain.max().x) || jump(&net.v, net.q, net.nv, domain.max().y) {
        return Ok(false);
    }
    let us = knot_cuts(&net.u, net.p, net.nu, domain.min().x, domain.max().x, 0)?;
    let vs = knot_cuts(&net.v, net.q, net.nv, domain.min().y, domain.max().y, 0)?;
    let mut visited = 0;
    for &(ua, ub, ku) in &us {
        for &(va, vb, kv) in &vs {
            let Some(controls) = patch(net, [ua, ub], [va, vb], ku, kv) else {
                return Ok(false);
            };
            let Some(coefficients) = residual(&controls, net.p, net.q, point, normal, upper) else {
                return Ok(false);
            };
            let mut pending = vec![Cell {
                coefficients,
                depth: 0,
            }];
            while let Some(cell) = pending.pop() {
                k.check_cancelled(OP)?;
                visited += 1;
                if visited > MAX_CELLS || cell.coefficients.iter().any(|&x| !x.is_finite()) {
                    return Ok(false);
                }
                if cell.coefficients.iter().all(|x| x.lo >= 0.0) {
                    continue;
                }
                if cell.depth == MAX_DEPTH || cell.coefficients.iter().all(|x| x.hi < 0.0) {
                    return Ok(false);
                }
                let Some((left, right)) = split(
                    &cell.coefficients,
                    2 * net.p,
                    2 * net.q,
                    cell.depth % 2 == 0,
                ) else {
                    return Ok(false);
                };
                pending.push(Cell {
                    coefficients: right,
                    depth: cell.depth + 1,
                });
                pending.push(Cell {
                    coefficients: left,
                    depth: cell.depth + 1,
                });
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden_peak() -> Nurbs {
        let mut pw = Vec::new();
        for x in [0.0, 2.0, 0.0] {
            for y in [0.0, 2.0, 0.0] {
                pw.extend([x, y, 0.0, 1.0]);
            }
        }
        Nurbs {
            p: 2,
            q: 2,
            nu: 3,
            nv: 3,
            u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            pw,
        }
    }

    fn unit_domain() -> Extent2d {
        {
            let mut domain = Extent2d::new();
            domain
                .set_min_max(crate::math::Vec2::ZERO, crate::math::Vec2::new(1.0, 1.0))
                .unwrap();
            domain
        }
    }

    #[test]
    fn homogeneous_hidden_peak_bound_is_sharp_without_retention_slack() {
        let net = hidden_peak();
        let upper = 1.4142135623731238;
        assert!(maximum_is_bounded(
            &Kernel::default(),
            &net,
            &unit_domain(),
            Vec3::ZERO,
            Vec3::Z,
            upper
        )
        .unwrap());
        assert!(!maximum_is_bounded(
            &Kernel::default(),
            &net,
            &unit_domain(),
            Vec3::ZERO,
            Vec3::Z,
            1.4
        )
        .unwrap());
    }

    #[test]
    fn subdomain_blossom_matches_quadratic_bernstein_controls() {
        let net = hidden_peak();
        let controls = patch(&net, [0.0, 0.5], [0.0, 0.5], 2, 2).unwrap();
        for (i, value) in controls.iter().enumerate() {
            let expected_x = [0.0, 1.0, 1.0][i / 3];
            let expected_y = [0.0, 1.0, 1.0][i % 3];
            assert!(value[0].lo <= expected_x && expected_x <= value[0].hi);
            assert!(value[1].lo <= expected_y && expected_y <= value[1].hi);
            assert_eq!(value[3], Ivl::from(1.0));
        }
    }

    #[test]
    fn blossom_extracts_both_non_bezier_knot_spans() {
        let mut pw = Vec::new();
        for x in [0.0, 2.0, 4.0, 8.0] {
            for _ in 0..2 {
                pw.extend([x, 0.0, 0.0, 1.0]);
            }
        }
        let net = Nurbs {
            p: 2,
            q: 1,
            nu: 4,
            nv: 2,
            u: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        };
        for (domain, span, expected) in [
            ([0.0, 0.5], 2, [0.0, 2.0, 3.0]),
            ([0.5, 1.0], 3, [3.0, 4.0, 8.0]),
        ] {
            let controls = patch(&net, domain, [0.0, 1.0], span, 1).unwrap();
            for (i, control) in controls.iter().enumerate() {
                assert!(control[0].lo <= expected[i / 2] && expected[i / 2] <= control[0].hi);
                assert_eq!(control[3], Ivl::from(1.0));
            }
        }
    }

    #[test]
    fn rational_weights_and_nonunit_normal_preserve_the_bound() {
        // S=(2u,2v,0)/(1+u). Squared distance is at most 4:
        // 4(1+u)^2 - 4u^2 - 4v^2 = 4(1+2u-v^2) >= 0.
        for homogeneous_scale in [1.0, 3.0] {
            let mut pw = Vec::new();
            for u in [0.0, 1.0] {
                for v in [0.0, 1.0] {
                    pw.extend([2.0 * u, 2.0 * v, 0.0, 1.0 + u].map(|x| x * homogeneous_scale));
                }
            }
            let net = Nurbs {
                p: 1,
                q: 1,
                nu: 2,
                nv: 2,
                u: vec![0.0, 0.0, 1.0, 1.0],
                v: vec![0.0, 0.0, 1.0, 1.0],
                pw,
            };
            assert!(maximum_is_bounded(
                &Kernel::default(),
                &net,
                &unit_domain(),
                Vec3::ZERO,
                Vec3::Z * 2.0,
                8.0 + 1e-12
            )
            .unwrap());
            assert!(!maximum_is_bounded(
                &Kernel::default(),
                &net,
                &unit_domain(),
                Vec3::ZERO,
                Vec3::Z * 2.0,
                7.99
            )
            .unwrap());
        }
    }

    #[test]
    fn right_valued_upper_jump_is_not_certified_by_left_patch() {
        let mut pw = Vec::new();
        for x in [0.0, 1.0, 100.0, 100.0] {
            for _ in 0..2 {
                pw.extend([x, 0.0, 0.0, 1.0]);
            }
        }
        let net = Nurbs {
            p: 1,
            q: 1,
            nu: 4,
            nv: 2,
            u: vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0],
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        };
        let mut domain = unit_domain();
        domain.set_u_max(0.5).unwrap();
        assert!(
            !maximum_is_bounded(&Kernel::default(), &net, &domain, Vec3::ZERO, Vec3::Z, 2.0)
                .unwrap()
        );
    }

    #[test]
    fn rational_sphere_and_trim_band_close_at_point_rounding_bounds() {
        use crate::blend::fillet_section::station::as_b_spline;
        use crate::surface::sphere::{Sphere, SphereOptions};
        use crate::surface::surface::Surface;
        for (latitude, upper) in [(-60.0, 0.8605551275464095), (15.0, 0.8435180406909434)] {
            let k = Kernel::default();
            let mut sphere = Sphere::new(
                &k,
                Vec3::new(0.0, 0.0, -1.0),
                Vec3::X,
                Vec3::Y,
                Extent2d::from_bounds(0.0, latitude, 360.0, 60.0),
                0.5,
                SphereOptions::default(),
            )
            .unwrap();
            sphere.make_nurb(&k).unwrap();
            let surface = Surface::from(sphere);
            let bs = as_b_spline(&k, &surface).unwrap().unwrap();
            let net = Nurbs::of(&bs).unwrap();
            assert!(
                maximum_is_bounded(
                    &k,
                    &net,
                    &unit_domain(),
                    Vec3::new(0.2, 0.3, 2.0),
                    Vec3::Z,
                    upper
                )
                .unwrap(),
                "latitude={latitude}, upper={upper:.17}"
            );
        }
    }

    #[test]
    fn supported_degree_boundary_is_explicit() {
        for degree in [8, 9] {
            let mut pw = Vec::new();
            for i in 0..=degree {
                for j in 0..=degree {
                    pw.extend([i as f64 / degree as f64, j as f64 / degree as f64, 0.0, 1.0]);
                }
            }
            let mut knots = vec![0.0; degree + 1];
            knots.extend(vec![1.0; degree + 1]);
            let net = Nurbs {
                p: degree,
                q: degree,
                nu: degree + 1,
                nv: degree + 1,
                u: knots.clone(),
                v: knots,
                pw,
            };
            // Both valid nets lie inside [0,1]^2 by their convex hull, so 2 is
            // a generous true bound. Degree9 refuses solely at the documented
            // construction boundary, never by a geometric or retention test.
            assert_eq!(
                maximum_is_bounded(
                    &Kernel::default(),
                    &net,
                    &unit_domain(),
                    Vec3::ZERO,
                    Vec3::Z,
                    2.0
                )
                .unwrap(),
                degree == 8
            );
        }
    }
}
