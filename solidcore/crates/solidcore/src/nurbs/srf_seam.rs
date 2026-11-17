//! Which control points of two B-spline surfaces are one point across an edge they share
//! ([`seam_twins`]): each surface's boundary row along the edge, paired point for point when the
//! two rows are the control polygon of one curve.
//!
//! Not upstream. Two B-spline curves of one degree on one knot vector (up to the affine map
//! between their parameters) are the same curve exactly when their control points agree: the
//! compatibility skinning and Gordon surfaces rely on (Piegl and Tiller, *The NURBS Book*, 2nd
//! ed., §10.3, "Skinned Surfaces", where the section curves are first made compatible). A
//! surface's boundary curve is its boundary row of control points on the knots along it when
//! its knots across are clamped at that side (*The NURBS Book* §3.4). So where two surfaces'
//! boundary rows along a sewn edge are compatible and their points within the tolerance of each
//! other, moving a point of one row and its twin of the other to one position keeps the two
//! boundaries one curve, and the seam stays shut by construction.
//!
//! Rows that are not compatible (another degree, other knots, other weights, points apart) have
//! no twins: the reason is returned as data ([`SeamMismatch`]), decided from the rows before
//! anything moves, and nothing is rebuilt to make them compatible.
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::error::{Result, SolidError};
use crate::geom::vector3d::Point3;
use crate::math::EFF_ZERO_PARAM;
use crate::nurbs::crv_basic::Boundary;
use crate::nurbs::math::NOW;
use crate::nurbs::srf_basic::Surface;

const OP: &str = "nurbs::srf_seam::seam_twins";

/// A control point by its u and v indices.
pub type Index = (usize, usize);

/// Why two boundary rows are not one curve's control polygon, so a seam along them has no
/// twins. The rows are `a`'s (the first surface's) and `b`'s; an `index` counts along `a`'s row
/// (or its knots) from its start.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SeamMismatch {
    /// A surface's knots across the seam are not clamped at its side, so its boundary row is
    /// not its boundary curve: `true` for `a`, `false` for `b`.
    Unclamped {
        /// Whether it is `a`'s side (else `b`'s).
        a: bool,
    },
    /// The rows' degrees along the seam differ.
    Degree {
        /// `a`'s degree along the seam.
        a: usize,
        /// `b`'s.
        b: usize,
    },
    /// The rows have different numbers of points (so different knots) along the seam.
    KnotCount {
        /// `a`'s knots along the seam.
        a: usize,
        /// `b`'s.
        b: usize,
    },
    /// Knot `index` along the seam is at another place on the two rows (the knots mapped
    /// affinely onto `[0, 1]`, the way round the rows' ends pair).
    Knots {
        /// The knot's index along `a`'s knots.
        index: usize,
    },
    /// The weight of point `index` differs on the two rows (each row's weights over its first
    /// point's): the curves are not one.
    Weights {
        /// The point's index along `a`'s row.
        index: usize,
    },
    /// Point `index` of `a`'s row and its twin are `gap` apart, past the tolerance. For the
    /// ends (`index` 0), no way round of `b`'s row pairs both of `a`'s first two points with
    /// it: `gap` is the nearer way round's larger distance.
    Points {
        /// The point's index along `a`'s row.
        index: usize,
        /// How far apart it and its twin are.
        gap: f64,
    },
}

impl core::fmt::Display for SeamMismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unclamped { a } => write!(
                f,
                "the {} face's knots across it are not clamped",
                if *a { "first" } else { "second" }
            ),
            Self::Degree { a, b } => write!(f, "its rows are of degree {a} and {b}"),
            Self::KnotCount { a, b } => write!(f, "its rows have {a} and {b} knots"),
            Self::Knots { index } => write!(f, "its rows' knot {index} differs"),
            Self::Weights { index } => write!(f, "its rows' weight {index} differs"),
            Self::Points { index, gap } => {
                write!(f, "its rows' point {index} is {gap} from its twin")
            }
        }
    }
}

/// The twins across a seam ([`seam_twins`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Seam {
    /// The rows are one curve's: each point of `a`'s row with its twin on `b`'s, in the order
    /// of `a`'s row.
    Tied(Vec<(Index, Index)>),
    /// The rows are not one curve's, and why.
    Mismatched(SeamMismatch),
}

