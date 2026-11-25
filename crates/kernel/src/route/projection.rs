//! The router's planar projection ([`solidcraft_kernel_api::Projections`]), as
//! `solidcraft_kernel::projection`: sketch curves carried between planes, body edges read in a
//! plane as sketch geometry, and projected outlines tidied.

use solidcore_record::brep::{Brep, Curve};
use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::{OpKind, Projections};

pub use solidcore_record::projection::{OutlinePiece, PlanarPiece, PlanarSpline, PlaneRelation, Projection};
pub use solidcraft_kernel_api::projection::ProjectionRefused;

use super::{Which, which};
use crate::backend::Solidcore;

/// How `source` stands to `target`: parallel normals, seen edge on, an orientation reversed.
pub fn plane_relation(source: &Plane, target: &Plane) -> PlaneRelation {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.plane_relation(source, target),
    }
}

/// Points of `source` projected along `target`'s normal into `target`'s coordinates.
pub fn project_points(source: &Plane, target: &Plane, points: &[Vec2]) -> Vec<Vec2> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.project_points(source, target, points),
    }
}

/// A control spline of `source` (its `degree`, and its knots and weights where `metadata`
/// has them) projected into `target`; `collapse` when the planes are seen edge on.
pub fn project_control(source: &Plane, target: &Plane, points: &[Vec2], degree: usize, metadata: Option<(&[f64], &[f64])>, collapse: bool, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.project_control(source, target, points, degree, metadata, &[f64])>, collapse, max_coord),
    }
}

/// Cubic Bézier pieces of `source` projected into `target`.
pub fn project_beziers(source: &Plane, target: &Plane, pieces: &[[Vec2; 4]], collapse: bool, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.project_beziers(source, target, pieces, collapse, max_coord),
    }
}

/// A fit spline through `points` of `source` projected into `target`, at most `limit` points.
pub fn project_fit(source: &Plane, target: &Plane, points: &[Vec2], periodic: bool, limit: usize, collapse: bool, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.project_fit(source, target, points, periodic, limit, collapse, max_coord),
    }
}

/// Circles and arcs of `source` projected into `target` exactly; `None` where they don't
/// project to circles, arcs, ellipses or lines.
pub fn project_arcs(source: &Plane, target: &Plane, arcs: &[Seg2], max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.project_arcs(source, target, arcs, max_coord),
    }
}

/// Pieces read in `target` classified into sketch geometry, splines past `limit` points split.
pub fn classify_pieces(target: &Plane, pieces: &[PlanarPiece], limit: usize, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.classify_pieces(target, pieces, limit, max_coord),
    }
}

/// The part `interval` of `curve` (ends `ends`, `forward` or reversed) read exactly in `plane`
/// to `tol`, spending at most `work` steps; `None` where it isn't read exactly.
pub fn project_curve(curve: &Curve, interval: [f64; 2], forward: bool, ends: [Vec3; 2], plane: &Plane, tol: f64, work: &mut usize) -> Option<Vec<PlanarPiece>> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.project_curve(curve, interval, forward, ends, plane, tol, work),
    }
}

/// The part `interval` of `curve` lying in `plane` as sketch geometry; `None` where it isn't
/// read exactly.
pub fn projected_part(curve: &Curve, interval: [f64; 2], ends: [Vec3; 2], plane: &Plane, tol: f64, limit: usize, max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.projected_part(curve, interval, ends, plane, tol, limit, max_coord),
    }
}

/// Edge `index` of `body` projected into `plane` as sketch geometry; `None` where it isn't
/// projected exactly.
pub fn projected_edge(body: &Brep, plane: &Plane, index: usize, tol: f64, limit: usize, max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.projected_edge(body, plane, index, tol, limit, max_coord),
    }
}

/// The projected pieces of one stretch of an edge, tidied: a point (an edge seen end on)
/// dropped, a conic spline as its conic spans, a conic seen edge on as its line, circular
/// conics as arcs of their circle, joined.
pub fn tidy_stretch(geom: Vec<OutlinePiece>) -> Vec<OutlinePiece> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.tidy_stretch(geom),
    }
}

/// One silhouette's pieces, tidied: a conic seen edge on as its line, circular conics as arcs
/// of their circle, joined.
pub fn tidy_silhouette(geom: Vec<OutlinePiece>) -> Vec<OutlinePiece> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.tidy_silhouette(geom),
    }
}

/// Whether `p` lies within `tol` of the piece `g`.
pub fn near(g: &OutlinePiece, p: Vec2, tol: f64) -> bool {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.near(g, p, tol),
    }
}

/// The pieces, an end within `gap` of an end met before moved onto it.
pub fn meet_at_shared_ends(geom: Vec<OutlinePiece>, gap: f64) -> Vec<OutlinePiece> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.meet_at_shared_ends(geom, gap),
    }
}

/// The pieces, those that coincide within `tol` drawn once.
pub fn drop_coincident(geom: Vec<OutlinePiece>, tol: f64) -> Vec<OutlinePiece> {
    match which(OpKind::Projection) {
        Which::Solidcore => Solidcore.drop_coincident(geom, tol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_ops::{conic_projection as conic, outline};

    #[test]
    fn routed_projection_is_solidcores() {
        let (a, b) = (Plane::XY, Plane::XZ);
        let pts = [Vec2::new(0.0, 0.0), Vec2::new(3.0, 1.0), Vec2::new(5.0, -2.0)];
        assert_eq!(plane_relation(&a, &b), conic::plane_relation(&a, &b));
        assert_eq!(project_points(&a, &Plane::XY, &pts), conic::project_points(&a, &Plane::XY, &pts));
        let fit = project_fit(&a, &Plane::XY, &pts, false, 100, false, 1e6).unwrap();
        assert_eq!(fit, conic::project_fit(&a, &Plane::XY, &pts, false, 100, false, 1e6).unwrap());
        let line = vec![OutlinePiece::Line(Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)), OutlinePiece::Line(Vec2::new(1.0, 1e-9), Vec2::new(2.0, 0.0))];
        assert_eq!(meet_at_shared_ends(line.clone(), 1e-6), outline::meet_at_shared_ends(line.clone(), 1e-6));
        assert_eq!(tidy_silhouette(line.clone()), outline::tidy_silhouette(line.clone()));
        assert!(near(&line[0], Vec2::new(0.5, 1e-9), 1e-6));
        // a refusal carries solidcore's words whole (a coordinate past the limit)
        let far = [Vec2::new(0.0, 0.0), Vec2::new(1e9, 0.0)];
        let direct = conic::project_control(&a, &Plane::XY, &far, 1, None, false, 1.0).map_err(|e| ProjectionRefused(e.to_string()));
        assert_eq!(project_control(&a, &Plane::XY, &far, 1, None, false, 1.0), direct);
    }
}
