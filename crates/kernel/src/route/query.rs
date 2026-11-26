//! The router's exact record queries ([`solidcraft_kernel_api::Queries`]): a body's record made
//! ready ([`Query`]) answers where its edges run, where a point lies on its faces and which way
//! they face, how far a point is from an edge, which faces a ray meets, and its outline.

use solidcore_record::Vec3;
use solidcore_record::brep::Brep;
use solidcraft_kernel_api::{OpKind, Queries};

pub use solidcraft_kernel_api::query::{OutlineStretch, QueryRefused, SurfacePoint};

use super::{Which, which};
use crate::backend::Solidcore;

/// A record ready for exact queries, in the kernel's own form. This build holds solidcore only,
/// so the form is solidcore's query named directly (`<Solidcore as Queries>::Query<'a>`, the same
/// type): a projection would make `Query` invariant in `'a`, where a borrowed record's query is
/// covariant.
pub struct Query<'a> {
    record: &'a Brep,
    form: solidcore_ops::query::Query<'a>,
}

impl std::fmt::Debug for Query<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Query").field("record", &"..").finish_non_exhaustive()
    }
}

/// The kernel that answers queries.
fn kernel() -> Solidcore {
    match which(OpKind::Query) {
        Which::Solidcore => Solidcore,
    }
}

impl<'a> Query<'a> {
    /// `record` made ready. A curve or surface the kernel cannot read is one the queries cannot
    /// evaluate, as a malformed one.
    #[must_use]
    pub fn new(record: &'a Brep) -> Query<'a> {
        Query { record, form: kernel().query(record) }
    }

    /// The record it reads.
    #[must_use]
    pub fn record(&self) -> &'a Brep {
        self.record
    }

    /// Edge `e`'s point and derivative at `t` on its parameter. `None` for a traced intersection
    /// curve (its tangent is not held).
    #[must_use]
    pub fn edge_at(&self, e: usize, t: f64) -> Option<(Vec3, Vec3)> {
        kernel().query_edge_at(&self.form, e, t)
    }

    /// Edge `e`'s interval on its parameter, from its start vertex to its end vertex.
    #[must_use]
    pub fn edge_interval(&self, e: usize) -> Option<[f64; 2]> {
        kernel().query_edge_interval(&self.form, e)
    }

    /// The faces edge `e` bounds, once for each time a loop uses it.
    #[must_use]
    pub fn edge_faces(&self, e: usize) -> Vec<usize> {
        kernel().query_edge_faces(&self.form, e)
    }

    /// Points of edge `e` from its start to its end, each with its parameter, as many as keep the
    /// curve within `sag` mm of the chords between them (a sample, not a bound).
    #[must_use]
    pub fn edge_points(&self, e: usize, sag: f64) -> Option<Vec<(f64, Vec3)>> {
        kernel().query_edge_points(&self.form, e, sag)
    }

    /// The stretches of the body's edges on its outline seen along `view` (a unit vector toward the
    /// viewer), for a body of size `size` (mm), edge by edge.
    ///
    /// # Errors
    /// The kernel's refusal.
    pub fn outline_stretches(&self, view: Vec3, size: f64) -> Result<Vec<OutlineStretch>, QueryRefused> {
        kernel().query_outline_stretches(&self.form, view, size)
    }

    /// How far `p` is from edge `e`.
    #[must_use]
    pub fn edge_distance(&self, e: usize, p: Vec3, sag: f64) -> Option<f64> {
        kernel().query_edge_distance(&self.form, e, p, sag)
    }

    /// The parameters on face `f` of `p`, a point within `accept` mm of its surface.
    #[must_use]
    pub fn face_uv(&self, f: usize, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        kernel().query_face_uv(&self.form, f, p, accept)
    }

    /// Face `f`'s unit normal at its parameters `uv`, out of the material.
    #[must_use]
    pub fn face_normal(&self, f: usize, uv: [f64; 2]) -> Option<Vec3> {
        kernel().query_face_normal(&self.form, f, uv)
    }

    /// Whether `p` lies on face `f`: within `accept` mm of its surface, and inside its loops or
    /// within `accept` mm of one of them.
    #[must_use]
    pub fn face_contains(&self, f: usize, p: Vec3, accept: f64) -> bool {
        kernel().query_face_contains(&self.form, f, p, accept)
    }

    /// The faces `p` lies on, nearest first, each with how far `p` is from it.
    #[must_use]
    pub fn faces_at(&self, p: Vec3, accept: f64) -> Vec<(f64, usize)> {
        kernel().query_faces_at(&self.form, p, accept)
    }

