//! Planar projection ([`Projections`]): sketch curves carried from one plane onto another, edges
//! of a body read exactly in a plane and classified into sketch geometry, and a projected
//! outline's pieces tidied. The values are the record's (`solidcore_record::projection`), so the
//! application holds no kernel type.

use solidcore_record::brep::{Brep, Curve};
use solidcore_record::projection::{OutlinePiece, PlanarPiece, PlaneRelation, Projection};
use solidcore_record::{Plane, Seg2, Vec2, Vec3};

use crate::Backend;

/// A projection the kernel refuses, in its words: input not finite or out of range, a spline it
/// cannot carry, an image past the coordinate limit.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ProjectionRefused(pub String);

/// The planar projection of one kernel ([`crate::OpKind::Projection`]).
pub trait Projections: Backend {
    /// How `source` stands to `target`: parallel normals, seen edge on, an orientation reversed.
    fn plane_relation(&self, source: &Plane, target: &Plane) -> PlaneRelation;
    /// Points of `source` projected along `target`'s normal into `target`'s coordinates.
    fn project_points(&self, source: &Plane, target: &Plane, points: &[Vec2]) -> Vec<Vec2>;
    /// A control spline of `source` (its `degree`, and its knots and weights where `metadata`
    /// has them) projected into `target`; `collapse` when the planes are seen edge on.
    fn project_control(
        &self,
        source: &Plane,
        target: &Plane,
        points: &[Vec2],
        degree: usize,
        metadata: Option<(&[f64], &[f64])>,
        collapse: bool,
        max_coord: f64,
    ) -> Result<Vec<Projection>, ProjectionRefused>;
    /// Cubic Bézier pieces of `source` projected into `target`.
    fn project_beziers(
        &self,
        source: &Plane,
        target: &Plane,
        pieces: &[[Vec2; 4]],
        collapse: bool,
        max_coord: f64,
    ) -> Result<Vec<Projection>, ProjectionRefused>;
    /// A fit spline through `points` of `source` projected into `target`, at most `limit` points.
    fn project_fit(
        &self,
        source: &Plane,
        target: &Plane,
        points: &[Vec2],
        periodic: bool,
        limit: usize,
        collapse: bool,
        max_coord: f64,
    ) -> Result<Vec<Projection>, ProjectionRefused>;
    /// Circles and arcs of `source` projected into `target` exactly; `None` where they don't
    /// project to circles, arcs, ellipses or lines.
    fn project_arcs(&self, source: &Plane, target: &Plane, arcs: &[Seg2], max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused>;
    /// Pieces read in `target` classified into sketch geometry, splines past `limit` points split.
    fn classify_pieces(&self, target: &Plane, pieces: &[PlanarPiece], limit: usize, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused>;
    /// The part `interval` of `curve` (ends `ends`, `forward` or reversed) read exactly in `plane`
    /// to `tol`, spending at most `work` steps; `None` where it isn't read exactly.
    fn project_curve(
        &self,
        curve: &Curve,
        interval: [f64; 2],
        forward: bool,
        ends: [Vec3; 2],
        plane: &Plane,
        tol: f64,
        work: &mut usize,
    ) -> Option<Vec<PlanarPiece>>;
    /// The part `interval` of `curve` lying in `plane` as sketch geometry; `None` where it isn't
    /// read exactly.
    fn projected_part(
        &self,
        curve: &Curve,
        interval: [f64; 2],
        ends: [Vec3; 2],
        plane: &Plane,
        tol: f64,
        limit: usize,
        max_coord: f64,
    ) -> Result<Option<Vec<Projection>>, ProjectionRefused>;
    /// Edge `index` of `body` projected into `plane` as sketch geometry; `None` where it isn't
    /// projected exactly.
    fn projected_edge(
        &self,
        body: &Brep,
        plane: &Plane,
        index: usize,
        tol: f64,
        limit: usize,
        max_coord: f64,
    ) -> Result<Option<Vec<Projection>>, ProjectionRefused>;
    /// The projected pieces of one stretch of an edge, tidied: a point (an edge seen end on)
    /// dropped, a conic spline as its conic spans, a conic seen edge on as its line, circular
    /// conics as arcs of their circle, joined.
    fn tidy_stretch(&self, geom: Vec<OutlinePiece>) -> Vec<OutlinePiece>;
    /// One silhouette's pieces, tidied: a conic seen edge on as its line, circular conics as arcs
    /// of their circle, joined.
    fn tidy_silhouette(&self, geom: Vec<OutlinePiece>) -> Vec<OutlinePiece>;
    /// Whether `p` lies within `tol` of the piece `g`.
    fn near(&self, g: &OutlinePiece, p: Vec2, tol: f64) -> bool;
    /// The pieces, an end within `gap` of an end met before moved onto it.
    fn meet_at_shared_ends(&self, geom: Vec<OutlinePiece>, gap: f64) -> Vec<OutlinePiece>;
    /// The pieces, those that coincide within `tol` drawn once.
    fn drop_coincident(&self, geom: Vec<OutlinePiece>, tol: f64) -> Vec<OutlinePiece>;
}
