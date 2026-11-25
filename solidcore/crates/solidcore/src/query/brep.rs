//! B-rep bounding boxes and point classification, the polygon decimation attributes,
//! and the checked surface evaluation entry points of `SM_API`.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`,
//! `source/SM_API/src/SmApiSurfaces.cpp`, `source/SMLib/inc/SmPolyDecimate.h`,
//! `source/SMLib/src/SmPolyDecimate.cpp`.
//!
//! | Rust                                         | Upstream                                       |
//! |----------------------------------------------|------------------------------------------------|
//! | [`calculate_bounding_box`], [`BoundingBoxOptions`] | `SmBrep::CalculateBoundingBox`           |
//! | [`calculate_tight_bounding_box`]             | `SmBrep::CalculateTightBoundingBox`            |
//! | [`lock_bounding_box`]                        | `SmBrep::LockBoundingBox`                      |
//! | [`unlock_bounding_box`]                      | `SmBrep::UnLockBoundingBox`                    |
//! | [`BrepQuery::point_3d_classify`]             | `SmBrep::Point3DClassify`                      |
//! | [`BrepQuery::first_face_hit`], [`Ray`], [`FaceRayHit`] | (none: the first face a ray meets, on `SmBrep::RayFire`'s solve) |
//! | [`face_hits`], [`FaceHit`]                   | (none: every face a ray meets, on `SmApiIntersectCurveFace` per face) |
//! | [`BrepQuery`], [`BrepIdentity`]              | the B-rep's `SM_OC_BREP` cache (`CacheMakeOrValidate`) and pointer identities |
//! | [`LazyBrep`]                                 | the `this` of the `const` box methods, which may edit the B-rep |
//! | [`BrepClassifier`]                           | `Point3DClassify` on a B-rep held `const` (its lazily made UV trim curves on a copy) |
//! | [`evaluate_surface_point`]                   | `SmApiEvaluateSurfacePoint`                    |
//! | [`evaluate_surface_normal`]                  | `SmApiEvaluateSurfaceNormal`                   |
//! | [`evaluate_surface_derivatives`]             | `SmApiEvaluateSurfaceDerivatives`              |
//! | [`PolyVertexClass`]                          | `SmPolyVertexClass` (`SmPoly.h`)               |
//! | [`decimate_vertex_attribute`], [`DecimateVertexData`] | `SmDecimateVertexAttr::SmDecimateVertexAttr` (`~` is `Drop`) |
//! | [`DecimateVertexData::make_copy`]            | `SmDecimateVertexAttr::MakeCopy`               |
//! | [`quadric_vertex_attribute`], [`QuadricVertexData`] | `SmQuadricVertexAttr::SmQuadricVertexAttr` (`~` is `Drop`) |
//! | [`Attribute::make_copy`] (data cloned)       | `SmQuadricVertexAttr::MakeCopy`                |
//! | [`quadric_edge_attribute`], [`QuadricEdgeData`] | `SmQuadricEdgeAttr::SmQuadricEdgeAttr` (`~` is `Drop`) |
//! | [`QuadricEdgeData::make_copy`]               | `SmQuadricEdgeAttr::MakeCopy`                  |
//!
//! Deviations:
//! - **Tolerances.** Upstream is built with `SM_USE_OLDTOL`, so the zone tolerance
//!   expansions use `GetTolerance()` of the vertex, edge and face (not
//!   `SmTol::GetZoneTol3d`).
//! - **Locked box.** `m_pLockedBoundingBox` is [`Brep::locked_bounding_box`], an
//!   `Option` instead of a heap pointer.
//! - **`const` boxes that edit.** A tight box classifies points on faces, which may
//!   attach missing UV trim curves and raise edge tolerances (upstream does this to
//!   the B-rep from its `const` methods). [`calculate_bounding_box`] and
//!   [`calculate_tight_bounding_box`] take a [`LazyBrep`]: pass `&mut Brep` for the
//!   upstream behaviour; with `&Brep` the edits are made on a private copy and lost.
//! - **Passed-on statuses.** Upstream [`calculate_tight_bounding_box`] ignores the
//!   statuses of the edge and face boxes and keeps what a failed call leaves. As the
//!   fork does (60a8a51), the four statuses are passed on instead.
//! - **Point classification.** [`BrepQuery::point_3d_classify`] returns the
//!   classification instead of filling the caller's (every upstream caller constructs a
//!   fresh one just before and drops it on failure), with [`EntityRef`] objects. Its
//!   boundary solve and ray fires need the B-rep's cache and object identities, which
//!   upstream reaches through the B-rep (`CacheMakeOrValidate`): a [`BrepQuery`] holds
//!   the identities and builds the cache lazily, on the first solve that needs it (so
//!   single-region classification without boundary intersections skips it, as upstream).
//!   A caller keeps one query while
//!   the B-rep is not edited and makes a fresh one after an edit (upstream revalidates).
//!   The query also keeps the face surface caches its boundary solves and ray fires
//!   make, for its next solve, as upstream keeps each on its surface: each face's
//!   cache is built once per query, not once per solve. They serve a solve only in the
//!   kernel and editing state they were made in.
//!   [`BrepQuery::of_brep`] and [`BrepClassifier::of_brep`] resolve a lone B-rep's
//!   identities from its [`Brep::naming`] scheme and proven owner keys
//!   ([`BrepIdentity::default`] assumes [`arena_identity`]). Face boundary solves prepare
//!   face caches, and ray fires may add missing UV trim curves, which upstream's `const`
//!   method does to the B-rep itself. [`BrepClassifier`] serves callers that hold the
//!   B-rep shared and makes a private copy on the first face boundary solve or ray fire
//!   ([`Brep::clone_standing_for_self`]: its geometry keeps its owners, which the copy
//!   constructor resets, so the copy's caches still hold its curves and surfaces).
//!   A ray fire that fails with an ordinary error moves on to the next direction,
//!   as a failing status does upstream; [`SolidError::Unported`] propagates.
//! - **Out-params.** The evaluated point, normal or point and derivatives are the
//!   return value; on failure there is none (upstream evaluates into locals and leaves
//!   the caller's outputs untouched).
//! - **Pointers.** The `NULL` surface the entry points test for is `None`.
//! - **Statuses.** `SM_ERR_INVALID_INPUT` is [`SolidError::InvalidInput`]; errors of
//!   the surface evaluations pass through unchanged.
//! - **Decimation attributes.** They are [`Attribute`]s whose [`AttributeValue`] is
//!   the class's data, as for the other attribute classes; `MakeCopy` is
//!   [`Attribute::make_copy`] (same id and behaviour, no users, upstream's `new
//!   (crContext)` has no counterpart). The destructors only overwrite fields of an
//!   object being freed (`~SmDecimateVertexAttr`); `Drop` frees it. `SmQuadricEdgeAttr`'s
//!   constructor leaves `m_lSortIndex` uninitialised: it is `None` until set
//!   ([`QuadricEdgeData::sort_index`]). `SmPolyVertexClass` (`SmPoly.h`) is defined
//!   here, the only place that uses it so far.

mod ray;

pub(crate) use ray::solve_mutable_brep_point;
pub(crate) use ray::PreparationRequest;
pub use ray::RayPreparation;
pub(crate) use ray::RaySurfaceContext;
pub use ray::{
    BrepFaceServices, CurveFaceObject, CurveFaceServices, FaceCaches, LiveBrepObjects, SideObject,
    SidedFaceServices,
};

use core::cell::OnceCell;

use crate::curve::curve::Curve;
use crate::error::intersect::IntersectFailure;
use crate::error::query::QueryFailure;
use crate::error::{native_failure, EntityRef, Result, SolidError};
use crate::geom::attribute::{Attribute, AttributeValue, Behavior};
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::ObjectRef;
use crate::geom::solution_array::Solution;
use crate::geom::tol::{brep_zone_tol_3d, SmallTopoType};
use crate::intersect::api_intersectors::{
    intersect_curve_face, intersect_curve_face_within, CurveHits,
};
use crate::intersect::point_class::{PointClassType, PointClassification};
use crate::intersect::topology_solver::p1::TopologySolver;
use crate::intersect::topology_solver::p2::TopologySolveOptions;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::nurbs::knots::Side;
use crate::primitive::api_curves::create_line_segment;
use crate::query::api_queries::surface_closest_point;
use crate::query::edge as qe;
use crate::query::face::{p1 as qf1, p3 as qf3};
use crate::surface::surface::Surface;
use crate::topology::brep::{p1 as bp1, p2 as bp2};
use crate::topology::brep_cache::{
    arena_identity, make_brep_cache, BrepCache, BrepObjects, CacheObject, CacheSource,
};
use crate::topology::{edge, face, vertex, Brep, BrepId, FaceId, NamingScheme, RegionId};

const OP_TIGHT_BOX: &str = "query::brep::calculate_tight_bounding_box";
const OP_CLASSIFY: &str = "query::brep::BrepQuery::point_3d_classify";
const OP_RAY: &str = "query::brep::Ray::new";
const OP_FIRST_HIT: &str = "query::brep::BrepQuery::first_face_hit";
const OP_EVAL_POINT: &str = "query::brep::evaluate_surface_point";
const OP_EVAL_NORMAL: &str = "query::brep::evaluate_surface_normal";
const OP_EVAL_DERIVS: &str = "query::brep::evaluate_surface_derivatives";

/// The axis aligned box only (`CalculateBoundingBox(&box, NULL, NULL, ..)`).
const NORMAL_BOX: BoundingBoxRequest = BoundingBoxRequest {
    normal: true,
    pseudo: false,
    polar: false,
};

// ---- bounding boxes -----------------------------------------------------------------

/// With [`BoundingBoxOptions::approx`], a B-rep of more vertices than this (and more
/// edges than [`APPROX_EDGES`]) is boxed by its vertices only.
pub const APPROX_VERTICES: usize = 500;
/// See [`APPROX_VERTICES`].
pub const APPROX_EDGES: usize = 100;

/// The arguments of [`calculate_bounding_box`]; [`Default`] gives upstream's default
/// arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBoxOptions {
    /// `bApprox` (default `FALSE`): leave out the faces and, on a B-rep with more than
    /// [`APPROX_VERTICES`] vertices and more than [`APPROX_EDGES`] edges, the edges too.
    pub approx: bool,
    /// `bTight` (default `FALSE`): the tight box of every edge and face (expensive).
    pub tight: bool,
    /// `bExpandByZoneTol3d` (default `TRUE`): expand each vertex, edge and face box by
    /// that entity's tolerance.
    pub expand_by_zone_tol_3d: bool,
}

impl Default for BoundingBoxOptions {
    fn default() -> Self {
        Self {
            approx: false,
            tight: false,
            expand_by_zone_tol_3d: true,
        }
    }
}

/// `SmBrep::CalculateBoundingBox`: the locked box when there is one; else the union of
/// the B-rep's vertex points, the boxes of its edges with a curve and, unless
/// `approx`, the boxes of its faces (each expanded by the entity's tolerance with
/// `expand_by_zone_tol_3d`), in list order. With `approx` on a B-rep of more than
/// [`APPROX_VERTICES`] vertices and [`APPROX_EDGES`] edges only the vertices are used.
///
/// # Errors
///
/// Those of the B-rep lists, [`qe::calculate_bounding_box`] and
/// [`qf1::calculate_bounding_box`].
pub fn calculate_bounding_box<'a>(
    k: &Kernel,
    b: impl Into<LazyBrep<'a>>,
    opts: BoundingBoxOptions,
) -> Result<Extent3d> {
    let mut lb = b.into();
    let b = lb.get();
    // when LockedBoundingBox exists - return that value
    if let Some(locked) = b.locked_bounding_box() {
        return Ok(locked);
    }

    let mut brep_bbox = Extent3d::new();

    // add every Brep->Vertex->point to Bounding box
    let vertices = bp1::vertices(b, None)?;

    // for every vertex - union location into BBox
    for &v in &vertices {
        let mut v_bbox = Extent3d::from_point(vertex::point(b, v)?);

        if opts.expand_by_zone_tol_3d {
            v_bbox.expand_absolute(vertex::tolerance(b, v)?); // SM_USE_OLDTOL
        }

        // union vertex bounding box into brep bounding box
        brep_bbox = brep_bbox.union(&v_bbox);
    } // end iter every vertex

    // Union in every Brep->Edge bounding box
    let edges = bp1::edges(b, None)?;

    // when asked - if this is a large brep, settle for a fast approximation
    if opts.approx && vertices.len() > APPROX_VERTICES && edges.len() > APPROX_EDGES {
        return Ok(brep_bbox);
    }

    // for every edge - union together edge BBoxes
    for &e in &edges {
        if b.edge(e)?.curve.is_some() {
            let mut e_bbox = qe::calculate_bounding_box(k, b, e, NORMAL_BOX, opts.tight)?
                .normal
                .unwrap_or_default();

            if opts.expand_by_zone_tol_3d {
                e_bbox.expand_absolute(edge::tolerance(b, e)?); // SM_USE_OLDTOL
            }

            // union edge bounding box into brep bounding box
            brep_bbox = brep_bbox.union(&e_bbox);
        }
    }

    // when asked - union in every Brep->Face bounding box
    if !opts.approx {
        // for every face
        for f in bp1::faces(b, None)? {
            let mut f_bbox = qf1::calculate_bounding_box_in(k, &mut lb, f, opts.tight)?;

            if opts.expand_by_zone_tol_3d {
                f_bbox.expand_absolute(face::p2::tolerance(lb.get(), f)?); // SM_USE_OLDTOL
            }

            // union face bounding box into brep bounding box
            brep_bbox = brep_bbox.union(&f_bbox);
        }
    }

    Ok(brep_bbox)
}

