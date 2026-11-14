//! CADIR's curve and surface carriers as record curves and surfaces.
//!
//! A curve keeps its parameter exactly: an edge's interval on the CADIR carrier is its interval
//! on the record curve. CADIR's line is `origin + t direction` with a unit direction, its circle
//! and ellipse are at the angle `t` from the frame's reference direction (the major axis of an
//! ellipse) about its axis, and a NURBS curve is on its own knots: the record's line through
//! `origin` and `origin + direction`, and its circle, ellipse and spline, have the same `t`. A
//! polyline is the degree-one spline on its own sample parameters.
//!
//! A surface need not keep its (u, v): the assembly makes every pcurve from the 3D edges. What
//! a surface must keep is its side, so each comes back with whether the record's natural normal
//! is opposite CADIR's (`reversed`), which turns the face's sense against it. CADIR's plane,
//! cylinder, circular cone, sphere and torus are the record's with the same frame (x the
//! reference direction, z the axis) and the same formulas, so their natural normals agree, but
//! for a sphere or torus with a negative (tube) radius, whose points CADIR places on the other
//! side of the centre (`c + r n` for `r < 0`): the record holds the radius's magnitude and its
//! normal is the opposite. A NURBS surface's oriented normal is opposite `Pu × Pv` when CADIR says
//! `normal_reversed`. A procedural carrier is read by the solved form the producer cached.
//!
//! A placed carrier (`Transformed`) is its basis mapped by its affine placement, when that is a
//! rotation and a uniform scale (with a move): frames turn, lengths scale and parameters are
//! kept. A placement that shears, scales unevenly or mirrors is refused.

use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::units::OrthonormalFrame3;
use solidcore::Vec3;
use solidcore_record::brep::{Curve as BCurve, Frame, Surface as BSurface};
use solidcore_step::assembly::{spline_curve, spline_surface};

/// A reason a carrier is not read.
pub(crate) type R<T> = Result<T, String>;

/// Deepest chain of placed carriers followed (CADIR caps its own nesting too).
const MAX_PLACEMENTS: usize = 16;
/// How far a placement's columns may be from one length and from square to each other, relative
/// to their length, for it to be taken as a rotation and a uniform scale: the rounding of an
/// orthonormal matrix written to double precision (a few units in the last place of each of its
/// dot products), not a modelling tolerance, so a placement that really scales unevenly by as
/// little as 1e-12 is refused.
const SIMILARITY: f64 = 64.0 * f64::EPSILON;

fn unsupported(what: impl std::fmt::Display) -> String {
    format!("not supported yet: {what}")
}

