//! A certified bound on how far a B-spline surface is from closing on itself in one
//! direction ([`BSplineSurface::boundary_gap_bound`]), read off the control net.
//!
//! Not upstream: upstream decides closure by sampling (`IsPeriodic`, six interior
//! samples), which proves nothing between the samples. Here the gap between the surface's
//! two boundary curves in a direction is bounded over the whole of the other parameter
//! from the net's two boundary lines (docs/definitions-and-certificates.md §4.2, §7.6).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};

const OP: &str = "nurbs::b_spline_surface::closure::BSplineSurface::boundary_gap_bound";

impl BSplineSurface {
    /// A certified upper bound on `sup_t |S(end, t) - S(start, t)|`: the largest distance
    /// between the surface's two boundary curves in `param`'s direction (`start`, `end`
    /// its first and last knot), over the whole of the other parameter. `Some(0.0)` for
    /// boundary lines that are equal point for point.
    ///
    /// In a clamped direction the boundary curves are the curves of the net's first and
    /// last lines `a_j`, `b_j` (weights `v_j`, `w_j`) on one knot vector, so for the
    /// non-negative basis `N_j(t)`, with `alpha_j = N_j v_j / sum N v` and
    /// `beta_j = N_j w_j / sum N w`,
    ///
    /// `S(end) - S(start) = sum beta_j (b_j - a_j) + sum (beta_j - alpha_j)(a_j - c)`
    ///
    /// for any point `c`. `beta_j = alpha_j rho_j / sum alpha rho` with
    /// `rho_j = w_j / v_j`, so `sum |beta_j - alpha_j| <= rho_max / rho_min - 1` and
    ///
    /// `|S(end) - S(start)| <= max_j |b_j - a_j| + (rho_max / rho_min - 1) max_j |a_j - a_0|`
    ///
    /// (§4.2's net distance, with one weight ratio per pair). The second term is 0 for a
    /// non-rational net and for lines whose weights are proportional. The bound counts
    /// its rounding (§4.4): a weight's division `gamma_2` of each Euclidean point (none
    /// for a non-rational net), `gamma_8` on each computed length, `gamma_6` on the ratio
    /// and `gamma_4` on the sum.
    ///
    /// `None` where the net certifies nothing: no NURB, fewer than two lines in `param`'s
    /// direction, the direction not clamped (its first and last lines are then not the
    /// boundary curves; the kernel's natural domain runs from the first knot to the
    /// last, which only a clamped net evaluates on), or a weight not positive (the
    /// convex hull no longer holds the curve), and for `param` neither U nor V.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a control point or weight of either boundary line that is not
    /// finite, or a bound that overflows; those of [`Self::knots`].
    pub fn boundary_gap_bound(&self, param: SurfParamType) -> Result<Option<f64>> {
        let Some(sur) = self.gw_nurb() else {
            return Ok(None);
        };
        let (n, m) = (sur.n(), sur.m());
        let pairs: Vec<(CPoint, CPoint)> = match param {
            SurfParamType::U if n > 0 => {
                let (first, last) = (sur.row(0)?, sur.row(n)?);
                first.iter().copied().zip(last.iter().copied()).collect()
            }
            SurfParamType::V if m > 0 => {
                let mut pairs = Vec::new();
                for row in sur.rows() {
                    match (row.first(), row.get(m)) {
                        (Some(&a), Some(&b)) => pairs.push((a, b)),
                        _ => return Err(SolidError::invalid_input(OP, "a net row is short")),
                    }
                }
                pairs
            }
            _ => return Ok(None),
        };
        if !self.is_clamped_in(param)? {
            return Ok(None);
        }

        // (a point's z is 0 for a 2D net; its weight 1 for a non-rational one)
        let finite = |p: &CPoint| {
            p.x.is_finite()
                && p.y.is_finite()
                && (p.z == NOZ || p.z.is_finite())
                && (p.w == NOW || p.w.is_finite())
        };
        if !pairs.iter().all(|(a, b)| finite(a) && finite(b)) {
            return Err(SolidError::invalid_input(
                OP,
                "a boundary control point or weight is not finite",
            ));
        }
        let weight = |p: &CPoint| if p.w == NOW { 1.0 } else { p.w };
        if !pairs
            .iter()
            .all(|(a, b)| weight(a) > 0.0 && weight(b) > 0.0)
        {
            return Ok(None);
        }
        let rational = pairs.iter().any(|(a, b)| a.w != NOW || b.w != NOW);

        // the Euclidean points: exact for a non-rational net; within gamma_2 |x| of each
        // coordinate x (a reciprocal and a product, `CPoint::to_euclid`) for a rational one
        let division = if rational { gamma(2) } else { 0.0 };
        // `|x - y|` from the computed points: the length within gamma_8 of the computed
        // difference's, and each point within `division` of its own length (each length
        // itself within gamma_4 of the computed one)
        let distance = |x: Vec3, y: Vec3| -> f64 {
            (x - y).length() * (1.0 + gamma(8))
                + division * (x.length() + y.length()) * (1.0 + gamma(4))
        };
        let a0 = pairs.first().map_or(Vec3::ZERO, |(a, _)| a.to_euclid());
        let mut points = 0.0_f64;
        let mut reach = 0.0_f64;
        let (mut rho_min, mut rho_max) = (f64::INFINITY, 0.0_f64);
        for (a, b) in &pairs {
            let (pa, pb) = (a.to_euclid(), b.to_euclid());
            points = points.max(distance(pb, pa));
            reach = reach.max(distance(pa, a0));
            let rho = weight(b) / weight(a);
            rho_min = rho_min.min(rho);
            rho_max = rho_max.max(rho);
        }
        // `rho_max / rho_min` from the computed ratios: three divisions, each within u
        let spread = if rational {
            (rho_max / rho_min) * (1.0 + gamma(6)) - 1.0
        } else {
            0.0
        };
        let bound = (points + spread.max(0.0) * reach) * (1.0 + gamma(4));
        if bound.is_finite() {
            Ok(Some(bound))
        } else {
            Err(SolidError::invalid_input(
                OP,
                "the boundary gap bound overflows",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::srf_basic::Surface as Nurb;

    fn cp(x: f64, y: f64, z: f64) -> CPoint {
        CPoint::new(x, y, z, NOW)
    }

    fn cpw(x: f64, y: f64, z: f64, w: f64) -> CPoint {
        CPoint::new(x * w, y * w, z * w, w)
    }

    fn surface(
        rows: Vec<Vec<CPoint>>,
        p: usize,
        q: usize,
        u: Vec<f64>,
        v: Vec<f64>,
    ) -> BSplineSurface {
        let nurb = Nurb::new(rows, p, q, u, v).expect("a valid net");
        BSplineSurface::from_nurb(nurb, false)
    }

    /// A bilinear-in-v, quadratic-in-u tube piece whose first and last u rows are `first`
    /// and `last`.
    fn tube(first: [Vec3; 2], last: [Vec3; 2]) -> BSplineSurface {
        let row = |a: Vec3, b: Vec3| vec![cp(a.x, a.y, a.z), cp(b.x, b.y, b.z)];
        surface(
            vec![
                row(first[0], first[1]),
                row(Vec3::new(5.0, 5.0, 0.0), Vec3::new(5.0, 5.0, 1.0)),
                row(last[0], last[1]),
            ],
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
    }

    #[test]
    fn equal_lines_bound_zero() {
        let a = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let s = tube(a, a);
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), Some(0.0));
    }

    #[test]
    fn the_bound_holds_the_gap_and_is_tight_for_a_translation() {
        let a = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let d = Vec3::new(0.0, 3e-4, 0.0);
        let s = tube(a, [a[0] + d, a[1] + d]);
        let b = s.boundary_gap_bound(SurfParamType::U).unwrap().unwrap();
        // the boundary curves are the lines' segments: the gap is 3e-4 everywhere
        assert!(b >= 3e-4 && b <= 3e-4 * (1.0 + 1e-14), "{b}");
    }

    #[test]
    fn proportional_weights_add_no_spread() {
        // a rational net whose last line is the first with every weight doubled: the
        // same curve
        let rows = vec![
            vec![cpw(1.0, 0.0, 0.0, 1.0), cpw(1.0, 0.0, 1.0, 0.5)],
            vec![cpw(5.0, 5.0, 0.0, 1.0), cpw(5.0, 5.0, 1.0, 1.0)],
            vec![cpw(1.0, 0.0, 0.0, 2.0), cpw(1.0, 0.0, 1.0, 1.0)],
        ];
        let s = surface(
            rows,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        let b = s.boundary_gap_bound(SurfParamType::U).unwrap().unwrap();
        // only the rounding of the Euclidean points and the ratio: a few u of the size
        assert!(b < 1e-14, "{b}");
    }

    #[test]
    fn unequal_weights_bound_the_rational_gap() {
        // equal points, unequal weight ratios: the curves differ between the ends
        let rows = vec![
            vec![cpw(0.0, 0.0, 0.0, 1.0), cpw(0.0, 0.0, 1.0, 1.0)],
            vec![cpw(5.0, 5.0, 0.0, 1.0), cpw(5.0, 5.0, 1.0, 1.0)],
            vec![cpw(0.0, 0.0, 0.0, 1.0), cpw(0.0, 0.0, 1.0, 3.0)],
        ];
        let s = surface(
            rows,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        let b = s.boundary_gap_bound(SurfParamType::U).unwrap().unwrap();
        // at v = 1/2 the first boundary is at z = 1/2 and the last at z = 3/4: a gap of
        // 1/4, under the bound (rho 1..3, reach 1: 2)
        assert!(b >= 0.25 && b <= 2.0 * (1.0 + 1e-14), "{b}");
    }

    #[test]
    fn the_v_direction_reads_the_columns() {
        let s = tube(
            [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
            [Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)],
        );
        // the first and last rows' ends agree; the middle row's are 1 apart
        let b = s.boundary_gap_bound(SurfParamType::V).unwrap().unwrap();
        assert!(b >= 1.0 && b <= 1.0 + 1e-14, "{b}");
    }

    #[test]
    fn an_unclamped_direction_certifies_nothing() {
        let rows = vec![
            vec![cp(0.0, 0.0, 0.0), cp(0.0, 0.0, 1.0)],
            vec![cp(1.0, 0.0, 0.0), cp(1.0, 0.0, 1.0)],
            vec![cp(0.0, 0.0, 0.0), cp(0.0, 0.0, 1.0)],
        ];
        let s = surface(
            rows,
            2,
            1,
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
    }

    #[test]
    fn a_weight_not_positive_certifies_nothing() {
        let rows = vec![
            vec![cpw(0.0, 0.0, 0.0, 1.0), cpw(0.0, 0.0, 1.0, 1.0)],
            vec![cpw(5.0, 5.0, 0.0, -1.0), cpw(5.0, 5.0, 1.0, 1.0)],
            vec![cpw(0.0, 0.0, 0.0, 1.0), cpw(0.0, 0.0, 1.0, -2.0)],
        ];
        let s = surface(
            rows,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
    }

    #[test]
    fn a_nan_refuses() {
        let a = [Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let b = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        assert!(tube(a, b).boundary_gap_bound(SurfParamType::U).is_err());
    }
}