/// `SmBrep::CalculateTightBoundingBox`: the union of the vertex points, then of the
/// tight box of every edge and face whose loose box is not already inside the box so
/// far (the locked box is not used, nothing is expanded). The statuses of the edge and
/// face boxes are ignored (see the module's deviations).
///
/// # Errors
///
/// Those of the B-rep lists and vertex points; [`SolidError::Unported`] from the edge
/// and face boxes.
pub fn calculate_tight_bounding_box<'a>(
    k: &Kernel,
    b: impl Into<LazyBrep<'a>>,
) -> Result<Extent3d> {
    let mut lb = b.into();
    let b = lb.get();
    let mut brep_bbox = Extent3d::new();

    for v in bp1::vertices(b, None)? {
        let v_bbox = Extent3d::from_point(vertex::point(b, v)?);
        brep_bbox = brep_bbox.union(&v_bbox);
    }

    // Union all of the Tight Edge bounding boxes. (not upstream) cooperative cancellation
    // once per edge and face (solidcore#3247).
    for e in bp1::edges(b, None)? {
        k.check_cancelled(OP_TIGHT_BOX)?;
        // (CalculateBoundingBox initialises the box first)
        let mut edge_bbox = qe::calculate_bounding_box(k, b, e, NORMAL_BOX, false)?
            .normal
            .unwrap_or_default();
        if edge_bbox.is_contained_by(&brep_bbox, 0.0) {
            continue;
        }
        // (an edge without curve leaves the loose box)
        if let Some(tight) = qe::calculate_tight_bounding_box(k, b, e)? {
            edge_bbox = tight;
        }
        brep_bbox = brep_bbox.union(&edge_bbox);
    }

    // Union all of the Tight Face bounding boxes (upstream keeps `rFaceBBox` from face
    // to face, which matters only after a failure; the fork passes failures on).
    for f in bp1::faces(b, None)? {
        k.check_cancelled(OP_TIGHT_BOX)?;
        let mut face_bbox = qf1::calculate_bounding_box(k, lb.get(), f, false)?;
        if face_bbox.is_contained_by(&brep_bbox, 0.0) {
            continue;
        }
        qf3::calculate_tight_bounding_box_into(k, &mut lb, f, &mut face_bbox)?;
        brep_bbox = brep_bbox.union(&face_bbox);
    }

    Ok(brep_bbox)
}

/// `SmBrep::LockBoundingBox(bTight)` (upstream default `FALSE`): unlocks the box, then
/// computes it ([`calculate_tight_bounding_box`] with `tight`, else
/// [`calculate_bounding_box`] with the default arguments) and locks it. On failure the
/// box stays unlocked.
///
/// # Errors
///
/// Those of the box computation.
pub fn lock_bounding_box(k: &Kernel, b: &mut Brep, tight: bool) -> Result<()> {
    if b.locked_bounding_box().is_some() {
        unlock_bounding_box(b);
    }

    let bbox = if tight {
        calculate_tight_bounding_box(k, &mut *b)?
    } else {
        calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?
    };

    // Okay, now lock it.
    b.set_locked_bounding_box(Some(bbox));
    Ok(())
}

/// `SmBrep::UnLockBoundingBox`: forgets the locked box, if any.
pub fn unlock_bounding_box(b: &mut Brep) {
    b.set_locked_bounding_box(None);
}

// ---- point classification -----------------------------------------------------------

/// The identities a B-rep's cache is keyed by (upstream: the B-rep's own pointers):
/// the B-rep in the caller's shape store, the key of each of its cache objects, and
/// the owners of intermediate geometry (as for `Curve::edge`).
#[derive(Clone, Copy)]
pub struct BrepIdentity<'a> {
    /// The B-rep in the caller's shape store.
    pub brep_id: BrepId,
    /// The identity of each cache object.
    pub keys: &'a dyn Fn(CacheObject) -> ObjectRef,
    /// The owner of an intermediate generated curve or edge use.
    pub owner_of: &'a dyn Fn(ObjectRef) -> Option<ObjectRef>,
}

impl core::fmt::Debug for BrepIdentity<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BrepIdentity")
            .field("brep_id", &self.brep_id)
            .finish_non_exhaustive()
    }
}

/// No intermediate geometry owners.
fn no_owner(_: ObjectRef) -> Option<ObjectRef> {
    None
}

impl BrepIdentity<'static> {
    /// The identities of a B-rep whose geometry is owned by its records'
    /// [`arena_identity`] (such as imported B-reps), without intermediate owners. For a
    /// B-rep whose naming scheme may be [`NamingScheme::Notify`], use [`Self::of_brep`].
    #[must_use]
    pub fn arena(brep_id: BrepId) -> Self {
        Self {
            brep_id,
            keys: &arena_identity,
            owner_of: &no_owner,
        }
    }
}

impl BrepIdentity<'static> {
    /// The base identities of `b` at `brep_id` by its recorded naming scheme alone
    /// ([`Brep::naming`]): the Euler operators' notification identities for a
    /// [`NamingScheme::Notify`] B-rep (one built through `new_brep` and the face
    /// builders), else [`BrepIdentity::arena`]. It ignores the proven owner keys `b`
    /// records (`Brep::geometry_owner_keys`), so it misses the geometry of a copy whose
    /// owners are the caller's keys ([`crate::topology::brep::p3::api_brep_copy`] under
    /// a caller `key_of`): prefer [`Self::of_brep_with_id`].
    #[must_use]
    pub fn of_naming_with_id(brep_id: BrepId, b: &Brep) -> Self {
        match b.naming() {
            NamingScheme::Notify => Self {
                brep_id,
                keys: &notify_identity,
                owner_of: &no_owner,
            },
            _ => Self::arena(brep_id),
        }
    }

    /// The identities `b`'s geometry owners use, at `brep_id`: its naming scheme
    /// ([`Self::of_naming_with_id`]) overridden by the proven producer owner keys it
    /// records (`Brep::geometry_owner_keys`), as [`ProvenBrepIdentity::new`]. The
    /// overrides are owned, so the result is a [`ProvenBrepIdentity`]; pass
    /// [`ProvenBrepIdentity::identity`] where a [`BrepIdentity`] is taken. A query
    /// under the wrong identities resolves no geometry owner and finds nothing
    /// (`plan/findings.md`, "two owner-naming schemes"; #1440).
    #[must_use]
    pub fn of_brep_with_id(brep_id: BrepId, b: &Brep) -> ProvenBrepIdentity<'static> {
        ProvenBrepIdentity::new(Self::of_naming_with_id(brep_id, b), b)
    }

    /// [`Self::of_brep_with_id`] at [`BrepId`] `0`.
    #[must_use]
    pub fn of_brep(b: &Brep) -> ProvenBrepIdentity<'static> {
        Self::of_brep_with_id(BrepId::from_index(0), b)
    }
}

type ProvenKeyFn<'a> = Box<dyn Fn(CacheObject) -> ObjectRef + 'a>;

/// A [`BrepIdentity`] with the proven producer owner keys of its B-rep
/// (`Brep::geometry_owner_keys`) overriding the base keys where they differ: what
/// [`BrepIdentity::of_brep`] and [`BrepQuery::of_brep`] resolve a B-rep's records by.
/// It owns its overrides and does not borrow the B-rep.
pub struct ProvenBrepIdentity<'a> {
    base: BrepIdentity<'a>,
    proven_keys: Option<ProvenKeyFn<'a>>,
}

impl core::fmt::Debug for ProvenBrepIdentity<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProvenBrepIdentity")
            .field("base", &self.base)
            .field("overridden", &self.proven_keys.is_some())
            .finish()
    }
}

impl<'a> ProvenBrepIdentity<'a> {
    /// `base` without overrides.
    #[must_use]
    pub const fn plain(base: BrepIdentity<'a>) -> Self {
        Self {
            base,
            proven_keys: None,
        }
    }

    /// `base`, overriding edge/face keys where `b` records a proven producer
    /// geometry-owner key (`Brep::geometry_owner_keys`) that differs from `base.keys`
    /// for geometry actually owned by it. Unowned (`None`) and foreign-owned geometry
    /// not recorded in `b.geometry_owner_keys()` is still excluded.
    #[must_use]
    pub fn new(base: BrepIdentity<'a>, b: &Brep) -> Self {
        let base_keys = base.keys;
        let mut overrides = std::collections::BTreeMap::new();
        for (&entity, &proven) in b.geometry_owner_keys() {
            let matches_base = match entity {
                EntityRef::Edge(e) => b
                    .edges()
                    .get(e)
                    .and_then(|edge| edge.curve.as_ref())
                    .and_then(Curve::owner)
                    .is_some_and(|actual| actual == base_keys(CacheObject::Edge(e))),
                EntityRef::Face(f) => b
                    .faces()
                    .get(f)
                    .and_then(|face| face.surface.as_ref())
                    .and_then(Surface::owner)
                    .is_some_and(|actual| actual == base_keys(CacheObject::Face(f))),
                _ => true,
            };
            if !matches_base {
                overrides.insert(entity, proven);
            }
        }
        let proven_keys: Option<ProvenKeyFn<'a>> = (!overrides.is_empty()).then(|| {
            Box::new(move |object| {
                let entity = match object {
                    CacheObject::Edge(e) | CacheObject::Curve(e) => Some(EntityRef::Edge(e)),
                    CacheObject::Face(f) | CacheObject::Surface(f) => Some(EntityRef::Face(f)),
                    _ => None,
                };
                entity
                    .and_then(|e| overrides.get(&e).copied())
                    .unwrap_or_else(|| base_keys(object))
            }) as ProvenKeyFn<'a>
        });
        Self { base, proven_keys }
    }

    /// The base identities, without the overrides.
    #[must_use]
    pub const fn base(&self) -> BrepIdentity<'a> {
        self.base
    }

    /// The effective identities: the base with the proven overrides applied.
    #[must_use]
    pub fn identity(&self) -> BrepIdentity<'_> {
        BrepIdentity {
            brep_id: self.base.brep_id,
            keys: match &self.proven_keys {
                Some(keys) => keys.as_ref(),
                None => self.base.keys,
            },
            owner_of: self.base.owner_of,
        }
    }
}

/// The cache-object identities of a [`NamingScheme::Notify`] B-rep
/// ([`crate::topology::loopuse::notify_ref`] for its edges, faces and vertices).
fn notify_identity(object: CacheObject) -> ObjectRef {
    crate::topology::brep::p2::caller_cache_key(&crate::topology::loopuse::notify_ref, object)
}

impl Default for BrepIdentity<'_> {
    /// [`BrepIdentity::arena`] of the first B-rep of the caller's store (a caller
    /// querying one B-rep at a time).
    fn default() -> Self {
        BrepIdentity::arena(BrepId::from_index(0))
    }
}

/// The ray directions of [`BrepQuery::point_3d_classify`], in firing order: each axis
/// forward, then backward when the forward ray fails.
const CLASSIFY_RAYS: [(Vec3, Vec3); 3] = [
    (Vec3::new(1.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)),
    (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0)),
    (Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0)),
];

/// A ray (not upstream): a finite origin and a unit direction, so a distance along it is its
/// parameter. Made only by [`Ray::new`], which refuses what is no ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    origin: Point3,
    direction: Vec3,
}

impl Ray {
    /// The ray from `origin` along `direction`, made unit at any finite length
    /// ([`Vec3::length_and_unit`]).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a coordinate that is not finite, or a direction of no
    /// length: one whose length is below `SM_EFF_ZERO` ([`EFF_ZERO`]).
    pub fn new(origin: Point3, direction: Vec3) -> Result<Self> {
        let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        if !(finite(origin) && finite(direction)) {
            return Err(SolidError::invalid_input(
                OP_RAY,
                "a coordinate that is not finite",
            ));
        }
        let no_length = || SolidError::invalid_input(OP_RAY, "a direction of no length");
        let (length, direction) = direction.length_and_unit().ok_or_else(no_length)?;
        if length < EFF_ZERO {
            return Err(no_length());
        }
        Ok(Self { origin, direction })
    }

    /// Where the ray starts.
    #[must_use]
    pub const fn origin(&self) -> Point3 {
        self.origin
    }

    /// Its unit direction.
    #[must_use]
    pub const fn direction(&self) -> Vec3 {
        self.direction
    }
}

/// The first face a ray meets ([`BrepQuery::first_face_hit`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceRayHit {
    /// The face.
    pub face: FaceId,
    /// The (u, v) of the face's surface at the hit.
    pub uv: Point2,
    /// The surface's point there.
    pub point: Point3,
    /// The ray's parameter at the hit: its distance from the origin along the ray.
    pub distance: f64,
}

/// A point where a ray meets a face ([`face_hits`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceHit {
    /// The face.
    pub face: FaceId,
    /// The point on the ray where it meets the face.
    pub point: Point3,
    /// Its distance from the ray's origin along the ray.
    pub distance: f64,
}

