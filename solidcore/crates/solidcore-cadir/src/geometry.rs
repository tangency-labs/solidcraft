//! CADIR's curve and surface carriers as record curves and surfaces.
//!
//! A curve keeps its parameter exactly: an edge's interval on the CADIR carrier is its interval
//! on the record curve. CADIR's line is `origin + t direction` with a unit direction, its circle
//! and ellipse are at the angle `t` from the frame's reference direction (the major axis of an
//! ellipse) about its axis, and a NURBS curve is on its own knots: the record's line through
//! `origin` and `origin + direction`, and its circle, ellipse and spline, have the same `t`. A
//! polyline is the degree-one spline on its own sample parameters; its chords stand within the
//! chordal deflection its producer states of the curve they sample, which its edges' tolerances
//! hold ([`curve_deviation`]).
//!
//! A surface need not keep its (u, v): the assembly makes every pcurve from the 3D edges. What
//! a surface must keep is its side, so each comes back with whether the record's natural normal
//! is opposite CADIR's (`reversed`), which turns the face's sense against it. CADIR's plane,
//! cylinder, circular cone, sphere and torus are the record's with the same frame (x the
//! reference direction, z the axis) and the same formulas, so their natural normals agree, but
//! for a sphere with a negative radius, whose points CADIR places on the other side of the centre
//! (`c + r n` for `r < 0`) while its normal `∂u × ∂v` still points along `n`: the record holds the
//! radius's magnitude, whose normal points out, so it is the opposite. A torus with a negative
//! tube radius is the same surface with `v` moved by π, and its normal `∂u × ∂v` points away from
//! the tube's centre for either sign, as the record's does: it keeps its side. A NURBS surface's oriented normal is opposite `Pu × Pv` when CADIR says
//! `normal_reversed`. A procedural carrier is read by the solved form the producer cached; where
//! its construction states that cache a fit within a tolerance, the edges on it hold that
//! tolerance, so the cache is not taken for the exact surface or curve.
//!
//! A cone, and a procedural linear sweep or revolution with no solved form cached, have a record
//! form that depends on the face on them: each is read as its definition ([`FaceSurface::Swept`])
//! for the assembly to make the record surface from the face's boundary
//! (`Assembly::swept_surface`), as the STEP reader does: a cone within the tolerance of its limit
//! plane over the face is that plane, and a swept profile is bounded where the face needs it.
//! CADIR's parameters on those surfaces decide their normal: a linear sweep and an extrusion run
//! u along the directrix and v along the direction (∂directrix × direction, the definition's own
//! normal, unless the extrusion says its directrix runs reversed); a revolution about an axis
//! runs u round the axis and v along the directrix (∂angle × ∂directrix, the definition's own),
//! and a native revolution does too when `transposed`, else the other way round.
//!
//! A placed carrier (`Transformed`) is its basis mapped by its affine placement, when that is a
//! rotation and a uniform scale (with a move): frames turn, lengths scale and parameters are
//! kept. A placement that shears, scales unevenly or mirrors is refused.

use cadmpeg_ir::geometry::{
    CurveGeometry, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    SurfaceGeometry,
};
use cadmpeg_ir::units::OrthonormalFrame3;
use solidcore::Vec3;
use solidcore_record::brep::{Curve as BCurve, Frame, Surface as BSurface};
use solidcore_step::assembly::{SweptSurface, spline_curve, spline_surface};

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

