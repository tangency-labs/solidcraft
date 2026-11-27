//! A certified bound on how far a B-spline surface is from closing on itself in one
//! direction ([`BSplineSurface::boundary_gap_bound`]), read off the control net, and the
//! closure test a seam join relies on ([`BSplineSurface::closes_within`]).
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
    /// Whether the surface closes on itself in `param`'s direction within `tol` in the way
    /// a seam join needs: [`Self::boundary_gap_bound`] is at most `tol`, and the boundary
    /// lines' weights are equal pair by pair (bit for bit; `NOW` counting as 1).
    ///
    /// The join of the two ends (`srf_tool::join`) replaces each pair by the average of
    /// its homogeneous points. With equal weights `w` that average is `(w (a + b) / 2, w)`:
    /// the Euclidean midpoint at the same weight, so each control point moves by at most
    /// `|b - a| / 2` and, by the convex hull of the non-negative rational basis (every
    /// weight of the net positive, which the bound requires), the surface by at most half
    /// the gap bound. The join then removes the joint knot as often as its own tolerance
    /// allows (`remove_knot_conditional`, at most `q` removals for degree `q`, each
    /// accepted by an uncertified floating comparison against that tolerance): a seam
    /// join at the face's tolerance `tol` moves the surface by about `tol / 2 + q tol`.
    /// The certificate covers the averaging only; the removal is upstream's heuristic. With unequal weights the average reweights
    /// the joint line and can move the surface far past the gap (a proportional weight
    /// ratio leaves the boundary curves equal but changes the curves next to them), so
    /// the surface does not close for the join. `false` wherever the bound is `None`.
    ///
    /// # Errors
    ///
    /// Those of [`Self::boundary_gap_bound`].
    pub fn closes_within(&self, param: SurfParamType, tol: f64) -> Result<bool> {
        let Some(gap) = self.boundary_gap_bound(param)? else {
            return Ok(false);
        };
        // (a NaN tolerance closes nothing)
        let within = gap <= tol;
        if !within {
            return Ok(false);
        }
        let Some(sur) = self.gw_nurb() else {
            return Ok(false);
        };
        let weight = |p: &CPoint| if p.w == NOW { 1.0 } else { p.w };
        let (n, m) = (sur.n(), sur.m());
        let equal = match param {
            SurfParamType::U => {
                let (first, last) = (sur.row(0)?, sur.row(n)?);
                first
                    .iter()
                    .zip(last.iter())
                    .all(|(a, b)| weight(a) == weight(b))
            }
            SurfParamType::V => sur.rows().all(|row| match (row.first(), row.get(m)) {
                (Some(a), Some(b)) => weight(a) == weight(b),
                _ => false,
            }),
            _ => false,
        };
        Ok(equal)
    }

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
    /// non-rational net and for lines whose weights are proportional. A pair whose
    /// homogeneous points are equal bit for bit contributes an exact 0 to the first term
    /// (no conversion to Euclidean points). The bound counts
    /// its rounding (§4.4): a weight's division `gamma_2` of each Euclidean point (none
    /// for a weight of 1), `gamma_8` on each computed length, `gamma_6` on the ratio
    /// (none for weights equal pair by pair: the spread is then exactly 0) and `gamma_4`
    /// on the sum.
    ///
    /// The listed factors leave about `3u` of slack each, which also covers the roundings
    /// not listed: the `- 1` after the ratio, and the subtraction, squares, sum and root
    /// inside each length. A length whose squares could underflow (largest component
    /// below `2^-500`) is bounded by twice its largest component instead (at least
    /// `sqrt 3` times it), and each converted point adds `f64::MIN_POSITIVE`, which holds
    /// the absolute error of a coordinate that converts to a subnormal number.
    ///
    /// `None` where the net certifies nothing: no NURB, fewer than two lines in `param`'s
    /// direction, either direction not clamped with exactly `degree + 1` end knots (the
    /// kernel's natural domain runs from the first knot to the last: with fewer end
    /// knots the first and last lines are not the boundary curves, with more the end
    /// lines are not either, and an unclamped other direction evaluates its end spans
    /// with basis values that can be negative, outside the convex hull; a knot vector
    /// that validates is clamped exactly so, so no valid surface lands here), a weight of
    /// any control point that is not finite, not a positive normal number or with a reciprocal
    /// that is not normal (the convex hull no longer holds the surface, or the conversions
    /// and ratios lose their relative error bounds), a weight ratio that is not a normal number, a bound that overflows,
    /// and for `param` neither U nor V.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a control point or weight of either boundary line that is not
    /// finite, whatever the knot vectors; those of [`Self::knots`].
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
        if !(self.is_exactly_clamped(SurfParamType::U)?
            && self.is_exactly_clamped(SurfParamType::V)?)
        {
            return Ok(None);
        }
        let weight = |p: &CPoint| if p.w == NOW { 1.0 } else { p.w };
        // every weight of the net, not only the boundary lines': the convex hull that
        // carries the join's control point moves to the surface needs all of them
        // positive, and the relative error bounds of the conversions and ratios below
        // need them and their reciprocals normal (`is_normal` is false for 0, subnormals,
        // infinities and NaN; a weight past 2^1022 has a subnormal reciprocal, whose
        // rounding is no longer relative to the converted coordinate)
        let normal = |p: &CPoint| {
            let w = weight(p);
            w > 0.0 && w.is_normal() && (1.0 / w).is_normal()
        };
        if !sur.rows().all(|row| row.iter().all(normal)) {
            return Ok(None);
        }
        // weights equal pair by pair (bit for bit): every ratio is 1, so `beta_j = alpha_j`
        // exactly and the second term is 0 without rounding
        let equal_weights = pairs.iter().all(|(a, b)| weight(a) == weight(b));

        // a point's Euclidean form: exact for weight 1 (`NOW` or 1.0: a reciprocal of 1
        // and a product by 1); within gamma_2 |x| of each coordinate x otherwise (a
        // reciprocal and a product, `CPoint::to_euclid`)
        let division = |p: &CPoint| if weight(p) == 1.0 { 0.0 } else { gamma(2) };
        // `|x - y|` from the computed points: the length within gamma_8 of the computed
        // difference's, and each point within its division error of its own length (each
        // length itself within gamma_4 of the computed one). Any overflow, here or below,
        // and the net certifies nothing (`f64::max` would drop a NaN).
        let distance = |(x, px): (Vec3, &CPoint), (y, py): (Vec3, &CPoint)| -> Option<f64> {
            // the same homogeneous point: the same Euclidean point, exactly
            if px == py {
                return Some(0.0);
            }
            let mut d = norm_bound(x - y) * (1.0 + gamma(8));
            for (point, cp) in [(x, px), (y, py)] {
                let e = division(cp);
                if e != 0.0 {
                    d += e * norm_bound(point) * (1.0 + gamma(4)) + f64::MIN_POSITIVE;
                }
            }
            d.is_finite().then_some(d)
        };
        let Some((first, _)) = pairs.first() else {
            return Ok(None);
        };
        let a0 = (first.to_euclid(), first);
        let mut points = 0.0_f64;
        let mut reach = 0.0_f64;
        let (mut rho_min, mut rho_max) = (f64::INFINITY, 0.0_f64);
        for (a, b) in &pairs {
            let (pa, pb) = ((a.to_euclid(), a), (b.to_euclid(), b));
            let (Some(gap), Some(far)) = (distance(pb, pa), distance(pa, a0)) else {
                return Ok(None);
            };
            points = points.max(gap);
            reach = reach.max(far);
            let rho = weight(b) / weight(a);
            // (a ratio that underflows or overflows loses its relative error bound)
            if !rho.is_normal() {
                return Ok(None);
            }
            rho_min = rho_min.min(rho);
            rho_max = rho_max.max(rho);
        }
        // `rho_max / rho_min` from the computed ratios: three divisions, each within u; the
        // ratio is at least 1 (`rho_max >= rho_min`, rounding is monotone), so the spread
        // is not negative
        let spread = if equal_weights {
            0.0
        } else {
            (rho_max / rho_min) * (1.0 + gamma(6)) - 1.0
        };
        if !spread.is_finite() {
            return Ok(None);
        }
        let bound = (points + spread * reach) * (1.0 + gamma(4));
        Ok(bound.is_finite().then_some(bound))
    }

    /// Whether `param`'s knot vector has exactly `degree + 1` equal knots at each end.
    fn is_exactly_clamped(&self, param: SurfParamType) -> Result<bool> {
        let (_, multiplicities) = self.knots(param, None)?;
        let degree = self.degree(param);
        let ends = |m: Option<&usize>| m.is_some_and(|&m| Some(m) == degree.checked_add(1));
        Ok(ends(multiplicities.first()) && ends(multiplicities.last()))
    }
}

