//! A certified bound of how far apart two B-spline surfaces are at the same parameters.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust                   | Upstream |
//! |------------------------|----------|
//! | [`certified_distance`] | none     |
//!
//! The surface form of [`crate::nurbs::crv_deviation::certified_distance`]. Both surfaces are
//! moved into their own frame (minus the first's corner control point), cut into Bézier
//! patches over the union of their breakpoints in u and in v ([`insert_knots`],
//! [`decompose_to_bez`]) and each patch raised to the higher degrees ([`elevate_degree`], on a
//! patch with no interior knot only the Bézier elevation). On a common patch the difference of
//! two patches of one degree is the patch of the differences of their control points, which
//! lies in their convex hull (Farouki and Rajan 1987; Piegl and Tiller, *The NURBS Book*, 2nd
//! ed., §5.6): at no parameter of the patch are the surfaces farther apart than the largest
//! difference.
//!
//! Rounding (docs/definitions-and-certificates.md §4.4), as for curves: each knot insertion
//! forms each new control point as one convex combination (`γ_12 M` per coordinate, `M` the
//! nets' largest coordinate in their frame), a point carrying at most the insertions of both
//! directions, `I` in all: `γ_(12 (I + 1))` of twice `M`; the elevations `γ_(8 (q_u + q_v))`
//! of `M`; the move into the frame `γ_2` of `M`; each difference's length a scaled norm
//! rounded up by `γ_8`; a coordinate error `e` moves a point by at most `2 e`.
//!
//! Deviations: none (no upstream counterpart). Non-rational 3D surfaces only, of degrees 1 to
//! [`MAX_DEGREE`], their knot vectors clamped on one domain.

use crate::error::{Result, SolidError};
use crate::nurbs::crv_deviation::norm_up;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_tool::p1::{decompose_to_bez, insert_knots};
use crate::nurbs::srf_tool::p2::elevate_degree;

const OP: &str = "nurbs::srf_deviation::certified_distance";

/// A bound `d` with `|a(u, v) - b(u, v)| <= d` at every parameter of the surfaces' common
/// domain, every rounding of computing it included: the largest distance between the control
/// points of their Bézier patches raised to one pair of degrees over common patches (module
/// docs). A surface and itself, with knots inserted or degrees raised, are within rounding.
///
/// # Errors
///
/// `InvalidInput` for a rational or 2D surface, a number not finite, a degree outside
/// `1..=MAX_DEGREE` or knots that do not fit the net, knots not clamped, or two surfaces on
/// different domains; a query failure when their patches do not match or the bound is not
/// finite; those of the refinement, the extraction and the elevation.
pub fn certified_distance(a: &Surface, b: &Surface) -> Result<f64> {
    check(a)?;
    check(b)?;
    let ends = |k: &[f64]| (k.first().copied(), k.last().copied());
    if ends(a.u_knots()) != ends(b.u_knots()) || ends(a.v_knots()) != ends(b.v_knots()) {
        return Err(SolidError::invalid_input(
            OP,
            "the surfaces are not on one domain",
        ));
    }
    // In the surfaces' own frame, so the rounding counted follows their size.
    let c0 = a.cpt(0, 0)?;
    let local = |s: &Surface| -> Result<Surface> {
        let net = s
            .rows()
            .map(|row| {
                row.iter()
                    .map(|p| CPoint::new(p.x - c0.x, p.y - c0.y, p.z - c0.z, NOW))
                    .collect()
            })
            .collect();
        Surface::new(
            net,
            s.p(),
            s.q(),
            s.u_knots().to_vec(),
            s.v_knots().to_vec(),
        )
    };
    let (a_local, b_local) = (local(a)?, local(b)?);
    let (a, b) = (&a_local, &b_local);
    let degrees = (a.p().max(b.p()), a.q().max(b.q()));
    let size = largest_coordinate(a).max(largest_coordinate(b));
    let moved = gamma(2) * size;
    let (pa, ea) = patches(a, b, degrees, size)?;
    let (pb, eb) = patches(b, a, degrees, size)?;
    let mismatch = || SolidError::query_failed(OP, "the surfaces' patches do not match");
    if pa.len() != pb.len() {
        return Err(mismatch());
    }
    let mut largest = 0.0_f64;
    for (ra, rb) in pa.iter().zip(&pb) {
        if ra.len() != rb.len() {
            return Err(mismatch());
        }
        for (x, y) in ra.iter().zip(rb) {
            if ends(x.u_knots()) != ends(y.u_knots())
                || ends(x.v_knots()) != ends(y.v_knots())
                || x.degrees() != y.degrees()
            {
                return Err(mismatch());
            }
            for (row_x, row_y) in x.rows().zip(y.rows()) {
                if row_x.len() != row_y.len() {
                    return Err(mismatch());
                }
                for (p, r) in row_x.iter().zip(row_y) {
                    let d = norm_up(p.x - r.x, p.y - r.y, p.z - r.z)
                        .ok_or_else(|| SolidError::query_failed(OP, "the bound is not finite"))?;
                    largest = largest.max(d);
                }
            }
        }
    }
    let bound = (largest + 2.0 * (ea + eb + 2.0 * moved)) * (1.0 + gamma(2));
    if !bound.is_finite() {
        return Err(SolidError::query_failed(OP, "the bound is not finite"));
    }
    Ok(bound)
}