pub(crate) fn unsupported(what: impl std::fmt::Display) -> String {
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

    /// The uniform scale.
    pub(crate) fn scale(&self) -> f64 {
        self.scale
    }

    /// Whether it is a rigid motion (no scale), its columns unit to `SIMILARITY`.
    pub(crate) fn is_rigid(&self) -> bool {
        (self.scale - 1.0).abs() <= SIMILARITY
    }

    /// The map as a column-major 4 × 4 matrix (`m[j][i]` row i of column j), or `None` at the
    /// identity.
    pub(crate) fn column_major(&self) -> Option<[[f64; 4]; 4]> {
        let r = self.rows;
        let m = [
            [r[0][0], r[1][0], r[2][0], 0.0],
            [r[0][1], r[1][1], r[2][1], 0.0],
            [r[0][2], r[1][2], r[2][2], 0.0],
            [r[0][3], r[1][3], r[2][3], 1.0],
        ];
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        (m != identity).then_some(m)
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
    pub(crate) fn direction(&self, d: [f64; 3]) -> [f64; 3] {
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
pub(crate) fn curve(g: &CurveGeometry) -> R<Option<Carrier>> {
    let solved = g
        .solved()
        .ok_or_else(|| unsupported("a procedural curve without its solved form"))?;
    solved_curve(solved, 0)
}

/// How far, as its producer states it, a curve carrier may lie from the curve it stands for (mm):
/// a polyline's chordal deflection, scaled with the placements it is under; 0 for any other.
/// A procedural carrier's construction states its cache's fit apart ([`curve`] reads the cache).
pub(crate) fn curve_deviation(g: &CurveGeometry) -> f64 {
    let mut at = g.solved();
    let mut scale = 1.0;
    for _ in 0..=MAX_PLACEMENTS {
        match at {
            Some(SolvedCurveGeometry::Polyline(p)) => return scale * p.chordal_deflection().get(),
            Some(SolvedCurveGeometry::Transformed(placed)) => {
                // A placement that is not a similarity refuses the curve ([`curve`]).
                match Similarity::of(placed.transform().rows3()) {
                    Ok(s) => scale *= s.scale(),
                    Err(_) => return 0.0,
                }
                at = Some(placed.basis());
            }
            _ => return 0.0,
        }
    }
    0.0
}

/// An edge's carrier as the record takes it.
#[derive(Clone, Debug)]
pub(crate) enum Carrier {
    /// A record curve with CADIR's parameter; `periodic`: a spline CADIR evaluates modulo its
    /// period, so an edge's interval on it may lie past its range or across its seam.
    Curve { curve: BCurve, periodic: bool },
    /// A parabola (`vertex + f t² x + 2 f t y`), which the record holds as the exact spline over
    /// each edge's interval.
    Parabola {
        vertex: [f64; 3],
        axis: [f64; 3],
        reference: [f64; 3],
        focal: f64,
    },
    /// A hyperbola (`center + a cosh t x + b sinh t y`), held likewise.
    Hyperbola {
        center: [f64; 3],
        axis: [f64; 3],
        reference: [f64; 3],
        radii: [f64; 2],
    },
}

fn solved_curve(g: &SolvedCurveGeometry, depth: usize) -> R<Option<Carrier>> {
    Ok(Some(match g {
        SolvedCurveGeometry::Parabola(p) => {
            let f = p.frame();
            Carrier::Parabola {
                vertex: point(p.vertex().get()),
                axis: a(unit(f.axis())),
                reference: a(unit(f.reference())),
                focal: p.focal_distance().get(),
            }
        }
        SolvedCurveGeometry::Hyperbola(h) => {
            let f = h.frame();
            Carrier::Hyperbola {
                center: point(h.center().get()),
                axis: a(unit(f.axis())),
                reference: a(unit(f.reference())),
                radii: [h.major_radius().get(), h.minor_radius().get()],
            }
        }
        SolvedCurveGeometry::Transformed(placed) => {
            if depth >= MAX_PLACEMENTS {
                return Err("placed curves nested too deeply".into());
            }
            let Some(basis) = solved_curve(placed.basis(), depth + 1)? else {
                return Ok(None);
            };
            let s = Similarity::of(placed.transform().rows3())?;
            match basis {
                Carrier::Curve { curve, periodic } => Carrier::Curve {
                    curve: s.curve(curve)?,
                    periodic,
                },
                Carrier::Parabola {
                    vertex,
                    axis,
                    reference,
                    focal,
                } => Carrier::Parabola {
                    vertex: s.point(vertex),
                    axis: s.direction(axis),
                    reference: s.direction(reference),
                    focal: focal * s.scale(),
                },
                Carrier::Hyperbola {
                    center,
                    axis,
                    reference,
                    radii,
                } => Carrier::Hyperbola {
                    center: s.point(center),
                    axis: s.direction(axis),
                    reference: s.direction(reference),
                    radii: radii.map(|r| r * s.scale()),
                },
            }
        }
        _ => {
            let periodic = matches!(g, SolvedCurveGeometry::Nurbs(n) if n.periodic());
            match solved_plain(g)? {
                Some(curve) => Carrier::Curve { curve, periodic },
                None => return Ok(None),
            }
        }
    }))
}

/// The record curve of a carrier that is one, with CADIR's parameter, or `None` for a degenerate
/// curve. Parabolas, hyperbolas and placed carriers are [`solved_curve`]'s.
fn solved_plain(g: &SolvedCurveGeometry) -> R<Option<BCurve>> {
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
        SolvedCurveGeometry::Transformed(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_) => {
            return Err(unsupported(
                "a placed or open conic carrier read as a plain one",
            ));
        }
        SolvedCurveGeometry::Composite { .. } => return Err(unsupported("a composite curve")),
        SolvedCurveGeometry::Unknown { .. } => {
            return Err(unsupported("a curve of a kind the producer did not state"));
        }
    }))
}

/// A face's surface as the record takes it: a record surface, or a definition whose record form
/// depends on the face, which the assembly makes from the face's boundary.
#[derive(Clone, Debug)]
pub(crate) enum FaceSurface {
    Record(BSurface),
    Swept(SweptSurface),
}

/// A face's surface, and whether the record's natural normal (a definition's own normal, for
/// [`FaceSurface::Swept`]) is opposite CADIR's; `None` for a procedural surface with no solved
/// form cached, or one of a kind the producer did not state, which is read by its definition
/// ([`swept`]) where it has one.
pub(crate) fn surface(g: &SurfaceGeometry) -> R<Option<(FaceSurface, bool)>> {
    match g.solved() {
        Some(SolvedSurfaceGeometry::Unknown { .. }) if g.procedural_construction().is_some() => {
            Ok(None)
        }
        Some(solved) => solved_surface(solved, 0).map(Some),
        None => Ok(None),
    }
}

