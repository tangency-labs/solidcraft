//! (not upstream) An edge's STEP interval from its vertices: the parameters of an
//! `EDGE_CURVE`'s start and end vertices on its curve, in the curve's STEP parameter
//! space, and whether the edge runs with the curve or against it.
//!
//! Upstream: none (solidcore's STEP reader, #2578). The rules follow ISO 10303-42
//! (`EDGE_CURVE.same_sense`; a closed curve's parameter is periodic) and take the
//! idea of SolidCraft's reader (`crates/step/src/topology.rs` `map_edge`, `conic`,
//! `open_conic`, `open_or_closed`, `project`, `angle_of`, `turn`, and
//! `topology/swept.rs` `open_conic_params`, at a5b71ca2; MIT OR Apache-2.0). The
//! computations are the kernel's own: [`Curve::step_inversion`] for lines, conics,
//! parabolas and hyperbolas, [`Curve::drop_point`] and the vertex drop of
//! [`crate::topology::brep_constructor::BrepConstructor::set_limits`] for the other
//! kinds.
//!
//! | Rust                       | SolidCraft (`crates/step`)                                |
//! |----------------------------|-----------------------------------------------------------|
//! | [`edge_interval`]          | `map_edge` (its parameters and direction; no curve copy)  |
//! | [`EdgeVertices`]           | `map_edge`'s `ps`, `pe` and `closed`                      |
//! | [`StepEdgeInterval`]       | `Mapped` (`interval`, `with_step`, `sense_mismatch`)      |
//!
//! Deviations: not a port. See [`edge_interval`] for the units and the rules.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::math::Point3;
use crate::topology::brep_constructor::drop_limit;

const OP: &str = "curve::step_edge::edge_interval";

/// The vertices bounding an `EDGE_CURVE`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeVertices {
    /// One vertex is both the start and the end (`edge_start` and `edge_end` are the
    /// same entity): the edge is the whole of a closed curve.
    Single(Point3),
    /// Two vertices (two entities, even when they lie at one point).
    Pair {
        /// The `edge_start` vertex's point.
        start: Point3,
        /// The `edge_end` vertex's point.
        end: Point3,
    },
}

/// An edge's parameters on its curve, in the curve's STEP parameter space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepEdgeInterval {
    /// The STEP parameter of the edge's start vertex.
    pub start: f64,
    /// The STEP parameter of the edge's end vertex.
    pub end: f64,
    /// The edge runs along the curve's parameter (`start < end`).
    pub with_curve: bool,
    /// On an open curve, `same_sense` disagrees with the order of the vertices'
    /// parameters (the order decides; see [`edge_interval`]). Always `false` on a
    /// closed curve, where `same_sense` decides.
    pub sense_mismatch: bool,
    /// The distance from the start vertex to the curve's point at [`Self::start`].
    pub start_gap: f64,
    /// The distance from the end vertex to the curve's point at [`Self::end`].
    pub end_gap: f64,
}

impl StepEdgeInterval {
    /// The interval from the smaller parameter to the larger.
    #[must_use]
    pub fn interval(&self) -> Extent1d {
        Extent1d::from_min_max(self.start.min(self.end), self.start.max(self.end))
    }
}

