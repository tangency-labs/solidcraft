//! The topology solver's view of two B-reps, as a library caller of the body-to-body
//! queries (`brep_closest_point`, `brep_relationship`, `brep_distance`) builds it: each
//! B-rep read live through [`LiveBrepObjects`] under its own identities, with a B-rep
//! cache of each. `Object(i, ..)` names an object of B-rep `i`. (The adapter of
//! goldens' `query_api_queries` unit, on the B-reps' proven identities.)

#![allow(dead_code)] // test helpers

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ShapeMember;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::intersect::topology_solver::p1::TopologyObjects;
use solidcore::query::brep::{BrepIdentity, LiveBrepObjects};
use solidcore::surface::surface::p1::SurfaceSolveContext;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::face_context::FaceQueries;
use solidcore::surface::surface_cache::SurfaceCache;
use solidcore::topology::brep_cache::{BrepCache, CacheObject, CacheSource};
use solidcore::{Brep, BrepId, FaceId, Kernel, Point2, Point3, Result, SolidError};

use solidcore::curve::curve::Curve;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Object(pub usize, pub CacheObject);

impl SolutionObject for Object {
    fn kind(&self) -> ObjectKind {
        self.1.kind()
    }
}

/// The two B-reps' solver objects (see the module docs).
pub struct Pair<'a> {
    pub lives: [LiveBrepObjects<'a, 'a>; 2],
    caches: [BrepCache<Object>; 2],
    /// The B-reps as they were when the pair was built, for the queries that take a
    /// `&Brep` (the live B-reps are held mutably; the solves only add caches).
    snapshots: [Brep; 2],
}

impl<'a> Pair<'a> {
    /// The pair of `breps` under `identities` (`BrepId` 0 and 1).
    ///
    /// # Errors
    ///
    /// Those of the cache builds.
    pub fn new(
        k: &Kernel,
        breps: [&'a mut Brep; 2],
        identities: &'a [BrepIdentity<'a>; 2],
    ) -> Result<Self> {
        let mut caches = [
            BrepCache::new(solidcore::geom::a_object::BREP_TYPE)?,
            BrepCache::new(solidcore::geom::a_object::BREP_TYPE)?,
        ];
        for (i, ((cache, brep), id)) in caches.iter_mut().zip(&breps).zip(identities).enumerate() {
            cache.build_trees(k, CacheSource::Brep(brep), |o| Object(i, o), id.keys)?;
        }
        // read-only stand-ins that keep their geometry's owners (a `Brep` clone has none)
        let snapshots = [
            breps[0].clone_standing_for_self()?,
            breps[1].clone_standing_for_self()?,
        ];
        let [b0, b1] = breps;
        let [i0, i1] = identities;
        Ok(Self {
            lives: [
                LiveBrepObjects::new(b0, i0, None)?,
                LiveBrepObjects::new(b1, i1, None)?,
            ],
            caches,
            snapshots,
        })
    }

    fn live(&self, o: Object) -> Result<&LiveBrepObjects<'a, 'a>> {
        self.lives
            .get(o.0)
            .ok_or(SolidError::sm_err("brep pair handle"))
    }

    fn map(o: Object, c: Option<CacheObject>) -> Option<Object> {
        c.map(|c| Object(o.0, c))
    }
}

/// The owning-face services of the two B-reps' solves.
struct Contexts<'a>([Box<dyn SurfaceSolveContext<CacheObject> + 'a>; 2]);

impl std::fmt::Debug for Contexts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Contexts").finish_non_exhaustive()
    }
}

impl Contexts<'_> {
    fn queries(&self, o: Object) -> Result<&dyn FaceQueries<CacheObject>> {
        self.0
            .get(o.0)
            .and_then(|c| c.face_queries())
            .ok_or(SolidError::sm_err("brep pair face queries"))
    }
}

impl SurfaceSolveContext<Object> for Contexts<'_> {
    fn build_surface_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<Object>,
    ) -> Result<SurfaceCache<Object>> {
        let Some(Object(i, c)) = handle else {
            return Err(SolidError::sm_err("brep pair surface handle"));
        };
        let context = self
            .0
            .get_mut(i)
            .ok_or(SolidError::sm_err("brep pair scene handle"))?;
        context
            .build_surface_cache(k, surface, Some(c))?
            .map_handles(|c| Ok(Object(i, c)))
    }
    fn face_queries(&self) -> Option<&dyn FaceQueries<Object>> {
        Some(self)
    }
}

