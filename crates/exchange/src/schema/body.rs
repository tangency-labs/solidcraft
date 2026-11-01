//! Bodies: a welded triangle mesh with the B-rep's faces, edges and corners on it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::geometry::{Direction, Mm, Point, Radians};
use super::ids::{BodyName, EdgeIndex, EdgeName, FaceIndex, FaceName, FeatureId, MaterialName, MeshVertex, VertexIndex, VertexName};

/// One body: a watertight, welded triangle mesh whose vertices its faces share.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Body {
    /// Unique in the snapshot. A part's bodies keep their names; in an assembly the first
    /// placement of a component's body keeps the body's name and every further placement adds
    /// its occurrence path in brackets (`Body1 [Arm:2]`).
    pub name: BodyName,
    /// The physical material assigned to the body, if any (one of [`super::Snapshot::materials`]).
    pub material: Option<MaterialName>,
    /// Every edge of the mesh is shared by exactly two triangles, once each way (false for a
    /// surface body or a mesh that did not close).
    pub closed: bool,
    /// Vertex positions, world coordinates; [`MeshVertex`] indexes them.
    pub positions: Vec<Point>,
    /// The B-rep faces, in the body's face order ([`FaceIndex`]).
    pub faces: Vec<Face>,
    /// The B-rep edges, in the body's edge order ([`EdgeIndex`]).
    pub edges: Vec<Edge>,
    /// The B-rep corners, in the body's corner order ([`VertexIndex`]).
    pub vertices: Vec<Vertex>,
    /// Key/value data for downstream tools (empty for now).
    pub attributes: BTreeMap<String, String>,
    /// The kernel's size of the body: the diagonal of its B-rep vertices' bounding box (curved
    /// faces may bulge past it). For default tolerances and sizes.
    pub size: Mm,
    /// The boundary shells: one for a solid in one piece without voids.
    pub shells: u32,
    /// Mass properties from the kernel, when asked for ([`crate::Options::with_mass`]).
    pub mass: Option<Mass>,
}

/// A body's mass properties, as the kernel measures them (converged tessellations, closer than
/// the snapshot's own mesh).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Mass {
    /// mm³.
    pub volume: f64,
    /// mm².
    pub area: f64,
    pub centroid: Point,
    /// The bounding box's corners.
    pub min: Point,
    pub max: Point,
}

/// One B-rep face and its triangles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Face {
    pub index: FaceIndex,
    pub name: FaceName,
    pub surface: Surface,
    /// Counter-clockwise seen from outside.
    pub triangles: Vec<[MeshVertex; 3]>,
    /// The features that made or shaped the face, in the order its name mentions them.
    pub features: Vec<FeatureId>,
    /// The name was worked out from the geometry rather than followed through the history the
    /// design recorded, so an upstream edit can fool it.
    pub inferred: bool,
    /// Key/value data for downstream tools (empty for now).
    pub attributes: BTreeMap<String, String>,
}

/// One B-rep edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Edge {
    pub index: EdgeIndex,
    #[serde(default)]
    pub name: EdgeName,
    /// The faces it bounds.
    pub bounds: Bounds,
    #[serde(default)]
    pub curve: Curve,
    /// The corners at its start and end (the same one for a closed edge).
    #[serde(default)]
    pub ends: Option<[VertexIndex; 2]>,
    /// The mesh vertices along it from start to end: the triangle sides on the edge, exactly.
    /// Empty when the mesh does not hold the edge.
    #[serde(default)]
    pub path: Vec<MeshVertex>,
}

/// The faces an edge bounds. JSON: `{"between": [2, 5]}` or `{"seam": 3}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bounds {
    /// Where two faces meet.
    Between(FaceIndex, FaceIndex),
    /// Where one face meets itself (the seam of a closed surface).
    Seam(FaceIndex),
}

impl Bounds {
    /// The faces, one or two.
    pub fn faces(self) -> impl Iterator<Item = FaceIndex> {
        let (a, b) = match self {
            Bounds::Between(a, b) => (a, Some(b)),
            Bounds::Seam(a) => (a, None),
        };
        std::iter::once(a).chain(b)
    }

    pub fn contains(self, face: FaceIndex) -> bool {
        self.faces().any(|f| f == face)
    }
}

/// One B-rep corner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Vertex {
    pub index: VertexIndex,
    pub name: VertexName,
    pub point: Point,
    /// The mesh vertex at the corner.
    pub position: Option<MeshVertex>,
    /// The edges that end there.
    pub edges: Vec<EdgeIndex>,
}