/// (Not upstream) Every point where `ray` meets a face of `b`, nearest first. The ray, as a
/// segment from its origin past the far side of `b`'s box ([`calculate_bounding_box`], the
/// default options), is solved against each face both by [`intersect_curve_face`] (its fixed
/// `0.001`, which finds a shallow crossing) and by its solve within the B-rep's zone tolerance
/// ([`brep_zone_tol_3d`], the tolerance of [`BrepQuery::first_face_hit`]; it finds a grazing
/// one), and a point found is a hit only where it lies on `b`'s boundary within that
/// tolerance: a ray passing the body farther off meets nothing, one
/// within it touches it, and one crossing a face at a grazing angle meets it at each crossing.
/// A point is a hit of every face meeting there: the face it lies in, both faces of an edge,
/// every face round a vertex. Points of one face within twice the tolerance of each other (a
/// crossing solved at both parameters of a seam or pole, or found again from a neighbouring
/// face) are one hit. So a face the ray crosses twice (a cylinder's side) gives two hits, a
/// crossing on its seam or a sphere's pole one, and a ray through an edge or a vertex, or
/// grazing an edge from outside, one hit of each face there, at one point and distance. Hits
/// at one distance are in the faces' order (`GetFaces`); [`BrepQuery::first_face_hit`] names
/// one of them, its solve's first. A hit at the ray's origin is at distance 0. Where the ray
/// lies along a face within the tolerance over a stretch, that face has no hit along the
/// stretch, its ends included, except at an end where the ray goes into or out of it: one
/// inside the face, one on its own seam or pole where no other stretch of it goes on, or one
/// on an edge whose other face the ray lies along there too (a rim grazed); not one at the
/// segment's ends. So a ray grazing a curved face within the tolerance meets it twice. Empty
/// when the ray meets no face, including a ray that starts past the box. The solves may make
/// missing UV trim curves on `b`.
///
/// [`intersect_curve_face`]: crate::intersect::api_intersectors::intersect_curve_face
///
/// # Errors
///
/// Those of the box, the segment and the solves (a face without a surface among them),
/// except a face's [`IntersectFailure::NoIntersection`], which is no hit; those of the
/// stretches' evaluations, the cache, the boundary solve of each point, the faces of its
/// vertex or edge and the seam and pole checks.
pub fn face_hits(k: &Kernel, b: &mut Brep, ray: Ray) -> Result<Vec<FaceHit>> {
    let corners = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?.corners();
    let reach = corners
        .iter()
        .fold(0.0_f64, |m, c| m.max(c.distance(ray.origin)));
    if reach <= EFF_ZERO {
        return Ok(Vec::new());
    }
    let far = 2.0 * reach;
    let segment: Curve = create_line_segment(ray.origin, ray.origin + ray.direction * far)?.into();
    let tolerance = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
    let along_ray = |p: Point3| (p - ray.origin).dot(ray.direction);
    let faces = bp1::faces(b, None)?;
    let mut points = Vec::new();
    // each stretch the ray lies along a face: the face and its range along the ray
    let mut stretches: Vec<(FaceId, f64, f64)> = Vec::new();
    let mut ends: Vec<(FaceId, Point3)> = Vec::new();
    for &f in &faces {
        if let Some(found) = solved(intersect_curve_face_within(k, &segment, b, f, tolerance))? {
            for curve in &found.curves {
                let interval = curve.natural_interval();
                let p = curve.evaluate_point(k, interval.min())?;
                let q = curve.evaluate_point(k, interval.max())?;
                let (s, t) = (along_ray(p), along_ray(q));
                stretches.push((f, s.min(t), s.max(t)));
                for (end, at) in [(p, s), (q, t)] {
                    if at > tolerance && at < far - tolerance {
                        ends.push((f, end));
                    }
                }
            }
            points.extend(found.points);
        }
        // the 0.001 solve finds the shallow crossings the zone-tolerance one misses; its
        // points off the body fail the boundary check below
        if let Some(found) = solved(intersect_curve_face(k, &segment, b, f))? {
            points.extend(found.points);
        }
    }
    if points.is_empty() && ends.is_empty() {
        return Ok(Vec::new());
    }
    // each hit with its face's place in `faces`, for ties
    let mut hits: Vec<(Option<usize>, FaceHit)> = Vec::new();
    let query = BrepQuery::of_brep(b);
    let on_stretch = |face: FaceId, at: f64| {
        stretches
            .iter()
            .any(|&(g, s, t)| g == face && at >= s - tolerance && at <= t + tolerance)
    };
    // another stretch of the face starts where this one ends (split at its seam or pole)
    let goes_on = |face: FaceId, end: Point3| {
        ends.iter()
            .filter(|&&(g, p)| g == face && p.distance(end) <= 2.0 * tolerance)
            .count()
            > 1
    };
    // a stretch's end where the ray goes into or out of its face
    for &(f, end) in &ends {
        let at = along_ray(end);
        let entity = query.entity_at(k, b, end, tolerance)?;
        let seam_or_pole = match entity {
            Some(EntityRef::Edge(e)) => edge::face_on_both_sides(b, e, f)?,
            Some(EntityRef::Vertex(v)) => vertex::is_on_pole(k, b, v)?,
            _ => false,
        };
        let crossing = if seam_or_pole {
            !goes_on(f, end)
        } else {
            match entity {
                Some(EntityRef::Face(g)) => g == f,
                // a rim grazed: the face beyond the edge lies along the ray there too
                Some(EntityRef::Edge(e)) => edge::faces(b, e, None)?
                    .into_iter()
                    .any(|g| g != f && on_stretch(g, at)),
                _ => false,
            }
        };
        if crossing {
            add_hit(&mut hits, &faces, f, end, at, tolerance);
        }
    }
    for point in points {
        let distance = along_ray(point).max(0.0);
        let faces_there = match query.entity_at(k, b, point, tolerance)? {
            Some(EntityRef::Vertex(v)) => vertex::faces(b, v)?,
            Some(EntityRef::Edge(e)) => edge::faces(b, e, None)?,
            Some(EntityRef::Face(f)) => vec![f],
            _ => Vec::new(),
        };
        for face in faces_there {
            if !on_stretch(face, distance) {
                add_hit(&mut hits, &faces, face, point, distance, tolerance);
            }
        }
    }
    hits.sort_by(|(r, p), (s, q)| p.distance.total_cmp(&q.distance).then(r.cmp(s)));
    Ok(hits.into_iter().map(|(_, h)| h).collect())
}

/// Adds the hit of `face` at `point` to [`face_hits`]' `hits`, with the face's place in
/// `faces`, unless `face` has one within twice `tolerance` of it: two solved points of one
/// crossing are each within the tolerance of it.
fn add_hit(
    hits: &mut Vec<(Option<usize>, FaceHit)>,
    faces: &[FaceId],
    face: FaceId,
    point: Point3,
    distance: f64,
    tolerance: f64,
) {
    let again = hits
        .iter()
        .any(|(_, h)| h.face == face && h.point.distance(point) <= 2.0 * tolerance);
    if !again {
        hits.push((
            faces.iter().position(|&g| g == face),
            FaceHit {
                face,
                point,
                distance,
            },
        ));
    }
}

/// A curve/face solve's hits, none for its [`IntersectFailure::NoIntersection`].
fn solved(found: Result<CurveHits>) -> Result<Option<CurveHits>> {
    match found {
        Ok(found) => Ok(Some(found)),
        Err(e)
            if matches!(
                e.intersect_failure(),
                Some(IntersectFailure::NoIntersection)
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// A B-rep's queries over its cache: the identities and the `SM_OC_BREP` cache, which
/// upstream builds when a query first needs it and keeps on the B-rep
/// (`SmSAGObject::CacheMakeOrValidate`). Keep one per B-rep state and reuse it across
/// queries; edits to the B-rep invalidate it.
pub struct BrepQuery<'a> {
    identity: ProvenBrepIdentity<'a>,
    cache: OnceCell<BrepCache>,
    /// The face surface caches the classification solves made, kept for the next solve
    /// as upstream keeps them on the surfaces (`smsurf_GetSurfaceCache`).
    face_caches: ray::KeptFaceCaches,
    bbox: OnceCell<Extent3d>,
}

impl core::fmt::Debug for BrepQuery<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BrepQuery")
            .field("identity", &self.identity)
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

impl Default for BrepQuery<'static> {
    /// The queries of a B-rep with the default identities ([`BrepIdentity::default`]).
    fn default() -> Self {
        Self::new(BrepIdentity::default())
    }
}

impl BrepQuery<'static> {
    /// Queries of `b` at [`BrepId`] `0`, resolving records by `b`'s naming scheme and
    /// proven producer geometry-owner identities (`Brep::geometry_owner_keys`).
    #[must_use]
    pub fn of_brep(b: &Brep) -> Self {
        Self::of_brep_with_id(BrepId::from_index(0), b)
    }

    /// Queries of `b` at `brep_id`, resolving records by `b`'s naming scheme and proven
    /// producer geometry-owner identities (`Brep::geometry_owner_keys`).
    #[must_use]
    pub fn of_brep_with_id(brep_id: BrepId, b: &Brep) -> Self {
        Self::from_proven(BrepIdentity::of_brep_with_id(brep_id, b))
    }
}

impl<'a> BrepQuery<'a> {
    /// The queries of the B-rep with `identity`; its cache is built on first use.
    #[must_use]
    pub const fn new(identity: BrepIdentity<'a>) -> Self {
        Self::from_proven(ProvenBrepIdentity::plain(identity))
    }

    /// The queries of the B-rep with `identity` (proven overrides included).
    #[must_use]
    pub const fn from_proven(identity: ProvenBrepIdentity<'a>) -> Self {
        Self {
            identity,
            cache: OnceCell::new(),
            face_caches: ray::KeptFaceCaches::new(),
            bbox: OnceCell::new(),
        }
    }

    /// Queries of `b` with `identity` as the base naming, overriding edge/face keys where
    /// `b` records a proven producer geometry-owner key (`Brep::geometry_owner_keys`)
    /// ([`ProvenBrepIdentity::new`]).
    #[must_use]
    pub fn with_proven_owners(identity: BrepIdentity<'a>, b: &Brep) -> Self {
        Self::from_proven(ProvenBrepIdentity::new(identity, b))
    }

    /// The queries over a cache the caller already holds for the B-rep.
    #[must_use]
    pub fn with_cache(identity: BrepIdentity<'a>, cache: BrepCache) -> Self {
        Self {
            identity: ProvenBrepIdentity::plain(identity),
            cache: OnceCell::from(cache),
            face_caches: ray::KeptFaceCaches::new(),
            bbox: OnceCell::new(),
        }
    }

    /// The kept face caches of this query.
    #[must_use]
    pub(crate) const fn face_caches(&self) -> &ray::KeptFaceCaches {
        &self.face_caches
    }

    /// Cached 3D bounding box of `b` with default options.
    ///
    /// # Errors
    ///
    /// Those of [`calculate_bounding_box`].
    pub fn bounding_box(&self, k: &Kernel, b: &mut Brep) -> Result<Extent3d> {
        if let Some(&bbox) = self.bbox.get() {
            return Ok(bbox);
        }
        let bbox = calculate_bounding_box(k, b, BoundingBoxOptions::default())?;
        Ok(*self.bbox.get_or_init(|| bbox))
    }

