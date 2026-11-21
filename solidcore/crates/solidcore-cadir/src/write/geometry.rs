//! Record curves, surfaces and pcurves as CADIR carriers: the inverse of [`crate::geometry`].
//!
//! A curve is written on a parameter that is an increasing affine image of the record's
//! ([`Param`]), so an edge's interval and its pcurves' knots go through the same map. A circle
//! and a spline keep their parameter; a line, which CADIR holds by a unit direction, runs `|end -
//! start|` times as fast; an ellipse whose second radius is the larger, which CADIR holds by its
//! major axis, is the same ellipse on a frame turned a quarter turn about its axis, at a
//! parameter a quarter turn behind. A polyline is the degree-one spline on its own parameter
//! (segment i over [i, i + 1]), which is what the reader makes of CADIR's polyline, so it is
//! exact and has no chordal deflection to state.
//!
//! A surface's (u, v) is an affine image of the record's ([`Chart`]): a plane is written on the
//! orthonormal frame of its axes, and a torus's tube starts at its outer equator (CADIR's has no
//! tube start). Cylinders, cones (of either sign of half angle), spheres and splines keep their
//! (u, v). No chart here reverses orientation; one that did would turn the natural normal, which
//! the face's sense would say.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis, WeightedPole3,
};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, WeightedPole2};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroReal;
use solidcore::Vec3;
use solidcore_record::brep::{Curve as BCurve, Curve2, Frame, Surface as BSurface};

use super::WriteError;

type W<T> = Result<T, WriteError>;

fn refused(what: impl std::fmt::Display) -> WriteError {
    WriteError::Refused(what.to_string())
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p3(p: Vec3) -> Point3 {
    Point3::new(p.x, p.y, p.z)
}

fn at(p: [f64; 3]) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn d3(p: Vec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

/// `a` scaled to unit length, or why not.
fn unit(a: Vec3, what: &str) -> W<Vec3> {
    let n = a.length();
    if n.is_finite() && n > 0.0 {
        Ok(a * (1.0 / n))
    } else {
        Err(refused(format!("a degenerate {what}")))
    }
}

/// A record frame's axis (z = x × y) and reference direction (x), each scaled to unit length.
fn axes(f: &Frame) -> W<(Vector3, Vector3)> {
    let x = unit(v(f.x_axis), "frame")?;
    let z = unit(x.cross(v(f.y_axis)), "frame")?;
    Ok((d3(z), d3(x)))
}

/// CADIR's parameter of a curve against the record's: `t' = scale t + shift`, `scale` > 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Param {
    pub(crate) scale: f64,
    pub(crate) shift: f64,
}

impl Param {
    pub(crate) const IDENTITY: Param = Param {
        scale: 1.0,
        shift: 0.0,
    };

    pub(crate) fn apply(&self, t: f64) -> f64 {
        self.scale * t + self.shift
    }
}

/// CADIR's (u, v) of a surface against the record's: `(u', v') = m (u, v) + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Chart {
    pub(crate) m: [[f64; 2]; 2],
    pub(crate) offset: [f64; 2],
}

impl Chart {
    pub(crate) const IDENTITY: Chart = Chart {
        m: [[1.0, 0.0], [0.0, 1.0]],
        offset: [0.0, 0.0],
    };

    /// CADIR's (u, v) at the record's.
    pub(crate) fn apply(&self, [u, w]: [f64; 2]) -> [f64; 2] {
        let [a, b] = self.m;
        [
            a[0] * u + a[1] * w + self.offset[0],
            b[0] * u + b[1] * w + self.offset[1],
        ]
    }

    /// The record's natural normal is CADIR's: both are ∂S/∂u × ∂S/∂v, so they differ by the
    /// sign of the chart's determinant.
    pub(crate) fn keeps_normal(&self) -> bool {
        let [a, b] = self.m;
        a[0] * b[1] - a[1] * b[0] > 0.0
    }
}

/// The degree of a spline of `count` control points on `knots` (full, each knot repeated by its
/// multiplicity).
fn degree(knots: &[f64], count: usize) -> W<u32> {
    knots
        .len()
        .checked_sub(count + 1)
        .filter(|d| *d >= 1)
        .and_then(|d| u32::try_from(d).ok())
        .ok_or_else(|| refused("a spline whose knots do not fit its points"))
}

fn weight(w: f64) -> W<NonZeroReal> {
    NonZeroReal::new(w)
        .filter(|w| w.get() > 0.0)
        .ok_or_else(|| refused("a non-positive rational weight"))
}