/// Refuses a surface the bound does not take (see [`certified_distance`]'s errors).
fn check(s: &Surface) -> Result<()> {
    let (p, q) = s.degrees();
    let rows = s.n().saturating_add(1);
    let cols = s.m().saturating_add(1);
    if !((1..=MAX_DEGREE).contains(&p)
        && (1..=MAX_DEGREE).contains(&q)
        && s.u_knots().len() == rows + p + 1
        && s.v_knots().len() == cols + q + 1)
    {
        return Err(SolidError::invalid_input(
            OP,
            "a degree of 0 or past the most, or knots that do not fit the net",
        ));
    }
    let points_ok = s.rows().all(|row| {
        row.iter()
            .all(|c| c.x.is_finite() && c.y.is_finite() && c.z.is_finite())
    });
    let knots_ok = s.u_knots().iter().chain(s.v_knots()).all(|k| k.is_finite());
    if !(points_ok && knots_ok) {
        return Err(SolidError::invalid_input(OP, "numbers not finite"));
    }
    if s.rows().flatten().any(|c| c.w != NOW) {
        return Err(SolidError::invalid_input(OP, "a rational surface"));
    }
    if s.rows().flatten().any(|c| c.z == NOZ) {
        return Err(SolidError::invalid_input(OP, "a 2D surface"));
    }
    if !clamped(s.u_knots(), p) || !clamped(s.v_knots(), q) {
        return Err(SolidError::invalid_input(OP, "knots not clamped"));
    }
    Ok(())
}

/// Whether the first and last `degree + 1` of `knots` are each one value.
fn clamped(knots: &[f64], degree: usize) -> bool {
    let flat =
        |run: Option<&[f64]>| run.is_some_and(|r| r.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(degree + 1);
    flat(knots.get(..=degree)) && flat(last.and_then(|s| knots.get(s..)))
}

/// The largest absolute coordinate of `s`'s control points.
fn largest_coordinate(s: &Surface) -> f64 {
    s.rows().flatten().fold(0.0_f64, |m, p| {
        m.max(p.x.abs()).max(p.y.abs()).max(p.z.abs())
    })
}

/// The interior knots of `other` that `knots` lacks, once each, in order.
fn missing(knots: &[f64], other: &[f64]) -> Vec<f64> {
    let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
        return Vec::new();
    };
    let mut out: Vec<f64> = Vec::new();
    for &u in other {
        if u > lo && u < hi && !knots.contains(&u) && out.last() != Some(&u) {
            out.push(u);
        }
    }
    out
}

