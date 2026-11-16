//! Record curves as sweep curves ([`Curve::from_record`]): each kind of the record's curves over
//! an interval of its parameter as the sweep curves that are it, exactly. The geometry is the
//! kernel's: a line's ends and a polyline's corners are its points
//! (`solidcore::curve::from_parameters`), a circle's arc its definition
//! (`solidcore::curve::arc_of`), a spline's or an ellipse's net its B-spline cut to the interval
//! (`BSplineCurve::trim`). Ops checks, sequences and converts.

use solidcore::curve::arc_of::arc_of_circle;
use solidcore::curve::curve::{Curve as KCurve, CurveKind};
use solidcore::curve::from_parameters::{from_parameters, EdgeCurve};
use solidcore::eval::Curve as Eval;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::SolidError;
use solidcore_record::brep;

use super::{argument_or_status, Curve, Out};
use crate::session::run;
use crate::{Error, Result};

fn coords(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// The kernel's curve of `record` over `interval`; a refusal of its numbers is
/// [`Error::Argument`].
fn made(k: &Kernel, record: &Eval, interval: [f64; 2]) -> solidcore::Result<Out<EdgeCurve>> {
    match from_parameters(k, record, interval) {
        Ok(made) => Ok(Ok(made)),
        Err(SolidError::InvalidInput { .. }) => Ok(Err(Error::Argument)),
        Err(e) => Err(e),
    }
}

/// The point of `curve` at `t`.
fn point(k: &Kernel, curve: &KCurve, t: f64) -> solidcore::Result<[f64; 3]> {
    curve.evaluate_point(k, t).map(coords)
}

/// `b` cut to `interval` (its own parameter) as a [`Curve::Nurbs`]: its degree, its Euclidean
/// control points with their weights (1 for a polynomial curve) and its whole knot vector.
fn nurbs(k: &Kernel, b: &BSplineCurve, made: &EdgeCurve) -> solidcore::Result<Out<Curve>> {
    let mut b = b.clone();
    let mut interval = made.interval;
    if let Err(e) = b.trim(k, &mut interval, TrimMode::Silent) {
        return Ok(Err(argument_or_status(e)));
    }
    let net = b.canonical()?;
    let points: Vec<[f64; 4]> = if net.weights.is_empty() {
        net.control_points
            .iter()
            .map(|p| [p.x, p.y, p.z, 1.0])
            .collect()
    } else {
        net.control_points
            .iter()
            .zip(&net.weights)
            .map(|(p, w)| [p.x, p.y, p.z, *w])
            .collect()
    };
    let knots: Vec<f64> = net
        .knots
        .iter()
        .zip(&net.knot_multiplicities)
        .flat_map(|(u, m)| std::iter::repeat_n(*u, *m))
        .collect();
    Ok(Ok(Curve::Nurbs {
        degree: net.degree,
        points,
        knots,
    }))
}

impl Curve {
    /// The record's `curve` over `interval` of its parameter as the sweep curves that are it,
    /// exactly, in order from the interval's start: a line as one [`Curve::Line`] between its
    /// points at the interval's ends; a polyline as a line for each of its segments the
    /// interval reaches (cut where the interval ends inside one; the interval within its range); a circle as one
    /// [`Curve::Arc`] over the same angles (at most a full turn); an ellipse, a spline or a
    /// rational spline as one [`Curve::Nurbs`], its B-spline cut to the interval.
    ///
    /// # Errors
    /// [`Error::Argument`] for an interval that is not finite and increasing (or, on a polyline,
    /// past its range), a traced
    /// intersection curve (no exact sweep curve), a circle's interval past a full turn, or
    /// numbers the kernel refuses; the kernel's status.
    pub fn from_record(curve: &brep::Curve, interval: [f64; 2]) -> Result<Vec<Curve>> {
        let [lo, hi] = interval;
        if !(lo.is_finite() && hi.is_finite() && lo < hi) {
            return Err(Error::Argument);
        }
        if matches!(curve, brep::Curve::SurfaceIntersection { .. }) {
            return Err(Error::Argument);
        }
        let Ok(record) = solidcore_convert::curve(curve) else {
            return Err(Error::Argument);
        };
        run("sweep::Curve::from_record", |k| {
            if let Eval::Circle { frame, radius } = &record {
                return Ok(match arc_of_circle(frame, *radius, interval) {
                    Ok(arc) => Ok(vec![Curve::Arc {
                        center: coords(arc.centre),
                        axis: coords(arc.axis),
                        reference: coords(arc.reference),
                        radius: arc.radius,
                        start: arc.start,
                        end: arc.end,
                    }]),
                    Err(e) => Err(argument_or_status(e)),
                });
            }
            let made = match made(k, &record, interval)? {
                Ok(made) => made,
                Err(e) => return Ok(Err(e)),
            };
            match (&record, made.curve.kind()) {
                (Eval::Line { .. }, _) => Ok(Ok(vec![Curve::Line {
                    start: point(k, &made.curve, lo)?,
                    end: point(k, &made.curve, hi)?,
                }])),
                (Eval::Polyline { points }, _) => {
                    // the interval within the polyline's range [0, n - 1]: a curve shorter than
                    // asked is no answer
                    let last = points.len().saturating_sub(1) as f64;
                    if lo < 0.0 || hi > last {
                        return Ok(Err(Error::Argument));
                    }
                    // segment i spans [i, i + 1]: those the interval reaches, cut to it
                    let mut lines = Vec::new();
                    for i in 0..points.len().saturating_sub(1) {
                        let (a, b) = (i as f64, (i + 1) as f64);
                        let (a, b) = (a.max(lo), b.min(hi));
                        if a < b {
                            lines.push(Curve::Line {
                                start: point(k, &made.curve, a)?,
                                end: point(k, &made.curve, b)?,
                            });
                        }
                    }
                    Ok(if lines.is_empty() {
                        Err(Error::Argument)
                    } else {
                        Ok(lines)
                    })
                }
                (_, CurveKind::BSpline(b)) => Ok(nurbs(k, b, &made)?.map(|c| vec![c])),
                (_, CurveKind::Ellipse(e)) => Ok(nurbs(k, e.base(), &made)?.map(|c| vec![c])),
                _ => Ok(Err(Error::Argument)),
            }
        })?
    }
}

#[cfg(test)]
// The tests check against closed forms of the record curves (cos, sin, squares); ops itself computes none.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use solidcore_record::brep::{Curve as Record, Frame};

    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    /// The point of a record curve at `t`, by the kernel's evaluator.
    fn record_at(c: &Record, t: f64) -> [f64; 3] {
        let e = solidcore::eval::CurveEval::new(&solidcore_convert::curve(c).unwrap()).unwrap();
        let p = e.point(t).unwrap();
        [p.x, p.y, p.z]
    }

    /// The point of one sweep curve at the fraction `s` of it, through the record's own
    /// kinds (a NURBS as a rational spline, an arc by its angle).
    fn sweep_at(c: &Curve, s: f64) -> [f64; 3] {
        match c {
            Curve::Line { start, end } => [0, 1, 2].map(|i| start[i] + s * (end[i] - start[i])),
            Curve::Arc {
                center,
                axis,
                reference,
                radius,
                start,
                end,
            } => {
                let t = start + s * (end - start);
                let y = [
                    axis[1] * reference[2] - axis[2] * reference[1],
                    axis[2] * reference[0] - axis[0] * reference[2],
                    axis[0] * reference[1] - axis[1] * reference[0],
                ];
                [0, 1, 2].map(|i| center[i] + radius * (t.cos() * reference[i] + t.sin() * y[i]))
            }
            Curve::Nurbs { points, knots, .. } => {
                let homogeneous = points
                    .iter()
                    .map(|p| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
                    .collect();
                let r = Record::RationalSpline {
                    knots: knots.clone(),
                    homogeneous,
                };
                let (a, b) = (knots[0], knots[knots.len() - 1]);
                record_at(&r, a + s * (b - a))
            }
        }
    }

    /// The pieces' points at fractions of each against the record's at the same parameter
    /// (each piece spanning `spans[i]` of the record's parameter, linearly for lines and
    /// arcs; the NURBS keep the record's parameter).
    fn same(record: &Record, pieces: &[Curve], spans: &[[f64; 2]]) {
        assert_eq!(pieces.len(), spans.len(), "{pieces:?}");
        for (c, [a, b]) in pieces.iter().zip(spans) {
            for i in 0..=8 {
                let s = f64::from(i) / 8.0;
                let (p, q) = (sweep_at(c, s), record_at(record, a + s * (b - a)));
                assert!(near(p, q), "{c:?} at {s}: {p:?} against {q:?}");
            }
        }
    }

    #[test]
    fn a_line_is_its_points_at_the_interval_ends() {
        let r = Record::Line {
            start: [1.0, 2.0, 3.0],
            end: [4.0, 6.0, 3.0],
        };
        let c = Curve::from_record(&r, [-1.0, 2.0]).unwrap();
        same(&r, &c, &[[-1.0, 2.0]]);
    }

    #[test]
    fn a_polyline_is_a_line_a_segment_cut_to_the_interval() {
        let r = Record::Polyline {
            points: vec![[0.0; 3], [5.0, 0.0, 0.0], [5.0, 5.0, 0.0], [0.0, 5.0, 2.0]],
        };
        let c = Curve::from_record(&r, [0.5, 2.25]).unwrap();
        same(&r, &c, &[[0.5, 1.0], [1.0, 2.0], [2.0, 2.25]]);
        // past either end of its range [0, 3]: refused, not cut short
        assert_eq!(Curve::from_record(&r, [-0.5, 1.0]), Err(Error::Argument));
        assert_eq!(Curve::from_record(&r, [2.0, 3.5]), Err(Error::Argument));
        // its whole range
        let c = Curve::from_record(&r, [0.0, 3.0]).unwrap();
        same(&r, &c, &[[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]]);
    }

    #[test]
    fn a_circle_becomes_an_arc() {
        let r = Record::Circle {
            frame: Frame {
                origin: [1.0, 2.0, 3.0],
                x_axis: [0.0, 1.0, 0.0],
                y_axis: [0.0, 0.0, 1.0],
            },
            radius: 4.0,
        };
        let c = Curve::from_record(&r, [0.5, 2.5]).unwrap();
        assert!(matches!(c[..], [Curve::Arc { .. }]), "{c:?}");
        same(&r, &c, &[[0.5, 2.5]]);
        // past a full turn: no arc
        assert_eq!(Curve::from_record(&r, [0.0, 7.0]), Err(Error::Argument));
    }

    #[test]
    fn splines_and_ellipses_are_their_nets_cut_to_the_interval() {
        let spline = Record::Spline {
            knots: vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0],
            points: vec![[0.0; 3], [2.0, 3.0, 0.0], [5.0, -1.0, 1.0], [8.0, 2.0, 0.0]],
        };
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let quarter = Record::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![
                [10.0, 0.0, 0.0, 1.0],
                [10.0 * w, 10.0 * w, 0.0, w],
                [0.0, 10.0, 0.0, 1.0],
            ],
        };
        for (r, interval) in [
            (&spline, [0.0, 1.0]),
            (&spline, [0.2, 0.7]),
            (&quarter, [0.1, 0.9]),
        ] {
            let c = Curve::from_record(r, interval).unwrap();
            assert!(matches!(c[..], [Curve::Nurbs { .. }]), "{c:?}");
            // the cut net keeps the record's parameter
            let Curve::Nurbs { knots, .. } = &c[0] else {
                unreachable!()
            };
            assert!(
                (knots[0] - interval[0]).abs() < 1e-12
                    && (knots[knots.len() - 1] - interval[1]).abs() < 1e-12
            );
            same(r, &c, &[interval]);
        }
        let ellipse = Record::Ellipse {
            frame: Frame {
                origin: [0.0; 3],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
            },
            radii: [6.0, 3.0],
        };
        let c = Curve::from_record(&ellipse, [0.3, 2.0]).unwrap();
        assert!(matches!(c[..], [Curve::Nurbs { .. }]), "{c:?}");
        // on the ellipse, from its point at 0.3 to its point at 2.0
        let on = |p: [f64; 3]| ((p[0] / 6.0).powi(2) + (p[1] / 3.0).powi(2) - 1.0).abs() < 1e-9;
        for i in 0..=8 {
            assert!(on(sweep_at(&c[0], f64::from(i) / 8.0)));
        }
        assert!(near(sweep_at(&c[0], 0.0), record_at(&ellipse, 0.3)));
        assert!(near(sweep_at(&c[0], 1.0), record_at(&ellipse, 2.0)));
    }

    #[test]
    fn bad_intervals_and_traced_curves_are_refused() {
        let r = Record::Line {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0],
        };
        assert_eq!(Curve::from_record(&r, [1.0, 1.0]), Err(Error::Argument));
        assert_eq!(
            Curve::from_record(&r, [0.0, f64::NAN]),
            Err(Error::Argument)
        );
    }
}