fn v(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn a(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn point(p: cadmpeg_ir::math::Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn unit(u: &cadmpeg_ir::units::UnitVector3) -> Vec3 {
    let r = u.as_raw();
    Vec3::new(r.x, r.y, r.z)
}

/// The record frame of a CADIR frame at `origin`: x its reference direction, z its axis.
fn frame(origin: [f64; 3], f: &OrthonormalFrame3) -> Frame {
    let (z, x) = (unit(f.axis()), unit(f.reference()));
    Frame {
        origin,
        x_axis: a(x),
        y_axis: a(z.cross(x)),
    }
}

/// A rotation, a uniform scale and a move: `p ↦ s Q p + t`, from a placement's rows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Similarity {
    rows: [[f64; 4]; 3],
    scale: f64,
}

impl Similarity {
    /// The placement `rows` (row i: `x_i' = r_i0 x + r_i1 y + r_i2 z + r_i3`), when it is a
    /// similarity that keeps handedness; else why not.
    pub(crate) fn of(rows: [[f64; 4]; 3]) -> R<Similarity> {
        if rows.iter().flatten().any(|x| !x.is_finite()) {
            return Err("a placement that is not finite".into());
        }
        let [[a00, a01, a02, _], [a10, a11, a12, _], [a20, a21, a22, _]] = rows;
        let (c0, c1, c2) = (
            Vec3::new(a00, a10, a20),
            Vec3::new(a01, a11, a21),
            Vec3::new(a02, a12, a22),
        );
        let scale = c0.length();
        let near = |x: f64, y: f64| (x - y).abs() <= SIMILARITY * scale.max(f64::MIN_POSITIVE);
        let square = near(c0.dot(c1) / scale, 0.0)
            && near(c1.dot(c2) / scale, 0.0)
            && near(c0.dot(c2) / scale, 0.0);
        if !(scale > 0.0) || !square || !near(c1.length(), scale) || !near(c2.length(), scale) {
            return Err(unsupported("a placement that shears or scales unevenly"));
        }
        if !(c0.cross(c1).dot(c2) > 0.0) {
            return Err(unsupported("a placement that mirrors"));
        }
        Ok(Similarity { rows, scale })
    }

    pub(crate) fn point(&self, p: [f64; 3]) -> [f64; 3] {
        self.rows
            .map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3])
    }

    /// A vector (a direction times a length): scaled with the placement.
    fn vector(&self, d: [f64; 3]) -> [f64; 3] {
        self.rows.map(|r| r[0] * d[0] + r[1] * d[1] + r[2] * d[2])
    }

    /// A unit direction: turned, still unit.
    fn direction(&self, d: [f64; 3]) -> [f64; 3] {
        self.vector(d).map(|x| x / self.scale)
    }

    fn frame(&self, f: &Frame) -> Frame {
        Frame {
            origin: self.point(f.origin),
            x_axis: self.direction(f.x_axis),
            y_axis: self.direction(f.y_axis),
        }
    }

    fn curve(&self, c: BCurve) -> R<BCurve> {
        let s = self.scale;
        Ok(match c {
            // The direction is scaled with the line, so `t` stays where it was.
            BCurve::Line { start, end } => {
                let d = self.vector([end[0] - start[0], end[1] - start[1], end[2] - start[2]]);
                let start = self.point(start);
                BCurve::Line {
                    start,
                    end: [start[0] + d[0], start[1] + d[1], start[2] + d[2]],
                }
            }
            BCurve::Circle { frame, radius } => BCurve::Circle {
                frame: self.frame(&frame),
                radius: radius * s,
            },
            BCurve::Ellipse { frame, radii } => BCurve::Ellipse {
                frame: self.frame(&frame),
                radii: radii.map(|r| r * s),
            },
            BCurve::Polyline { points } => BCurve::Polyline {
                points: points.into_iter().map(|p| self.point(p)).collect(),
            },
            BCurve::Spline { knots, points } => BCurve::Spline {
                knots,
                points: points.into_iter().map(|p| self.point(p)).collect(),
            },
            // A rational spline's weights are kept: an affine map acts on its points alone.
            BCurve::RationalSpline { knots, homogeneous } => BCurve::RationalSpline {
                knots,
                homogeneous: homogeneous
                    .into_iter()
                    .map(|h| {
                        let w = h[3];
                        let p = self.point([h[0] / w, h[1] / w, h[2] / w]);
                        [p[0] * w, p[1] * w, p[2] * w, w]
                    })
                    .collect(),
            },
            BCurve::SurfaceIntersection { .. } => {
                return Err(unsupported("a placed intersection curve"));
            }
        })
    }

    fn surface(&self, s: BSurface) -> R<BSurface> {
        let k = self.scale;
        Ok(match s {
            BSurface::Plane {
                origin,
                u_axis,
                v_axis,
            } => BSurface::Plane {
                origin: self.point(origin),
                u_axis: self.direction(u_axis),
                v_axis: self.direction(v_axis),
            },
            BSurface::Cylinder { frame, radius } => BSurface::Cylinder {
                frame: self.frame(&frame),
                radius: radius * k,
            },
            BSurface::Cone {
                frame,
                radius_at_origin,
                half_angle,
            } => BSurface::Cone {
                frame: self.frame(&frame),
                radius_at_origin: radius_at_origin * k,
                half_angle,
            },
            BSurface::Sphere { frame, radius } => BSurface::Sphere {
                frame: self.frame(&frame),
                radius: radius * k,
            },
            BSurface::Torus {
                frame,
                major_radius,
                minor_radius,
                tube_start,
            } => BSurface::Torus {
                frame: self.frame(&frame),
                major_radius: major_radius * k,
                minor_radius: minor_radius * k,
                tube_start,
            },
            BSurface::Spline {
                u_knots,
                v_knots,
                points,
            } => BSurface::Spline {
                u_knots,
                v_knots,
                points: points
                    .into_iter()
                    .map(|r| r.into_iter().map(|p| self.point(p)).collect())
                    .collect(),
            },
            BSurface::RationalSpline {
                u_knots,
                v_knots,
                homogeneous,
            } => BSurface::RationalSpline {
                u_knots,
                v_knots,
                homogeneous: homogeneous
                    .into_iter()
                    .map(|r| {
                        r.into_iter()
                            .map(|h| {
                                let w = h[3];
                                let p = self.point([h[0] / w, h[1] / w, h[2] / w]);
                                [p[0] * w, p[1] * w, p[2] * w, w]
                            })
                            .collect()
                    })
                    .collect(),
            },
            other => return Err(unsupported(format!("a placed {}", surface_kind(&other)))),
        })
    }
}