/// The Bézier patches of `s` over the union of its breakpoints and `other`'s, each raised to
/// `degrees`, and the error each of their coordinates may carry (module docs), `size` the
/// largest coordinate of both nets.
fn patches(
    s: &Surface,
    other: &Surface,
    (qu, qv): (usize, usize),
    size: f64,
) -> Result<(Vec<Vec<Surface>>, f64)> {
    let mut refined = s.clone();
    let in_u = missing(s.u_knots(), other.u_knots());
    if !in_u.is_empty() {
        refined = insert_knots(&refined, &in_u, Direction::U)?;
    }
    let in_v = missing(s.v_knots(), other.v_knots());
    if !in_v.is_empty() {
        refined = insert_knots(&refined, &in_v, Direction::V)?;
    }
    let (p, q) = s.degrees();
    let pieces = decompose_to_bez(&refined)?;
    // knots inserted: those of the patches' joined knot vectors, 2 (p + 1) + (k - 1) p for k
    // patches along a direction, past s's own
    let joined = |count: usize, degree: usize| {
        count
            .saturating_sub(1)
            .saturating_mul(degree)
            .saturating_add(2 * (degree + 1))
    };
    let along_v = pieces.first().map_or(0, Vec::len);
    let inserted = joined(pieces.len(), p)
        .saturating_sub(s.u_knots().len())
        .saturating_add(joined(along_v, q).saturating_sub(s.v_knots().len()));
    let mut err = gamma(inserted.saturating_add(1).saturating_mul(12)) * 2.0 * size;
    let (ru, rv) = (qu.saturating_sub(p), qv.saturating_sub(q));
    if ru == 0 && rv == 0 {
        return Ok((pieces, err));
    }
    err += gamma(qu.saturating_add(qv).saturating_mul(8)) * (size + err);
    let mut raised = Vec::new();
    for row in &pieces {
        let mut out = Vec::new();
        for patch in row {
            let mut x = patch.clone();
            if ru > 0 {
                x = elevate_degree(&x, ru, Direction::U)?;
            }
            if rv > 0 {
                x = elevate_degree(&x, rv, Direction::V)?;
            }
            out.push(x);
        }
        raised.push(out);
    }
    Ok((raised, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::nurbs::geom::Rectangle;
    use crate::nurbs::knots::{clamped_uniform, Side};

    /// A bicubic on a 5 x 4 net over [0, 1]², a gentle hill.
    fn hill() -> Surface {
        let net = (0..5)
            .map(|i| {
                (0..4)
                    .map(|j| {
                        let (x, y) = (f64::from(i) * 2.0, f64::from(j) * 3.0);
                        let z = f64::from((i * j) % 3) - 0.5 * f64::from(j);
                        CPoint::from_point(Vec3::new(x, y, z))
                    })
                    .collect()
            })
            .collect();
        Surface::new(
            net,
            3,
            3,
            clamped_uniform(5, 3).unwrap(),
            clamped_uniform(4, 3).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_surface_is_no_distance_from_itself_or_its_refinements() {
        let s = hill();
        let d = certified_distance(&s, &s).unwrap();
        assert!(d < 1e-12, "{d}");
        let more = insert_knots(&s, &[0.3, 0.7], Direction::U).unwrap();
        let higher = elevate_degree(&s, 2, Direction::V).unwrap();
        for other in [&more, &higher] {
            let d = certified_distance(&s, other).unwrap();
            assert!(d < 1e-12, "{d}");
            let d = certified_distance(other, &s).unwrap();
            assert!(d < 1e-12, "{d}");
        }
    }

    #[test]
    fn a_moved_control_point_is_bounded_by_its_move_and_holds_every_sampled_gap() {
        let s = hill();
        let mut m = s.clone();
        m.cpt_mut(2, 1).unwrap().z += 1.0;
        let d = certified_distance(&s, &m).unwrap();
        assert!(d <= 1.0 + 1e-12, "{d}");
        for i in 0..=40 {
            for j in 0..=40 {
                let (u, v) = (f64::from(i) / 40.0, f64::from(j) / 40.0);
                let p = s.eval(u, v, Side::Left, Side::Left).unwrap();
                let q = m.eval(u, v, Side::Left, Side::Left).unwrap();
                let gap2 = (p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2);
                assert!(gap2 <= d * d, "at ({u}, {v}): {gap2} past {d}²");
            }
        }
    }

    #[test]
    fn the_bound_follows_the_surfaces_size_not_their_place() {
        let mut far = hill();
        for i in 0..5 {
            for j in 0..4 {
                far.cpt_mut(i, j).unwrap().x += 1e9;
            }
        }
        let d = certified_distance(&far, &far).unwrap();
        assert!(d < 1e-9, "{d}");
    }

    #[test]
    fn surfaces_the_bound_does_not_take_are_refused() {
        let s = hill();
        let mut other = s.clone();
        other
            .reparam_to_interval(
                Rectangle {
                    ul: 0.0,
                    ur: 2.0,
                    vb: 0.0,
                    vt: 1.0,
                },
                Direction::U,
            )
            .unwrap();
        assert!(matches!(
            certified_distance(&s, &other),
            Err(SolidError::InvalidInput { .. })
        ));
        let mut rational = s.clone();
        rational.non_rat_to_rat();
        assert!(matches!(
            certified_distance(&s, &rational),
            Err(SolidError::InvalidInput { .. })
        ));
        let mut nan = s.clone();
        nan.cpt_mut(1, 1).unwrap().y = f64::NAN;
        assert!(matches!(
            certified_distance(&s, &nan),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