/// A surface's boundary row along side `side`: the indices of its points in order, the knots
/// along it, its degree along it, and whether the knots across are clamped at that side.
struct Row {
    at: Vec<Index>,
    knots: Vec<f64>,
    degree: usize,
    clamped: bool,
}

/// Whether the first (`start`) or last `degree + 1` of `knots` are one value.
fn clamped_at(knots: &[f64], degree: usize, start: bool) -> bool {
    let ends = if start {
        knots.get(..=degree)
    } else {
        knots
            .len()
            .checked_sub(degree.saturating_add(1))
            .and_then(|s| knots.get(s..))
    };
    ends.is_some_and(|e| e.windows(2).all(|w| w.first() == w.get(1)))
}

fn row(s: &Surface, side: Boundary) -> Row {
    let (n, m) = (s.n(), s.m());
    let (p, q) = s.degrees();
    match side {
        Boundary::Left | Boundary::Right => {
            let start = side == Boundary::Left;
            let i = if start { 0 } else { n };
            Row {
                at: (0..=m).map(|j| (i, j)).collect(),
                knots: s.v_knots().to_vec(),
                degree: q,
                clamped: clamped_at(s.u_knots(), p, start),
            }
        }
        Boundary::Bottom | Boundary::Top => {
            let start = side == Boundary::Bottom;
            let j = if start { 0 } else { m };
            Row {
                at: (0..=n).map(|i| (i, j)).collect(),
                knots: s.u_knots().to_vec(),
                degree: p,
                clamped: clamped_at(s.v_knots(), q, start),
            }
        }
    }
}

/// Point `at` of `s`, Euclidean, with its weight (1 for a polynomial surface).
fn point(s: &Surface, (i, j): Index) -> Result<(Point3, f64)> {
    let (p, w) = s.cpt(i, j)?.to_euclid_and_w();
    Ok((p, if w == NOW { 1.0 } else { w }))
}

/// Knot `k` of `knots` mapped affinely onto `[0, 1]`, counted from the end when `reversed` (and
/// then measured from it).
fn unit(knots: &[f64], k: usize, reversed: bool) -> Result<f64> {
    let short = || SolidError::invalid_input(OP, "a knot vector shorter than its row");
    let (&first, &last) = (
        knots.first().ok_or_else(short)?,
        knots.last().ok_or_else(short)?,
    );
    let span = last - first;
    if !(span > 0.0) {
        return Err(SolidError::invalid_input(OP, "a knot vector with no span"));
    }
    let len = knots.len();
    let at = |i: usize| knots.get(i).copied().ok_or_else(short);
    if reversed {
        let i = len.checked_sub(k.saturating_add(1)).ok_or_else(short)?;
        Ok((last - at(i)?) / span)
    } else {
        Ok((at(k)? - first) / span)
    }
}

