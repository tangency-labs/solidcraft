//! Exact interval extraction and control-budget splitting for projected splines.
//!
//! Not upstream: moved from SolidCraft's spline projection. Boehm insertion keeps
//! both blend factors as knot differences, retaining small factors beside heavy weights.
//! References: Piegl and Tiller, The NURBS Book, A5.1 and §5.3.
//! Deviations: no upstream counterpart. No recursion; degree and net sizes are bounded.

use crate::error::{Result, SolidError};
use crate::eval::{Curve, Frame};
use crate::math::Vec3;
use crate::prism::read::{PlanarPiece, PlanarSpline, Run};

const OP: &str = "curve::spline_projection";
const MAX_DEGREE: usize = 32;
const MAX_CONTROLS: usize = 8192;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// Validate the clamped knot domain and positive controls before any geometric construction.
fn validate(degree: usize, knots: &[f64], ctrl: &[[f64; 4]]) -> Result<()> {
    let count = ctrl.len();
    if !(1..=MAX_DEGREE).contains(&degree)
        || count <= degree
        || count > MAX_CONTROLS
        || knots.len() != count + degree + 1
        || knots.iter().any(|t| !t.is_finite())
        || knots.windows(2).any(|w| matches!(w, [a,b] if a > b))
        || ctrl
            .iter()
            .any(|p| p.iter().any(|v| !v.is_finite()) || p[3] <= 0.0)
    {
        return Err(invalid("invalid positive-weight spline"));
    }
    let lo = *knots
        .get(degree)
        .ok_or_else(|| invalid("missing domain start"))?;
    let hi = *knots
        .get(count)
        .ok_or_else(|| invalid("missing domain end"))?;
    if !(lo < hi && (hi - lo).is_finite()) {
        return Err(invalid("invalid spline domain"));
    }
    Ok(())
}

/// Insert one knot with both coefficients computed from differences, never `1-alpha`.
/// The curve and its parameter are unchanged to rounding.
pub fn insert_knot(
    degree: usize,
    knots: &[f64],
    ctrl: &[[f64; 4]],
    u: f64,
) -> Result<(Vec<f64>, Vec<[f64; 4]>)> {
    validate(degree, knots, ctrl)?;
    let n = ctrl.len();
    let lo = *knots.get(degree).ok_or_else(|| invalid("domain"))?;
    let hi = *knots.get(n).ok_or_else(|| invalid("domain"))?;
    if !(u.is_finite() && lo <= u && u <= hi) || n == MAX_CONTROLS {
        return Err(invalid("knot outside domain or net budget exceeded"));
    }
    let mut span = if u == hi {
        n - 1
    } else {
        let active = knots
            .get(degree..n)
            .ok_or_else(|| invalid("active knots"))?;
        degree
            + active
                .partition_point(|t| *t <= u)
                .checked_sub(1)
                .ok_or_else(|| invalid("knot span"))?
    };
    while span > degree && knots.get(span) >= knots.get(span + 1) {
        span -= 1;
    }
    let point = |i| {
        ctrl.get(i)
            .copied()
            .ok_or_else(|| invalid("insertion control"))
    };
    let knot = |i| {
        knots
            .get(i)
            .copied()
            .ok_or_else(|| invalid("insertion knot"))
    };
    let mut out = Vec::new();
    for i in 0..=n {
        let p = if i + degree <= span {
            point(i)?
        } else if i > span {
            point(i - 1)?
        } else {
            let (a, b) = (knot(i)?, knot(i + degree)?);
            if b > a {
                let alpha = (u - a) / (b - a);
                let beta = (b - u) / (b - a);
                let (p, q) = (point(i)?, point(i - 1)?);
                [
                    p[0] * alpha + q[0] * beta,
                    p[1] * alpha + q[1] * beta,
                    p[2] * alpha + q[2] * beta,
                    p[3] * alpha + q[3] * beta,
                ]
            } else {
                point(i)?
            }
        };
        if p.iter().any(|x| !x.is_finite()) || p[3] <= 0.0 {
            return Err(invalid("unrepresentable inserted control"));
        }
        out.push(p);
    }
    let mut new_knots = Vec::new();
    new_knots.extend_from_slice(knots.get(..=span).ok_or_else(|| invalid("knot prefix"))?);
    new_knots.push(u);
    new_knots.extend_from_slice(
        knots
            .get(span + 1..)
            .ok_or_else(|| invalid("knot suffix"))?,
    );
    Ok((new_knots, out))
}

