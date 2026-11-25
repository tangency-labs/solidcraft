//! A body as hidden lines read it: its vertices' points, its edges' curves and NURBS forms on their
//! intervals, and its faces' surfaces, NURBS forms, domains and edge loops, in the evaluator's
//! kinds ([`crate::eval::geometry`]). `solidcore-ops`' `hlr` fills it from a record through
//! `solidcore-convert`; a blend face comes as its NURBS form (the blend's definition is no line of
//! a drawing).

use crate::eval::{Curve, Surface};
use crate::math::{Transform, Vec3};

/// A vertex: its point.
#[derive(Clone, Debug, PartialEq)]
pub struct Vertex {
    /// Its point.
    pub point: [f64; 3],
}

/// An edge: its curve as its producer names it, its NURBS form where it is parameterised on one,
/// and its interval on that parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    /// What the edge is: a line, a circle, a spline…
    pub curve: Curve,
    /// The NURBS form its interval is on, where it differs from `curve`'s own parameter.
    pub nurbs: Option<Curve>,
    /// Its interval, from its start vertex to its end vertex.
    pub interval: [f64; 2],
}

impl Edge {
    /// The curve its interval is on: its NURBS form where it has one.
    #[must_use]
    pub fn parameters(&self) -> &Curve {
        self.nurbs.as_ref().unwrap_or(&self.curve)
    }
}

/// One use of an edge by a face's loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrientedEdge {
    /// Into the body's edges.
    pub edge: u32,
    /// Whether the loop runs along the edge.
    pub forward: bool,
}

/// A face's loop.
#[derive(Clone, Debug, PartialEq)]
pub enum Loop {
    /// Edge uses, in the order the loop runs.
    Edges {
        /// The uses.
        edges: Vec<OrientedEdge>,
    },
    /// A single vertex (a pole).
    Vertex {
        /// Into the body's vertices.
        vertex: u32,
    },
}

/// A face: its surface as its producer names it, its NURBS form where its parameters are on one,
/// its modelled rectangle in those parameters and its loops.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// What the face is: a plane, a cylinder, a spline…
    pub surface: Surface,
    /// The NURBS form its parameters are on, where it differs from `surface`'s own.
    pub nurbs: Option<Surface>,
    /// `[[u0, u1], [v0, v1]]` in its parameters.
    pub domain: [[f64; 2]; 2],
    /// Its loops, the outer first.
    pub loops: Vec<Loop>,
}

impl Face {
    /// The surface its parameters are on: its NURBS form where it has one.
    #[must_use]
    pub fn parameters(&self) -> &Surface {
        self.nurbs.as_ref().unwrap_or(&self.surface)
    }
}

/// A body: vertices, edges and faces.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Brep {
    /// The vertices.
    pub vertices: Vec<Vertex>,
    /// The edges.
    pub edges: Vec<Edge>,
    /// The faces.
    pub faces: Vec<Face>,
}

fn point(p: [f64; 3], t: [f64; 3]) -> [f64; 3] {
    [p[0] + t[0], p[1] + t[1], p[2] + t[2]]
}

fn homogeneous(h: [f64; 4], t: [f64; 3]) -> [f64; 4] {
    let w = h[3];
    [h[0] + w * t[0], h[1] + w * t[1], h[2] + w * t[2], w]
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn a3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The frame moved by `t`: its origin moved, its axes made orthonormal again as a rigid move of
/// them is ([`Transform::orthonormal_frame`]); `None` where they are not finite or the kernel
/// refuses them.
fn frame(f: &crate::eval::geometry::Frame, t: [f64; 3]) -> Option<crate::eval::geometry::Frame> {
    let [x, y] = Transform::orthonormal_frame(v3(f.x_axis), v3(f.y_axis)).ok()?;
    Some(crate::eval::geometry::Frame {
        origin: point(f.origin, t),
        x_axis: a3(x),
        y_axis: a3(y),
    })
}

/// The move by `t`, as a matrix (rows 0–2 the images of x, y and z, row 3 the translation).
fn shift(t: [f64; 3]) -> Transform {
    Transform {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [t[0], t[1], t[2], 1.0],
        ],
    }
}

/// `curve` moved by `t`.
fn curve(c: &Curve, t: [f64; 3]) -> Option<Curve> {
    Some(match c {
        Curve::Line { start, end } => Curve::Line {
            start: point(*start, t),
            end: point(*end, t),
        },
        Curve::Circle { frame: f, radius } => Curve::Circle {
            frame: frame(f, t)?,
            radius: *radius,
        },
        Curve::Ellipse { frame: f, radii } => Curve::Ellipse {
            frame: frame(f, t)?,
            radii: *radii,
        },
        Curve::Polyline { points } => Curve::Polyline {
            points: points.iter().map(|p| point(*p, t)).collect(),
        },
        Curve::Spline { knots, points } => Curve::Spline {
            knots: knots.clone(),
            points: points.iter().map(|p| point(*p, t)).collect(),
        },
        Curve::RationalSpline {
            knots,
            homogeneous: h,
        } => Curve::RationalSpline {
            knots: knots.clone(),
            homogeneous: h.iter().map(|h| homogeneous(*h, t)).collect(),
        },
        Curve::SurfaceIntersection {
            surfaces,
            leader,
            tolerance,
        } => Curve::SurfaceIntersection {
            surfaces: {
                let [a, b] = surfaces;
                [Box::new(surface(a, t)?), Box::new(surface(b, t)?)]
            },
            leader: Box::new(curve(leader, t)?),
            tolerance: *tolerance,
        },
    })
}

