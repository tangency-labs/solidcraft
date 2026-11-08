//! Parameters of points on a surface, on the evaluator's own surface form (so the parameters
//! the plan stores are the ones the evaluators use), and the tolerance-driven divisions of a
//! face's parameter box.

use std::cmp::Ordering;

use crate::vec::VecExt as _;
use solidcore::eval::EvalSurface;
use solidcore_convert as kernel;
use solidcore_record::Vec3;

use super::edges::{QUARTER_TURN, turn};
use super::size::Rules;
use super::{NoValue, TooManyCopies};

/// Division of `range` along direction `d` so that between neighbouring lines the surface sags
/// at most the tolerance from its chord, the chord is at most `h` long and the normal turns
/// within the rules' facet angle, checked along iso-lines across `across`; at most `max_lines`
/// lines, ends included. A point the surface does not evaluate at, where the check needs one, is
/// the error where it is the face's (`mine`: inside its loops): the surface refuses the face
/// there. The iso-lines run across the face's whole parameter box, which reaches past a trimmed
/// face; a point outside the face measures nothing, as a size sample there does
/// ([`super::size::FaceSize::unevaluated`]), and is passed over.
pub(crate) fn divide(
    s: &EvalSurface,
    d: usize,
    range: (f64, f64),
    across: (f64, f64),
    rules: &Rules,
    h: f64,
    max_lines: usize,
    mine: Mine,
) -> Result<Vec<f64>, Unmeasured> {
    let (a0, a1) = range;
    if a1.partial_cmp(&a0) != Some(Ordering::Greater) {
        return Ok(vec![a0, a1]);
    }
    // The whole range first: the limits split it, and no interval spans more than a quarter
    // turn of its iso-lines, so a closed iso-line is never taken for a flat one. Both ends
    // exactly: the end lines are the face's own parameter box, which the boundary nodes sit on.
    split(&[a0, a1], &limits(s, d, across, rules, h, mine), max_lines)
}

/// Why a division stopped: a point of the face the surface does not evaluate at, or one it does
/// not evaluate at that `Mine` cannot place inside the face or out (its loops span more periods
/// than it looks through), with the point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Unmeasured {
    NoValue(NoValue),
    TooManyCopies(TooManyCopies, [f64; 2]),
}

impl From<NoValue> for Unmeasured {
    fn from(e: NoValue) -> Self {
        Unmeasured::NoValue(e)
    }
}

/// `r`'s value; `None` where the surface does not evaluate at a point that is not the face's
/// (`mine`), which measures nothing; the error where it is, or where `mine` cannot tell.
fn kept<T>(mine: Mine, r: Result<T, NoValue>) -> Result<Option<T>, Unmeasured> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(NoValue(q)) => match mine(q) {
            Ok(false) => Ok(None),
            Ok(true) => Err(Unmeasured::NoValue(NoValue(q))),
            Err(c) => Err(Unmeasured::TooManyCopies(c, q)),
        },
    }
}

/// Whether a parameter point is the face's (inside its loops): where the surface does not
/// evaluate at a point that is not, the point measures nothing and is passed over. Its error:
/// the face's loops span too many periods to tell.
pub(crate) type Mine<'a> = &'a (dyn Fn([f64; 2]) -> Result<bool, TooManyCopies> + Sync);

/// A point anywhere is the face's: every point the surface does not evaluate at refuses it.
#[cfg(test)]
#[allow(clippy::unnecessary_wraps)] // a `Mine`
pub(crate) fn everywhere(_: [f64; 2]) -> Result<bool, TooManyCopies> {
    Ok(true)
}

/// Steps from an excused end of an iso-line interval towards its other end, looking for the
/// nearest step that is the face's.
const INWARD: u32 = 16;

/// `lines` (increasing, ends included) with every interval that fails the same limits as
/// [`divide`] split until it passes: lines taken from a face's edge nodes meet the edges node for
/// node, but nothing else checks how far the surface turns between them inside the face. The
/// error as in [`divide`].
pub(crate) fn refine(
    s: &EvalSurface,
    d: usize,
    lines: &[f64],
    across: (f64, f64),
    rules: &Rules,
    h: f64,
    max_lines: usize,
    mine: Mine,
) -> Result<Vec<f64>, Unmeasured> {
    let flat = limits(s, d, across, rules, h, mine);
    split(lines, &flat, max_lines)
}