/// A record curve as a CADIR curve, with CADIR's parameter against the record's; `None` for a
/// curve CADIR has no exact form for (a traced intersection).
pub(crate) fn curve(c: &BCurve) -> W<Option<(SolvedCurveGeometry, Param)>> {
    let nurbs = |n: Result<NurbsCurve, _>| -> W<SolvedCurveGeometry> {
        n.map(SolvedCurveGeometry::Nurbs)
            .map_err(|e: cadmpeg_ir::geometry::nurbs::NurbsError| refused(e))
    };
    Ok(Some(match c {
        BCurve::Line { start, end } => {
            let d = v(*end) - v(*start);
            let length = d.length();
            let direction = unit(d, "line")?;
            let line = LineCurve::try_new(at(*start), d3(direction)).map_err(refused)?;
            (
                SolvedCurveGeometry::Line(line),
                Param {
                    scale: length,
                    shift: 0.0,
                },
            )
        }
        BCurve::Circle { frame, radius } => {
            let (z, x) = axes(frame)?;
            let circle = CircleCurve::try_new(at(frame.origin), z, x, *radius).map_err(refused)?;
            (SolvedCurveGeometry::Circle(circle), Param::IDENTITY)
        }
        BCurve::Ellipse { frame, radii } => {
            let (z, x) = axes(frame)?;
            let [a, b] = *radii;
            if a >= b {
                let e = EllipseCurve::try_new(at(frame.origin), z, x, a, b).map_err(refused)?;
                (SolvedCurveGeometry::Ellipse(e), Param::IDENTITY)
            } else {
                // `o + a cos t x + b sin t y` is `o + b cos s y + a sin s (−x)` at s = t − π/2:
                // the major axis along y, the frame turned a quarter turn about z.
                let y = d3(unit(v(frame.y_axis), "frame")?);
                let e = EllipseCurve::try_new(at(frame.origin), z, y, b, a).map_err(refused)?;
                (
                    SolvedCurveGeometry::Ellipse(e),
                    Param {
                        scale: 1.0,
                        shift: -std::f64::consts::FRAC_PI_2,
                    },
                )
            }
        }
        BCurve::Polyline { points } => {
            let n = points.len();
            let last = n.saturating_sub(1) as f64;
            let knots: Vec<f64> = std::iter::once(0.0)
                .chain((0..n).map(|i| i as f64))
                .chain(std::iter::once(last))
                .collect();
            let poles = NurbsPoles3::Polynomial {
                points: points.iter().map(|p| at(*p)).collect(),
            };
            (
                nurbs(NurbsCurve::new(1, knots, poles, false))?,
                Param::IDENTITY,
            )
        }
        BCurve::Spline { knots, points } => {
            let d = degree(knots, points.len())?;
            let poles = NurbsPoles3::Polynomial {
                points: points.iter().map(|p| at(*p)).collect(),
            };
            (
                nurbs(NurbsCurve::new(d, knots.clone(), poles, false))?,
                Param::IDENTITY,
            )
        }
        BCurve::RationalSpline { knots, homogeneous } => {
            let d = degree(knots, homogeneous.len())?;
            let mut points = Vec::new();
            for h in homogeneous {
                let w = weight(h[3])?;
                points.push(WeightedPole3 {
                    point: Point3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]),
                    weight: w,
                });
            }
            let poles = NurbsPoles3::Rational { points };
            (
                nurbs(NurbsCurve::new(d, knots.clone(), poles, false))?,
                Param::IDENTITY,
            )
        }
        BCurve::SurfaceIntersection { .. } => return Ok(None),
    }))
}

/// A record surface as a CADIR carrier, with CADIR's (u, v) against the record's; `None` for a
/// label that stands for its face's NURBS form, which a valid record holds with it: a ball
/// patch's, a blend's, and a placed revolution's (whose form's side is not measured against the
/// revolution's yet).
pub(crate) fn surface(s: &BSurface) -> W<Option<(SolvedSurfaceGeometry, Chart)>> {
    let solved = |g: SolvedSurfaceGeometry, chart: Chart| Ok(Some((g, chart)));
    match s {
        BSurface::Plane {
            origin,
            u_axis,
            v_axis,
        } => plane(*origin, v(*u_axis), v(*v_axis)),
        BSurface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => plane(*origin, v(*u_point) - v(*origin), v(*v_point) - v(*origin)),
        BSurface::Cylinder { frame, radius } => {
            let (z, x) = axes(frame)?;
            let c = CylinderSurface::try_new(at(frame.origin), z, x, *radius).map_err(refused)?;
            solved(SolvedSurfaceGeometry::Cylinder(c), Chart::IDENTITY)
        }
        BSurface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => {
            // CADIR's cone is the record's, its half angle signed alike: `radius + v tan(angle)`.
            let (z, x) = axes(frame)?;
            let c =
                ConeSurface::try_new(at(frame.origin), z, x, *radius_at_origin, 1.0, *half_angle)
                    .map_err(refused)?;
            solved(SolvedSurfaceGeometry::Cone(c), Chart::IDENTITY)
        }
        BSurface::Sphere { frame, radius } => {
            let (z, x) = axes(frame)?;
            let c = SphereSurface::try_new(at(frame.origin), z, x, *radius).map_err(refused)?;
            solved(SolvedSurfaceGeometry::Sphere(c), Chart::IDENTITY)
        }
        BSurface::Torus {
            frame,
            major_radius,
            minor_radius,
            tube_start,
        } => {
            let (z, x) = axes(frame)?;
            let c = TorusSurface::try_new(at(frame.origin), z, x, *major_radius, *minor_radius)
                .map_err(refused)?;
            // CADIR's meridian angle is the record's `v + tube_start`.
            let chart = Chart {
                offset: [0.0, *tube_start],
                ..Chart::IDENTITY
            };
            solved(SolvedSurfaceGeometry::Torus(c), chart)
        }
        BSurface::Spline {
            u_knots,
            v_knots,
            points,
        } => {
            let rows: Vec<Vec<Point3>> = points
                .iter()
                .map(|r| r.iter().map(|p| at(*p)).collect())
                .collect();
            let columns = points.first().map_or(0, Vec::len);
            let grid = NurbsPoleGrid::Polynomial { rows };
            let n = spline_surface(u_knots, v_knots, [points.len(), columns], grid)?;
            solved(SolvedSurfaceGeometry::Nurbs(n), Chart::IDENTITY)
        }
        BSurface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => {
            let mut rows = Vec::new();
            for r in homogeneous {
                let mut row = Vec::new();
                for h in r {
                    row.push(WeightedPole3 {
                        point: Point3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]),
                        weight: weight(h[3])?,
                    });
                }
                rows.push(row);
            }
            let n = spline_surface(
                u_knots,
                v_knots,
                [homogeneous.len(), homogeneous.first().map_or(0, Vec::len)],
                NurbsPoleGrid::Rational { rows },
            )?;
            solved(SolvedSurfaceGeometry::Nurbs(n), Chart::IDENTITY)
        }
        BSurface::BallPatch { .. } | BSurface::Blend { .. } | BSurface::PlacedRevolution { .. } => {
            Ok(None)
        }
    }
}

