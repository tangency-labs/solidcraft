//! Exact queries of a body's record ([`Queries`]): points and tangents of its edges, where a point
//! lies on its faces and which way they face, how far a point is from an edge, the faces a ray
//! meets, the stretches of its edges on its outline. A record made ready for queries is the
//! kernel's own form ([`Queries::Query`]), held by the router; so is a face's exact surface
//! ([`Queries::FaceSurface`]), which finds the point of it nearest a given point.

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;

use crate::Backend;

/// One stretch of an edge on a body's outline ([`Queries::query_outline_stretches`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlineStretch {
    /// Into the record's edges.
    pub edge: usize,
    /// Its interval on the edge's parameter.
    pub range: [f64; 2],
    /// Its points at the ends of `range`.
    pub ends: [Vec3; 2],
    /// It is the whole edge.
    pub whole: bool,
}

/// The point of a face's surface nearest a given point ([`Queries::face_surface_closest`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePoint {
    /// Its (u, v), the face's.
    pub uv: [f64; 2],
    /// The point.
    pub point: [f64; 3],
    /// Its distance from the given point.
    pub distance: f64,
}

/// Where a ray meets a face of a body (the router's `Body::face_hits_by_ray`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceHit {
    /// The record face.
    pub face: usize,
    /// The point on the ray where it meets the face.
    pub point: [f64; 3],
    /// The distance from the ray's origin to the point, along the ray.
    pub distance: f64,
}

/// The closest points of a curve or point and a body's chosen faces and edges, by the kernel's
/// minimum-distance solve on the exact geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MembersClosest {
    /// The solve's distance between the two points (a solve's, not certified).
    pub distance: f64,
    /// On the curve or point.
    pub from: [f64; 3],
    /// On the faces or edges.
    pub to: [f64; 3],
}

/// A query the kernel refuses, in its words: a view not a finite unit vector, a size not
/// finite and positive, an edge or face it cannot read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct QueryRefused(pub String);

/// The exact record queries of one kernel ([`crate::OpKind::Query`]).
pub trait Queries: Backend {
    /// A record made ready for exact queries, in the kernel's own form.
    type Query<'a>
    where
        Self: 'a;
    /// `record` made ready. A curve or surface the kernel cannot read is one the queries cannot
    /// evaluate, as a malformed one.
    fn query<'a>(&self, record: &'a Brep) -> Self::Query<'a>;

    /// Edge `e`'s point and derivative at `t` on its parameter. `None` for a traced intersection
    /// curve (its tangent is not held).
    fn query_edge_at(&self, query: &Self::Query<'_>, e: usize, t: f64) -> Option<(Vec3, Vec3)>;
    /// Edge `e`'s interval on its parameter, from its start vertex to its end vertex.
    fn query_edge_interval(&self, query: &Self::Query<'_>, e: usize) -> Option<[f64; 2]>;
    /// The faces edge `e` bounds, once for each time a loop uses it.
    fn query_edge_faces(&self, query: &Self::Query<'_>, e: usize) -> Vec<usize>;
    /// Points of edge `e` from its start to its end, each with its parameter, as many as keep the
    /// curve within `sag` mm of the chords between them (a sample, not a bound).
    fn query_edge_points(&self, query: &Self::Query<'_>, e: usize, sag: f64) -> Option<Vec<(f64, Vec3)>>;
    /// The stretches of the body's edges on its outline seen along `view` (a unit vector toward the
    /// viewer), for a body of size `size` (mm), edge by edge.
    fn query_outline_stretches(&self, query: &Self::Query<'_>, view: Vec3, size: f64) -> Result<Vec<OutlineStretch>, QueryRefused>;
    /// How far `p` is from edge `e`.
    fn query_edge_distance(&self, query: &Self::Query<'_>, e: usize, p: Vec3, sag: f64) -> Option<f64>;
    /// The parameters on face `f` of `p`, a point within `accept` mm of its surface.
    fn query_face_uv(&self, query: &Self::Query<'_>, f: usize, p: Vec3, accept: f64) -> Option<[f64; 2]>;
    /// Face `f`'s unit normal at its parameters `uv`, out of the material.
    fn query_face_normal(&self, query: &Self::Query<'_>, f: usize, uv: [f64; 2]) -> Option<Vec3>;
    /// Whether `p` lies on face `f`: within `accept` mm of its surface, and inside its loops or
    /// within `accept` mm of one of them.
    fn query_face_contains(&self, query: &Self::Query<'_>, f: usize, p: Vec3, accept: f64) -> bool;
    /// The faces `p` lies on, nearest first, each with how far `p` is from it.
    fn query_faces_at(&self, query: &Self::Query<'_>, p: Vec3, accept: f64) -> Vec<(f64, usize)>;
    /// The faces that face against the curves `samples` lie on, ascending: each sample a point on a
    /// curve and its direction there, a face through it meant to face along that direction crossed
    /// with `normal`.
    fn query_faces_against(&self, query: &Self::Query<'_>, normal: Vec3, samples: &[(Vec3, Vec3)], accept: f64) -> Vec<usize>;
    /// Where the ray from `o` along `d` meets the faces ahead of `o`: each meeting's distance, face
    /// and point, nearest first; the first face it cannot meet exactly (a torus, a spline, a
    /// revolution or a blend) as the error.
    fn query_ray(&self, query: &Self::Query<'_>, o: Vec3, d: Vec3, accept: f64) -> Result<Vec<(f64, usize, Vec3)>, usize>;

    /// A body face's exact surface, in the kernel's own form.
    type FaceSurface: std::fmt::Debug;
    /// Face `face` of `record`'s exact surface.
    fn face_surface(&self, record: &Brep, face: usize) -> Result<Self::FaceSurface, QueryRefused>;
    /// The face's parameter box, `[[u0, u1], [v0, v1]]`, in its own (u, v).
    fn face_surface_domain(&self, surface: &Self::FaceSurface) -> [[f64; 2]; 2];
    /// The point of the face's surface nearest `p`, from the guess `guess` (the face's (u, v)),
    /// within the face's parameter box.
    fn face_surface_closest(&self, surface: &Self::FaceSurface, p: [f64; 3], guess: [f64; 2]) -> Result<SurfacePoint, QueryRefused>;
}