/// The twins of the boundary row of `a` along its side `side_a` on the boundary row of `b`
/// along `side_b`, the two meeting along one edge: each point of `a`'s row with the point of
/// `b`'s that is the same control point of the edge's curve, or why the rows are not one curve's
/// control polygon (see the module notes).
///
/// The rows are paired the way round their ends meet: `a`'s first two points within `tol` of
/// `b`'s first two, or of its last two (taking the first two, not only the ends, so a closed
/// row is paired the way it runs). Then, the knots along both rows mapped affinely onto `[0, 1]`
/// that way round must agree to the kernel's zero for parameters ([`EFF_ZERO_PARAM`]), as must
/// each point's weight over its row's first, and every pair of points must be within `tol` (a
/// distance in model units: the body's tolerance).
///
/// # Errors
/// `InvalidInput` for a `tol` not finite or below zero, or a knot vector with no span or
/// shorter than its row; those of the control-point reads.
pub fn seam_twins(
    a: &Surface,
    side_a: Boundary,
    b: &Surface,
    side_b: Boundary,
    tol: f64,
) -> Result<Seam> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a tolerance not finite or below zero",
        ));
    }
    let (ra, rb) = (row(a, side_a), row(b, side_b));
    if !ra.clamped {
        return Ok(Seam::Mismatched(SeamMismatch::Unclamped { a: true }));
    }
    if !rb.clamped {
        return Ok(Seam::Mismatched(SeamMismatch::Unclamped { a: false }));
    }
    if ra.degree != rb.degree {
        return Ok(Seam::Mismatched(SeamMismatch::Degree {
            a: ra.degree,
            b: rb.degree,
        }));
    }
    if ra.knots.len() != rb.knots.len() || ra.at.len() != rb.at.len() {
        return Ok(Seam::Mismatched(SeamMismatch::KnotCount {
            a: ra.knots.len(),
            b: rb.knots.len(),
        }));
    }
    let len = ra.at.len();
    let short = || SolidError::invalid_input(OP, "a row of fewer than two points");
    let pa = |k: usize| point(a, *ra.at.get(k).ok_or_else(short)?);
    let pb = |k: usize| point(b, *rb.at.get(k).ok_or_else(short)?);
    let last = len.checked_sub(1).filter(|&l| l > 0).ok_or_else(short)?;
    let gap = |i: usize, j: usize| -> Result<f64> { Ok(pa(i)?.0.distance(pb(j)?.0)) };
    let forward = gap(0, 0)?.max(gap(1, 1)?);
    let backward = gap(0, last)?.max(gap(1, last.saturating_sub(1))?);
    let reversed = if forward <= tol {
        false
    } else if backward <= tol {
        true
    } else {
        return Ok(Seam::Mismatched(SeamMismatch::Points {
            index: 0,
            gap: forward.min(backward),
        }));
    };
    let twin = |k: usize| if reversed { last.saturating_sub(k) } else { k };
    for k in 0..ra.knots.len() {
        let d = unit(&ra.knots, k, false)? - unit(&rb.knots, k, reversed)?;
        if !(d.abs() <= EFF_ZERO_PARAM) {
            return Ok(Seam::Mismatched(SeamMismatch::Knots { index: k }));
        }
    }
    let (wa0, wb0) = (pa(0)?.1, pb(twin(0))?.1);
    let mut pairs = Vec::new();
    for (k, (&ia, _)) in ra.at.iter().zip(&rb.at).enumerate() {
        let ((p, wa), (q, wb)) = (pa(k)?, pb(twin(k))?);
        if !((wa / wa0 - wb / wb0).abs() <= EFF_ZERO_PARAM) {
            return Ok(Seam::Mismatched(SeamMismatch::Weights { index: k }));
        }
        let d = p.distance(q);
        if !(d <= tol) {
            return Ok(Seam::Mismatched(SeamMismatch::Points { index: k, gap: d }));
        }
        let ib = *rb.at.get(twin(k)).ok_or_else(short)?;
        pairs.push((ia, ib));
    }
    Ok(Seam::Tied(pairs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::nurbs::math::CPoint;

    /// A polynomial surface through the rows of points `rows` (each along v) on the knots `u`
    /// and `v`, its degrees from the counts.
    fn surface(rows: &[Vec<[f64; 3]>], u: &[f64], v: &[f64]) -> Surface {
        let net: Vec<Vec<CPoint>> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| CPoint::from_point(Vec3::new(c[0], c[1], c[2])))
                    .collect()
            })
            .collect();
        let p = u.len() - rows.len() - 1;
        let q = v.len() - rows[0].len() - 1;
        Surface::new(net, p, q, u.to_vec(), v.to_vec()).unwrap()
    }

    /// The seam row: four points along x at y = 0, a cubic on `knots`.
    fn seam_row() -> Vec<[f64; 3]> {
        vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.5],
            [2.0, 0.0, -0.5],
            [3.0, 0.0, 0.0],
        ]
    }

    const CUBIC: [f64; 8] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    const LINEAR: [f64; 4] = [0.0, 0.0, 1.0, 1.0];

    /// A ruled panel from the seam row (its `u = 0` side) to the seam row moved by `dy` in y,
    /// on the knots `v` along the seam.
    fn panel(dy: f64, v: &[f64]) -> Surface {
        let far: Vec<[f64; 3]> = seam_row().iter().map(|p| [p[0], dy, p[2]]).collect();
        surface(&[seam_row(), far], &LINEAR, v)
    }

    #[test]
    fn two_panels_on_one_row_pair_each_point_with_itself() {
        let a = panel(1.0, &CUBIC);
        let b = panel(-1.0, &CUBIC);
        let seam = seam_twins(&a, Boundary::Left, &b, Boundary::Left, 1e-9).unwrap();
        let want: Vec<(Index, Index)> = (0..4).map(|j| ((0, j), (0, j))).collect();
        assert_eq!(seam, Seam::Tied(want));
    }

    /// `b`'s row runs the other way, on knots scaled to `[2, 7]`: still one curve, the pairs
    /// reversed.
    #[test]
    fn a_row_running_the_other_way_on_scaled_knots_pairs_reversed() {
        let a = panel(1.0, &CUBIC);
        let reversed: Vec<[f64; 3]> = seam_row().into_iter().rev().collect();
        let far: Vec<[f64; 3]> = reversed.iter().map(|p| [p[0], -1.0, p[2]]).collect();
        let knots = [2.0, 2.0, 2.0, 2.0, 7.0, 7.0, 7.0, 7.0];
        // b's seam is its v = vmax side: its rows run in u
        let b = surface(
            &(0..4)
                .map(|j| vec![far[j], reversed[j]])
                .collect::<Vec<_>>(),
            &knots,
            &LINEAR,
        );
        let seam = seam_twins(&a, Boundary::Left, &b, Boundary::Top, 1e-9).unwrap();
        let want: Vec<(Index, Index)> = (0..4).map(|j| ((0, j), (3 - j, 1))).collect();
        assert_eq!(seam, Seam::Tied(want));
    }

    #[test]
    fn rows_of_other_knots_or_degrees_or_apart_are_named() {
        let a = panel(1.0, &CUBIC);
        // the same points on a quadratic with an inner knot: four points, other knots
        let b = panel(-1.0, &[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0]);
        assert_eq!(
            seam_twins(&a, Boundary::Left, &b, Boundary::Left, 1e-9).unwrap(),
            Seam::Mismatched(SeamMismatch::Degree { a: 3, b: 2 })
        );
        // a cubic on five points (a knot inserted): another count
        let five = vec![
            [0.0, 0.0, 0.0],
            [0.5, 0.0, 0.25],
            [1.5, 0.0, 0.0],
            [2.5, 0.0, -0.25],
            [3.0, 0.0, 0.0],
        ];
        let far: Vec<[f64; 3]> = five.iter().map(|p| [p[0], -1.0, p[2]]).collect();
        let knots5 = [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0];
        let b = surface(&[five.clone(), far.clone()], &LINEAR, &knots5);
        assert_eq!(
            seam_twins(&a, Boundary::Left, &b, Boundary::Left, 1e-9).unwrap(),
            Seam::Mismatched(SeamMismatch::KnotCount { a: 8, b: 9 })
        );
        // five points each, the inner knot at 0.5 and at 0.4
        let a5 = surface(&[five.clone(), far.clone()], &LINEAR, &knots5);
        let b5 = surface(
            &[five.clone(), far],
            &LINEAR,
            &[0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0],
        );
        assert_eq!(
            seam_twins(&a5, Boundary::Left, &b5, Boundary::Left, 1e-9).unwrap(),
            Seam::Mismatched(SeamMismatch::Knots { index: 4 })
        );
        // the third point lifted by 0.1
        let mut lifted = seam_row();
        lifted[2][2] += 0.1;
        let b = surface(&[lifted, seam_row()], &LINEAR, &CUBIC);
        match seam_twins(&a, Boundary::Left, &b, Boundary::Left, 1e-3).unwrap() {
            Seam::Mismatched(SeamMismatch::Points { index: 2, gap }) => {
                assert!((gap - 0.1).abs() < 1e-12, "{gap}");
            }
            other => panic!("{other:?}"),
        }
        assert!(seam_twins(&a, Boundary::Left, &b, Boundary::Left, f64::NAN).is_err());
    }

    /// A panel's knots across the seam not clamped at its seam side (a linear on `[0, 1, 2, 3]`):
    /// its boundary row is not its boundary curve, named for whichever side it is.
    #[test]
    fn a_row_whose_knots_across_are_not_clamped_is_named() {
        let clamped = panel(1.0, &CUBIC);
        let far: Vec<[f64; 3]> = seam_row().iter().map(|p| [p[0], -1.0, p[2]]).collect();
        let unclamped = surface(&[seam_row(), far], &[0.0, 1.0, 2.0, 3.0], &CUBIC);
        assert_eq!(
            seam_twins(&unclamped, Boundary::Left, &clamped, Boundary::Left, 1e-9).unwrap(),
            Seam::Mismatched(SeamMismatch::Unclamped { a: true })
        );
        assert_eq!(
            seam_twins(&clamped, Boundary::Left, &unclamped, Boundary::Left, 1e-9).unwrap(),
            Seam::Mismatched(SeamMismatch::Unclamped { a: false })
        );
    }

    /// A rational panel: the seam row (its `u = 0` side) with the weights `w`, the far row
    /// (moved by `dy` in y) with weight 1.
    fn weighted_panel(dy: f64, w: [f64; 4]) -> Surface {
        let at = |c: [f64; 3], w: f64| CPoint::weighted(Vec3::new(c[0], c[1], c[2]), w);
        let seam: Vec<CPoint> = seam_row()
            .into_iter()
            .zip(w)
            .map(|(c, w)| at(c, w))
            .collect();
        let far: Vec<CPoint> = seam_row()
            .into_iter()
            .map(|c| at([c[0], dy, c[2]], 1.0))
            .collect();
        Surface::new(vec![seam, far], 1, 3, LINEAR.to_vec(), CUBIC.to_vec()).unwrap()
    }

    /// Rows of one rational curve: weights that are one common factor apart pair (the same
    /// curve), weights that are not are named at the first point where they part.
    #[test]
    fn rational_rows_pair_up_to_one_weight_factor() {
        let a = weighted_panel(1.0, [1.0, 2.0, 2.0, 1.0]);
        let thrice = weighted_panel(-1.0, [3.0, 6.0, 6.0, 3.0]);
        let want: Vec<(Index, Index)> = (0..4).map(|j| ((0, j), (0, j))).collect();
        assert_eq!(
            seam_twins(&a, Boundary::Left, &thrice, Boundary::Left, 1e-9).unwrap(),
            Seam::Tied(want)
        );
        let other = weighted_panel(-1.0, [1.0, 2.0, 3.0, 1.0]);
        assert_eq!(
            seam_twins(&a, Boundary::Left, &other, Boundary::Left, 1e-9).unwrap(),
            Seam::Mismatched(SeamMismatch::Weights { index: 2 })
        );
    }

    /// A closed seam row (its first point its last), a linear round a unit square on knots
    /// symmetric about their middle: `b`'s row running the same way pairs each point with
    /// itself, and running the other way pairs reversed, decided by the first two points
    /// (the ends alone pair both ways).
    #[test]
    fn a_closed_row_pairs_the_way_it_runs() {
        let square = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        let knots = [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0];
        let lifted = |r: &[[f64; 3]], dz: f64| -> Vec<[f64; 3]> {
            r.iter().map(|p| [p[0], p[1], dz]).collect()
        };
        let a = surface(&[square.clone(), lifted(&square, 1.0)], &LINEAR, &knots);
        let same = surface(&[square.clone(), lifted(&square, -1.0)], &LINEAR, &knots);
        let want: Vec<(Index, Index)> = (0..5).map(|j| ((0, j), (0, j))).collect();
        assert_eq!(
            seam_twins(&a, Boundary::Left, &same, Boundary::Left, 1e-9).unwrap(),
            Seam::Tied(want)
        );
        let back: Vec<[f64; 3]> = square.iter().rev().copied().collect();
        let other_way = surface(&[back.clone(), lifted(&back, -1.0)], &LINEAR, &knots);
        let want: Vec<(Index, Index)> = (0..5).map(|j| ((0, j), (0, 4 - j))).collect();
        assert_eq!(
            seam_twins(&a, Boundary::Left, &other_way, Boundary::Left, 1e-9).unwrap(),
            Seam::Tied(want)
        );
    }
}