    /// The base identities (without proven overrides).
    #[must_use]
    pub const fn identity(&self) -> BrepIdentity<'a> {
        self.identity.base()
    }

    /// The effective [`BrepIdentity`], including any proven geometry-owner overrides
    /// recorded on the B-rep when built with [`Self::of_brep`] or
    /// [`Self::with_proven_owners`] (the same as [`BrepIdentity::of_brep`]'s
    /// [`ProvenBrepIdentity::identity`]).
    #[must_use]
    pub fn effective_identity(&self) -> BrepIdentity<'_> {
        self.identity.identity()
    }

    /// The cache of `b`, built keyed by the identities when first asked for.
    ///
    /// # Errors
    ///
    /// Those of [`make_brep_cache`].
    pub fn cache(&self, k: &Kernel, b: &Brep) -> Result<&BrepCache> {
        if let Some(cache) = self.cache.get() {
            return Ok(cache);
        }
        let identity = self.effective_identity();
        let cache = make_brep_cache(k, CacheSource::Brep(b), identity.keys)?;
        Ok(self.cache.get_or_init(|| cache))
    }

    /// Mutable ray solve used by region nesting after stitching. Keeps lazy trim
    /// preparation in the original solve order.
    pub(crate) fn ray_fire_live(
        &self,
        k: &Kernel,
        b: &mut Brep,
        start: Point3,
        direction: Vec3,
        tolerance: f64,
    ) -> Result<crate::topology::brep::p2::RayFireResult> {
        let identity = self.effective_identity();
        let cache = self.cache(k, b)?;
        let bbox = self.bounding_box(k, b)?;
        crate::topology::brep::p2::ray_fire_with_bbox_and_solver(
            k,
            b,
            bbox,
            start,
            direction,
            tolerance,
            None,
            |b, point, options| {
                ray::solve_mutable_brep_point_kept(
                    k,
                    b,
                    &identity,
                    cache,
                    &self.face_caches,
                    point,
                    options,
                )
            },
        )
    }

    /// (Not upstream) The first face of `b` that `ray` meets, with the (u, v) of the face's
    /// surface there, the surface's point and the distance along the ray; `None` when the ray
    /// meets no face. The hits are the ray fire's solve ([`bp2::ray_fire`]'s: the B-rep point
    /// solve, [`SolverOperation::Rayfire`], every solution), each a face's surface met by the
    /// ray (`GlobalLineIntersect`) inside the face's trim, at the B-rep's zone tolerance
    /// ([`brep_zone_tol_3d`]) plus the face's own, and no farther behind the origin than the
    /// solve's zero. A solution on an edge or a vertex (a ray through a seam, an edge or a
    /// corner) is a hit of each face using it, with the (u, v) of that face's surface point
    /// nearest the ray's point there. The first is, of the hits within twice the zone
    /// tolerance of the least distance, the one on the first face in `b`'s face order
    /// (`GetFaces`), as [`face_hits`] orders the faces it meets at one point; for that face a
    /// solution on the face before one on its edge or vertex, then the nearer. Its distance
    /// is the solution's, clamped at 0 (a hit within the solve's zero behind the origin is at
    /// the origin), and its point is the surface's at its (u, v). A sheet is hit from either
    /// side. The solve may make missing UV trim curves on `b`, as a ray fire does.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a hit face without a surface; those of the cache, the
    /// solve, the faces of an edge or vertex hit, the surface's evaluation, and, for a hit on
    /// an edge or vertex, its closest point solve ([`surface_closest_point`]: its
    /// `NoClosestPoint` when the drop to the surface fails).
    pub fn first_face_hit(&self, k: &Kernel, b: &mut Brep, ray: Ray) -> Result<Option<FaceRayHit>> {
        let tolerance = brep_zone_tol_3d(k, b, SmallTopoType::NormalSize);
        let identity = self.effective_identity();
        let cache = self.cache(k, b)?;
        let vectors = [ray.direction];
        let solutions = ray::solve_mutable_brep_point_kept(
            k,
            b,
            &identity,
            cache,
            &self.face_caches,
            ray.origin,
            TopologySolveOptions {
                operation: SolverOperation::Rayfire,
                requested: SolutionRequested::All,
                tolerance_3d: tolerance,
                best_answer_so_far_sq: BIG_DOUBLE,
                opt_vectors: &vectors,
            },
        )?;
        // every face a solution is on: its own, or those using its edge or vertex
        let mut met: Vec<(f64, &Solution<CacheObject>, Vec<FaceId>)> = Vec::new();
        for solution in solutions.as_slice() {
            let using = match solution.objects.first().copied().flatten() {
                Some(CacheObject::Face(face)) => vec![face],
                Some(CacheObject::Edge(e)) => edge::faces(b, e, None)?,
                Some(CacheObject::Vertex(v)) => vertex::faces(b, v)?,
                _ => continue,
            };
            met.push((solution.start.solution_value, solution, using));
        }
        let Some(least) = met.iter().map(|m| m.0).reduce(f64::min) else {
            return Ok(None);
        };
        // of the faces met within twice the tolerance of the least distance, the first in
        // `GetFaces` order; for that face a face solution, then the nearest
        let order = bp1::faces(b, None)?;
        let rank = |f: FaceId| order.iter().position(|&g| g == f).unwrap_or(usize::MAX);
        let mut best: Option<(usize, bool, f64)> = None;
        let mut first: Option<(FaceId, &Solution<CacheObject>)> = None;
        for (distance, solution, using) in &met {
            if *distance > least + 2.0 * tolerance {
                continue;
            }
            let on_edge = !matches!(
                solution.objects.first().copied().flatten(),
                Some(CacheObject::Face(_))
            );
            for &face in using {
                let key = (rank(face), on_edge, *distance);
                if best.is_none_or(|b| key < b) {
                    best = Some(key);
                    first = Some((face, *solution));
                }
            }
        }
        let Some((face, solution)) = first else {
            return Ok(None);
        };
        let on_edge = !matches!(
            solution.objects.first().copied().flatten(),
            Some(CacheObject::Face(_))
        );
        let surface = face::p2::surface(b, face)?
            .ok_or_else(|| SolidError::invalid_input(OP_FIRST_HIT, "a face without a surface"))?;
        let uv = if on_edge {
            // the solution's parameters are the edge's or vertex's: the face's (u, v) is
            // that of its surface's point nearest the ray's point there
            let on_ray = ray.origin + ray.direction * solution.start.solution_value;
            let nearest = surface_closest_point(k, surface, on_ray)?;
            *nearest.uvs.first().ok_or_else(|| {
                SolidError::query(
                    OP_FIRST_HIT,
                    QueryFailure::Defect {
                        why:
                            "a closest point is found at a (u, v) of the surface, which it returns",
                    },
                )
            })?
        } else {
            Point2::new(solution.start.param(0)?, solution.start.param(1)?)
        };
        Ok(Some(FaceRayHit {
            face,
            uv,
            point: surface.evaluate_point(k, uv)?,
            distance: solution.start.solution_value.max(0.0),
        }))
    }

    /// (Not upstream) The lowest dimensional vertex, edge or face of `b` within `tolerance`
    /// of `point` (the boundary solve of [`Self::point_3d_classify`]); none when it is on no
    /// face.
    fn entity_at(
        &self,
        k: &Kernel,
        b: &mut Brep,
        point: Point3,
        tolerance: f64,
    ) -> Result<Option<EntityRef>> {
        let identity = self.effective_identity();
        let cache = self.cache(k, b)?;
        let solutions = ray::solve_mutable_brep_point_kept(
            k,
            b,
            &identity,
            cache,
            &self.face_caches,
            point,
            TopologySolveOptions {
                operation: SolverOperation::Intersect,
                requested: SolutionRequested::Single,
                tolerance_3d: tolerance,
                best_answer_so_far_sq: 0.0,
                opt_vectors: &[],
            },
        )?;
        let Some(object) = solutions
            .get(0)
            .and_then(|s| s.objects.first().copied().flatten())
        else {
            return Ok(None);
        };
        Ok(Some(bp2::cache_entity(object)?))
    }

    /// `SmBrep::Point3DClassify`: classifies `point` (zone tolerance
    /// `point_zone_tol_3d`) against `b`, the B-rep this query was built for. With
    /// `do_boundary_intersections` the lowest dimensional vertex, edge or face the point
    /// lies on (`BrepPointSolve`, `SM_SO_INTERSECT`, single solution) is the answer when
    /// there is one. Otherwise the region: the only one when there is one, else the
    /// region a ray fire ([`bp2::ray_fire`]) from the point finds along `+x` (then `-x`
    /// when that fails), then `+y`/`-y`, then `+z`/`-z` while none was found. The ray
    /// fires may make missing UV trim curves on `b` (upstream's `const` method makes them
    /// too); a caller holding the B-rep shared classifies on a copy.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when no ray finds a region; those of the boundary solve, the region list
    /// and the classification load; errors of the ray fires that have no upstream
    /// status ([`SolidError::Unported`], a port invariant, a stale id), which are not
    /// treated as a missed ray.
    pub fn point_3d_classify(
        &self,
        k: &Kernel,
        b: &mut Brep,
        point: Point3,
        point_zone_tol_3d: f64,
        do_boundary_intersections: bool,
    ) -> Result<PointClassification<EntityRef>> {
        self.classify(
            k,
            Target::Own(b),
            point,
            point_zone_tol_3d,
            do_boundary_intersections,
        )
    }

    /// [`BrepQuery::point_3d_classify`] on `target`.
    fn classify(
        &self,
        k: &Kernel,
        mut target: Target<'_>,
        point: Point3,
        point_zone_tol_3d: f64,
        do_boundary_intersections: bool,
    ) -> Result<PointClassification<EntityRef>> {
        let identity = self.effective_identity();
        let BrepIdentity {
            brep_id,
            keys,
            owner_of,
        } = identity;

        // init output
        let mut pc = PointClassification::new(point_zone_tol_3d);
        pc.set_class_object(PointClassType::Unknown, None);
        pc.set_src_zone_tol_3d(point_zone_tol_3d);

        // Intersect the point with the boundary elements of the Brep
        if do_boundary_intersections {
            let options = TopologySolveOptions {
                operation: SolverOperation::Intersect,
                requested: SolutionRequested::Single,
                tolerance_3d: point_zone_tol_3d,
                best_answer_so_far_sq: 0.0,
                opt_vectors: &[],
            };
            // XSects point with the Brep verts, edges, and faces returning lowest dim
            // topology intersection found.
            // A face-owned surface cache needs mutable access to its actual face.
            // A shared target without faces cannot reach that lookup and can use
            // the read-only solver without copying the B-rep.
            let solutions = if matches!(&target, Target::Shared { .. })
                && target.shared().faces().ids().next().is_none()
            {
                let b = target.shared();
                let objects = BrepObjects {
                    preparation: None,
                    brep: b,
                    brep_id,
                    cache: Some(self.cache(k, b)?),
                    keys,
                    owner_of,
                };
                TopologySolver::brep_point_solve(
                    k,
                    &objects,
                    Some(CacheObject::Brep),
                    point,
                    options,
                )?
            } else {
                let b = target.mutable()?;
                let cache = self.cache(k, b)?;
                ray::solve_mutable_brep_point_kept(
                    k,
                    b,
                    &identity,
                    cache,
                    &self.face_caches,
                    point,
                    options,
                )?
            };

            // when the point is on a Brep Face
            if let Some(sol) = solutions.get(0) {
                // Just grab the first one -- we asked for Single.
                let object = sol.objects[0].ok_or_else(|| {
                    crate::error::Failure::new(OP_CLASSIFY, QueryFailure::Defect { why: "the B-rep point solve fills the first object slot of each solution it returns" }).with_status(crate::error::SM_ERR_NULL_POINTER).into_error()
                })?;
                pc.load_from_intersection(
                    point_zone_tol_3d,
                    bp2::cache_entity(object)?,
                    &sol.start,
                    0,
                    BIG_DOUBLE,
                )?;
                return Ok(pc);
            }
        } // end bDoBoundaryIntersections check

        // get Brep->Regions
        let regions = bp1::regions(target.shared(), None)?;

        // Check for special case where only one region exists then we
        // do not have to do any ray firing.
        if let [region] = regions.as_slice() {
            pc.set_class_object(PointClassType::Region, Some(EntityRef::Region(*region)));
            pc.set_gap_3d(0.0);
            pc.set_pre_snap_param(BIG_DOUBLE);
            return Ok(pc);
        }

        // Now do a ray fire to determine which region this lies in. Fire extra rays
        // only if the first rays fail: `Some(region)` on success (the region may be
        // none), `None` on failure.
        // (the solve of each ray fire builds the cache when needed; the face caches its
        // solves make are kept for the next, `bp2::ray_fire` on them)
        let mut fire = |direction: Vec3| -> Result<Option<Option<RegionId>>> {
            let b = target.mutable()?;
            let fired = self.cache(k, b).and_then(|cache| {
                let bbox = self.bounding_box(k, b)?;
                bp2::ray_fire_with_bbox_and_solver(
                    k,
                    b,
                    bbox,
                    point,
                    direction,
                    point_zone_tol_3d,
                    None,
                    |b, point, options| {
                        ray::solve_mutable_brep_point_kept(
                            k,
                            b,
                            &identity,
                            cache,
                            &self.face_caches,
                            point,
                            options,
                        )
                    },
                )
            });
            Ok(native_failure(fired)?.map(|r| r.region))
        };
        let mut region = None;
        for (forward, backward) in CLASSIFY_RAYS {
            region = match fire(forward)? {
                Some(r) => r,
                None => fire(backward)?.flatten(),
            };
            if region.is_some() {
                break;
            }
        }

        let region =
            region.ok_or_else(|| SolidError::query(OP_CLASSIFY, QueryFailure::RegionNotFound))?;

        // load output
        pc.set_class_object(PointClassType::Region, Some(EntityRef::Region(region)));
        pc.set_deviation(0.0); // SM_USE_OLDTOL
        pc.set_pre_snap_param(BIG_DOUBLE);
        Ok(pc)
    }
}

/// The B-rep's ray queries at a shell (`SmShell::RayFire`): the shape solve reads the
/// B-rep's geometry under these identities, without the B-rep's cache (a shape has
/// its own trees), and the hits are its vertices, edges and faces.
impl crate::topology::shell::ShellRayWorld<CacheObject> for BrepQuery<'_> {
    type Objects<'a>
        = BrepObjects<'a>
    where
        Self: 'a;

    fn objects<'a>(&'a self, brep: (BrepId, &'a Brep)) -> Result<BrepObjects<'a>> {
        let (brep_id, brep) = brep;
        let identity = self.effective_identity();
        if brep_id != identity.brep_id {
            return Err(SolidError::invalid_input(
                "query::brep::BrepQuery::objects",
                "ray fire on another B-rep than the query's",
            ));
        }
        Ok(BrepObjects {
            preparation: None,
            brep,
            brep_id,
            cache: None,
            keys: identity.keys,
            owner_of: identity.owner_of,
        })
    }

    fn objects_for_preparation<'a>(
        &'a self,
        brep: (BrepId, &'a Brep),
        preparation: &'a RayPreparation,
    ) -> Result<BrepObjects<'a>> {
        let mut objects = self.objects(brep)?;
        objects.preparation = Some(preparation);
        Ok(objects)
    }

    fn solve_shape_ray(
        &self,
        k: &Kernel,
        brep: &mut Brep,
        brep_id: BrepId,
        shape: &crate::geom::object::Shape,
        point: Point3,
        options: TopologySolveOptions<'_>,
    ) -> Result<crate::geom::solution_array::SolutionArray<CacheObject>> {
        let identity = self.effective_identity();
        if brep_id != identity.brep_id {
            return Err(SolidError::invalid_input(
                "query::brep::BrepQuery::solve_shape_ray",
                "ray fire on another B-rep",
            ));
        }
        ray::solve_mutable_ray(k, brep, &identity, shape, point, options)
    }

    fn class_entity(&self, object: CacheObject) -> Option<EntityRef> {
        bp2::cache_entity(object).ok()
    }
}

/// A B-rep an upstream `const` query may still edit (`PointClassify` attaches missing
/// UV trim curves, and `SetUVTrimCurve` raises the edge tolerances to fit them): the
/// caller's, lent mutably, where the edits persist as upstream; or one the caller holds
/// shared, edited on a private copy made at the first edit, whose edits are lost when
/// the query returns. Built with `From` from `&mut Brep` or `&Brep`.
#[derive(Debug)]
pub enum LazyBrep<'a> {
    /// The caller's B-rep, edited in place.
    Own(&'a mut Brep),
    /// A B-rep the caller holds shared.
    Shared {
        /// The caller's B-rep.
        source: &'a Brep,
        /// Its private copy, made at the first edit.
        copy: Option<Box<Brep>>,
    },
    /// A B-rep read only, with its UV trim curves made: an edit is not made, on it or on
    /// a copy; the query that would make one returns
    /// [`SolidError::PreparationRequired`] instead.
    Prepared(&'a Brep),
}

impl<'a> From<&'a mut Brep> for LazyBrep<'a> {
    fn from(b: &'a mut Brep) -> Self {
        Self::Own(b)
    }
}

impl<'a> From<&'a Brep> for LazyBrep<'a> {
    fn from(source: &'a Brep) -> Self {
        Self::Shared { source, copy: None }
    }
}

impl LazyBrep<'_> {
    /// The B-rep as it stands (the copy once made).
    #[must_use]
    pub fn get(&self) -> &Brep {
        match self {
            Self::Own(b) => b,
            Self::Shared { source, copy } => copy.as_deref().unwrap_or(source),
            Self::Prepared(b) => b,
        }
    }

    /// The B-rep to edit, copying a shared one first: a copy that stands for it
    /// ([`Brep::clone_standing_for_self`]: the same ids, its geometry's owners kept).
    ///
    /// # Errors
    ///
    /// Those of [`Brep::clone_standing_for_self`], at the first edit of a shared B-rep;
    /// [`SolidError::PreparationRequired`] for a [`LazyBrep::Prepared`] one.
    pub fn get_mut(&mut self) -> Result<&mut Brep> {
        match self {
            Self::Own(b) => Ok(b),
            Self::Shared { source, copy } => match copy {
                Some(c) => Ok(c),
                slot @ None => Ok(slot.insert(Box::new(source.clone_standing_for_self()?))),
            },
            Self::Prepared(_) => Err(SolidError::PreparationRequired {
                operation: OP_TIGHT_BOX,
            }),
        }
    }
}