/// Depth-first bisection of the intervals between `starts` until `flat` holds for each; `flat`'s
/// error ends it.
fn split(
    starts: &[f64],
    flat: &dyn Fn(f64, f64) -> Result<bool, Unmeasured>,
    max_lines: usize,
) -> Result<Vec<f64>, Unmeasured> {
    let Some(&first) = starts.first() else {
        return Ok(Vec::new());
    };
    let mut out = vec![first];
    let mut stack: Vec<(f64, f64, u32)> = starts
        .iter()
        .zip(starts.iter().skip(1))
        .rev()
        .map(|(&a, &b)| (a, b, 0))
        .collect();
    while let Some((a, b, depth)) = stack.pop() {
        if depth >= 20 || out.len() + stack.len() >= max_lines.max(2) || flat(a, b)? {
            out.push(b);
        } else {
            let m = 0.5 * (a + b);
            stack.push((m, b, depth + 1));
            stack.push((a, m, depth + 1));
        }
    }
    Ok(out)
}

/// Whether the surface between the parameter points `a` and `b` is within the tolerance (at
/// their middle), the side length `h` and the facet angle: no point is needed between them. The
/// parameters of a point the check needs and the surface does not evaluate at are the error.
pub(crate) fn fine_between(
    s: &EvalSurface,
    rules: &Rules,
    h: f64,
    a: [f64; 2],
    b: [f64; 2],
) -> Result<bool, NoValue> {
    let at = |q: [f64; 2]| {
        s.derivs(q[0], q[1])
            .map(kernel::record_triple)
            .ok_or(NoValue(q))
    };
    let ((pa, _, _), (pb, _, _)) = (at(a)?, at(b)?);
    if pa.dist(pb) > h * (1.0 + 1e-9) {
        return Ok(false);
    }
    let mid = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
    let sag = at(mid)?.0.dist((pa + pb) * 0.5);
    if sag > rules.tol {
        return Ok(false);
    }
    let Some(t) = rules.turn else { return Ok(true) };
    // At a pole the normal is undefined: take it from just inside the segment.
    let normal = |q: [f64; 2], toward: [f64; 2]| -> Result<Option<Vec3>, NoValue> {
        let n = |q: [f64; 2]| at(q).map(|(_, su, sv)| su.cross(sv).normalized());
        match n(q)? {
            Some(n) => Ok(Some(n)),
            None => n([
                q[0] + 1e-4 * (toward[0] - q[0]),
                q[1] + 1e-4 * (toward[1] - q[1]),
            ]),
        }
    };
    Ok(match (normal(a, b)?, normal(b, a)?) {
        // Proven: the clamp bounds are the constants -1.0 <= 1.0, neither NaN.
        (Some(na), Some(nb)) => !t.exceeds(na.dot(nb).clamp(-1.0, 1.0).acos(), sag),
        _ => true,
    })
}

/// An iso-line's extent below which it is a point, as a fraction of the chord tolerance (which
/// is never finer than a millionth of the body, so this is far above rounding).
const POINT: f64 = 1e-6;