/// Extract `[a,b]` without snapping its endpoints onto nearby knots.
/// Every retained point has the source's parameter and homogeneous weight.
pub fn segment(
    degree: usize,
    knots: &[f64],
    ctrl: &[[f64; 4]],
    a: f64,
    b: f64,
) -> Result<(Vec<f64>, Vec<[f64; 4]>)> {
    validate(degree, knots, ctrl)?;
    if !(knots.get(degree).is_some_and(|lo| *lo <= a)
        && a < b
        && knots.get(ctrl.len()).is_some_and(|hi| b <= *hi))
    {
        return Err(invalid("interval outside spline domain"));
    }
    let (mut k, mut c) = (knots.to_vec(), ctrl.to_vec());
    for t in [a, b] {
        for _ in 0..degree {
            if k.iter().filter(|x| **x == t).count() >= degree {
                break;
            }
            (k, c) = insert_knot(degree, &k, &c, t)?;
        }
    }
    slice_segment(degree, &k, &c, a, b)
}

/// Extract an interval after its boundary knots have reached their required multiplicities.
fn slice_segment(
    degree: usize,
    k: &[f64],
    c: &[[f64; 4]],
    a: f64,
    b: f64,
) -> Result<(Vec<f64>, Vec<[f64; 4]>)> {
    let j0 = k
        .iter()
        .rposition(|x| *x == a)
        .ok_or_else(|| invalid("missing segment start"))?;
    let j1 = k
        .iter()
        .position(|x| *x == b)
        .ok_or_else(|| invalid("missing segment end"))?;
    let start = j0
        .checked_sub(degree)
        .ok_or_else(|| invalid("invalid segment start"))?;
    let points = c
        .get(start..j1)
        .ok_or_else(|| invalid("invalid segment controls"))?
        .to_vec();
    let inner = k
        .get(j0 + 1..j1)
        .ok_or_else(|| invalid("invalid segment knots"))?;
    let knots: Vec<_> = std::iter::repeat_n(a, degree + 1)
        .chain(inner.iter().copied())
        .chain(std::iter::repeat_n(b, degree + 1))
        .collect();
    if knots.len() != points.len() + degree + 1 {
        return Err(invalid("invalid segment counts"));
    }
    Ok((knots, points))
}

/// Decompose a positive-weight spline into exact homogeneous Bézier spans.
pub fn bezier_spans(degree: usize, knots: &[f64], ctrl: &[[f64; 4]]) -> Result<Vec<Vec<[f64; 4]>>> {
    validate(degree, knots, ctrl)?;
    let a = *knots.get(degree).ok_or_else(|| invalid("domain"))?;
    let b = *knots.get(ctrl.len()).ok_or_else(|| invalid("domain"))?;
    let (mut k, mut c) = segment(degree, knots, ctrl, a, b)?;
    let mut interior: Vec<_> = k.iter().copied().filter(|t| *t > a && *t < b).collect();
    interior.dedup();
    for u in interior {
        for _ in 0..degree {
            if k.iter().filter(|t| **t == u).count() >= degree {
                break;
            }
            (k, c) = insert_knot(degree, &k, &c, u)?;
        }
    }
    let mut spans = Vec::new();
    for i in degree..c.len() {
        if k.get(i) < k.get(i + 1) {
            spans.push(
                c.get(i - degree..=i)
                    .ok_or_else(|| invalid("invalid Bezier span"))?
                    .to_vec(),
            );
        }
    }
    Ok(spans)
}