/// The B-rep a classification runs on: the caller's, lent mutably, or one it holds
/// shared, copied when a face boundary solve or ray fire needs it mutably.
enum Target<'t> {
    Own(&'t mut Brep),
    Shared {
        source: &'t Brep,
        copy: &'t mut Option<Brep>,
    },
}

impl Target<'_> {
    /// The B-rep as it stands (the copy once made: it may hold new UV trim curves).
    fn shared(&self) -> &Brep {
        match self {
            Self::Own(b) => b,
            Self::Shared { source, copy } => copy.as_ref().unwrap_or(source),
        }
    }

    /// The B-rep to edit, copying a shared one first: the copy stands for the B-rep
    /// ([`Brep::clone_standing_for_self`]), so its geometry keeps naming its edges and
    /// faces and the face boundary solves' and ray fires' caches hold them.
    fn mutable(&mut self) -> Result<&mut Brep> {
        match self {
            Self::Own(b) => Ok(b),
            Self::Shared { source, copy } => match copy {
                Some(b) => Ok(b),
                None => Ok(copy.insert(source.clone_standing_for_self()?)),
            },
        }
    }
}

/// Point classification against a B-rep the caller holds shared: its [`BrepQuery`], and
/// a private copy for face boundary solves and ray fires, which may add missing UV trim
/// curves (upstream's `const` `Point3DClassify` adds them to the B-rep itself). The
/// copy is made when either solve first needs it. Keep one per B-rep state and classify
/// every point through it: the copy and the cache are made once.
#[derive(Debug)]
pub struct BrepClassifier<'a, 'b> {
    query: BrepQuery<'a>,
    source: &'b Brep,
    copy: Option<Brep>,
}

impl<'b> BrepClassifier<'static, 'b> {
    /// The classifier of `brep` at [`BrepId`] `0`, resolving records by `brep`'s naming
    /// scheme and proven producer geometry-owner identities ([`BrepQuery::of_brep`]).
    #[must_use]
    pub fn of_brep(brep: &'b Brep) -> Self {
        Self::of_brep_with_id(BrepId::from_index(0), brep)
    }

    /// The classifier of `brep` at `brep_id`, resolving records by `brep`'s naming
    /// scheme and proven producer geometry-owner identities
    /// ([`BrepQuery::of_brep_with_id`]).
    #[must_use]
    pub fn of_brep_with_id(brep_id: BrepId, brep: &'b Brep) -> Self {
        Self {
            query: BrepQuery::of_brep_with_id(brep_id, brep),
            source: brep,
            copy: None,
        }
    }
}

impl<'a, 'b> BrepClassifier<'a, 'b> {
    /// The classifier of `brep` with `identity` (nothing is copied or built yet).
    #[must_use]
    pub const fn new(brep: &'b Brep, identity: BrepIdentity<'a>) -> Self {
        Self {
            query: BrepQuery::new(identity),
            source: brep,
            copy: None,
        }
    }

    /// The B-rep the classifications run on (the private copy once made; its regions
    /// are the caller's).
    #[must_use]
    pub fn brep(&self) -> &Brep {
        self.copy.as_ref().unwrap_or(self.source)
    }

    /// Whether a classification has copied the B-rep.
    #[must_use]
    pub const fn has_copied(&self) -> bool {
        self.copy.is_some()
    }

    /// [`BrepQuery::point_3d_classify`] on the B-rep, copied when a face boundary solve
    /// or ray fire first needs it.
    ///
    /// # Errors
    ///
    /// Those of [`BrepQuery::point_3d_classify`].
    pub fn point_3d_classify(
        &mut self,
        k: &Kernel,
        point: Point3,
        point_zone_tol_3d: f64,
        do_boundary_intersections: bool,
    ) -> Result<PointClassification<EntityRef>> {
        let target = Target::Shared {
            source: self.source,
            copy: &mut self.copy,
        };
        self.query.classify(
            k,
            target,
            point,
            point_zone_tol_3d,
            do_boundary_intersections,
        )
    }
}

// ---- SM_API surface evaluation ------------------------------------------------------

/// The input checks the three evaluation entry points share: a surface, and a
/// parameter inside its natural domain (bounds included; `GetNaturalUVDomain`, which
/// makes a lazy analytic's missing NURB).
fn checked_surface<'s>(
    k: &Kernel,
    surface: Option<&'s Surface>,
    uv: Point2,
    op: &'static str,
) -> Result<&'s Surface> {
    let Some(surface) = surface else {
        return Err(SolidError::invalid_input(op, "no surface"));
    };

    // Negated in-range test so a NaN parameter is rejected too.
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let (lo, hi) = (domain.min(), domain.max());
    if !(uv.x >= lo.x && uv.x <= hi.x && uv.y >= lo.y && uv.y <= hi.y) {
        return Err(SolidError::invalid_input(
            op,
            "parameter outside the natural domain",
        ));
    }
    Ok(surface)
}

/// `SmApiEvaluateSurfacePoint`: the point of `surface` at `uv`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] without a surface or for a parameter outside (or NaN
/// against) the surface's natural domain; those of [`Surface::evaluate_point`].
pub fn evaluate_surface_point(k: &Kernel, surface: Option<&Surface>, uv: Point2) -> Result<Point3> {
    checked_surface(k, surface, uv, OP_EVAL_POINT)?.evaluate_point(k, uv)
}

/// `SmApiEvaluateSurfaceNormal`: the unit normal of `surface` at `uv`, from the upper
/// interval in both directions on a knot (`EvaluateNormal(uv, TRUE, TRUE)`).
///
/// # Errors
///
/// Those of [`evaluate_surface_point`]'s input checks; those of
/// [`Surface::evaluate_normal`].
pub fn evaluate_surface_normal(k: &Kernel, surface: Option<&Surface>, uv: Point2) -> Result<Vec3> {
    checked_surface(k, surface, uv, OP_EVAL_NORMAL)?.evaluate_normal(k, uv, Side::Left, Side::Left)
}

/// `SmApiEvaluateSurfaceDerivatives`: the point and first derivatives `(P, Du, Dv)` of
/// `surface` at `uv`, from the upper interval in both directions on a knot
/// (`Evaluate1stDerivatives(uv, TRUE, TRUE)`).
///
/// # Errors
///
/// Those of [`evaluate_surface_point`]'s input checks; those of
/// [`Surface::evaluate_1st_derivatives`].
pub fn evaluate_surface_derivatives(
    k: &Kernel,
    surface: Option<&Surface>,
    uv: Point2,
) -> Result<(Point3, Vec3, Vec3)> {
    checked_surface(k, surface, uv, OP_EVAL_DERIVS)?.evaluate_1st_derivatives(k, uv, true, true)
}

// ---- polygon decimation attributes ---------------------------------------------------

/// `SmPolyVertexClass`: the class of a polygon vertex during decimation; the
/// discriminant is the upstream value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolyVertexClass {
    /// `SM_PVC_NOT_COMPUTED`: not yet computed.
    NotComputed = 0,
    /// `SM_PVC_UNKNOWN`: connected to a spine edge, or non-manifold.
    Unknown = 1,
    /// `SM_PVC_SIMPLE`: an interior vertex on no lamina, feature or silhouette edge.
    Simple = 2,
    /// `SM_PVC_BOUNDARY`: on the boundary of the mesh (has lamina edges).
    Boundary = 3,
    /// `SM_PVC_INTERIOR`: an interior point between two feature edges.
    Interior = 4,
    /// `SM_PVC_CORNER`: an interior point between three or more feature edges.
    Corner = 5,
}

impl PolyVertexClass {
    /// Every class, in discriminant order.
    pub const ALL: [Self; 6] = [
        Self::NotComputed,
        Self::Unknown,
        Self::Simple,
        Self::Boundary,
        Self::Interior,
        Self::Corner,
    ];
}

/// The data of an `SmDecimateVertexAttr` (attribute id `SM_AI_VERTEX_DECIMATION_VALUES`).
#[derive(Debug, Clone, PartialEq)]
pub struct DecimateVertexData {
    /// `m_lSortIndex`.
    pub sort_index: u64,
    /// `m_eVertClass`.
    pub vertex_class: PolyVertexClass,
    /// `m_dVertexDistance`: the weighted chord height from the vertex to the baseline
    /// between its feature or boundary edges.
    pub vertex_distance: f64,
    /// `m_dMaxFaceError`: the largest error of the faces around the vertex.
    pub max_face_error: f64,
    /// `m_vAveragePlaneNormal`: the area-weighted normal of the faces around the vertex.
    pub average_plane_normal: Vec3,
}

impl DecimateVertexData {
    /// `SmDecimateVertexAttr::MakeCopy`'s data: everything but the sort index, which
    /// keeps the constructor's `0`.
    #[must_use]
    pub fn make_copy(&self) -> Self {
        Self {
            sort_index: 0,
            ..self.clone()
        }
    }
}

/// The data of an `SmQuadricVertexAttr` (attribute id `SM_AI_QUADRIC_VERTEX_VALUES`): the
/// quadric error form `NewVert . Q . NewVert + 2 b . NewVert + c` of the planes of the
/// original polygons around a vertex.
#[derive(Debug, Clone, PartialEq)]
pub struct QuadricVertexData {
    /// `m_Q`: the rows of the quadratic error matrix.
    pub q: [Vec3; 3],
    /// `m_b`.
    pub b: Vec3,
    /// `m_c`.
    pub c: f64,
}

/// The data of an `SmQuadricEdgeAttr` (attribute id `SM_AI_QUADRIC_EDGE_VALUES`): the
/// vertex an edge contracts to and its error.
#[derive(Debug, Clone, PartialEq)]
pub struct QuadricEdgeData {
    /// `m_vNewPoint`.
    pub new_point: Point3,
    /// `m_dError`.
    pub error: f64,
    /// `m_lSortIndex`; `None` while uninitialised (as the constructor leaves it).
    pub sort_index: Option<u64>,
}

impl QuadricEdgeData {
    /// `SmQuadricEdgeAttr::MakeCopy`'s data: the point and error, sort index `0`.
    #[must_use]
    pub fn make_copy(&self) -> Self {
        Self {
            new_point: self.new_point,
            error: self.error,
            sort_index: Some(0),
        }
    }
}

/// `SmDecimateVertexAttr(lAttributeID, eBehavior)` (upstream default
/// [`Behavior::Copy`]): sort index `0`, class not computed, vertex distance
/// `-SM_BIG_DOUBLE`, no face error, zero average normal.
#[must_use]
pub fn decimate_vertex_attribute(attribute_id: u64, behavior: Behavior) -> Attribute {
    Attribute::with_value(
        attribute_id,
        behavior,
        AttributeValue::DecimateVertex(DecimateVertexData {
            sort_index: 0,
            vertex_class: PolyVertexClass::NotComputed,
            vertex_distance: -BIG_DOUBLE,
            max_face_error: 0.0,
            average_plane_normal: Vec3::new(0.0, 0.0, 0.0),
        }),
    )
}

/// `SmQuadricVertexAttr(lAttributeID, eBehavior)` (upstream default
/// [`Behavior::Copy`]): the zero error form.
#[must_use]
pub fn quadric_vertex_attribute(attribute_id: u64, behavior: Behavior) -> Attribute {
    let zero = Vec3::new(0.0, 0.0, 0.0);
    Attribute::with_value(
        attribute_id,
        behavior,
        AttributeValue::QuadricVertex(QuadricVertexData {
            q: [zero, zero, zero],
            b: zero,
            c: 0.0,
        }),
    )
}