fn surface_kind(s: &BSurface) -> &'static str {
    match s {
        BSurface::Plane { .. } | BSurface::PlaneThrough { .. } => "plane",
        BSurface::Cylinder { .. } => "cylinder",
        BSurface::Cone { .. } => "cone",
        BSurface::Sphere { .. } => "sphere",
        BSurface::Torus { .. } => "torus",
        BSurface::BallPatch { .. } => "ball patch",
        BSurface::Spline { .. } | BSurface::RationalSpline { .. } => "spline surface",
        BSurface::PlacedRevolution { .. } => "surface of revolution",
        BSurface::Blend { .. } => "blend surface",
    }
}

/// An edge's curve: the record curve with CADIR's parameter, or `None` for a degenerate curve
/// (an edge collapsed to a point: a pole of the faces it bounds).
pub(crate) fn curve(g: &CurveGeometry) -> R<Option<BCurve>> {
    let solved = g
        .solved()
        .ok_or_else(|| unsupported("a procedural curve without its solved form"))?;
    solved_curve(solved, 0)
}

fn solved_curve(g: &SolvedCurveGeometry, depth: usize) -> R<Option<BCurve>> {
    Ok(Some(match g {
        SolvedCurveGeometry::Line(l) => {
            let (o, d) = (point(l.origin().get()), unit(&l.direction()));
            BCurve::Line {
                start: o,
                end: a(v(o) + d),
            }
        }
        SolvedCurveGeometry::Circle(c) => BCurve::Circle {
            frame: frame(point(c.center().get()), c.frame()),
            radius: c.radius().get(),
        },
        SolvedCurveGeometry::Ellipse(e) => BCurve::Ellipse {
            frame: frame(point(e.center().get()), e.frame()),
            radii: [e.major_radius().get(), e.minor_radius().get()],
        },
        SolvedCurveGeometry::Nurbs(n) => {
            let points: Vec<[f64; 3]> = n
                .control_points()
                .into_iter()
                .map(|p| point(p.get()))
                .collect();
            let weights: Option<Vec<f64>> = n
                .weights()
                .map(|w| w.into_iter().map(|w| w.get()).collect());
            spline_curve(
                n.degree() as usize,
                n.knots().as_slice().to_vec(),
                &points,
                weights.as_deref(),
            )?
        }
        SolvedCurveGeometry::Polyline(p) => {
            let mut points: Vec<[f64; 3]> = p.points().map(|q| point(q.get())).collect();
            let mut params: Vec<f64> = match p.parameters() {
                Some(t) => t.map(|t| t.get()).collect(),
                None => (0..points.len()).map(|i| i as f64).collect(),
            };
            // CADIR admits parameters that decrease along the samples: the same map from
            // parameter to point, read with both lists the other way round.
            if params.windows(2).all(|w| matches!(w, [x, y] if x > y)) {
                points.reverse();
                params.reverse();
            }
            if params.len() != points.len()
                || params.windows(2).any(|w| !matches!(w, [x, y] if x < y))
            {
                return Err(
                    "a polyline whose parameters do not change monotonically with its points"
                        .into(),
                );
            }
            let (Some(first), Some(last)) = (params.first(), params.last()) else {
                return Err("an empty polyline".into());
            };
            let knots: Vec<f64> = std::iter::once(*first)
                .chain(params.iter().copied())
                .chain(std::iter::once(*last))
                .collect();
            spline_curve(1, knots, &points, None)?
        }
        SolvedCurveGeometry::Degenerate(_) => return Ok(None),
        SolvedCurveGeometry::Transformed(placed) => {
            if depth >= MAX_PLACEMENTS {
                return Err("placed curves nested too deeply".into());
            }
            let Some(basis) = solved_curve(placed.basis(), depth + 1)? else {
                return Ok(None);
            };
            Similarity::of(placed.transform().rows3())?.curve(basis)?
        }
        SolvedCurveGeometry::Parabola(_) => return Err(unsupported("a parabola")),
        SolvedCurveGeometry::Hyperbola(_) => return Err(unsupported("a hyperbola")),
        SolvedCurveGeometry::Composite { .. } => return Err(unsupported("a composite curve")),
        SolvedCurveGeometry::Unknown { .. } => {
            return Err(unsupported("a curve of a kind the producer did not state"));
        }
    }))
}