/// The STEP interval of the edge bounded by `vertices` on `curve` (the edge's basis
/// curve as the kernel builds it from the file: for a `CIRCLE` or `ELLIPSE` the full
/// conic), which runs with the curve's parameter when `same_sense`; `tol` is the
/// edge's 3D tolerance (the file's distance uncertainty).
///
/// **Units.** The parameters are the kernel's STEP parameters of the curve, those of
/// [`Curve::evaluate_step_point`]: degrees on a circle or an ellipse (the angle `t`
/// of `C + a cos t X + b sin t Y`), `t` of `P + t * magnitude * V` on a line, the
/// parameter of `C + F (t^2 X + 2 t Y)` on a parabola and of
/// `C + a cosh t X + b sinh t Y` on a hyperbola, and the curve's own (natural)
/// parameter on a B-spline and the other kinds.
///
/// **A vertex's parameter.** Lines, circles, ellipses, parabolas and hyperbolas
/// invert in closed form ([`Curve::step_inversion`]); the other kinds drop the vertex
/// on the curve's natural interval ([`Curve::drop_point`], on a closed curve the drop
/// of [`crate::topology::brep_constructor::BrepConstructor::set_limits`]). A vertex
/// off its curve is not refused: it takes that parameter (on a line and a circle the
/// nearest curve point; on an ellipse, parabola or hyperbola the inversion of its
/// coordinates in the curve's frame, the nearest point only for points on the curve;
/// on the other kinds the nearest point), and its distance to the curve point there
/// is [`StepEdgeInterval::start_gap`] / [`StepEdgeInterval::end_gap`], for the caller
/// to compare with its tolerance.
///
/// **The rules.**
/// - *Circles and ellipses* are periodic in the angle (period 360). The edge runs
///   from the start vertex's angle up (`same_sense`) or down to the end vertex's,
///   less than a full turn; one vertex, or two whose angles are equal, is a full
///   turn. The interval is kept within the kernel's angular limits
///   `-360 <= min <= max <= 360` ([`crate::curve::ellipse::Ellipse`]) by moving both
///   ends one period, so an arc over the seam (300 to 60 degrees) is -60..60.
/// - *Open curves* (lines, parabolas, hyperbolas, B-splines whose ends are farther
///   apart than `tol`): on an open curve the only edge between two points is the
///   parameter interval between them, so the vertices' order decides the direction;
///   `same_sense` against that order is an inconsistent file, shown as
///   [`StepEdgeInterval::sense_mismatch`].
/// - *Closed curves that are not periodic* (B-splines and other kinds whose ends lie
///   within `tol` of each other, [`Curve::is_closed`]): `same_sense` decides. The
///   vertex at the interval's lower end is dropped towards the curve's start and the
///   one at its upper end towards the curve's end, so a vertex on the seam takes the
///   end the sense needs. One vertex on the seam is the whole curve.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` that is negative or not finite, a vertex
/// that is not finite or gets no finite parameter, one vertex on an open curve, or
/// two vertices at one parameter of an open curve. [`SolidError::Unsupported`] for an
/// edge across the seam of a closed curve that is not periodic, or a full turn of one
/// from a vertex off its seam (the curve would have to be restarted at the vertex).
/// Those of [`Curve::step_inversion`], [`Curve::drop_point`], [`Curve::is_closed`]
/// and [`Curve::evaluate_step_point`].
pub fn edge_interval(
    k: &Kernel,
    curve: &Curve,
    vertices: EdgeVertices,
    same_sense: bool,
    tol: f64,
) -> Result<StepEdgeInterval> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "tolerance negative or not finite",
        ));
    }
    let (start_point, end_point) = match vertices {
        EdgeVertices::Single(v) => (v, v),
        EdgeVertices::Pair { start, end } => (start, end),
    };
    if !(is_finite(start_point) && is_finite(end_point)) {
        return Err(SolidError::invalid_input(OP, "vertex not finite"));
    }

    let (start, end, sense_mismatch) = match shape(k, curve, tol)? {
        Shape::Periodic => periodic(k, curve, vertices, same_sense)?,
        Shape::Open => open(k, curve, vertices, same_sense, tol)?,
        Shape::Closed => closed(k, curve, vertices, same_sense, tol)?,
    };
    Ok(StepEdgeInterval {
        start,
        end,
        with_curve: start < end,
        sense_mismatch,
        start_gap: gap(k, curve, start, start_point)?,
        end_gap: gap(k, curve, end, end_point)?,
    })
}

/// The STEP period of circles and ellipses, in degrees.
const PERIOD: f64 = 360.0;