/// Whether the surface between the iso-lines at `a` and `b` (direction `d`, sampled along
/// iso-lines across `across`) is within the tolerance, the side length `h` and the facet angle,
/// and spans at most a quarter turn of each iso-line (as an edge segment does, see
/// [`QUARTER_TURN`]): a turn read from the points, since a spline's parameter is no angle. A point
/// the surface does not evaluate at is the error where it is the face's (`mine`), and else
/// measures nothing: an end of an iso-line's interval there is moved inwards, in [`INWARD`]
/// steps, to the nearest step that is the face's, and the interval is measured from there (the
/// surface having no point there refuses the face); an iso-line with no step that is the face's
/// is not measured (a part of the face narrower than a step is left to the sag refinement), and
/// a point between the ends is not taken into its sag, turn or facet angle.
fn limits<'a>(
    s: &'a EvalSurface,
    d: usize,
    across: (f64, f64),
    rules: &'a Rules,
    h: f64,
    mine: Mine<'a>,
) -> impl Fn(f64, f64) -> Result<bool, Unmeasured> + 'a {
    const ACROSS: usize = 5;
    let tol = rules.tol;
    let uv = move |x: f64, y: f64| if d == 0 { [x, y] } else { [y, x] };
    let derivs = move |x: f64, y: f64| {
        let q = uv(x, y);
        s.derivs(q[0], q[1])
            .map(kernel::record_triple)
            .ok_or(NoValue(q))
    };
    let at = move |x: f64, y: f64| derivs(x, y).map(|(p, _, _)| p);
    // `None` at a pole, where the normal is undefined.
    let normal = move |x: f64, y: f64| derivs(x, y).map(|(_, su, sv)| su.cross(sv).normalized());
    let ys: Vec<f64> = (0..ACROSS)
        .map(|k| across.0 + (across.1 - across.0) * k as f64 / (ACROSS - 1) as f64)
        .collect();
    // The end `from` of an interval towards `to` on the iso-line at `y`: itself where it
    // evaluates, else the nearest step inwards that is the face's; `None` where no step is.
    let end = move |from: f64, to: f64, y: f64| -> Result<Option<(f64, Vec3)>, Unmeasured> {
        if let Some(p) = kept(mine, at(from, y))? {
            return Ok(Some((from, p)));
        }
        for k in 1..INWARD {
            let x = from + (to - from) * f64::from(k) / f64::from(INWARD);
            let q = uv(x, y);
            // A step that is the face's is where the interval is measured from, or, the surface
            // having no point there, the face's refusal (as `kept` makes it); one that is not the
            // face's is passed over.
            if mine(q).map_err(|c| Unmeasured::TooManyCopies(c, q))? {
                return at(x, y).map(|p| Some((x, p))).map_err(Unmeasured::NoValue);
            }
        }
        Ok(None)
    };
    move |a0: f64, b0: f64| {
        for &y in &ys {
            let Some((a, pa)) = end(a0, b0, y)? else {
                continue;
            };
            let Some((b, pb)) = end(b0, a, y)? else {
                continue;
            };
            if pa.dist(pb) > h * (1.0 + 1e-9) {
                return Ok(false);
            }
            let mut sag: f64 = 0.0;
            for f in [0.25, 0.5, 0.75] {
                if let Some(p) = kept(mine, at(a + (b - a) * f, y))? {
                    sag = sag.max(p.dist_to_segment(pa, pb));
                }
            }
            if sag > tol {
                return Ok(false);
            }
            // The two half chords of an arc turning θ differ by θ/2; a closed iso-line's by π.
            // An iso-line within a millionth of the tolerance of one point (a pole) turns no
            // amount: its directions are rounding.
            if let Some(pm) = kept(mine, at(0.5 * (a + b), y))?
                && pa.dist(pm).max(pm.dist(pb)) > POINT * tol
                && 2.0 * turn(pm - pa, pb - pm) > QUARTER_TURN
            {
                return Ok(false);
            }
            let Some(t) = rules.turn else { continue };
            // At a pole the normal is undefined: take it from just inside the segment.
            let normal_near = |x: f64, inward: f64| -> Result<Option<Vec3>, Unmeasured> {
                Ok(match kept(mine, normal(x, y))? {
                    Some(Some(n)) => Some(n),
                    Some(None) => kept(mine, normal(x + inward, y))?.flatten(),
                    None => None,
                })
            };
            let na = normal_near(a, 1e-4 * (b - a))?;
            let nb = normal_near(b, -1e-4 * (b - a))?;
            if let (Some(na), Some(nb)) = (na, nb)
                // Proven: the clamp bounds are the constants -1.0 <= 1.0, neither NaN.
                && t.exceeds(na.dot(nb).clamp(-1.0, 1.0).acos(), sag)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::eval::EvalCurve;

    fn cylinder() -> EvalSurface {
        EvalSurface::Revolved {
            profile: EvalCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![[3.0, 0.0, 0.0, 1.0], [3.0, 0.0, 4.0, 1.0]],
            ),
            origin: solidcore::Vec3::ZERO,
            axis: solidcore::Vec3::Z,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            swap: false,
        }
    }

    #[test]
    fn divides_the_turn_but_not_the_straight_direction() {
        let s = cylinder();
        let turn = divide(
            &s,
            1,
            (0.0, std::f64::consts::TAU),
            (0.0, 1.0),
            &Rules {
                tol: 0.01,
                max_len: f64::INFINITY,
                turn: None,
            },
            f64::INFINITY,
            10_000,
            &everywhere,
        )
        .unwrap();
        let along = divide(
            &s,
            0,
            (0.0, 1.0),
            (0.0, std::f64::consts::TAU),
            &Rules {
                tol: 0.01,
                max_len: f64::INFINITY,
                turn: None,
            },
            f64::INFINITY,
            10_000,
            &everywhere,
        )
        .unwrap();
        assert_eq!(along, vec![0.0, 1.0]);
        // Chord sag r(1 − cos(θ/2)) ≤ 0.01 with r = 3 needs θ ≤ 0.163: at least 39 segments.
        assert!(turn.len() >= 40 && turn.len() < 80, "{}", turn.len());
        // With a longest side the straight direction is divided too: 4 mm in sides ≤ 0.5.
        let along = divide(
            &s,
            0,
            (0.0, 1.0),
            (0.0, std::f64::consts::TAU),
            &Rules {
                tol: 0.01,
                max_len: f64::INFINITY,
                turn: None,
            },
            0.5,
            10_000,
            &everywhere,
        )
        .unwrap();
        assert_eq!(along.len(), 9);
    }

    /// A quarter of a cylinder as a rational quadratic spline (as SMLib holds a bend), its range a
    /// rounding over π/2 as a boundary's pcurves give it: the quarter turn is read from the
    /// surface, not the parameter, so at a tolerance above its sag it is one interval; and a whole
    /// turn is still divided into quarters however coarse the tolerance.
    #[test]
    fn a_quarter_turn_is_read_from_the_surface_not_the_parameter() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let q = std::f64::consts::FRAC_PI_2;
        let s = EvalSurface::nurbs(
            [1, 2],
            [vec![0.0, 0.0, 40.0, 40.0], vec![0.0, 0.0, 0.0, q, q, q]],
            [2, 3],
            vec![
                [1.0, 0.0, 0.0, 1.0],
                [1.0, 0.0, 40.0, 1.0],
                [w, w, 0.0, w],
                [w, w, 40.0 * w, w],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 1.0, 40.0, 1.0],
            ],
        );
        let coarse = Rules {
            tol: 3.0,
            max_len: f64::INFINITY,
            turn: None,
        };
        assert_eq!(
            divide(
                &s,
                1,
                (-6.7e-13, q),
                (0.0, 40.0),
                &coarse,
                f64::INFINITY,
                10_000,
                &everywhere
            )
            .unwrap(),
            vec![-6.7e-13, q]
        );
        let whole = divide(
            &cylinder(),
            1,
            (0.0, std::f64::consts::TAU),
            (0.0, 1.0),
            &Rules {
                tol: 100.0,
                ..coarse
            },
            f64::INFINITY,
            10_000,
            &everywhere,
        )
        .unwrap();
        assert_eq!(whole.len(), 5, "{whole:?}");
    }

    /// The first and last lines are the range's ends to the last bit: the boundary nodes sit on
    /// them, and a line a rounding inside would be taken for an interior one (a drill point's
    /// cone, where a0 + (a1 − a0) is not a1).
    #[test]
    fn divides_with_the_ends_exactly() {
        let (a0, a1) = (-1.010485317689877, 1.083909784703316);
        let lines = divide(
            &cylinder(),
            1,
            (a0, a1),
            (0.0, 1.0),
            &Rules {
                tol: 0.01,
                max_len: f64::INFINITY,
                turn: None,
            },
            f64::INFINITY,
            10_000,
            &everywhere,
        )
        .unwrap();
        assert_eq!(lines.first().copied(), Some(a0));
        assert_eq!(lines.last().copied(), Some(a1));
    }
}