/// An upper bound on `|v|` from the computed `v`: its computed length (within the
/// rounding the caller's factor covers) where no square can underflow, and twice its
/// largest component (at least `sqrt 3` times it, so at least `|v|`, exactly) where one
/// could: below `2^-500` a square falls under `2^-1000` and towards the subnormal range,
/// where it loses its relative accuracy or becomes 0 (a nonzero difference of `2^-600`
/// would otherwise have length 0). Exactly 0 for the zero vector.
fn norm_bound(v: Vec3) -> f64 {
    let largest = v.x.abs().max(v.y.abs()).max(v.z.abs());
    if largest == 0.0 {
        return 0.0;
    }
    // 2^-500
    let tiny = f64::from_bits(523_u64 << 52);
    if largest < tiny {
        2.0 * largest
    } else {
        v.length()
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
    fn equal_lines_of_weight_one_bound_zero() {
        // explicit weights of 1.0: the Euclidean points are exact and the ratios all 1,
        // so no rounding term is added
        let row = |z: f64| {
            vec![
                cpw(1.0, 0.0, z, 1.0),
                cpw(0.0, 1.0, z, 1.0),
                cpw(1.0, 0.0, z, 1.0),
            ]
        };
        let s = surface(
            vec![row(0.0), row(1.0)],
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 2.0, 2.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::V).unwrap(), Some(0.0));
        assert!(s.closes_within(SurfParamType::V, 0.0).unwrap());
    }

    #[test]
    fn the_bound_holds_the_gap_and_is_tight_for_a_translation() {
        let a = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let d = Vec3::new(0.0, 3e-4, 0.0);
        let s = tube(a, [a[0] + d, a[1] + d]);
        let b = s.boundary_gap_bound(SurfParamType::U).unwrap().unwrap();
        // the boundary curves are the lines' segments: the gap is 3e-4 everywhere
        assert!((3e-4..=3e-4 * (1.0 + 1e-14)).contains(&b), "{b}");
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
        assert!((0.25..=2.0 * (1.0 + 1e-14)).contains(&b), "{b}");
    }

    #[test]
    fn the_v_direction_reads_the_columns() {
        let s = tube(
            [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
            [Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)],
        );
        // the first and last rows' ends agree; the middle row's are 1 apart
        let b = s.boundary_gap_bound(SurfParamType::V).unwrap().unwrap();
        assert!((1.0..=1.0 + 1e-14).contains(&b), "{b}");
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
    fn an_interior_weight_not_positive_certifies_nothing() {
        // positive, equal boundary weights; the middle line's weight is negative, so the
        // rational basis is not a convex combination and the join's control point moves
        // do not bound the surface's
        let rows = vec![
            vec![cpw(0.0, 0.0, 0.0, 1.0), cpw(0.0, 0.0, 1.0, 1.0)],
            vec![cpw(5.0, 5.0, 0.0, -1.0), cpw(5.0, 5.0, 1.0, 1.0)],
            vec![cpw(0.0, 0.0, 0.0, 1.0), cpw(0.0, 0.0, 1.0, 1.0)],
        ];
        let s = surface(
            rows,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
        assert!(!s.closes_within(SurfParamType::U, 1.0).unwrap());
    }

    #[test]
    fn a_nan_refuses_in_an_unclamped_net_too() {
        let rows = vec![
            vec![cp(f64::NAN, 0.0, 0.0), cp(0.0, 0.0, 1.0)],
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
        assert!(s.boundary_gap_bound(SurfParamType::U).is_err());
    }

    #[test]
    fn a_gap_whose_length_underflows_is_not_zero() {
        // 2^-600 apart: the squares of the difference underflow to 0, so its computed
        // length is 0; the bound must still hold the gap
        let t = f64::from_bits(423_u64 << 52);
        assert_eq!(t, 2.0_f64.powi(-600));
        let o = Vec3::new(0.0, 0.0, 0.0);
        let d = Vec3::new(t, 0.0, 0.0);
        let b = tube([o, o], [d, d])
            .boundary_gap_bound(SurfParamType::U)
            .unwrap()
            .unwrap();
        assert!(b >= t && b <= 4.0 * t, "{b}");
    }

    #[test]
    fn equal_homogeneous_lines_bound_zero_at_any_weight() {
        // weight 0.5 at 1e12: the Euclidean conversion rounds, but the two lines are the
        // same homogeneous points, so their curves are the same and the gap is exactly 0
        let row = |w: f64| vec![cpw(1e12, 0.0, 0.0, w), cpw(1e12, 0.0, 1.0, w)];
        let s = surface(
            vec![row(0.5), row(1.0), row(0.5)],
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), Some(0.0));
        assert!(s.closes_within(SurfParamType::U, 0.0).unwrap());
    }

    #[test]
    fn subnormal_weight_ratios_certify_nothing() {
        // ratios 2^-1074 and 1.25 * 2^-1074 both round to the subnormal 2^-1074: the
        // computed spread would be 0 while the curves are 1/18 apart at the middle
        let big = 2.0_f64.powi(1022);
        let small = 2.0_f64.powi(-52);
        let rows = vec![
            vec![cpw(0.0, 0.0, 0.0, big), cpw(1.0, 0.0, 0.0, big)],
            vec![cpw(0.0, 0.0, 0.0, small), cpw(1.0, 0.0, 0.0, 1.25 * small)],
        ];
        let s = surface(
            rows,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
    }

    #[test]
    fn a_subnormal_weight_certifies_nothing() {
        let tiny = f64::from_bits(1);
        let row = |w: f64| vec![cpw(0.0, 0.0, 0.0, w), cpw(1.0, 0.0, 0.0, w)];
        let s = surface(
            vec![row(1.0), row(tiny), row(1.0)],
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
    }

    #[test]
    fn a_weight_with_a_subnormal_reciprocal_certifies_nothing() {
        // 1 / 2^1023 is subnormal: the conversion's rounding is then absolute, not
        // relative to the coordinate, and the gap's relative allowance no longer covers it
        let huge = f64::from_bits(2046_u64 << 52);
        let row = |w: f64| vec![cpw(0.0, 0.0, 0.0, w), cpw(1.0, 0.0, 0.0, w)];
        let s = surface(
            vec![row(1.0), row(huge), row(1.0)],
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

    #[test]
    fn an_unclamped_other_direction_certifies_nothing() {
        // U clamped, V not: on V's natural domain [0, 5] the end spans evaluate with
        // basis values that are negative (at v = 0: 4.5, -5.5, 2), so the U boundary
        // lines' differences (delta, -delta, delta) give a gap of 12 delta, past their
        // largest difference
        let d = 1e-3;
        let rows = vec![
            vec![cp(0.0, 0.0, 0.0), cp(0.0, 1.0, 0.0), cp(0.0, 2.0, 0.0)],
            vec![cp(d, 0.0, 0.0), cp(-d, 1.0, 0.0), cp(d, 2.0, 0.0)],
        ];
        let s = surface(
            rows,
            1,
            2,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
        assert!(!s.closes_within(SurfParamType::U, 1.0).unwrap());
    }

    #[test]
    fn end_knots_past_the_degree_certify_nothing() {
        // p + 2 equal knots at each end: S at the ends is the second and the
        // second-to-last row, not the net's first and last
        let rows = vec![
            vec![cp(0.0, 0.0, 0.0), cp(0.0, 0.0, 1.0)],
            vec![cp(1.0, 0.0, 0.0), cp(1.0, 0.0, 1.0)],
            vec![cp(2.0, 0.0, 0.0), cp(2.0, 0.0, 1.0)],
            vec![cp(0.0, 0.0, 0.0), cp(0.0, 0.0, 1.0)],
        ];
        let s = surface(
            rows,
            2,
            1,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
    }

    #[test]
    fn an_overflowing_bound_certifies_nothing() {
        let a = [Vec3::new(1e308, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let b = [Vec3::new(-1e308, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let s = tube(a, b);
        assert_eq!(s.boundary_gap_bound(SurfParamType::U).unwrap(), None);
        assert!(!s.closes_within(SurfParamType::U, f64::MAX).unwrap());
    }

    #[test]
    fn closes_within_needs_the_gap_within_the_tolerance() {
        let a = [Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 1.0)];
        let d = Vec3::new(0.0, 3e-4, 0.0);
        let s = tube(a, [a[0] + d, a[1] + d]);
        assert!(s.closes_within(SurfParamType::U, 1e-3).unwrap());
        assert!(!s.closes_within(SurfParamType::U, 1e-4).unwrap());
        assert!(!s.closes_within(SurfParamType::U, f64::NAN).unwrap());
    }

    #[test]
    fn closes_within_needs_equal_boundary_weights() {
        // the boundary curves are equal (proportional weights) and so are the
        // weights of the equal-weight net, but the join's homogeneous average of the
        // proportional pair reweights the joint line: only equal weights close
        let net = |last_w: f64| {
            surface(
                vec![
                    vec![cpw(1.0, 0.0, 0.0, 1.0), cpw(1.0, 0.0, 1.0, 0.5)],
                    vec![cpw(5.0, 5.0, 0.0, 1.0), cpw(5.0, 5.0, 1.0, 1.0)],
                    vec![cpw(1.0, 0.0, 0.0, last_w), cpw(1.0, 0.0, 1.0, 0.5 * last_w)],
                ],
                2,
                1,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![0.0, 0.0, 1.0, 1.0],
            )
        };
        let proportional = net(2.0);
        assert!(
            proportional
                .boundary_gap_bound(SurfParamType::U)
                .unwrap()
                .unwrap()
                < 1e-14
        );
        assert!(!proportional.closes_within(SurfParamType::U, 1e-6).unwrap());
        assert!(net(1.0).closes_within(SurfParamType::U, 1e-6).unwrap());
    }

    #[test]
    fn full_revolutions_close_in_their_sweep() {
        use crate::geom::extent2d::Extent2d;
        use crate::kernel::Kernel;
        use crate::surface::sphere::{Sphere, SphereOptions};
        use crate::surface::torus::{Torus, TorusOptions};

        let k = Kernel::default();
        let (x, y) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let mut sphere = Sphere::new(
            &k,
            Vec3::new(0.0, 0.0, -1.0),
            x,
            y,
            Extent2d::from_bounds(0.0, -60.0, 360.0, 60.0),
            0.5,
            SphereOptions::default(),
        )
        .unwrap();
        sphere.make_nurb(&k).unwrap();
        let net = sphere.base().base();
        assert!(net.closes_within(SurfParamType::U, 1e-9).unwrap());
        assert!(!net.closes_within(SurfParamType::V, 1e-9).unwrap());

        let mut torus = Torus::new(
            &k,
            Vec3::new(0.0, 0.0, 0.0),
            x,
            y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            3.0,
            1.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        let net = torus.base().base();
        assert!(net.closes_within(SurfParamType::U, 1e-9).unwrap());
        assert!(net.closes_within(SurfParamType::V, 1e-9).unwrap());
    }
}