/// The curve a procedural linear sweep, extrusion or revolution moves, or `None` for another
/// kind of procedural surface.
pub(crate) fn directrix(d: &ProceduralSurfaceDefinition) -> Option<&cadmpeg_ir::ids::CurveId> {
    match d {
        ProceduralSurfaceDefinition::LinearSweep(c) => Some(c.directrix()),
        ProceduralSurfaceDefinition::Extrusion(c) => Some(c.directrix()),
        ProceduralSurfaceDefinition::AxisRevolution(c) => Some(c.directrix()),
        ProceduralSurfaceDefinition::Revolution(c) => Some(c.directrix()),
        _ => None,
    }
}

/// A procedural linear sweep, extrusion or revolution of `profile` (its [`directrix`]) as the
/// assembly's definition, and whether that definition's own normal is opposite CADIR's.
pub(crate) fn swept(d: &ProceduralSurfaceDefinition, profile: Carrier) -> R<(SweptSurface, bool)> {
    let profile = match profile {
        Carrier::Curve { curve, .. } => curve,
        Carrier::Parabola { .. } | Carrier::Hyperbola { .. } => {
            return Err(unsupported("a parabola or hyperbola swept or revolved"));
        }
    };
    let vector = |x: cadmpeg_ir::math::Vector3| [x.x, x.y, x.z];
    Ok(match d {
        ProceduralSurfaceDefinition::LinearSweep(c) => (
            SweptSurface::LinearSweep {
                profile,
                direction: vector(c.direction().get()),
            },
            false,
        ),
        ProceduralSurfaceDefinition::Extrusion(c) => {
            // A revision form's first flag runs the directrix the other way (cadmpeg's
            // evaluator negates its parameter), which turns ∂directrix and the normal.
            let reversed = c
                .revision_form()
                .and_then(|f| f.flags.first().copied())
                .unwrap_or(false);
            (
                SweptSurface::LinearSweep {
                    profile,
                    direction: vector(c.direction().get()),
                },
                reversed,
            )
        }
        ProceduralSurfaceDefinition::AxisRevolution(c) => (
            SweptSurface::Revolution {
                profile,
                origin: point(c.axis_origin().get()),
                axis: a(unit(&c.axis_direction())),
            },
            false,
        ),
        ProceduralSurfaceDefinition::Revolution(c) => (
            SweptSurface::Revolution {
                profile,
                origin: point(c.axis_origin().get()),
                axis: a(unit(&c.axis_direction())),
            },
            // u along the directrix and v round the axis, unless transposed: ∂directrix × ∂angle.
            !*c.transposed(),
        ),
        _ => return Err(unsupported("a procedural surface of this kind")),
    })
}

fn solved_surface(g: &SolvedSurfaceGeometry, depth: usize) -> R<(FaceSurface, bool)> {
    let record = |s: BSurface, reversed: bool| (FaceSurface::Record(s), reversed);
    Ok(match g {
        SolvedSurfaceGeometry::Plane(p) => {
            let f = frame(point(p.origin().get()), p.frame());
            record(
                BSurface::Plane {
                    origin: f.origin,
                    u_axis: f.x_axis,
                    v_axis: f.y_axis,
                },
                false,
            )
        }
        SolvedSurfaceGeometry::Cylinder(c) => record(
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
                record(BSurface::Cylinder { frame: f, radius }, false)
            } else {
                // Its record form depends on the face: the plane it nearly is, when the face
                // stays within the tolerance of that.
                (
                    FaceSurface::Swept(SweptSurface::Cone {
                        frame: f,
                        radius,
                        half_angle,
                    }),
                    false,
                )
            }
        }
        SolvedSurfaceGeometry::Sphere(s) => {
            let r = s.radius().get();
            record(
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
            // A negative tube radius moves v by π: the normal still points away from the tube's
            // centre, so the side is kept.
            record(torus, false)
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
            record(surface, n.normal_reversed())
        }
        SolvedSurfaceGeometry::Transformed(placed) => {
            if depth >= MAX_PLACEMENTS {
                return Err("placed surfaces nested too deeply".into());
            }
            let (basis, reversed) = solved_surface(placed.basis(), depth + 1)?;
            let s = Similarity::of(placed.transform().rows3())?;
            let placed = match basis {
                FaceSurface::Record(b) => FaceSurface::Record(s.surface(b)?),
                FaceSurface::Swept(SweptSurface::Cone {
                    frame,
                    radius,
                    half_angle,
                }) => FaceSurface::Swept(SweptSurface::Cone {
                    frame: s.frame(&frame),
                    radius: radius * s.scale(),
                    half_angle,
                }),
                FaceSurface::Swept(_) => {
                    return Err(unsupported("a placed linear sweep or revolution"));
                }
            };
            (placed, reversed)
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
pub(crate) trait Rows3 {
    fn rows3(&self) -> [[f64; 4]; 3];
}

impl Rows3 for cadmpeg_ir::transform::Transform {
    fn rows3(&self) -> [[f64; 4]; 3] {
        let r = self.rows();
        [r[0], r[1], r[2]]
    }
}