/// How an edge's parameters lie on its curve.
enum Shape {
    /// A circle or an ellipse: periodic in its STEP angle.
    Periodic,
    /// Every point has one parameter.
    Open,
    /// The ends coincide: the seam point has two parameters, the curve's ends.
    Closed,
}

fn shape(k: &Kernel, curve: &Curve, tol: f64) -> Result<Shape> {
    Ok(match curve.kind() {
        CurveKind::Ellipse(_) => Shape::Periodic,
        CurveKind::Line(_) | CurveKind::Parabola(_) | CurveKind::Hyperbola(_) => Shape::Open,
        _ => {
            if curve.is_closed(k, &curve.natural_interval(), tol)? {
                Shape::Closed
            } else {
                Shape::Open
            }
        }
    })
}

fn is_finite(p: Point3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

/// A finite parameter, or [`SolidError::InvalidInput`].
fn finite(t: f64) -> Result<f64> {
    if t.is_finite() {
        Ok(t)
    } else {
        Err(SolidError::invalid_input(
            OP,
            "no finite parameter for a vertex",
        ))
    }
}

/// The distance from `vertex` to the curve's point at the STEP parameter `t`.
fn gap(k: &Kernel, curve: &Curve, t: f64, vertex: Point3) -> Result<f64> {
    Ok(curve.evaluate_step_point(k, t)?.distance(vertex))
}

/// The STEP parameter of `point` on an open curve: the closed-form inversion of the
/// analytic kinds, else the curve point nearest it.
fn open_parameter(k: &Kernel, curve: &Curve, point: Point3, tol: f64) -> Result<f64> {
    let t = match curve.kind() {
        CurveKind::Line(_)
        | CurveKind::Parabola(_)
        | CurveKind::Hyperbola(_)
        | CurveKind::Ellipse(_) => curve.step_inversion(k, point)?,
        _ => {
            let natural = curve.natural_interval();
            let dropped = curve.drop_point(
                k,
                &natural,
                point,
                None,
                tol,
                None,
                SolverOperation::Minimize,
            )?;
            if !dropped.success {
                return Err(SolidError::invalid_input(OP, "no parameter for a vertex"));
            }
            curve.convert_t_from_nurbs_to_step(k, dropped.parameter)?
        }
    };
    finite(t)
}

/// The length from `from` up to `to` along the period, in `(0, PERIOD]` (equal
/// angles are a full turn).
fn turn(from: f64, to: f64) -> f64 {
    let d = (to - from).rem_euclid(PERIOD);
    if d > 0.0 && d < PERIOD {
        d
    } else {
        PERIOD
    }
}

fn periodic(
    k: &Kernel,
    curve: &Curve,
    vertices: EdgeVertices,
    same_sense: bool,
) -> Result<(f64, f64, bool)> {
    let (ts, d) = match vertices {
        EdgeVertices::Single(v) => (finite(curve.step_inversion(k, v)?)?, PERIOD),
        EdgeVertices::Pair { start, end } => {
            let ts = finite(curve.step_inversion(k, start)?)?;
            let te = finite(curve.step_inversion(k, end)?)?;
            let d = if same_sense {
                turn(ts, te)
            } else {
                turn(te, ts)
            };
            (ts, d)
        }
    };
    let te = if same_sense { ts + d } else { ts - d };
    // the kernel's angular limits, -360 <= min <= max <= 360
    let (lo, hi) = (ts.min(te), ts.max(te));
    let shift = if hi > PERIOD {
        -PERIOD
    } else if lo < -PERIOD {
        PERIOD
    } else {
        0.0
    };
    Ok((ts + shift, te + shift, false))
}

fn open(
    k: &Kernel,
    curve: &Curve,
    vertices: EdgeVertices,
    same_sense: bool,
    tol: f64,
) -> Result<(f64, f64, bool)> {
    let EdgeVertices::Pair { start, end } = vertices else {
        return Err(SolidError::invalid_input(OP, "one vertex on an open curve"));
    };
    let ts = open_parameter(k, curve, start, tol)?;
    let te = open_parameter(k, curve, end, tol)?;
    if ts == te {
        return Err(SolidError::invalid_input(
            OP,
            "both vertices at one parameter of an open curve",
        ));
    }
    Ok((ts, te, (ts < te) != same_sense))
}

/// The STEP parameter of `point` on a closed curve that is not periodic, dropped
/// towards the curve's end when `at_max`, else towards its start.
fn closed_parameter(
    k: &Kernel,
    curve: &Curve,
    point: Point3,
    tol: f64,
    at_max: bool,
) -> Result<f64> {
    let t = drop_limit(k, curve, point, tol, tol, at_max)?;
    finite(curve.convert_t_from_nurbs_to_step(k, t)?)
}

fn closed(
    k: &Kernel,
    curve: &Curve,
    vertices: EdgeVertices,
    same_sense: bool,
    tol: f64,
) -> Result<(f64, f64, bool)> {
    let natural = curve.natural_interval();
    let (lo, hi) = match vertices {
        EdgeVertices::Single(v) => {
            let t = closed_parameter(k, curve, v, tol, false)?;
            let seam = curve.convert_t_from_nurbs_to_step(k, natural.min())?;
            if t != seam {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "a full turn of a closed curve from a vertex off its seam",
                });
            }
            (t, curve.convert_t_from_nurbs_to_step(k, natural.max())?)
        }
        EdgeVertices::Pair { start, end } => {
            let (lo_point, hi_point) = if same_sense {
                (start, end)
            } else {
                (end, start)
            };
            (
                closed_parameter(k, curve, lo_point, tol, false)?,
                closed_parameter(k, curve, hi_point, tol, true)?,
            )
        }
    };
    // both are finite (closed_parameter)
    if lo >= hi {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "an edge across the seam of a closed curve",
        });
    }
    Ok(if same_sense {
        (lo, hi, false)
    } else {
        (hi, lo, false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::ellipse::Ellipse;
    use crate::curve::hyperbola::Hyperbola;
    use crate::curve::line::Line;
    use crate::curve::parabola::Parabola;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::math::Vec3;
    use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

    /// The edge tolerance of the cases (a usual STEP distance uncertainty).
    const TOL: f64 = 1e-6;

    fn k() -> Kernel {
        Kernel::default()
    }

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    fn pair(start: Point3, end: Point3) -> EdgeVertices {
        EdgeVertices::Pair { start, end }
    }

    /// `a` equals the closed form `b` to rounding.
    #[track_caller]
    fn assert_close(a: f64, b: f64) {
        assert!(
            (a - b).abs() <= 1e-12 * (1.0 + b.abs()),
            "{a} is not {b} (difference {})",
            a - b
        );
    }

    #[track_caller]
    fn assert_edge(e: &StepEdgeInterval, start: f64, end: f64, with_curve: bool) {
        assert_close(e.start, start);
        assert_close(e.end, end);
        assert_eq!(e.with_curve, with_curve, "{e:?}");
        assert!(!e.sense_mismatch, "{e:?}");
        assert!(e.start_gap <= 1e-12 && e.end_gap <= 1e-12, "{e:?}");
    }

    /// The point at `deg` degrees on the circle of radius `r` about the origin in XY.
    fn on_circle(r: f64, deg: f64) -> Point3 {
        let a = deg.to_radians();
        p(r * a.cos(), r * a.sin(), 0.0)
    }

    fn circle(k: &Kernel, r: f64) -> Curve {
        Curve::from(
            Ellipse::create_canonical_circle(k, &Axis2Placement::IDENTITY, r, None, None).unwrap(),
        )
    }

    /// A spline of `degree` through `points` with unique `knots` of multiplicities
    /// `mults`.
    fn spline(degree: usize, points: &[Point3], knots: &[f64], mults: &[usize]) -> Curve {
        Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree,
                control_points: points,
                form: BSplineCurveForm::PolylineForm,
                knot_multiplicities: mults,
                knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        )
    }

    /// The closed degree-1 spline around the square (0,0)-(4,0)-(4,4)-(0,4), knots
    /// 0..4 (one per side), its seam at the origin.
    fn square(k: &Kernel) -> Curve {
        let c = spline(
            1,
            &[
                p(0.0, 0.0, 0.0),
                p(4.0, 0.0, 0.0),
                p(4.0, 4.0, 0.0),
                p(0.0, 4.0, 0.0),
                p(0.0, 0.0, 0.0),
            ],
            &[0.0, 1.0, 2.0, 3.0, 4.0],
            &[2, 1, 1, 1, 2],
        );
        assert!(c.is_closed(k, &c.natural_interval(), TOL).unwrap());
        c
    }

    /// Circle r = 2, (2,0,0) to (0,2,0): with the sense the quarter 0..90 degrees;
    /// against it the three quarters from 0 down to -270 (the same point as 90).
    #[test]
    fn arc_of_a_circle_with_and_against_the_sense() {
        let k = k();
        let c = circle(&k, 2.0);
        let v = pair(p(2.0, 0.0, 0.0), p(0.0, 2.0, 0.0));
        let e = edge_interval(&k, &c, v, true, TOL).unwrap();
        assert_edge(&e, 0.0, 90.0, true);
        assert_close(e.interval().min(), 0.0);
        assert_close(e.interval().max(), 90.0);
        let e = edge_interval(&k, &c, v, false, TOL).unwrap();
        assert_edge(&e, 0.0, -270.0, false);
        assert_close(e.interval().min(), -270.0);
        assert_close(e.interval().max(), 0.0);
    }

    /// One vertex on a circle: a full turn from it, up with the sense and down
    /// against it. From 90 degrees with the sense the turn would end at 450, beyond
    /// the kernel's angular limit of 360, so it is the same turn one period lower.
    #[test]
    fn a_full_circle_from_one_vertex() {
        let k = k();
        let c = circle(&k, 2.0);
        let v = EdgeVertices::Single(p(2.0, 0.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            0.0,
            360.0,
            true,
        );
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            0.0,
            -360.0,
            false,
        );
        let v = EdgeVertices::Single(p(0.0, 2.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            -270.0,
            90.0,
            true,
        );
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            90.0,
            -270.0,
            false,
        );
    }

    /// Two vertices at one point on a circle are a full turn too (the edge cannot be
    /// empty).
    #[test]
    fn two_vertices_at_one_point_of_a_circle_are_a_full_turn() {
        let k = k();
        let c = circle(&k, 2.0);
        let v = pair(p(2.0, 0.0, 0.0), p(2.0, 0.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            0.0,
            360.0,
            true,
        );
    }

    /// An arc over the seam, 300 to 60 degrees: with the sense it is unwound across
    /// 0 (-60..60, the same points as 300..420); against it runs from 300 down to 60.
    #[test]
    fn an_arc_over_the_seam() {
        let k = k();
        let c = circle(&k, 2.0);
        let v = pair(on_circle(2.0, 300.0), on_circle(2.0, 60.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            -60.0,
            60.0,
            true,
        );
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            300.0,
            60.0,
            false,
        );
    }

    /// Ellipse with semi axes 3 and 1: the STEP angle t of `(3 cos t, sin t)`, not the
    /// polar angle of the point.
    #[test]
    fn an_ellipse_by_its_step_angle() {
        let k = k();
        let c = Curve::from(
            Ellipse::create_canonical(&k, &Axis2Placement::IDENTITY, 3.0, 1.0, None, None).unwrap(),
        );
        let t = 45f64.to_radians();
        let v = pair(p(3.0 * t.cos(), t.sin(), 0.0), p(-3.0, 0.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            45.0,
            180.0,
            true,
        );
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            45.0,
            -180.0,
            false,
        );
    }

    /// A line `(0,0,0) + t * 2 * (1,0,0)` (magnitude 2): the points x = 1 and x = 5
    /// are t = 0.5 and 2.5. The order of the vertices decides the direction.
    #[test]
    fn a_line_with_magnitude_two() {
        let k = k();
        let c = Curve::from(Line::infinite(
            p(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            3,
            false,
        ));
        let (a, b) = (p(1.0, 0.0, 0.0), p(5.0, 0.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, pair(a, b), true, TOL).unwrap(),
            0.5,
            2.5,
            true,
        );
        assert_edge(
            &edge_interval(&k, &c, pair(b, a), false, TOL).unwrap(),
            2.5,
            0.5,
            false,
        );
        // same_sense against the vertices' order: the order decides, the mismatch is shown
        let e = edge_interval(&k, &c, pair(a, b), false, TOL).unwrap();
        assert_close(e.start, 0.5);
        assert_close(e.end, 2.5);
        assert!(e.with_curve && e.sense_mismatch, "{e:?}");
    }

    /// Parabola `F (t^2, 2t)` with F = 1: (1,-2,0) is t = -1 and (2.25,3,0) is t = 1.5.
    #[test]
    fn a_parabola() {
        let k = k();
        let c = Curve::from(Parabola::create_canonical(&Axis2Placement::IDENTITY, 1.0));
        let v = pair(p(1.0, -2.0, 0.0), p(2.25, 3.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            -1.0,
            1.5,
            true,
        );
    }

    /// Hyperbola `(2 cosh t, sinh t)`: from t = 0.5 back to t = -1.
    #[test]
    fn a_hyperbola() {
        let k = k();
        let c = Curve::from(Hyperbola::new(
            p(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Extent1d::from_min_max(-5.0, 5.0),
            2.0,
            1.0,
            3,
        ));
        let at = |t: f64| p(2.0 * t.cosh(), t.sinh(), 0.0);
        let v = pair(at(0.5), at(-1.0));
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            0.5,
            -1.0,
            false,
        );
    }

    /// The degree-1 spline (0,0,0)-(4,0,0) over [0, 1]: (1,0,0) is t = 0.25.
    #[test]
    fn a_degree_one_spline() {
        let k = k();
        let c = spline(
            1,
            &[p(0.0, 0.0, 0.0), p(4.0, 0.0, 0.0)],
            &[0.0, 1.0],
            &[2, 2],
        );
        let v = pair(p(1.0, 0.0, 0.0), p(4.0, 0.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, true, TOL).unwrap(),
            0.25,
            1.0,
            true,
        );
        let v = pair(p(4.0, 0.0, 0.0), p(1.0, 0.0, 0.0));
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            1.0,
            0.25,
            false,
        );
    }

    /// The closed square spline with a vertex on its seam (the origin, t = 0 and
    /// t = 4) and one at (4,4,0) (t = 2): the sense picks which end of the seam the
    /// edge takes.
    #[test]
    fn a_closed_spline_with_a_vertex_on_its_seam() {
        let k = k();
        let c = square(&k);
        let (seam, mid) = (p(0.0, 0.0, 0.0), p(4.0, 4.0, 0.0));
        // seam to mid with the sense: the first half
        assert_edge(
            &edge_interval(&k, &c, pair(seam, mid), true, TOL).unwrap(),
            0.0,
            2.0,
            true,
        );
        // seam to mid against the sense: from the seam's far end down to mid
        assert_edge(
            &edge_interval(&k, &c, pair(seam, mid), false, TOL).unwrap(),
            4.0,
            2.0,
            false,
        );
        // mid to seam with the sense: on to the seam's far end
        assert_edge(
            &edge_interval(&k, &c, pair(mid, seam), true, TOL).unwrap(),
            2.0,
            4.0,
            true,
        );
        // mid to seam against the sense: down to the seam's near end
        assert_edge(
            &edge_interval(&k, &c, pair(mid, seam), false, TOL).unwrap(),
            2.0,
            0.0,
            false,
        );
        // one vertex on the seam: the whole curve, either way
        let one = EdgeVertices::Single(seam);
        assert_edge(
            &edge_interval(&k, &c, one, true, TOL).unwrap(),
            0.0,
            4.0,
            true,
        );
        assert_edge(
            &edge_interval(&k, &c, one, false, TOL).unwrap(),
            4.0,
            0.0,
            false,
        );
    }

    /// A full turn of a closed spline from a vertex off its seam, and an edge across
    /// the seam, have no single interval on the curve: refused (restarting the spline
    /// at the vertex is a separate construction).
    #[test]
    fn a_closed_spline_across_its_seam_is_refused() {
        let k = k();
        let c = square(&k);
        let mid = p(4.0, 4.0, 0.0);
        let err = edge_interval(&k, &c, EdgeVertices::Single(mid), true, TOL).unwrap_err();
        assert!(
            matches!(err, SolidError::Unsupported { operation: OP, .. }),
            "{err:?}"
        );
        let v = pair(p(4.0, 2.0, 0.0), p(2.0, 0.0, 0.0));
        let err = edge_interval(&k, &c, v, true, TOL).unwrap_err();
        assert!(
            matches!(err, SolidError::Unsupported { operation: OP, .. }),
            "{err:?}"
        );
        // the same vertices against the sense are the edge from t = 1.5 down to 0.5
        assert_edge(
            &edge_interval(&k, &c, v, false, TOL).unwrap(),
            1.5,
            0.5,
            false,
        );
    }

    /// A vertex off its curve takes the parameter of the curve's STEP inversion of it
    /// (on a circle the angle of its projection into the plane, scaled to the radius),
    /// and its distance to that curve point is reported: (3,0,0) and (0,2,1) are each
    /// 1 from the circle of radius 2.
    #[test]
    fn a_vertex_off_its_curve_reports_its_gap() {
        let k = k();
        let c = circle(&k, 2.0);
        let v = pair(p(3.0, 0.0, 0.0), p(0.0, 2.0, 1.0));
        let e = edge_interval(&k, &c, v, true, TOL).unwrap();
        assert_close(e.start, 0.0);
        assert_close(e.end, 90.0);
        assert_close(e.start_gap, 1.0);
        assert_close(e.end_gap, 1.0);
    }

    /// One vertex on an open curve, two vertices at one point of it, and a bad
    /// tolerance are refused.
    #[test]
    fn invalid_edges_are_refused() {
        let k = k();
        let line = Curve::from(Line::infinite(
            p(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            3,
            false,
        ));
        let invalid = |r: Result<StepEdgeInterval>| {
            assert!(
                matches!(r, Err(SolidError::InvalidInput { operation: OP, .. })),
                "{r:?}"
            );
        };
        let a = p(1.0, 0.0, 0.0);
        invalid(edge_interval(&k, &line, EdgeVertices::Single(a), true, TOL));
        invalid(edge_interval(&k, &line, pair(a, a), true, TOL));
        invalid(edge_interval(
            &k,
            &line,
            pair(a, p(2.0, 0.0, 0.0)),
            true,
            -1.0,
        ));
        invalid(edge_interval(
            &k,
            &line,
            pair(a, p(2.0, 0.0, 0.0)),
            true,
            f64::NAN,
        ));
        invalid(edge_interval(
            &k,
            &line,
            pair(a, p(f64::NAN, 0.0, 0.0)),
            true,
            TOL,
        ));
    }

    /// The result is the same on every call (no state is kept between them).
    #[test]
    fn deterministic() {
        let k = k();
        let c = square(&k);
        let v = pair(p(0.0, 0.0, 0.0), p(4.0, 4.0, 0.0));
        let a = edge_interval(&k, &c, v, false, TOL).unwrap();
        let b = edge_interval(&k, &c, v, false, TOL).unwrap();
        assert_eq!(a.start.to_bits(), b.start.to_bits());
        assert_eq!(a.end.to_bits(), b.end.to_bits());
    }
}