    /// The faces that face against the curves `samples` lie on, ascending: each sample a point on a
    /// curve and its direction there, a face through it meant to face along that direction crossed
    /// with `normal`.
    #[must_use]
    pub fn faces_against(&self, normal: Vec3, samples: &[(Vec3, Vec3)], accept: f64) -> Vec<usize> {
        kernel().query_faces_against(&self.form, normal, samples, accept)
    }

    /// Where the ray from `o` along `d` meets the faces ahead of `o`: each meeting's distance, face
    /// and point, nearest first; the first face it cannot meet exactly (a torus, a spline, a
    /// revolution or a blend) as the error.
    ///
    /// # Errors
    /// The kernel's refusal.
    pub fn ray(&self, o: Vec3, d: Vec3, accept: f64) -> Result<Vec<(f64, usize, Vec3)>, usize> {
        kernel().query_ray(&self.form, o, d, accept)
    }
}

/// A body face's exact surface, in the kernel's own form: the point of it nearest a given point.
#[derive(Debug)]
pub struct FaceSurface {
    form: <Solidcore as Queries>::FaceSurface,
}

impl FaceSurface {
    /// Face `face` of `record`'s exact surface.
    ///
    /// # Errors
    /// The kernel's refusal, in its words: a face past the record's, a record it cannot convert, a
    /// face whose (u, v) are not the kernel's, a blend whose definition is not certified.
    pub fn of(record: &Brep, face: usize) -> Result<FaceSurface, QueryRefused> {
        Ok(FaceSurface { form: kernel().face_surface(record, face)? })
    }

    /// The face's parameter box, `[[u0, u1], [v0, v1]]`, in its own (u, v).
    #[must_use]
    pub fn domain(&self) -> [[f64; 2]; 2] {
        kernel().face_surface_domain(&self.form)
    }

    /// The point of the face's surface nearest `p`, from the guess `guess` (the face's (u, v)),
    /// within the face's parameter box.
    ///
    /// # Errors
    /// The kernel's refusal: no convergence within its steps, a degenerate point.
    pub fn closest_point(&self, p: [f64; 3], guess: [f64; 2]) -> Result<SurfacePoint, QueryRefused> {
        kernel().face_surface_closest(&self.form, p, guess)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_routed_query_is_solidcores() {
        let b = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 3.0, 4.0)).unwrap();
        let routed = Query::new(b.record());
        let direct = solidcore_ops::query::Query::new(b.record());
        let p = Vec3::new(1.0, 1.0, 4.0);
        assert_eq!(routed.faces_at(p, 1e-6), direct.faces_at(p, 1e-6));
        for e in 0..b.record().edges.len() {
            assert_eq!(routed.edge_interval(e), direct.edge_interval(e));
            assert_eq!(routed.edge_faces(e), direct.edge_faces(e));
        }
        let o = Vec3::new(1.0, 1.0, 10.0);
        let d = Vec3::new(0.0, 0.0, -1.0);
        assert_eq!(routed.ray(o, d, 1e-6), direct.ray(o, d, 1e-6));
        // a refusal carries solidcore's words whole
        let words = direct.outline_stretches(Vec3::new(0.0, 0.0, 2.0), 10.0).unwrap_err().to_string();
        assert_eq!(routed.outline_stretches(Vec3::new(0.0, 0.0, 2.0), 10.0), Err(QueryRefused(words)));
    }

    #[test]
    fn a_routed_face_surface_is_solidcores() {
        use solidcore_ops::face_surface::FaceSurface as Direct;
        let b = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 3.0, 4.0)).unwrap();
        for f in 0..b.record().faces.len() {
            let (Ok(routed), Ok(direct)) = (FaceSurface::of(b.record(), f), Direct::of(b.record(), f)) else {
                assert_eq!(FaceSurface::of(b.record(), f).is_ok(), Direct::of(b.record(), f).is_ok());
                continue;
            };
            assert_eq!(routed.domain(), direct.domain());
            let [[u0, u1], [v0, v1]] = routed.domain();
            let guess = [(u0 + u1) / 2.0, (v0 + v1) / 2.0];
            let r = routed.closest_point([1.0, 1.0, 1.0], guess).ok().map(|c| (c.uv, c.point, c.distance));
            let d = direct.closest_point([1.0, 1.0, 1.0], guess).ok().map(|c| (c.uv, c.point, c.distance));
            assert_eq!(r, d);
        }
        // a refusal carries solidcore's words whole
        let words = Direct::of(b.record(), 99).unwrap_err().to_string();
        assert_eq!(FaceSurface::of(b.record(), 99).unwrap_err(), QueryRefused(words));
    }
}