/// `SmQuadricEdgeAttr(lAttributeID, eBehavior)` (upstream default [`Behavior::Copy`]):
/// the origin as new point, no error, no sort index.
#[must_use]
pub fn quadric_edge_attribute(attribute_id: u64, behavior: Behavior) -> Attribute {
    Attribute::with_value(
        attribute_id,
        behavior,
        AttributeValue::QuadricEdge(QuadricEdgeData {
            new_point: Vec3::new(0.0, 0.0, 0.0),
            error: 0.0,
            sort_index: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::geom::vector2d::Vec2;
    use crate::surface::plane::p1::Plane;

    fn unit_plane() -> Surface {
        Surface::from(Plane::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        ))
    }

    #[test]
    fn rejects_missing_surface_and_out_of_domain_parameters() {
        let k = Kernel::default();
        let s = unit_plane();
        let bad = |r: Result<Point3>| matches!(r, Err(SolidError::InvalidInput { .. }));
        assert!(bad(evaluate_surface_point(&k, None, Point2::new(0.5, 0.5))));
        assert!(bad(evaluate_surface_point(
            &k,
            Some(&s),
            Point2::new(1.5, 0.5)
        )));
        assert!(bad(evaluate_surface_point(
            &k,
            Some(&s),
            Point2::new(f64::NAN, 0.5)
        )));
        // the domain bounds are inside
        assert!(evaluate_surface_point(&k, Some(&s), Point2::new(1.0, 0.0)).is_ok());
    }

    #[test]
    fn bounding_box_lock_and_vertex_only_paths() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let v0 = b.add_vertex(Point3::new(0.0, 0.0, 0.0)).unwrap();
        b.add_vertex(Point3::new(1.0, 2.0, 3.0)).unwrap();
        b.vertex_mut(v0).unwrap().tolerance = 0.5;

        let expanded = calculate_bounding_box(&k, &b, BoundingBoxOptions::default()).unwrap();
        assert_eq!(expanded.min().x, -0.5);
        let plain = BoundingBoxOptions {
            expand_by_zone_tol_3d: false,
            ..BoundingBoxOptions::default()
        };
        let bare = calculate_bounding_box(&k, &b, plain).unwrap();
        assert_eq!((bare.min().x, bare.max().z), (0.0, 3.0));
        assert_eq!(calculate_tight_bounding_box(&k, &b).unwrap(), bare);

        lock_bounding_box(&k, &mut b, false).unwrap();
        b.vertex_mut(v0).unwrap().point = Point3::new(-9.0, 0.0, 0.0);
        // the locked box is returned whatever the arguments; the tight box ignores it
        assert_eq!(calculate_bounding_box(&k, &b, plain).unwrap(), expanded);
        assert_eq!(calculate_tight_bounding_box(&k, &b).unwrap().min().x, -9.0);
        // relocking recomputes
        lock_bounding_box(&k, &mut b, true).unwrap();
        assert_eq!(calculate_bounding_box(&k, &b, plain).unwrap().min().x, -9.0);
        unlock_bounding_box(&mut b);
        assert_eq!(b.locked_bounding_box(), None);
        assert_eq!(calculate_bounding_box(&k, &b, plain).unwrap().min().x, -9.0);
    }

    #[test]
    fn classification_builds_the_cache_only_when_a_solve_needs_it() {
        let k = Kernel::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        let query = BrepQuery::new(BrepIdentity::arena(BrepId::from_index(0)));
        let p = Point3::new(1.0, 2.0, 3.0);
        // one region: no ray fire, no cache (as upstream)
        let pc = query.point_3d_classify(&k, &mut b, p, 1e-6, false).unwrap();
        assert_eq!(pc.point_class(), PointClassType::Region);
        assert!(query.cache.get().is_none());
        // the boundary solve builds it; an empty B-rep has no boundary to hit
        let pc = query.point_3d_classify(&k, &mut b, p, 1e-6, true).unwrap();
        assert_eq!(pc.point_class(), PointClassType::Region);
        assert!(query.cache.get().is_some());

        let mut classifier = BrepClassifier::new(&b, BrepIdentity::arena(BrepId::from_index(0)));
        let pc = classifier.point_3d_classify(&k, p, 1e-6, true).unwrap();
        assert_eq!(pc.object(), b.infinite_region().map(EntityRef::Region));
    }

    #[test]
    fn classifier_fires_rays_on_a_copy_and_leaves_the_callers_brep_alone() {
        let k = Kernel::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        b.add_region_record(crate::topology::Region {
            in_brep: true,
            ..Default::default()
        })
        .unwrap();
        let before = format!("{b:?}");

        let mut classifier = BrepClassifier::new(&b, BrepIdentity::default());
        assert!(!classifier.has_copied());
        // two regions: the ray fire needs the B-rep mutable, so it works on a copy
        let _ = classifier.point_3d_classify(&k, Point3::new(1.0, 2.0, 3.0), 1e-6, false);
        assert!(classifier.has_copied());
        let copied: *const Brep = classifier.brep();
        let _ = classifier.point_3d_classify(&k, Point3::new(3.0, 2.0, 1.0), 1e-6, false);
        assert!(core::ptr::eq(classifier.brep(), copied), "copied once");
        assert_eq!(format!("{b:?}"), before);
    }

    /// A unit box from `create_box` without its UV trims (upstream's classification
    /// makes the ones its loop walks reach), and a query under the box's own keys
    /// (`notify_ref`, as `create_box` owns its geometry).
    fn box_without_trims(k: &mut Kernel) -> (Brep, BrepQuery<'static>) {
        fn keys(o: CacheObject) -> ObjectRef {
            use crate::topology::loopuse::notify_ref;
            match o {
                CacheObject::Edge(e) | CacheObject::Curve(e) => notify_ref(EntityRef::Edge(e)),
                CacheObject::Face(f) | CacheObject::Surface(f) => notify_ref(EntityRef::Face(f)),
                CacheObject::Vertex(v) => notify_ref(EntityRef::Vertex(v)),
                _ => notify_ref(EntityRef::Brep(BrepId::from_index(0))),
            }
        }
        fn no_owner(_: ObjectRef) -> Option<ObjectRef> {
            None
        }
        let mut b =
            crate::primitive::api_primitives::create_box(k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let uses: Vec<_> = b.edgeuses().ids().collect();
        for u in uses {
            b.edgeuse_mut(u).unwrap().uv_trim_curve = None;
        }
        let query = BrepQuery::new(BrepIdentity {
            brep_id: BrepId::from_index(0),
            keys: &keys,
            owner_of: &no_owner,
        });
        (b, query)
    }

    fn trim_count(b: &Brep) -> usize {
        b.edgeuses()
            .iter()
            .filter(|(_, u)| u.uv_trim_curve.is_some())
            .count()
    }

    #[test]
    fn ray_fires_make_missing_uv_trims_on_the_callers_brep() {
        let mut k = Kernel::default();
        let (mut b, query) = box_without_trims(&mut k);
        let solid = b
            .regions()
            .ids()
            .find(|&r| Some(r) != b.infinite_region())
            .map(EntityRef::Region);
        let inside = query
            .point_3d_classify(&k, &mut b, Point3::new(0.5, 0.5, 0.5), 1e-6, false)
            .unwrap();
        assert_eq!(inside.object(), solid);
        assert!(trim_count(&b) > 0, "the trims are made on the box itself");
    }

    /// A ray that enters a 100 mm cube 0.001 mm from a vertical edge and leaves 0.001 mm from
    /// the same edge: the entry point is not on the edge (it is 0.001 mm off it, far outside
    /// the edge's tolerance), so the hit is a hit on the face it enters, and the ray start is
    /// outside the cube. (The solve reports the nearest hit only here, so the hit is not moved
    /// onto the edge whatever tolerance the move is held to; that is covered where a ray meets
    /// two faces on an edge, `a_long_thin_elliptic_prism_faces_out_and_cuts_a_block`.)
    #[test]
    fn a_ray_entering_near_an_edge_hits_the_face() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 100.0, 100.0, 100.0)
                .unwrap();
        let query = BrepQuery::of_brep(&b);
        let outside = b.infinite_region();
        let start = Point3::new(-30.0, 30.001, 50.0);
        let direction = Vec3::new(1.0, -1.0, 0.0).unitize().unwrap();
        let hit = query
            .ray_fire_live(&k, &mut b, start, direction, 1e-6)
            .unwrap();
        assert_eq!(hit.region, outside, "the ray starts outside the cube");
        assert_eq!(
            hit.intersection.point_class(),
            PointClassType::Face,
            "{:?}",
            hit.intersection
        );
    }

    /// A ray along +x at y = 3, z = 10 crosses the side of the radius-5 cylinder about z at
    /// x = -4 and x = 4 (a 3-4-5 triangle): from x = -20 the first face hit is the side at
    /// (-4, 3, 10), 16 along the ray, its surface at the hit's (u, v) on the ray (within the
    /// B-rep's zone tolerance) and at that point;
    /// from the axis, the far side at 4; from x = 20 facing away, nothing. What is no ray is
    /// refused before any solve.
    #[test]
    fn the_first_face_a_ray_meets_is_the_nearer_of_its_hits() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let query = BrepQuery::of_brep(&b);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let ray = |o: Point3| Ray::new(o, x).unwrap();
        let near = query
            .first_face_hit(&k, &mut b, ray(Point3::new(-20.0, 3.0, 10.0)))
            .unwrap()
            .unwrap();
        assert!(
            near.point.distance(Point3::new(-4.0, 3.0, 10.0)) < 1e-9,
            "{near:?}"
        );
        assert!((near.distance - 16.0).abs() < 1e-9, "{near:?}");
        let surface = face::p2::surface(&b, near.face).unwrap().unwrap();
        let on = surface.evaluate_point(&k, near.uv).unwrap();
        let along = on - Point3::new(-20.0, 3.0, 10.0);
        let off_line = (along - x * along.dot(x)).length();
        let zone = brep_zone_tol_3d(&k, &b, SmallTopoType::NormalSize);
        assert!(off_line <= zone, "{on:?} is {off_line} off the ray");
        assert!(on.distance(Point3::new(-4.0, 3.0, 10.0)) < 1e-9, "{on:?}");
        let far = query
            .first_face_hit(&k, &mut b, ray(Point3::new(0.0, 3.0, 10.0)))
            .unwrap()
            .unwrap();
        assert!(
            far.point.distance(Point3::new(4.0, 3.0, 10.0)) < 1e-9,
            "{far:?}"
        );
        assert_eq!(far.face, near.face);
        assert_eq!(
            query
                .first_face_hit(&k, &mut b, ray(Point3::new(20.0, 3.0, 10.0)))
                .unwrap(),
            None
        );
        let refused = |r: Result<Ray>| matches!(r, Err(SolidError::InvalidInput { .. }));
        assert!(refused(Ray::new(Point3::ZERO, Vec3::ZERO)));
        assert!(refused(Ray::new(Point3::new(f64::NAN, 0.0, 0.0), x)));
        assert!(refused(Ray::new(
            Point3::ZERO,
            Vec3::new(0.0, f64::INFINITY, 0.0)
        )));
    }

    /// Every face a ray meets, nearest first: along +x at y = 3, z = 10 from x = -20 the
    /// radius-5 cylinder about z gives its side twice, at x = -4 (16 along) and x = 4 (24
    /// along), the first where `first_face_hit` lands; from the axis only x = 4; facing away,
    /// none. Up from (0.6, 0.8, -5) through the 10 x 10 x 10 box at (-5, -5, 0) and a sphere
    /// of radius 2 at (0, 0, 20), one B-rep of two lumps: the box's bottom (5) and top (15),
    /// then the sphere, 1 off its axis, at 25 -/+ sqrt(3).
    #[test]
    fn every_face_a_ray_meets_comes_nearest_first() {
        let mut k = Kernel::default();
        let mut b =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let x = Vec3::new(1.0, 0.0, 0.0);
        let along = |k: &Kernel, b: &mut Brep, o: Point3, d: Vec3| {
            face_hits(k, b, Ray::new(o, d).unwrap())
                .unwrap()
                .iter()
                .map(|h| (h.distance, h.point, h.face))
                .collect::<Vec<_>>()
        };
        let hits = along(&k, &mut b, Point3::new(-20.0, 3.0, 10.0), x);
        assert_eq!(hits.len(), 2, "{hits:?}");
        for (hit, (at, distance)) in hits.iter().zip([(-4.0, 16.0), (4.0, 24.0)]) {
            assert!((hit.0 - distance).abs() < 1e-9, "{hits:?}");
            assert!(
                hit.1.distance(Point3::new(at, 3.0, 10.0)) < 1e-9,
                "{hits:?}"
            );
        }
        let first = BrepQuery::of_brep(&b)
            .first_face_hit(
                &k,
                &mut b,
                Ray::new(Point3::new(-20.0, 3.0, 10.0), x).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(first.face, hits[0].2);
        assert!(first.point.distance(hits[0].1) < 1e-9, "{first:?} {hits:?}");
        let hits = along(&k, &mut b, Point3::new(0.0, 3.0, 10.0), x);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!((hits[0].0 - 4.0).abs() < 1e-9, "{hits:?}");
        assert!(along(&k, &mut b, Point3::new(20.0, 3.0, 10.0), x).is_empty());
        let block = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(-5.0, -5.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let ball = crate::primitive::api_primitives::create_sphere(
            &mut k,
            Point3::new(0.0, 0.0, 20.0),
            2.0,
        )
        .unwrap();
        let mut two = crate::boolean::api_brep::boolean_union(&mut k, block, ball).unwrap();
        // 1 off the sphere's axis (0.6, 0.8), clear of its poles: sqrt(3) each side of z = 20
        let hits = along(
            &k,
            &mut two,
            Point3::new(0.6, 0.8, -5.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        assert_eq!(hits.len(), 4, "{hits:?}");
        let r = 3.0_f64.sqrt();
        for (hit, want) in hits.iter().zip([5.0, 15.0, 25.0 - r, 25.0 + r]) {
            assert!((hit.0 - want).abs() < 1e-9, "{hits:?}");
        }
    }

    /// The rays of the #21325 review. Along +x at y = 0, z = 10 the radius-5 cylinder about z
    /// is crossed at x = -5 and on its seam at x = 5: two hits, at 15 and 25, and the same
    /// two from x = 20 the other way. Up the axis of the radius-2 sphere at (0, 0, 20) from
    /// the origin: its poles once each, at 18 and 22; along +x at z = 20, (2, 0, 20) on its
    /// seam once. Past the side y = 5 of the cube [-5, 5]^3 by 5e-4, outside the zone
    /// tolerance, nothing, as for `first_face_hit`. From (1, 1, 1) through the corner
    /// (5, 5, 5) the three faces there at that point and one distance, 4 sqrt 3, in the faces'
    /// order; from (1, 1, 0) through the edge x = y = 5 its two faces at (5, 5, 0), 4 sqrt 2.
    #[test]
    fn a_ray_through_a_seam_pole_edge_or_corner_hits_each_face_there_once() {
        let mut k = Kernel::default();
        let along = |k: &Kernel, b: &mut Brep, o: [f64; 3], d: [f64; 3]| {
            let ray = Ray::new(Point3::new(o[0], o[1], o[2]), Vec3::new(d[0], d[1], d[2]));
            face_hits(k, b, ray.unwrap()).unwrap()
        };
        let at = |hits: &[FaceHit], want: &[(f64, [f64; 3])]| {
            assert_eq!(hits.len(), want.len(), "{hits:?}");
            for (hit, (distance, [x, y, z])) in hits.iter().zip(want) {
                assert!((hit.distance - distance).abs() < 1e-9, "{hits:?}");
                assert!(
                    hit.point.distance(Point3::new(*x, *y, *z)) < 1e-9,
                    "{hits:?}"
                );
            }
        };
        let mut tube =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let hits = along(&k, &mut tube, [-20.0, 0.0, 10.0], [1.0, 0.0, 0.0]);
        at(
            &hits,
            &[(15.0, [-5.0, 0.0, 10.0]), (25.0, [5.0, 0.0, 10.0])],
        );
        let hits = along(&k, &mut tube, [20.0, 0.0, 10.0], [-1.0, 0.0, 0.0]);
        at(
            &hits,
            &[(15.0, [5.0, 0.0, 10.0]), (25.0, [-5.0, 0.0, 10.0])],
        );

        let mut ball = crate::primitive::api_primitives::create_sphere(
            &mut k,
            Point3::new(0.0, 0.0, 20.0),
            2.0,
        )
        .unwrap();
        let hits = along(&k, &mut ball, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        at(&hits, &[(18.0, [0.0, 0.0, 18.0]), (22.0, [0.0, 0.0, 22.0])]);
        let hits = along(&k, &mut ball, [-20.0, 0.0, 20.0], [1.0, 0.0, 0.0]);
        at(
            &hits,
            &[(18.0, [-2.0, 0.0, 20.0]), (22.0, [2.0, 0.0, 20.0])],
        );

        let mut cube = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(-5.0, -5.0, -5.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let past = [-20.0, 5.0005, 0.0];
        assert!(along(&k, &mut cube, past, [1.0, 0.0, 0.0]).is_empty());
        let first = |b: &mut Brep, o: [f64; 3], d: [f64; 3]| {
            let ray = Ray::new(Point3::new(o[0], o[1], o[2]), Vec3::new(d[0], d[1], d[2]));
            BrepQuery::of_brep(b)
                .first_face_hit(&k, b, ray.unwrap())
                .unwrap()
        };
        assert_eq!(first(&mut cube, past, [1.0, 0.0, 0.0]), None);
        let order = bp1::faces(&cube, None).unwrap();
        let rank = |f: FaceId| order.iter().position(|&g| g == f).unwrap();
        for (o, d, faces, distance, [x, y, z]) in [
            (
                [1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                3,
                4.0 * 3.0_f64.sqrt(),
                [5.0, 5.0, 5.0],
            ),
            (
                [1.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
                2,
                4.0 * 2.0_f64.sqrt(),
                [5.0, 5.0, 0.0],
            ),
        ] {
            let hits = along(&k, &mut cube, o, d);
            assert_eq!(hits.len(), faces, "{hits:?}");
            assert!((hits[0].distance - distance).abs() < 1e-9, "{hits:?}");
            assert!(
                hits[0].point.distance(Point3::new(x, y, z)) < 1e-9,
                "{hits:?}"
            );
            for pair in hits.windows(2) {
                assert_eq!(pair[0].point, pair[1].point, "{hits:?}");
                assert_eq!(pair[0].distance, pair[1].distance, "{hits:?}");
                assert!(rank(pair[0].face) < rank(pair[1].face), "{hits:?}");
            }
            let one = first(&mut cube, o, d).unwrap();
            assert!(hits.iter().any(|h| h.face == one.face), "{one:?} {hits:?}");
        }
    }

    /// The rays of the #21603 review. Along +x at z = 10, y = 4.9999 the radius-5 cylinder
    /// about z is crossed at a grazing angle at x = -+sqrt(25 - 4.9999^2), about 0.0316: two
    /// hits, the first where `first_face_hit` lands; the same for the radius-2 sphere at
    /// (0, 0, 20) along +x at y = 1.999, z = 20. Through the axis of the radius-4e-4 cylinder
    /// from 20 away, its two crossings 8e-4 apart are two hits, at 19.9996 and 20.0004, the
    /// first where `first_face_hit` lands. At y = 4.99999 the ray is within the tolerance of
    /// the cylinder's side over a stretch: its two ends, 20 -+ sqrt(25 - y^2), are the hits.
    /// Through the hole of the torus of radii 10 and 2 about z, along +x at y = 7.99999, the
    /// outer crossings at 20 -+ sqrt(144 - y^2) and the ends of the stretch past the hole at
    /// 20 -+ sqrt(64 - y^2): four hits of its one face. Along +x in the plane z = 5 of the cube
    /// [-5, 5]^3, the ray lies along that face: only the sides x = -5 and x = 5, at 15 and 25.
    #[test]
    fn a_grazing_or_close_pair_of_crossings_are_hits_and_a_stretch_along_a_face_is_none() {
        let mut k = Kernel::default();
        let ray = |o: [f64; 3]| Ray::new(Point3::new(o[0], o[1], o[2]), Vec3::new(1.0, 0.0, 0.0));
        let both = |k: &Kernel, b: &mut Brep, o: [f64; 3]| {
            let hits = face_hits(k, b, ray(o).unwrap()).unwrap();
            let first = BrepQuery::of_brep(b)
                .first_face_hit(k, b, ray(o).unwrap())
                .unwrap()
                .unwrap();
            (hits, first)
        };
        let mut tube =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let mut ball = crate::primitive::api_primitives::create_sphere(
            &mut k,
            Point3::new(0.0, 0.0, 20.0),
            2.0,
        )
        .unwrap();
        let mut thin = crate::primitive::api_primitives::create_cylinder(
            &mut k,
            Point3::new(1.0, 0.0, 0.0),
            4e-4,
            1.0,
        )
        .unwrap();
        for (b, o, half) in [
            (
                &mut tube,
                [-20.0, 4.9999, 10.0],
                (25.0 - 4.9999_f64 * 4.9999).sqrt(),
            ),
            (
                &mut ball,
                [-20.0, 1.999, 20.0],
                (4.0 - 1.999_f64 * 1.999).sqrt(),
            ),
            (&mut thin, [-19.0, 0.0, 0.5], 4e-4),
        ] {
            let (hits, first) = both(&k, b, o);
            assert_eq!(hits.len(), 2, "{hits:?}");
            assert!((hits[0].distance - (20.0 - half)).abs() < 1e-6, "{hits:?}");
            assert!((hits[1].distance - (20.0 + half)).abs() < 1e-6, "{hits:?}");
            assert!(
                (first.distance - hits[0].distance).abs() < 1e-6,
                "{first:?} {hits:?}"
            );
        }
        let close = |hits: &[FaceHit], want: &[f64]| {
            assert_eq!(hits.len(), want.len(), "{hits:?}");
            for (hit, distance) in hits.iter().zip(want) {
                assert!((hit.distance - distance).abs() < 1e-6, "{hits:?}");
            }
        };
        let (hits, first) = both(&k, &mut tube, [-20.0, 4.99999, 10.0]);
        let half = (25.0 - 4.99999_f64 * 4.99999).sqrt();
        close(&hits, &[20.0 - half, 20.0 + half]);
        assert!(
            (first.distance - hits[0].distance).abs() < 1e-6,
            "{first:?} {hits:?}"
        );
        let mut ring =
            crate::primitive::api_primitives::create_torus(&mut k, Point3::ZERO, 10.0, 2.0)
                .unwrap();
        let (hits, first) = both(&k, &mut ring, [-20.0, 7.99999, 0.0]);
        let outer = (144.0 - 7.99999_f64 * 7.99999).sqrt();
        let inner = (64.0 - 7.99999_f64 * 7.99999).sqrt();
        close(
            &hits,
            &[20.0 - outer, 20.0 - inner, 20.0 + inner, 20.0 + outer],
        );
        assert!(
            (first.distance - hits[0].distance).abs() < 1e-6,
            "{first:?} {hits:?}"
        );

        let mut cube = crate::primitive::api_primitives::create_box(
            &mut k,
            Point3::new(-5.0, -5.0, -5.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap();
        let hits = face_hits(&k, &mut cube, ray([-20.0, 0.0, 5.0]).unwrap()).unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!((hits[0].distance - 15.0).abs() < 1e-9, "{hits:?}");
        assert!((hits[1].distance - 25.0).abs() < 1e-9, "{hits:?}");
        assert_ne!(hits[0].face, hits[1].face, "{hits:?}");
    }

    /// The rays of the third #21603 review. Along +z at x = 1.99999, y = 0, the ray grazes
    /// the radius-2 sphere's equator in its seam's half-plane: the ends of its stretch, on the
    /// seam, are its crossings 20 -+ sqrt(4 - x^2). Along +x at y = 4.99999, z = 19.999995 it
    /// grazes the rim of the radius-5 cylinder of height 20: the ends of the side's stretch
    /// are its crossings 20 -+ sqrt(25 - y^2). At a slope e of 1e-3 and 1e-4, up the cylinder
    /// from (0, 5 - 30e, -20) along (0, e, 1), it enters the bottom at 20 sqrt(1 + e^2) and
    /// crosses the side at a shallow angle at 30 sqrt(1 + e^2); from (0, 5 + 5e, 5) along
    /// (0, -e, 1) it enters the side at 5 sqrt(1 + e^2) and leaves the top at
    /// 15 sqrt(1 + e^2), its side crossing on a span join of the side's surface (#21621). The
    /// first hit of each where `first_face_hit` lands, and on the shallow rays at the same face.
    #[test]
    fn a_ray_grazing_a_seam_or_a_rim_or_crossing_a_wall_shallowly_meets_it() {
        let mut k = Kernel::default();
        let hits_and_first = |k: &Kernel, b: &mut Brep, o: [f64; 3], d: [f64; 3]| {
            let ray = || Ray::new(Point3::new(o[0], o[1], o[2]), Vec3::new(d[0], d[1], d[2]));
            let hits = face_hits(k, b, ray().unwrap()).unwrap();
            let first = BrepQuery::of_brep(b)
                .first_face_hit(k, b, ray().unwrap())
                .unwrap()
                .unwrap();
            (hits, first)
        };
        let close = |hits: &[FaceHit], want: &[f64]| {
            assert_eq!(hits.len(), want.len(), "{hits:?}");
            for (hit, distance) in hits.iter().zip(want) {
                assert!((hit.distance - distance).abs() < 1e-6, "{hits:?}");
            }
        };
        let mut ball =
            crate::primitive::api_primitives::create_sphere(&mut k, Point3::ZERO, 2.0).unwrap();
        let (hits, first) = hits_and_first(&k, &mut ball, [1.99999, 0.0, -20.0], [0.0, 0.0, 1.0]);
        let half = (4.0 - 1.99999_f64 * 1.99999).sqrt();
        close(&hits, &[20.0 - half, 20.0 + half]);
        assert!(
            (first.distance - hits[0].distance).abs() < 1e-6,
            "{first:?} {hits:?}"
        );

        let mut tube =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let (hits, first) =
            hits_and_first(&k, &mut tube, [-20.0, 4.99999, 19.999995], [1.0, 0.0, 0.0]);
        let half = (25.0 - 4.99999_f64 * 4.99999).sqrt();
        close(&hits, &[20.0 - half, 20.0 + half]);
        assert!(
            (first.distance - hits[0].distance).abs() < 1e-6,
            "{first:?} {hits:?}"
        );

        for e in [1e-3, 1e-4] {
            let stretch = (1.0_f64 + e * e).sqrt();
            let (hits, first) =
                hits_and_first(&k, &mut tube, [0.0, 5.0 - 30.0 * e, -20.0], [0.0, e, 1.0]);
            close(&hits, &[20.0 * stretch, 30.0 * stretch]);
            assert!(
                (first.distance - hits[0].distance).abs() < 1e-6 && first.face == hits[0].face,
                "{e} {first:?} {hits:?}"
            );
            let (hits, first) =
                hits_and_first(&k, &mut tube, [0.0, 5.0 + 5.0 * e, 5.0], [0.0, -e, 1.0]);
            close(&hits, &[5.0 * stretch, 15.0 * stretch]);
            assert!(
                (first.distance - hits[0].distance).abs() < 1e-6 && first.face == hits[0].face,
                "{e} {first:?} {hits:?}"
            );
        }
    }

    /// #21621: rays crossing the side of the radius-5 cylinder of height 20 at a shallow
    /// slope e, through the point p = 5(cos t, sin t, 0) + (0, 0, 10) on it, at t = 0 (the
    /// seam), at 90, 180 and 270 degrees (the joins of the side surface's spans), at 89 (near
    /// a join) and at 45 (mid-arc), for e = 1e-4 and 1e-3 and the steeper 2e-3. From outside,
    /// from p - 5d along d = -e r + ẑ (r the radial direction there), the ray enters the side
    /// at 5|d| and leaves the top at 15|d|; from inside along d = e r + ẑ it leaves the side at
    /// 5|d|; `face_hits` and `first_face_hit` give these (the solves converge on the root).
    /// Skewed by s t̂ (t̂ the tangent), s = +-0.003, +-0.01, +-0.05, the ray p + kd is a chord:
    /// its squared distance from the axis is 25 + 10ck + (c^2 + s^2)k^2 for the radial part
    /// c = -+e, so it crosses the side at k = 0 and at k = -10c / (c^2 + s^2); both queries
    /// give the side first at the first of these past the origin (k = -5), where the ray's
    /// distance from the side changes at the rate e. A hit is a surface point within T, the
    /// zone tolerance plus the side's, of the ray, so the first is within T|d| / e of that
    /// crossing. The side's solve missed the crossings at and near the span joins at the two
    /// shallow slopes, and `first_face_hit` the one on the seam, a solution on its edge.
    #[test]
    fn a_shallow_ray_meets_a_cylinder_side_at_its_seam_and_span_joins() {
        let mut k = Kernel::default();
        let mut tube =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let first_hit = |k: &Kernel, tube: &mut Brep, ray: Ray| {
            BrepQuery::of_brep(tube)
                .first_face_hit(k, tube, ray)
                .unwrap()
        };
        let middle = Point3::new(0.0, 0.0, 5.0);
        let face_along = |d: Vec3| Ray::new(middle, d).unwrap();
        let side = first_hit(&k, &mut tube, face_along(Vec3::new(1.0, 1.0, 0.0)))
            .unwrap()
            .face;
        let top = first_hit(&k, &mut tube, face_along(Vec3::new(0.0, 0.0, 1.0)))
            .unwrap()
            .face;
        assert_ne!(side, top);
        let within = brep_zone_tol_3d(&k, &tube, SmallTopoType::NormalSize)
            + face::p2::tolerance(&tube, side).unwrap();
        for e in [1e-4, 1e-3, 2e-3] {
            for degrees in [0.0_f64, 45.0, 89.0, 90.0, 180.0, 270.0] {
                let (sin, cos) = degrees.to_radians().sin_cos();
                let radial = Vec3::new(cos, sin, 0.0);
                let tangent = Vec3::new(-sin, cos, 0.0);
                let p = Point3::new(5.0 * cos, 5.0 * sin, 10.0);
                for skew in [0.0, 0.003, -0.003, 0.01, -0.01, 0.05, -0.05] {
                    for inward in [true, false] {
                        let slope = if inward { -e } else { e };
                        let d = radial * slope + tangent * skew + Vec3::new(0.0, 0.0, 1.0);
                        let length = d.length();
                        let ray = Ray::new(p - d * 5.0, d).unwrap();
                        let at = (e, degrees, skew, inward);
                        let hits = face_hits(&k, &mut tube, ray).unwrap();
                        let first = first_hit(&k, &mut tube, ray);
                        assert!(first.is_some(), "{at:?}: no first hit {hits:?}");
                        let first = first.unwrap();
                        let near = within * length / e;
                        let other = -10.0 * slope / (slope * slope + skew * skew);
                        let crossing = if other > -5.0 && other < 0.0 {
                            (5.0 + other) * length
                        } else {
                            5.0 * length
                        };
                        assert!(
                            hits.first().is_some_and(
                                |h| h.face == side && (h.distance - crossing).abs() <= near
                            ),
                            "{at:?} {hits:?}"
                        );
                        assert!(
                            first.face == side && (first.distance - crossing).abs() <= near,
                            "{at:?} {first:?}"
                        );
                        let surface = face::p2::surface(&tube, side).unwrap().unwrap();
                        let on_ray = ray.origin + ray.direction * first.distance;
                        assert!(
                            surface.evaluate_point(&k, first.uv).unwrap() == first.point
                                && first.point.distance(on_ray) <= within,
                            "{at:?} {first:?}"
                        );
                        if skew == 0.0 {
                            let want: &[(FaceId, f64)] = if inward {
                                &[(side, 5.0 * length), (top, 15.0 * length)]
                            } else {
                                &[(side, 5.0 * length)]
                            };
                            assert_eq!(hits.len(), want.len(), "{at:?} {hits:?}");
                            for (hit, &(face, distance)) in hits.iter().zip(want) {
                                assert!(
                                    hit.face == face && (hit.distance - distance).abs() < 1e-6,
                                    "{at:?} {hits:?}"
                                );
                            }
                            assert!(
                                (first.distance - 5.0 * length).abs() < 1e-6,
                                "{at:?} {first:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// The rays of the #22030 review through an edge or a corner, where the ray solve may
    /// give the edge or vertex, or several faces at one distance. On the box [0, 10]^3: from
    /// (20, 12, 5) along (-10, -2, 0) and from (20, 15, 5) along (-10, -5, 0) through the
    /// edge x = y = 10, from (15, 20, 5) along (-5, -10, 0) through the same edge, from
    /// (20, 15, 17) along (-10, -5, -7) and from (20, 20, 20) along (-1, -1, -1) through the
    /// corner (10, 10, 10), from (-5, 5, 15) along (5, 0, -5) through the edge x = 0, z = 10.
    /// On the radius-5 cylinder of height 20, from (5 + 5 cos 45, 5 sin 45, 25) and
    /// (10, 0, 25) along (-5, 0, -5) through its top rim, the second on the seam.
    /// `first_face_hit` names `face_hits`' first face (the first in `GetFaces` order of those
    /// there) at its distance, with the point at its (u, v).
    #[test]
    fn first_face_hit_at_an_edge_or_corner_is_face_hits_first() {
        let mut k = Kernel::default();
        let mut block =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 10.0, 10.0, 10.0)
                .unwrap();
        let mut tube =
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap();
        let half = 0.5_f64.sqrt();
        let rays = [
            ([20.0, 12.0, 5.0], [-10.0, -2.0, 0.0], false),
            ([20.0, 15.0, 5.0], [-10.0, -5.0, 0.0], false),
            ([15.0, 20.0, 5.0], [-5.0, -10.0, 0.0], false),
            ([20.0, 15.0, 17.0], [-10.0, -5.0, -7.0], false),
            ([20.0, 20.0, 20.0], [-1.0, -1.0, -1.0], false),
            ([-5.0, 5.0, 15.0], [5.0, 0.0, -5.0], false),
            (
                [5.0 + 5.0 * half, 5.0 * half, 25.0],
                [-5.0, 0.0, -5.0],
                true,
            ),
            ([10.0, 0.0, 25.0], [-5.0, 0.0, -5.0], true),
        ];
        for (o, d, on_tube) in rays {
            let b = if on_tube { &mut tube } else { &mut block };
            let within = 2.0 * brep_zone_tol_3d(&k, b, SmallTopoType::NormalSize);
            let ray = Ray::new(Point3::new(o[0], o[1], o[2]), Vec3::new(d[0], d[1], d[2])).unwrap();
            let hits = face_hits(&k, b, ray).unwrap();
            let first = BrepQuery::of_brep(b).first_face_hit(&k, b, ray).unwrap();
            assert!(first.is_some(), "{o:?}: no first hit {hits:?}");
            let first = first.unwrap();
            assert!(
                hits.first().is_some_and(
                    |h| h.face == first.face && (h.distance - first.distance).abs() <= within
                ),
                "{o:?} {first:?} {hits:?}"
            );
            let surface = face::p2::surface(b, first.face).unwrap().unwrap();
            assert!(
                surface.evaluate_point(&k, first.uv).unwrap() == first.point,
                "{o:?} {first:?}"
            );
        }
    }

    /// A ray's direction is made unit whatever its finite length: (1e200, 0, 0), whose squared
    /// length overflows, is the unit +x; one shorter than `EFF_ZERO`, such as (1e-200, 0, 0) or
    /// (1e-13, 0, 0), is refused, while (2e-12, 0, 0) is a ray along +x.
    #[test]
    fn a_ray_direction_is_unit_at_any_finite_length_and_refused_below_zero() {
        let unit = |d: Vec3| Ray::new(Point3::ZERO, d).map(|r| r.direction());
        let x = Vec3::new(1.0, 0.0, 0.0);
        assert_eq!(unit(Vec3::new(1e200, 0.0, 0.0)), Ok(x));
        assert_eq!(unit(Vec3::new(-1e300, 0.0, 0.0)), Ok(-x));
        let huge = unit(Vec3::new(1e308, 1e308, 0.0)).unwrap();
        let half = core::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (huge - Vec3::new(half, half, 0.0)).length() < 1e-15,
            "{huge:?}"
        );
        assert!((unit(Vec3::new(2e-12, 0.0, 0.0)).unwrap() - x).length() < 1e-15);
        let refused = |d: Vec3| matches!(unit(d), Err(SolidError::InvalidInput { .. }));
        assert!(refused(Vec3::new(1e-200, 0.0, 0.0)));
        assert!(refused(Vec3::new(1e-13, 0.0, 0.0)));
    }

    /// A query keeps the face caches its ray solves build (upstream keeps them on the
    /// surfaces): a face's cache is built once however many points it classifies, and
    /// the classifications equal those of a fresh query per point (#2494).
    #[test]
    fn classifications_build_each_face_cache_once() {
        let mut k = Kernel::default();
        let mut cylinder = || {
            crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 20.0)
                .unwrap()
        };
        let (mut kept_brep, mut fresh_brep) = (cylinder(), cylinder());
        let points = [
            Point3::new(1.0, 1.0, 5.0),
            Point3::new(-2.0, 1.0, 15.0),
            Point3::new(0.5, -3.0, 10.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(0.0, 0.0, 30.0),
        ];
        let faces = kept_brep.faces().ids().count();

        let query = BrepQuery::of_brep(&kept_brep);
        let mut kept = Vec::new();
        for p in points {
            for boundary in [false, true] {
                let pc = query.point_3d_classify(&k, &mut kept_brep, p, 1e-6, boundary);
                kept.push(format!("{pc:?}"));
            }
        }
        let builds = query.face_caches.builds();
        assert!(builds > 0, "the ray fires reach a face cache");
        assert!(builds <= faces, "{builds} builds for {faces} faces");

        let mut fresh = Vec::new();
        let mut fresh_builds = 0;
        for p in points {
            for boundary in [false, true] {
                let query = BrepQuery::of_brep(&fresh_brep);
                let pc = query.point_3d_classify(&k, &mut fresh_brep, p, 1e-6, boundary);
                fresh.push(format!("{pc:?}"));
                fresh_builds += query.face_caches.builds();
            }
        }
        assert_eq!(kept, fresh);
        assert!(
            fresh_builds > builds,
            "{fresh_builds} fresh vs {builds} kept"
        );
        // what the solves may make on the B-rep: tolerances and UV trim curves
        let made = |b: &Brep| {
            let tolerances: Vec<_> = b
                .edges()
                .iter()
                .map(|(_, e)| e.tolerance.to_bits())
                .chain(b.vertices().iter().map(|(_, v)| v.tolerance.to_bits()))
                .collect();
            (tolerances, trim_count(b))
        };
        assert_eq!(made(&kept_brep), made(&fresh_brep));

        // the same points again: every cache is kept
        for p in points {
            let _ = query.point_3d_classify(&k, &mut kept_brep, p, 1e-6, false);
        }
        assert_eq!(query.face_caches.builds(), builds);

        // another kernel state (a tolerance) builds them again
        k.tolerances.approx_tol_3d *= 2.0;
        let _ = query.point_3d_classify(&k, &mut kept_brep, points[0], 1e-6, false);
        assert!(query.face_caches.builds() > builds);
    }

    #[test]
    fn lazy_brep_edits_the_callers_brep_or_a_copy() {
        let mut b = Brep::new();
        let v = b.add_vertex(Point3::new(1.0, 2.0, 3.0)).unwrap();

        let mut shared = LazyBrep::from(&b);
        shared.get_mut().unwrap().vertex_mut(v).unwrap().tolerance = 0.5;
        assert_eq!(shared.get().vertex(v).unwrap().tolerance, 0.5);
        drop(shared);
        assert_ne!(b.vertex(v).unwrap().tolerance, 0.5);

        let mut own = LazyBrep::from(&mut b);
        own.get_mut().unwrap().vertex_mut(v).unwrap().tolerance = 0.5;
        drop(own);
        assert_eq!(b.vertex(v).unwrap().tolerance, 0.5);
    }

    #[test]
    fn a_standing_copy_keeps_the_locked_box() {
        let k = Kernel::default();
        let mut b = Brep::new();
        b.add_vertex(Point3::new(1.0, 2.0, 3.0)).unwrap();
        lock_bounding_box(&k, &mut b, false).unwrap();
        let locked = b.locked_bounding_box();
        assert!(locked.is_some());
        assert_eq!(
            b.clone_standing_for_self().unwrap().locked_bounding_box(),
            locked
        );
    }

    #[test]
    fn decimation_attribute_copies_reset_sort_indices() {
        let mut a = decimate_vertex_attribute(7, Behavior::Reference);
        if let AttributeValue::DecimateVertex(d) = a.value_mut() {
            d.sort_index = 9;
            d.vertex_class = PolyVertexClass::Corner;
            d.max_face_error = 0.5;
        }
        let c = a.make_copy();
        assert_eq!((c.attribute_id(), c.behavior()), (7, Behavior::Reference));
        let AttributeValue::DecimateVertex(d) = c.value() else {
            panic!("class lost");
        };
        assert_eq!(
            (d.sort_index, d.vertex_class, d.max_face_error),
            (0, PolyVertexClass::Corner, 0.5)
        );
        assert_eq!(d.vertex_distance, -BIG_DOUBLE);

        let e = quadric_edge_attribute(8, Behavior::Copy);
        let AttributeValue::QuadricEdge(d) = e.make_copy().value().clone() else {
            panic!("class lost");
        };
        assert_eq!(d.sort_index, Some(0));
        assert_eq!(e.type_code(), crate::geom::attribute::type_code::ATTRIBUTE);
    }

    #[test]
    fn evaluates_a_plane() {
        let k = Kernel::default();
        let s = unit_plane();
        let uv = Point2::new(0.25, 0.75);
        let p = evaluate_surface_point(&k, Some(&s), uv).unwrap();
        assert_eq!((p.x, p.y, p.z), (0.25, 0.75, 0.0));
        let n = evaluate_surface_normal(&k, Some(&s), uv).unwrap();
        assert_eq!((n.x, n.y, n.z), (0.0, 0.0, 1.0));
        let (q, du, dv) = evaluate_surface_derivatives(&k, Some(&s), uv).unwrap();
        assert_eq!((q.x, q.y, q.z), (p.x, p.y, p.z));
        assert_eq!(
            (du.x, du.y, du.z, dv.x, dv.y, dv.z),
            (1.0, 0.0, 0.0, 0.0, 1.0, 0.0)
        );
    }

    #[test]
    fn notify_named_primitive_box_builds_nonempty_trees_and_classifies_interior() {
        let mut k = Kernel::default();
        let box_brep =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        assert_eq!(box_brep.naming(), NamingScheme::Notify);

        let query = BrepQuery::of_brep(&box_brep);
        let cache = query.cache(&k, &box_brep).unwrap();
        assert!(cache.vertex_tree().is_some());
        assert!(cache.curve_tree().is_some());
        assert!(cache.surface_tree().is_some());

        let inf = box_brep.infinite_region().unwrap();
        let mut classifier = BrepClassifier::of_brep(&box_brep);
        let inside = classifier
            .point_3d_classify(&k, Point3::new(0.5, 0.5, 0.5), 1e-5, false)
            .unwrap();
        assert_eq!(inside.point_class(), PointClassType::Region);
        let Some(EntityRef::Region(r_in)) = inside.object() else {
            panic!("expected region classification, got {inside:?}");
        };
        assert_ne!(r_in, inf);
        assert_eq!(classifier.brep().region(r_in).unwrap().is_void(), 0);

        let outside = classifier
            .point_3d_classify(&k, Point3::new(2.0, 2.0, 2.0), 1e-5, false)
            .unwrap();
        assert_eq!(outside.point_class(), PointClassType::Region);
        assert_eq!(outside.object(), Some(EntityRef::Region(inf)));
    }
}
