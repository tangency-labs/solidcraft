//! solidcore's exact record queries ([`Queries`]): ops' `Query` itself, each method the call
//! this crate's callers made before the interface existed.

use solidcore_ops::face_surface::FaceSurface;
use solidcore_ops::query::Query;
use solidcore_record::Vec3;
use solidcore_record::brep::Brep;
use solidcraft_kernel_api::query::{OutlineStretch, Queries, QueryRefused, SurfacePoint};

use super::Solidcore;

impl Queries for Solidcore {
    type Query<'a> = Query<'a>;
    type FaceSurface = FaceSurface;

    fn face_surface(&self, record: &Brep, face: usize) -> Result<FaceSurface, QueryRefused> {
        FaceSurface::of(record, face).map_err(|e| QueryRefused(e.to_string()))
    }

    fn face_surface_domain(&self, surface: &FaceSurface) -> [[f64; 2]; 2] {
        surface.domain()
    }

    fn face_surface_closest(&self, surface: &FaceSurface, p: [f64; 3], guess: [f64; 2]) -> Result<SurfacePoint, QueryRefused> {
        surface
            .closest_point(p, guess)
            .map(|c| SurfacePoint { uv: c.uv, point: c.point, distance: c.distance })
            .map_err(|e| QueryRefused(e.to_string()))
    }

    fn query<'a>(&self, record: &'a Brep) -> Query<'a> {
        Query::new(record)
    }

    fn query_edge_at(&self, query: &Query<'_>, e: usize, t: f64) -> Option<(Vec3, Vec3)> {
        query.edge_at(e, t)
    }

    fn query_edge_interval(&self, query: &Query<'_>, e: usize) -> Option<[f64; 2]> {
        query.edge_interval(e)
    }

    fn query_edge_faces(&self, query: &Query<'_>, e: usize) -> Vec<usize> {
        query.edge_faces(e)
    }

    fn query_edge_points(&self, query: &Query<'_>, e: usize, sag: f64) -> Option<Vec<(f64, Vec3)>> {
        query.edge_points(e, sag)
    }

    fn query_outline_stretches(&self, query: &Query<'_>, view: Vec3, size: f64) -> Result<Vec<OutlineStretch>, QueryRefused> {
        query
            .outline_stretches(view, size)
            .map(|s| s.into_iter().map(|s| OutlineStretch { edge: s.edge, range: s.range, ends: s.ends, whole: s.whole }).collect())
            .map_err(|e| QueryRefused(e.to_string()))
    }

    fn query_edge_distance(&self, query: &Query<'_>, e: usize, p: Vec3, sag: f64) -> Option<f64> {
        query.edge_distance(e, p, sag)
    }

    fn query_face_uv(&self, query: &Query<'_>, f: usize, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        query.face_uv(f, p, accept)
    }

    fn query_face_normal(&self, query: &Query<'_>, f: usize, uv: [f64; 2]) -> Option<Vec3> {
        query.face_normal(f, uv)
    }

    fn query_face_contains(&self, query: &Query<'_>, f: usize, p: Vec3, accept: f64) -> bool {
        query.face_contains(f, p, accept)
    }

    fn query_faces_at(&self, query: &Query<'_>, p: Vec3, accept: f64) -> Vec<(f64, usize)> {
        query.faces_at(p, accept)
    }

    fn query_faces_against(&self, query: &Query<'_>, normal: Vec3, samples: &[(Vec3, Vec3)], accept: f64) -> Vec<usize> {
        query.faces_against(normal, samples, accept)
    }

    fn query_ray(&self, query: &Query<'_>, o: Vec3, d: Vec3, accept: f64) -> Result<Vec<(f64, usize, Vec3)>, usize> {
        query.ray(o, d, accept)
    }
}