impl FaceQueries<Object> for Contexts<'_> {
    fn face_zone_tol_3d(&self, face: Object) -> Result<f64> {
        self.queries(face)?.face_zone_tol_3d(face.1)
    }
    fn face_point_classify(
        &self,
        k: &Kernel,
        face: Object,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<Object>)> {
        let (class, object) = self
            .queries(face)?
            .face_point_classify(k, face.1, uv, src_tol)?;
        Ok((class, Pair::map(face, object)))
    }
    fn face_point_classify_fast_drop(
        &self,
        k: &Kernel,
        face: Object,
        uv: Point2,
        src_tol: f64,
    ) -> Result<(i32, Option<Object>)> {
        let (class, object) = self
            .queries(face)?
            .face_point_classify_fast_drop(k, face.1, uv, src_tol)?;
        Ok((class, Pair::map(face, object)))
    }
}

impl TopologyObjects<Object> for Pair<'_> {
    fn surface_solve_context(&self) -> Option<Box<dyn SurfaceSolveContext<Object> + '_>> {
        let [l0, l1] = &self.lives;
        Some(Box::new(Contexts([
            l0.surface_solve_context()?,
            l1.surface_solve_context()?,
        ])))
    }
    fn classify_face_point(
        &self,
        k: &Kernel,
        face: Object,
        uv: Point2,
        tolerance: f64,
        boundaries: bool,
        use_uv: bool,
    ) -> Option<Result<i32>> {
        match self.live(face) {
            Ok(live) => live.classify_face_point(k, face.1, uv, tolerance, boundaries, use_uv),
            Err(e) => Some(Err(e)),
        }
    }
    fn classifies_on_face(
        &self,
        k: &Kernel,
        face: Object,
        point: Point3,
        tolerance: f64,
    ) -> Option<Result<bool>> {
        match self.live(face) {
            Ok(live) => live.classifies_on_face(k, face.1, point, tolerance),
            Err(e) => Some(Err(e)),
        }
    }
    fn face_entity(&self, face: Object) -> Option<(&Brep, FaceId)> {
        let CacheObject::Face(f) = face.1 else {
            return None;
        };
        self.snapshots.get(face.0).map(|b| (b, f))
    }
    fn brep_cache(&self, o: Object) -> Result<&BrepCache<Object>> {
        self.caches
            .get(o.0)
            .ok_or(SolidError::sm_err("brep pair cache"))
    }
    fn curve(&self, o: Object) -> Result<&Curve> {
        self.live(o)?.curve(o.1)
    }
    fn analytic_curve_handle(&self, o: Object) -> Option<Object> {
        Some(o)
    }
    fn vertex_point(&self, o: Object) -> Result<Point3> {
        self.live(o)?.vertex_point(o.1)
    }
    fn vertex_tolerance(&self, o: Object) -> Result<f64> {
        self.live(o)?.vertex_tolerance(o.1)
    }
    fn curve_edge(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.curve_edge(o.1)?))
    }
    fn edge_curve(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.edge_curve(o.1)?))
    }
    fn edge_interval(&self, o: Object) -> Result<Extent1d> {
        self.live(o)?.edge_interval(o.1)
    }
    fn edge_tolerance(&self, o: Object) -> Result<f64> {
        self.live(o)?.edge_tolerance(o.1)
    }
    fn surface_face(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.surface_face(o.1)?))
    }
    fn face_surface(&self, o: Object) -> Result<Object> {
        Ok(Object(o.0, self.live(o)?.face_surface(o.1)?))
    }
    fn surface(&self, o: Object) -> Result<&Surface> {
        self.live(o)?.surface(o.1)
    }
    fn face_uv_domain(&self, o: Object) -> Result<Extent2d> {
        self.live(o)?.face_uv_domain(o.1)
    }
    fn face_tolerance(&self, o: Object) -> Result<f64> {
        self.live(o)?.face_tolerance(o.1)
    }
    fn surface_owner(&self, o: Object) -> Result<Option<Object>> {
        Ok(Self::map(o, self.live(o)?.surface_owner(o.1)?))
    }
    fn object_zone_tol_3d(&self, k: &Kernel, o: Object) -> Result<f64> {
        self.live(o)?.object_zone_tol_3d(k, o.1)
    }
    fn shape_member(&self, o: Object) -> ShapeMember {
        let b = BrepId::from_index(o.0 as u32);
        match o.1 {
            CacheObject::Vertex(v) => ShapeMember::Vertex((b, v)),
            CacheObject::Edge(e) => ShapeMember::Edge((b, e)),
            CacheObject::Face(f) => ShapeMember::Face((b, f)),
            _ => ShapeMember::Other,
        }
    }
    fn shape_member_object(&self, member: ShapeMember) -> Result<Object> {
        Ok(match member {
            ShapeMember::Vertex((b, v)) => Object(b.index() as usize, CacheObject::Vertex(v)),
            ShapeMember::Edge((b, e)) => Object(b.index() as usize, CacheObject::Edge(e)),
            ShapeMember::Face((b, f)) => Object(b.index() as usize, CacheObject::Face(f)),
            ShapeMember::Other => return Err(SolidError::sm_err("brep pair shape handle")),
        })
    }
}
