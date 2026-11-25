//! solidcore's planar projection ([`Projections`]): each the solidcore-ops call this crate's
//! callers made before the interface existed, its refusal's words kept whole.

use solidcore_ops::{conic_projection as conic, outline, spline_projection as spline};
use solidcore_record::brep::{Brep, Curve};
use solidcore_record::projection::{OutlinePiece, PlanarPiece, PlaneRelation, Projection};
use solidcore_record::{Plane, Seg2, Vec2, Vec3};
use solidcraft_kernel_api::{ProjectionRefused, Projections};

use super::Solidcore;

/// solidcore's refusal, in its words.
fn refused(e: solidcore_ops::Error) -> ProjectionRefused {
    ProjectionRefused(e.to_string())
}

impl Projections for Solidcore {
    fn plane_relation(&self, source: &Plane, target: &Plane) -> PlaneRelation {
        conic::plane_relation(source, target)
    }

    fn project_points(&self, source: &Plane, target: &Plane, points: &[Vec2]) -> Vec<Vec2> {
        conic::project_points(source, target, points)
    }

    fn project_control(&self, source: &Plane, target: &Plane, points: &[Vec2], degree: usize, metadata: Option<(&[f64], &[f64])>, collapse: bool, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
        conic::project_control(source, target, points, degree, metadata, &[f64])>, collapse, max_coord).map_err(refused)
    }

    fn project_beziers(&self, source: &Plane, target: &Plane, pieces: &[[Vec2; 4]], collapse: bool, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
        conic::project_beziers(source, target, pieces, collapse, max_coord).map_err(refused)
    }

    fn project_fit(&self, source: &Plane, target: &Plane, points: &[Vec2], periodic: bool, limit: usize, collapse: bool, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
        conic::project_fit(source, target, points, periodic, limit, collapse, max_coord).map_err(refused)
    }

    fn project_arcs(&self, source: &Plane, target: &Plane, arcs: &[Seg2], max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused> {
        conic::project_arcs(source, target, arcs, max_coord).map_err(refused)
    }

    fn classify_pieces(&self, target: &Plane, pieces: &[PlanarPiece], limit: usize, max_coord: f64) -> Result<Vec<Projection>, ProjectionRefused> {
        conic::classify_pieces(target, pieces, limit, max_coord).map_err(refused)
    }

    fn project_curve(&self, curve: &Curve, interval: [f64; 2], forward: bool, ends: [Vec3; 2], plane: &Plane, tol: f64, work: &mut usize) -> Option<Vec<PlanarPiece>> {
        spline::project_curve(curve, interval, forward, ends, plane, tol, work)
    }

    fn projected_part(&self, curve: &Curve, interval: [f64; 2], ends: [Vec3; 2], plane: &Plane, tol: f64, limit: usize, max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused> {
        spline::projected_part(curve, interval, ends, plane, tol, limit, max_coord).map_err(refused)
    }

    fn projected_edge(&self, body: &Brep, plane: &Plane, index: usize, tol: f64, limit: usize, max_coord: f64) -> Result<Option<Vec<Projection>>, ProjectionRefused> {
        spline::projected_edge(body, plane, index, tol, limit, max_coord).map_err(refused)
    }

    fn tidy_stretch(&self, geom: Vec<OutlinePiece>) -> Vec<OutlinePiece> {
        outline::tidy_stretch(geom)
    }

    fn tidy_silhouette(&self, geom: Vec<OutlinePiece>) -> Vec<OutlinePiece> {
        outline::tidy_silhouette(geom)
    }

    fn near(&self, g: &OutlinePiece, p: Vec2, tol: f64) -> bool {
        outline::near(g, p, tol)
    }

    fn meet_at_shared_ends(&self, geom: Vec<OutlinePiece>, gap: f64) -> Vec<OutlinePiece> {
        outline::meet_at_shared_ends(geom, gap)
    }

    fn drop_coincident(&self, geom: Vec<OutlinePiece>, tol: f64) -> Vec<OutlinePiece> {
        outline::drop_coincident(geom, tol)
    }

}