/// `a · b` of column-major 4×4 matrices.
fn compose(a: &[[f64; 4]; 4], b: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, cell) in out_col.iter_mut().enumerate() {
            *cell = (0..4)
                .map(|k| {
                    a.get(k).and_then(|c| c.get(row)).copied().unwrap_or(0.0)
                        * b.get(col).and_then(|c| c.get(k)).copied().unwrap_or(0.0)
                })
                .sum();
        }
    }
    out
}

/// `surface` moved by `t`. A placed revolution keeps its profile and axis and takes the move into
/// its matrix.
fn surface(s: &Surface, t: [f64; 3]) -> Option<Surface> {
    Some(match s {
        Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => {
            let [u, v] = shift(t).rigid_plane_axes(v3(*u_axis), v3(*v_axis)).ok()?;
            Surface::Plane {
                origin: point(*origin, t),
                u_axis: a3(u),
                v_axis: a3(v),
            }
        }
        // Carried as a rigid move carries it: an origin and axes.
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let (o, [u, v]) = shift(t)
                .rigid_plane_through(v3(*origin), v3(*u_point), v3(*v_point))
                .ok()?;
            Surface::Plane {
                origin: a3(o),
                u_axis: a3(u),
                v_axis: a3(v),
            }
        }
        Surface::Cylinder { frame: f, radius } => Surface::Cylinder {
            frame: frame(f, t)?,
            radius: *radius,
        },
        Surface::Cone {
            frame: f,
            radius_at_origin,
            half_angle,
        } => Surface::Cone {
            frame: frame(f, t)?,
            radius_at_origin: *radius_at_origin,
            half_angle: *half_angle,
        },
        Surface::Sphere { frame: f, radius } => Surface::Sphere {
            frame: frame(f, t)?,
            radius: *radius,
        },
        Surface::BallPatch { center, radius } => Surface::BallPatch {
            center: point(*center, t),
            radius: *radius,
        },
        Surface::Torus {
            frame: f,
            major_radius,
            minor_radius,
            tube_start,
        } => Surface::Torus {
            frame: frame(f, t)?,
            major_radius: *major_radius,
            minor_radius: *minor_radius,
            tube_start: *tube_start,
        },
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => Surface::Spline {
            u_knots: u_knots.clone(),
            v_knots: v_knots.clone(),
            points: points
                .iter()
                .map(|row| row.iter().map(|p| point(*p, t)).collect())
                .collect(),
        },
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous: h,
        } => Surface::RationalSpline {
            u_knots: u_knots.clone(),
            v_knots: v_knots.clone(),
            homogeneous: h
                .iter()
                .map(|row| row.iter().map(|h| homogeneous(*h, t)).collect())
                .collect(),
        },
        Surface::PlacedRevolution {
            profile,
            origin,
            axis,
            matrix,
            forward,
        } => Surface::PlacedRevolution {
            profile: profile.clone(),
            origin: *origin,
            axis: *axis,
            matrix: compose(&shift(t).m, matrix),
            forward: *forward,
        },
    })
}

impl Brep {
    /// The body moved by `t` as a rigid move moves it: every point, frame origin and control
    /// point moved, every frame's axes made orthonormal again and a plane's axes carried
    /// ([`Transform::rigid_plane_axes`]; a plane through three points becomes one with an origin
    /// and axes), a placed revolution through its matrix; parameters, intervals, domains and loops
    /// as they are. `None` where a frame or plane cannot be carried.
    #[must_use]
    pub fn moved(&self, t: [f64; 3]) -> Option<Brep> {
        Some(Brep {
            vertices: self
                .vertices
                .iter()
                .map(|v| Vertex {
                    point: point(v.point, t),
                })
                .collect(),
            edges: self
                .edges
                .iter()
                .map(|e| {
                    Some(Edge {
                        curve: curve(&e.curve, t)?,
                        nurbs: match &e.nurbs {
                            Some(n) => Some(curve(n, t)?),
                            None => None,
                        },
                        interval: e.interval,
                    })
                })
                .collect::<Option<_>>()?,
            faces: self
                .faces
                .iter()
                .map(|f| {
                    Some(Face {
                        surface: surface(&f.surface, t)?,
                        nurbs: match &f.nurbs {
                            Some(n) => Some(surface(n, t)?),
                            None => None,
                        },
                        domain: f.domain,
                        loops: f.loops.clone(),
                    })
                })
                .collect::<Option<_>>()?,
        })
    }
}