/// A face's surface, and whether the record's natural normal is opposite CADIR's.
pub(crate) fn surface(g: &SurfaceGeometry) -> R<(BSurface, bool)> {
    let solved = g
        .solved()
        .ok_or_else(|| unsupported("a procedural surface without its solved form"))?;
    solved_surface(solved, 0)
}

fn solved_surface(g: &SolvedSurfaceGeometry, depth: usize) -> R<(BSurface, bool)> {
    Ok(match g {
        SolvedSurfaceGeometry::Plane(p) => {
            let f = frame(point(p.origin().get()), p.frame());
            (
                BSurface::Plane {
                    origin: f.origin,
                    u_axis: f.x_axis,
                    v_axis: f.y_axis,
                },
                false,
            )
        }
        SolvedSurfaceGeometry::Cylinder(c) => (
            BSurface::Cylinder {
                frame: frame(point(c.origin().get()), c.frame()),
                radius: c.radius().get(),
            },
            false,
        ),
        SolvedSurfaceGeometry::Cone(c) => {
            if c.ratio().get() != 1.0 {
                return Err(unsupported("an elliptical cone"));
            }
            let (f, radius, half_angle) = (
                frame(point(c.origin().get()), c.frame()),
                c.radius().get(),
                c.half_angle().get(),
            );
            if half_angle == 0.0 {
                // A cone of no angle is the cylinder over its section, in the same (u, v).
                if !(radius > 0.0) {
                    return Err(unsupported("a cone of no angle and no radius"));
                }
                (BSurface::Cylinder { frame: f, radius }, false)
            } else {
                (
                    BSurface::Cone {
                        frame: f,
                        radius_at_origin: radius,
                        half_angle,
                    },
                    false,
                )
            }
        }
        SolvedSurfaceGeometry::Sphere(s) => {
            let r = s.radius().get();
            (
                BSurface::Sphere {
                    frame: frame(point(s.center().get()), s.frame()),
                    radius: r.abs(),
                },
                r < 0.0,
            )
        }
        SolvedSurfaceGeometry::Torus(t) => {
            let minor = t.minor_radius().get();
            let torus = BSurface::Torus {
                frame: frame(point(t.center().get()), t.frame()),
                major_radius: t.major_radius().get(),
                minor_radius: minor.abs(),
                tube_start: 0.0,
            };
            (torus, minor < 0.0)
        }
        SolvedSurfaceGeometry::Nurbs(n) => {
            let points: Vec<Vec<[f64; 3]>> = n
                .control_grid()
                .into_iter()
                .map(|r| r.into_iter().map(|p| point(p.get())).collect())
                .collect();
            let weights: Option<Vec<Vec<f64>>> = n.weights().map(|w| {
                w.into_iter()
                    .map(|r| r.into_iter().map(|w| w.get()).collect())
                    .collect()
            });
            let surface = spline_surface(
                [n.u_degree() as usize, n.v_degree() as usize],
                [
                    n.u_knots().as_slice().to_vec(),
                    n.v_knots().as_slice().to_vec(),
                ],
                &points,
                weights.as_deref(),
            )?;
            (surface, n.normal_reversed())
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            if depth >= MAX_PLACEMENTS {
                return Err("placed surfaces nested too deeply".into());
            }
            let (basis, reversed) = solved_surface(placed.basis(), depth + 1)?;
            (
                Similarity::of(placed.transform().rows3())?.surface(basis)?,
                reversed,
            )
        }
        SolvedSurfaceGeometry::Polygonal(_) => return Err(unsupported("a polygonal surface")),
        SolvedSurfaceGeometry::Unknown { .. } => {
            return Err(unsupported(
                "a surface of a kind the producer did not state",
            ));
        }
    })
}

/// The rows of CADIR's affine placement, `[[f64; 4]; 3]`.
trait Rows3 {
    fn rows3(&self) -> [[f64; 4]; 3];
}

impl Rows3 for cadmpeg_ir::transform::Transform {
    fn rows3(&self) -> [[f64; 4]; 3] {
        let r = self.rows();
        [r[0], r[1], r[2]]
    }
}