/// The surface a face lies on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Surface {
    /// `normal` points out of the material.
    Plane {
        point: Point,
        normal: Direction,
    },
    Cylinder {
        origin: Point,
        axis: Direction,
        radius: Mm,
    },
    /// `axis` points from the apex into the cone.
    Cone {
        apex: Point,
        axis: Direction,
        half_angle: Radians,
    },
    Sphere {
        center: Point,
        radius: Mm,
    },
    /// `major` is the ring radius, negative on a spindle torus's inner sheet; `minor` the tube's.
    Torus {
        center: Point,
        axis: Direction,
        major: Mm,
        minor: Mm,
    },
    /// A B-spline or NURBS patch: `net[i][j]` over the knots `u` (index i) and `v` (index j);
    /// ∂S/∂u × ∂S/∂v points out of the material. The face is the part of the patch its edges
    /// bound.
    Nurbs {
        u: Knots,
        v: Knots,
        net: Vec<Vec<ControlPoint>>,
    },
    /// Any other surface (a spline swept round an axis, an imported mesh), or a kind this
    /// version does not know.
    #[default]
    #[serde(other)]
    Other,
}

/// A NURBS patch's knot vector in one direction: as many knots as control points there plus
/// the degree plus one, non-decreasing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Knots {
    pub degree: u32,
    pub knots: Vec<f64>,
}

/// A NURBS control point and its weight (1 for a B-spline).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ControlPoint {
    pub point: Point,
    pub weight: f64,
}

impl Knots {
    pub fn new(degree: u32, knots: Vec<f64>) -> Self {
        Knots { degree, knots }
    }
}

impl ControlPoint {
    pub fn new(point: Point, weight: f64) -> Self {
        ControlPoint { point, weight }
    }
}

/// The curve an edge lies on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Curve {
    Line {
        start: Point,
        end: Point,
    },
    /// A circle or an arc of one, run counter-clockwise about `axis` through `angle` (2π for a
    /// whole circle).
    Circle {
        center: Point,
        axis: Direction,
        radius: Mm,
        angle: Radians,
    },
    /// Any other curve, or a kind this version does not know.
    #[default]
    #[serde(other)]
    Other,
}

impl Body {
    /// The position of a mesh vertex.
    pub fn position(&self, v: MeshVertex) -> Option<Point> {
        self.positions.get(v.index()).copied()
    }

    pub fn face(&self, f: FaceIndex) -> Option<&Face> {
        self.faces.get(f.index())
    }

    pub fn edge(&self, e: EdgeIndex) -> Option<&Edge> {
        self.edges.get(e.index())
    }

    pub fn vertex(&self, v: VertexIndex) -> Option<&Vertex> {
        self.vertices.get(v.index())
    }

    /// The face with this persistent name.
    pub fn face_named(&self, name: &str) -> Option<&Face> {
        self.faces.iter().find(|f| f.name == *name)
    }

    /// The edge with this persistent name.
    pub fn edge_named(&self, name: &str) -> Option<&Edge> {
        self.edges.iter().find(|e| e.name == *name)
    }

    /// Every triangle with the face it is on and its corners' positions.
    pub fn triangles(&self) -> impl Iterator<Item = (&Face, [Point; 3])> + '_ {
        self.faces
            .iter()
            .flat_map(move |f| f.triangles.iter().filter_map(move |t| Some((f, [self.position(t[0])?, self.position(t[1])?, self.position(t[2])?]))))
    }

    /// The edges that bound `face`.
    pub fn edges_of(&self, face: FaceIndex) -> impl Iterator<Item = &Edge> + '_ {
        self.edges.iter().filter(move |e| e.bounds.contains(face))
    }

    /// The edges that end at `vertex`.
    pub fn edges_at(&self, vertex: VertexIndex) -> impl Iterator<Item = &Edge> + '_ {
        self.vertex(vertex).into_iter().flat_map(move |v| v.edges.iter().filter_map(move |e| self.edge(*e)))
    }
}

impl Edge {
    /// The positions along the edge, start to end.
    pub fn points<'a>(&'a self, body: &'a Body) -> impl Iterator<Item = Point> + 'a {
        self.path.iter().filter_map(move |v| body.position(*v))
    }
}

impl Face {
    /// The mesh vertices the face's triangles use, each once, in ascending order.
    pub fn mesh_vertices(&self) -> Vec<MeshVertex> {
        let mut v: Vec<MeshVertex> = self.triangles.iter().flatten().copied().collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}
