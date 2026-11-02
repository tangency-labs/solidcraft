//! Conics and Béziers as cubics (#4890 A3b): an ellipse or hyperbola arc (a conic of weight other
//! than 1) or a Bézier above degree 3 as cubic Béziers within a given distance of it, for the walk,
//! so every writer draws them with the cubics it already writes. Circular arcs are the shared
//! `paper::cubics` helper's; this takes the rest with its signature: the error asked for and the
//! bound that holds, both in the curve's own units, and [`Seg2::Cubic`] pieces.
//!
//! The bound is proved, not sampled. Each piece's cubic has the curve's ends and end tangents, and
//! is never further from it, at the same parameter, than:
//! - for a Bézier of degree n, the cubic whose end derivatives are the curve's: the largest
//!   distance between the curve's control points and the cubic's raised to degree n, since their
//!   difference is a Bézier with those differences as its control points;
//! - for a conic a, apex, b of weight w, the cubic a, a + k(apex − a), b + k(apex − b), b with
//!   k = 4w / (3(1 + w)) (exact for a parabola, and the usual cubic for a circular arc): its
//!   distance at t is |Q(t)·D(t) − N(t)| / D(t), with N and D the conic's numerator and denominator.
//!   The top is a polynomial of degree 5, no larger than its largest Bernstein coefficient, and D is
//!   never below (1 + w) / 2 on [0, 1] when w < 1, nor below 1 otherwise.
//!
//! A piece over the error is halved, exactly ([`Seg2::split_half`]), and each half tried again, up
//! to [`MAX_CUBICS`] pieces: past that the curve is refused, never drawn coarser.

use super::{Invalid, Result, require};
use crate::{Seg2, Vec2};

/// The most cubics one curve may become.
pub const MAX_CUBICS: usize = 4096;

/// `seg`, a conic or a Bézier above degree 3, as cubics within `error` of it, in its order, with
/// the largest bound any of them holds to. A cubic is itself, with a bound of 0.
pub fn cubic_pieces(seg: &Seg2, error: f64) -> Result<(Vec<Seg2>, f64)> {
    require(error.is_finite() && error > 0.0, "a curve's error is finite and above 0")?;
    let (mut pieces, mut bound) = (Vec::new(), 0.0f64);
    // Depth first, left half before right, so the pieces come out in the curve's order.
    let mut todo = vec![*seg];
    while let Some(piece) = todo.pop() {
        let (cubic, off) = cubic_of(&piece)?;
        if off <= error {
            pieces.push(cubic);
            bound = bound.max(off);
            continue;
        }
        require(pieces.len() + todo.len() + 2 <= MAX_CUBICS, "a curve needs more than 4096 cubics on paper")?;
        let (left, right) = piece.split_half();
        todo.extend([right, left]);
    }
    Ok((pieces, bound))
}

/// The cubic with `seg`'s ends and end tangents, and how far it may be from `seg`.
fn cubic_of(seg: &Seg2) -> Result<(Seg2, f64)> {
    match *seg {
        Seg2::Cubic { .. } => Ok((*seg, 0.0)),
        Seg2::Conic { a, apex, b, w } => {
            let k = 4.0 * w / (3.0 * (1.0 + w));
            let q = [a, a + (apex - a) * k, b + (apex - b) * k, b];
            // Q·D − N in degree 5, term by term: Q's coefficients times the denominator's (1, w, 1),
            // less the numerator's (a, w·apex, b) times 1 written in degree 3.
            let terms = [(1.0, a), (w, apex * w), (1.0, b)];
            let off = (0..=5usize)
                .map(|m| {
                    let mut top = Vec2::ZERO;
                    for (i, &qi) in q.iter().enumerate() {
                        if let Some(j) = m.checked_sub(i)
                            && let Some(&(dj, nj)) = terms.get(j)
                        {
                            top += (qi * dj - nj) * (choose(3, i) * choose(2, j));
                        }
                    }
                    (top / choose(5, m)).len()
                })
                .fold(0.0, f64::max);
            let least = if w < 1.0 { (1.0 + w) / 2.0 } else { 1.0 };
            Ok((Seg2::Cubic { p0: q[0], p1: q[1], p2: q[2], p3: q[3] }, off / least))
        }
        Seg2::Bezier { n, ref p } => {
            let degree = usize::from(n);
            let points = p.get(..=degree).filter(|_| degree > 3).ok_or(Invalid("Bezier degree"))?;
            let &[first, second, .., before_last, last] = points else { return Err(Invalid("Bezier degree")) };
            let r = degree as f64 / 3.0;
            let c = [first, first + (second - first) * r, last - (last - before_last) * r, last];
            // The cubic raised to the curve's degree, point by point against the curve's.
            let off = points
                .iter()
                .enumerate()
                .map(|(i, &point)| {
                    let raised = c
                        .iter()
                        .enumerate()
                        .filter(|&(j, _)| j <= i && i - j <= degree - 3)
                        .fold(Vec2::ZERO, |sum, (j, &cj)| sum + cj * (choose(3, j) * choose(degree - 3, i - j)));
                    (point - raised / choose(degree, i)).len()
                })
                .fold(0.0, f64::max);
            Ok((Seg2::Cubic { p0: c[0], p1: c[1], p2: c[2], p3: c[3] }, off))
        }
        Seg2::Line { .. } | Seg2::Arc { .. } => Err(Invalid("only conics and Béziers become cubic pieces here")),
    }
}

/// n choose k, for the small degrees here.
fn choose(n: usize, k: usize) -> f64 {
    (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64)
}

#[cfg(test)]
#[path = "cubic_pieces_tests.rs"]
mod tests;