/// The knots and homogeneous controls of one clamped spline.
pub type ClampedSpline = (Vec<f64>, Vec<[f64; 4]>);

/// Split at existing knots to keep each piece within `limit` controls.
/// Cut knots are inserted on the whole curve before slicing, so joins share a control exactly.
pub fn split_clamped(
    degree: usize,
    knots: &[f64],
    ctrl: &[[f64; 4]],
    limit: usize,
) -> Result<Vec<ClampedSpline>> {
    validate(degree, knots, ctrl)?;
    if limit <= degree {
        return Err(invalid("control budget smaller than one span"));
    }
    let lo = *knots.get(degree).ok_or_else(|| invalid("domain start"))?;
    let hi = *knots.get(ctrl.len()).ok_or_else(|| invalid("domain end"))?;
    if knots.iter().take(degree + 1).any(|t| *t != lo)
        || knots.iter().skip(ctrl.len()).any(|t| *t != hi)
    {
        return Err(invalid("splitting requires a clamped spline"));
    }
    let mut cuts = vec![lo];
    let mut count = degree + 1;
    let mut i = degree + 1;
    while i < ctrl.len() {
        let u = *knots.get(i).ok_or_else(|| invalid("split knot"))?;
        let mut end = i + 1;
        while end < ctrl.len() && knots.get(end) == Some(&u) {
            end += 1;
        }
        let multiplicity = end - i;
        if u <= lo || u >= hi || multiplicity > degree {
            return Err(invalid("invalid interior split knot"));
        }
        if count + multiplicity > limit {
            cuts.push(u);
            count = degree + 1;
        } else {
            count += multiplicity;
        }
        i = end;
    }
    cuts.push(hi);
    let (mut k, mut c) = (knots.to_vec(), ctrl.to_vec());
    for u in cuts.iter().copied().skip(1).take(cuts.len() - 2) {
        for _ in 0..degree {
            if k.iter().filter(|t| **t == u).count() >= degree {
                break;
            }
            (k, c) = insert_knot(degree, &k, &c, u)?;
        }
    }
    cuts.windows(2)
        .map(|ab| {
            let [a, b] = ab else {
                return Err(invalid("invalid cut interval"));
            };
            let piece = slice_segment(degree, &k, &c, *a, *b)?;
            if piece.1.len() > limit {
                return Err(invalid("split exceeded control budget"));
            }
            Ok(piece)
        })
        .collect()
}

/// Split a planar spline while retaining its projected controls, weights and parameter.
pub fn split_planar(s: &PlanarSpline, limit: usize) -> Result<Vec<PlanarSpline>> {
    if !s.weights.is_empty() && s.weights.len() != s.points.len() {
        return Err(invalid("weight count"));
    }
    let controls: Vec<_> = s
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = s.weights.get(i).copied().unwrap_or(1.0);
            [p.x * w, p.y * w, 0.0, w]
        })
        .collect();
    split_clamped(s.degree, &s.knots, &controls, limit)?
        .into_iter()
        .map(|(knots, ctrl)| {
            let points = ctrl
                .iter()
                .map(|p| crate::math::Vec2::new(p[0] / p[3], p[1] / p[3]))
                .collect();
            let weights = if s.weights.is_empty() {
                Vec::new()
            } else {
                ctrl.iter().map(|p| p[3]).collect()
            };
            Ok(PlanarSpline {
                degree: s.degree,
                knots,
                points,
                weights,
            })
        })
        .collect()
}

/// Project one native curve interval, preserving its selected direction and exact form.
/// The analytic decisions and their whole-span certificates are those of prism reading.
pub fn project_curve(
    curve: &Curve,
    interval: [f64; 2],
    forward: bool,
    ends: [Vec3; 2],
    frame: &Frame,
    tol: f64,
    work: &mut usize,
) -> Option<Vec<PlanarPiece>> {
    let k = crate::Kernel::default();
    let native = crate::curve::from_parameters::from_parameters(&k, curve, interval).ok()?;
    let run = Run {
        curve: &native.curve,
        interval: native.interval,
        forward,
        ends,
    };
    crate::prism::read::planar_curves(&k, &run, frame, tol, work)
}