/// The plane `origin + u a + v b` on the orthonormal frame of `a` and the normal `a × b`: the
/// record's (u, v) map linearly onto CADIR's, keeping the normal (the STEP writer's frame).
fn plane(origin: [f64; 3], a: Vec3, b: Vec3) -> W<Option<(SolvedSurfaceGeometry, Chart)>> {
    let x = unit(a, "plane")?;
    let z = unit(a.cross(b), "plane")?;
    let y = z.cross(x);
    let m = [[a.dot(x), b.dot(x)], [a.dot(y), b.dot(y)]];
    let p = PlaneSurface::try_new(at(origin), d3(z), d3(x)).map_err(refused)?;
    Ok(Some((
        SolvedSurfaceGeometry::Plane(p),
        Chart {
            m,
            offset: [0.0, 0.0],
        },
    )))
}

/// A NURBS surface of `grid`, `rows` × `columns` poles, on full knots.
fn spline_surface(
    u_knots: &[f64],
    v_knots: &[f64],
    [rows, columns]: [usize; 2],
    grid: NurbsPoleGrid<Point3>,
) -> W<NurbsSurface> {
    let (du, dv) = (degree(u_knots, rows)?, degree(v_knots, columns)?);
    NurbsSurface::new(
        NurbsSurfaceAxis::new(du, u_knots.to_vec(), false),
        NurbsSurfaceAxis::new(dv, v_knots.to_vec(), false),
        grid,
        false,
    )
    .map_err(refused)
}

/// A record pcurve in CADIR's (u, v) of its face (`chart`, exact on a B-spline's control points
/// and on a NURBS's homogeneous ones) and on CADIR's parameter of its edge (`param`, applied to
/// its knots).
pub(crate) fn pcurve(c: &Curve2, chart: Chart, param: Param) -> W<PcurveGeometry> {
    let p2 = |p: [f64; 2]| {
        let [u, w] = chart.apply(p);
        Point2::new(u, w)
    };
    let (knots, poles, count) = match c {
        Curve2::Spline { knots, points } => (
            knots,
            PcurveNurbsPoles::Polynomial {
                points: points.iter().map(|p| p2(*p)).collect(),
            },
            points.len(),
        ),
        Curve2::RationalSpline { knots, homogeneous } => {
            let mut points = Vec::new();
            for h in homogeneous {
                let w = weight(h[2])?;
                points.push(WeightedPole2 {
                    point: p2([h[0] / h[2], h[1] / h[2]]),
                    weight: w,
                });
            }
            (
                knots,
                PcurveNurbsPoles::Rational { points },
                homogeneous.len(),
            )
        }
    };
    let d = degree(knots, count)?;
    let knots: Vec<f64> = knots.iter().map(|t| param.apply(*t)).collect();
    let nurbs = PcurveNurbs::new(d, knots, poles, false).map_err(refused)?;
    Ok(PcurveGeometry::Nurbs { nurbs })
}

/// A point the model holds.
pub(crate) fn finite(p: [f64; 3]) -> W<FinitePoint3> {
    FinitePoint3::new(p3(v(p))).ok_or_else(|| refused("a point that is not finite"))
}