/// Propose a source plane from samples and project only its whole-span-certified arcs.
/// Samples never decide a circle's closure, radius or trimmed extent.
pub fn circle_projection(
    curve: &Curve,
    interval: [f64; 2],
    ends: [Vec3; 2],
    target: &Frame,
    tol: f64,
    max_coord: f64,
) -> Result<Option<Vec<super::conic_projection::Projection>>> {
    let Ok(eval) = crate::eval::CurveEval::new(curve) else {
        return Ok(None);
    };
    let points = (0..=16)
        .map(|i| eval.point(interval[0] + (interval[1] - interval[0]) * f64::from(i) / 16.0))
        .collect::<Option<Vec<_>>>();
    let Some(source) = points.and_then(|points| super::conic_projection::circle_plane(&points))
    else {
        return Ok(None);
    };
    let Some(certified) = project_curve(curve, interval, true, ends, &source, tol, &mut 262_144)
    else {
        return Ok(None);
    };
    super::conic_projection::project_arcs(target, &source, &certified, max_coord)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{CurveEval, EvalCurve};
    use crate::math::Vec2;
    use crate::prism::profile::to_local;

    fn at(c: &Curve, t: f64) -> Vec3 {
        CurveEval::new(c).unwrap().point(t).unwrap()
    }
    fn frame() -> Frame {
        Frame {
            origin: [0.0; 3],
            x_axis: [1.0, 0.0, 0.0],
            y_axis: [0.0, 1.0, 0.0],
        }
    }
    fn project(
        c: &Curve,
        interval: [f64; 2],
        forward: bool,
        frame: &Frame,
    ) -> Option<Vec<PlanarPiece>> {
        let ends = [at(c, interval[0]), at(c, interval[1])];
        let ends = if forward { ends } else { [ends[1], ends[0]] };
        project_curve(c, interval, forward, ends, frame, 1e-6, &mut 262_144)
    }
    fn projected_curve(s: &PlanarSpline) -> Curve {
        Curve::RationalSpline {
            knots: s.knots.clone(),
            homogeneous: s
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let w = s.weights.get(i).copied().unwrap_or(1.0);
                    [p.x * w, p.y * w, 0.0, w]
                })
                .collect(),
        }
    }

    #[test]
    fn planar_rational_spline_keeps_unequal_endpoint_weights_and_speed() {
        let curve = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.0, 20.0, 0.0, 2.0],
                [40.0, 40.0, 0.0, 4.0],
            ],
        };
        let pieces = project(&curve, [0.0, 1.0], true, &frame()).unwrap();
        let [PlanarPiece::Spline(s)] = pieces.as_slice() else {
            panic!("source NURBS must remain a spline: {pieces:?}")
        };
        let projected = projected_curve(s);
        for i in 0..=32 {
            let t = f64::from(i) / 32.0;
            assert!(at(&curve, t).distance(at(&projected, t)) < 1e-12);
        }
        assert!(at(&projected, 0.5).distance(Vec3::new(40.0 / 9.0, 80.0 / 9.0, 0.0)) < 1e-12);
    }

    #[test]
    fn tilted_spline_projection_preserves_the_selected_interval() {
        let frame = Frame {
            origin: [2.0, 3.0, 4.0],
            x_axis: [0.8, 0.0, 0.6],
            y_axis: [0.0, 1.0, 0.0],
        };
        for multispan in [false, true] {
            let mut points = vec![
                [0.0, 0.0, 0.0],
                [6.0, 18.0, 5.0],
                [18.0, -12.0, -3.0],
                [40.0, 9.0, 12.0],
            ];
            let knots = if multispan {
                points.extend([[48.0, -4.0, 3.0], [55.0, 2.0, 8.0]]);
                vec![0.0, 0.0, 0.0, 0.0, 0.3, 0.7, 1.0, 1.0, 1.0, 1.0]
            } else {
                vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]
            };
            for rational in [false, true] {
                let curve = if rational {
                    Curve::RationalSpline {
                        knots: knots.clone(),
                        homogeneous: points
                            .iter()
                            .zip([1.0, 0.8, 1.4, 1.0, 0.9, 1.2])
                            .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, w])
                            .collect(),
                    }
                } else {
                    Curve::Spline {
                        knots: knots.clone(),
                        points: points.clone(),
                    }
                };
                for interval in [[0.0, 1.0], [0.2, 0.8], [0.3, 0.7], [0.35, 0.65]] {
                    for forward in [false, true] {
                        let pieces = project(&curve, interval, forward, &frame);
                        assert!(
                            pieces.is_some(),
                            "a spline must project exactly over {interval:?}, forward={forward}"
                        );
                        let pieces = pieces.unwrap();
                        let [PlanarPiece::Spline(s)] = pieces.as_slice() else {
                            panic!("expected one spline: {pieces:?}")
                        };
                        let projected = projected_curve(s);
                        let (lo, hi) = (s.knots[s.degree], s.knots[s.points.len()]);
                        for i in 0..=32 {
                            let u = f64::from(i) / 32.0;
                            let t = if forward { u } else { 1.0 - u };
                            let expected = to_local(
                                &frame,
                                at(&curve, interval[0] + (interval[1] - interval[0]) * t),
                            );
                            let actual = at(&projected, lo + (hi - lo) * u);
                            assert!(
                                Vec2::new(actual.x, actual.y).distance(expected) < 1e-8,
                                "interval={interval:?}, forward={forward}, u={u}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn splitting_preserves_knots_geometry_and_shared_endpoints() {
        let knots = [0.0, 0.0, 0.0, 0.0, 0.2, 0.7, 0.7, 1.0, 1.0, 1.0, 1.0];
        let controls: Vec<_> = (0..7)
            .map(|i| {
                let x = f64::from(i);
                let w = 1.0 + x * 0.1;
                [x * w, x.sin() * w, 0.0, w]
            })
            .collect();
        let pieces = split_clamped(3, &knots, &controls, 5).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].1.last(), pieces[1].1.first());
        let source = EvalCurve::new(3, knots.to_vec(), controls.clone());
        for (knots, controls) in &pieces {
            assert!(controls.len() <= 5);
            let (a, b) = (knots[3], knots[controls.len()]);
            let curve = EvalCurve::new(3, knots.clone(), controls.clone());
            for i in 0..=64 {
                let t = a + (b - a) * f64::from(i) / 64.0;
                assert!(source.eval(t).unwrap().0.distance(curve.eval(t).unwrap().0) < 1e-12);
            }
        }
        let controls = [0.0, 1.0, 2.0, 3.0].map(|x| [x, 0.0, 0.0, 1.0]);
        assert!(split_clamped(
            2,
            &[-1e308, -1e308, -1e308, 0.0, 1e308, 1e308, 1e308],
            &controls,
            3
        )
        .is_err());
    }

    #[test]
    fn heavy_weight_knot_insertion_keeps_the_small_blend_factor() {
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let controls = [
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 1e12, 0.0, 1e12],
            [10.0, 0.0, 0.0, 1.0],
        ];
        let u = 1.0 - f64::EPSILON;
        let (k, c) = insert_knot(2, &knots, &controls, u).unwrap();
        let source = EvalCurve::new(2, knots.to_vec(), controls.to_vec());
        let curve = EvalCurve::new(2, k, c);
        for t in [0.0, 0.2, 0.7, u, 1.0] {
            assert!(source.eval(t).unwrap().0.distance(curve.eval(t).unwrap().0) < 1e-11);
        }
        let (_, trim) = segment(2, &knots, &controls, 0.0, u).unwrap();
        assert!(trim.last().unwrap()[3] > 1.0);
    }
}
