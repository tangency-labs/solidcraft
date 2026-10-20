//! Hidden curve removal: which parts of a B-rep's edges and surface silhouettes are
//! visible along a viewing direction, computed on the exact (NURBS) geometry.
//!
//! Upstream: `source/SMLib/src/SmHCR.cpp`, `source/SMLib/inc/SmHCR.h`.
//!
//! | Rust                                         | Upstream                                  |
//! |----------------------------------------------|-------------------------------------------|
//! | [`Hcr`]                                      | `SmHCR`                                   |
//! | [`Hcr::new`], [`view_transform_for_vector`]  | `SmHCR::SmHCR(.., const SmVector3d&, ..)` |
//! | [`Hcr::with_transform`]                      | `SmHCR::SmHCR(.., const SmAxis2Placement&, ..)`, `ConstructorInit` |
//! | [`Hcr::from_scene`]                          | `ConstructorInit` after the scene B-rep is built |
//! | `Drop` (implicit)                            | `SmHCR::~SmHCR`                           |
//! | [`Hcr::set_hcr_for_silhouettes`]             | `SetHCRForSilhouettes`                    |
//! | [`Hcr::scene_brep`]                          | `GetSceneBrep`                            |
//! | [`Hcr::silhouette_edges`]                    | `GetSilhouetteEdges`                      |
//! | [`Hcr::compute_curve_visibility`]            | `ComputeCurveVisibility`                  |
//! | [`Hcr::compute_global_visibility`]           | `ComputeGlobalVisibility`                 |
//! | [`Hcr::compute_edge_visibility`]             | `ComputeEdgeVisibility`                   |
//! | [`Hcr::compute_silhouettes`]                 | `ComputeSilhouettes`                      |
//! | [`Hcr::add_silhouette_edge`]                 | `ComputeSilhouettes`, its edge classification body |
//! | [`Hcr::is_silhouette_edge`]                  | `IsSilhouetteEdge`                        |
//! | [`Hcr::is_back_facing_edge`]                 | `IsBackFacingEdge`                        |
//! | [`Hcr::is_point_visible`]                    | `IsPointVisible`                          |
//! | [`Hcr::intersect_curve_with_silhouettes`]    | `IntersectCurveWithSilhouettes`           |
//! | [`HcrObject`], `Geometry` (private)          | the `SmObject*` of the classifications    |
//!
//! Deviations:
//! - The scene B-rep copy and its transformation (`SmBrep` copy constructor,
//!   `Transform`) and its bounding box
//!   (`SmBrep::CalculateBoundingBox`) belong to unported units, so [`Hcr::with_transform`]
//!   stops there; [`Hcr::from_scene`] is the rest of `ConstructorInit` for a caller that
//!   already holds the transformed scene and its box.
//! - The pointer maps `m_pSilVertices`, `m_pSilEdges` and `m_pBackFacingEdges` are id
//!   sets; [`Hcr::silhouette_edges`] returns the ids in ascending order (upstream
//!   `GetAllKeys` iterates the pointer hash, whose order is not reproducible).
//! - The traversal mark of `ComputeGlobalVisibility` (`SmNewMarkAndLock`, `Mark`,
//!   `IsMarked`) is a set of marked edges owned by the call: a freshly advanced mark
//!   marks nothing. [`Hcr::compute_edge_visibility`] takes that set.
//! - `ComputeEdgeVisibility`'s edge stack only ever holds its argument (its pushes are
//!   in `#ifdef FIX_ME`), so the edge is processed once without a stack.
//! - `IsSilhouetteEdge` returns the status of a failed `AngleBetween` from a
//!   `SmBoolean` function, which callers read as `TRUE`; the port returns `true`.
//! - Upstream defects: UB-169 (a zero view vector leaves the transform axes
//!   uninitialised: `InvalidInput`), UB-170 (the silhouette-edge loop of
//!   `IntersectCurveWithSilhouettes` is bounded by the vertex count: indexes past the
//!   edge list return `InvalidInput`; fewer vertices than edges skips edges, as
//!   upstream). Outputs that a failed callee leaves uninitialised upstream return
//!   `InvalidInput` instead of being read.
//! - `ComputeGlobalVisibility`'s copy of edge and face attributes onto the edge curves
//!   (display colours) is not ported: curves carry no attribute list here.
//! - `m_pFrontFaceTree`, `m_pVisiblePoints` and `m_pInvisiblePoints` (unused upstream)
//!   are not kept.
//! - **Surface identity.** `ComputeSilhouettes` tests whether the two faces of a split are
//!   on the same surface object (`SmHCR.cpp:1264-1266`, pointer equality). A port face
//!   owns its surface record, so the object is identified by the surface's owner
//!   (`SmSurface::m_pOwner`); see `same_surface`. Surface data is never compared.
//! - `SmBrep::MergeCurvesOnSurface` of `ComputeSilhouettes` is
//!   `topology::brep::p2::merge_curves_on_surface` through the B-rep cache's
//!   classification world (`query.brep`, as in `topology.brep_p10`), so
//!   [`Hcr::compute_silhouettes`] and [`Hcr::compute_global_visibility`] take the
//!   kernel mutably.
//! - `IsPointVisible` keeps the ray tracer's surface set and grid and rebuilds the
//!   tracer (which borrows the surfaces) for each ray.
//! - `DrawVisibleCurves`, `IsConcaveSilhouette` and `IsFrontFacingFace` are not owned by
//!   this unit (no caller, graphics only).

use std::collections::BTreeSet;

use crate::curve::curve::Curve;
use crate::error::{ignore_status, ignored, Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::grid::Grid;
use crate::geom::object::ObjectRef;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::{are_same, xsect_tol_3d_from_zones, zone_tol3d, UNINIT_TOL};
use crate::geom::tree::{NodeGeomType, Tree};
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::curve_class::p1::IntervalPosition;
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use crate::intersect::ray_tracer::RayTracer;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQ, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::volume::Volume;
use crate::topology::{edge, edgeuse, vertex, vertexuse};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, RegionId, VertexId};
use crate::EntityRef;

const OP: &str = "blend::hcr";

/// A read of a value upstream leaves uninitialised, or of a null pointer (undefined
/// behaviour upstream).
const fn undefined(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

// ---- classification handles ---------------------------------------------------------------

/// The objects a scene curve classification refers to (upstream `SmObject*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcrObject {
    /// The curve of a scene edge (the classified curve).
    Curve(EdgeId),
    /// A scene vertex (`SM_PC_VERTEX`).
    Vertex(VertexId),
    /// A scene edge (`SM_PC_EDGE`).
    Edge(EdgeId),
    /// A scene region (`SM_PC_REGION`).
    Region(RegionId),
}

impl ClassObject for HcrObject {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve(_) => ClassObjectKind::Curve,
            Self::Vertex(_) => ClassObjectKind::Vertex,
            Self::Edge(_) => ClassObjectKind::Edge,
            Self::Region(_) => ClassObjectKind::Region,
        }
    }
}

/// Resolves [`HcrObject`] handles in a scene B-rep. A handle that does not resolve is an
/// ordinary error of the classification code, never a deferral.
#[derive(Debug, Clone, Copy)]
pub struct Geometry<'a> {
    /// The scene B-rep.
    pub brep: &'a Brep,
}

impl ClassGeometry<HcrObject> for Geometry<'_> {
    fn curve(&self, o: HcrObject) -> Option<&Curve> {
        match o {
            HcrObject::Curve(e) | HcrObject::Edge(e) => edge::curve(self.brep, e).ok().flatten(),
            _ => None,
        }
    }
    fn surface(&self, _: HcrObject) -> Option<&Surface> {
        None
    }
    fn volume(&self, _: HcrObject) -> Option<&Volume> {
        None
    }
    fn point(&self, _: HcrObject) -> Option<Point3> {
        None
    }
    fn classification_vertex_point(&self, o: HcrObject) -> Option<Point3> {
        match o {
            HcrObject::Vertex(v) => self.brep.vertex(v).ok().map(|r| r.point),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: HcrObject) -> Option<f64> {
        match o {
            HcrObject::Vertex(v) => vertex::tolerance(self.brep, v).ok(),
            HcrObject::Edge(e) | HcrObject::Curve(e) => edge::tolerance(self.brep, e).ok(),
            HcrObject::Region(_) => None,
        }
    }
}

/// The solution handle of the unit's curve solves.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveHandle(Extent1d);

impl SolutionObject for CurveHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.0)
    }
}

// ---- unported callees ---------------------------------------------------------------------

/// `SmBrep::SmBrep(const SmBrep&)` (unit `topology.brep_p3`).
fn copy_brep(brep: &Brep) -> Result<Brep> {
    let _ = brep;
    // UNPORTED(topology.brep_p3): SmBrep::SmBrep(const SmBrep&)
    Err(SolidError::unported(
        "SmBrep::SmBrep(const SmBrep&)",
        "topology.brep_p3",
    ))
}

/// `SmBrep::Transform` (unit `topology.brep_p5`).
fn transform_brep(brep: &mut Brep, transform: &Axis2Placement) -> Result<()> {
    let _ = (brep, transform);
    // UNPORTED(topology.brep_p5): SmBrep::Transform
    Err(SolidError::unported(
        "SmBrep::Transform",
        "topology.brep_p5",
    ))
}

/// `SmBrep::FindAndSetInfiniteRegion()` with upstream's default arguments (no shell to
/// test, `SM_VS_INFINITEREGION_SWAP`, no bounding box).
fn find_and_set_infinite_region(brep: &mut Brep) -> Result<()> {
    const SM_VS_INFINITEREGION_SWAP: i32 = 2;
    crate::topology::brep::p5::find_and_set_infinite_region(
        brep,
        None,
        SM_VS_INFINITEREGION_SWAP,
        None,
    )
}

/// `SmBrep::CalculateBoundingBox` (unit `query.brep`).
fn brep_bounding_box(brep: &Brep) -> Result<Extent3d> {
    let _ = brep;
    // UNPORTED(query.brep): SmBrep::CalculateBoundingBox
    Err(SolidError::unported(
        "SmBrep::CalculateBoundingBox",
        "query.brep",
    ))
}

/// `SmBrep::GetInfiniteRegion` (unit `topology.brep_p1`).
const fn infinite_region(brep: &Brep) -> Option<RegionId> {
    brep.infinite_region()
}

/// `SmBrep::GetEdges(rEdges)` without an attribute filter (unit `topology.brep_p1`):
/// the B-rep's edges in `m_pEdgeListHead` order. Every call upstream's `SmHCR` makes
/// passes no attribute id.
fn brep_edges(brep: &Brep) -> Result<Vec<EdgeId>> {
    crate::topology::brep::p1::edges(brep, None)
}

/// `SmBrep::GetFaces` without an attribute filter (unit `topology.brep_p1`).
fn brep_faces(brep: &Brep) -> Result<Vec<FaceId>> {
    crate::topology::brep::p1::faces(brep, None)
}

/// `SmBrep::GetSurfaces` without an attribute filter (unit `topology.brep_p1`): each
/// surface owned by its face, by its face, in face order (the selection of
/// `topology::brep::p1::surfaces`, which returns the surfaces themselves).
fn brep_surfaces(brep: &Brep) -> Result<Vec<FaceId>> {
    let mut out = Vec::new();
    // for every face
    for f in crate::topology::brep::p1::faces(brep, None)? {
        let surface = brep
            .face(f)?
            .surface
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        // add all standalone face->Surfaces to the list
        if surface.owner()
            == Some(crate::topology::brep_cache::arena_identity(
                crate::topology::brep_cache::CacheObject::Face(f),
            ))
        {
            out.push(f);
        }
    }
    Ok(out)
}

/// `SmBrep::AddDeletedSurface` (unit `topology.brep_p1`): the scene keeps the copy
/// until it is destroyed (here a clone of the surface the ray tracer owns).
fn add_deleted_surface(brep: &mut Brep, surface: &Surface) {
    brep.add_deleted_surface(surface.clone());
}

/// `MergeCurvesOnSurface(*pSurface, m_dThisApproxTol3d, s3DCurves, &sUVBSCurves,
/// sNewFaces, sEdges, TRUE)` (`SmHCR.cpp:1229`): the edges of the curves, through the
/// B-rep cache's classification world (`query.brep`) as in `topology.brep_p10`.
fn merge_curves_on_surface(
    k: &mut Kernel,
    brep: &mut Brep,
    face: FaceId,
    approx_tol_3d: f64,
    curves: &[Curve],
    uv_curves: &[Curve],
) -> Result<Vec<EdgeId>> {
    let (_, edges) = crate::topology::brep::p10::merge_curves_on_surface(
        k,
        brep,
        face,
        approx_tol_3d,
        curves,
        uv_curves,
        true,
    )?;
    Ok(edges)
}

/// `SmEdgeuse::EvaluateBinormal(t, FALSE, ..)` (unit `topology.edgeuse_p3`): binormal,
/// tangent, face use normal. It works on the scene itself, as upstream: with
/// `bUse3DOnly = FALSE` a missing UV trim curve is built on the scene's edge use.
fn evaluate_binormal(
    k: &Kernel,
    brep: &mut Brep,
    eu: EdgeuseId,
    t: f64,
) -> Result<(Vec3, Vec3, Vec3)> {
    let r = edgeuse::p3::evaluate_binormal(
        k,
        brep,
        eu,
        t,
        false,
        &edgeuse::p3::BinormalOptions::default(),
    )?;
    Ok((r.binormal, r.tangent, r.face_normal))
}

/// `SmFace::FindIfRectangularTrim` (unit `topology.face_p1`): domain, rectangular,
/// natural. It works on the scene itself: the UV trim curves it makes stay on the
/// scene's faces, as upstream. The UV trim curves it copies are owned by their edge
/// use, keyed by the edge use's arena index.
fn find_if_rectangular_trim(
    k: &Kernel,
    brep: &mut Brep,
    face: FaceId,
) -> Result<(Extent2d, bool, bool)> {
    let key_of = |e: EntityRef| ObjectRef {
        type_code: 0,
        key: ObjectKey(match e {
            EntityRef::Edgeuse(eu) => u64::from(eu.index()),
            _ => 0,
        }),
    };
    let r = crate::topology::face::p1::find_if_rectangular_trim(k, brep, face, &key_of)?;
    Ok((r.uv_domain, r.rectangular, r.natural))
}

/// The classification of `curve` over `interval` that `ComputeEdgeVisibility` builds
/// (`SmCurveClassification(cpCurve, crInterval, NULL, tol, 64, pData)`, unit
/// `intersect.curve_class_p4`, whose body is `ReSet`, unit `intersect.curve_class_p2`;
/// CO-PORTED here on [`CurveClassification::with_curve_interval`]): one unclassified
/// interval; the argument converts to `SmZoneTol3d` (`fabs`, `SM_USE_DEFAULT` -> the
/// system zone tolerance, [`zone_tol3d`]), the source zone tolerance is that value when
/// positive, else `SM_UNINIT_TOL`, and the three point classifications keep it. The
/// borrowed 64-interval buffer only bounds when the array reallocates.
#[must_use]
pub fn new_curve_classification(
    k: &Kernel,
    curve: HcrObject,
    interval: Extent1d,
    src_zone_tol_3d: f64,
) -> CurveClassification<HcrObject> {
    // The `double` becomes the `SmZoneTol3d` parameter of the constructor: `fabs`, and
    // `SM_USE_DEFAULT` -> the system zone tolerance.
    let src_zone_tol_3d = zone_tol3d(k, src_zone_tol_3d);
    let mut cc = CurveClassification::with_curve_interval(k, curve, interval, src_zone_tol_3d);
    // `tol > 0.0 ? tol : SM_UNINIT_TOL` (NaN included)
    if src_zone_tol_3d.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
        cc.set_src_zone_tol_3d(UNINIT_TOL);
    }
    cc
}

/// `SM_CAST_PTR(SmBSplineSurface, pSurface)`: the surface as an `SmBSplineSurface`.
fn b_spline_part(surface: &Surface) -> Option<&BSplineSurface> {
    match surface.kind() {
        SurfaceKind::BSpline(b) => Some(b),
        SurfaceKind::Plane(p) => Some(p.base()),
        SurfaceKind::SurfOfRevolution(s) => Some(s.base()),
        _ => None,
    }
}

/// `sNewFaces[0]->GetSurface() == sNewFaces[1]->GetSurface()`: pointer identity of the two
/// faces' surface objects. A port face owns its surface record, so there is no pointer;
/// the identity of an upstream surface object is its owner (`SmSurface::m_pOwner`, the
/// face it was attached to), which every record standing for the same upstream object
/// carries. Two surfaces are the same when both faces have none (`NULL == NULL`), when
/// they are the same face's surface, or when both carry the same owner; equal surface
/// data with different (or no) owners is a different object, as upstream.
fn same_surface(brep: &Brep, a: FaceId, b: FaceId) -> Result<bool> {
    let (sa, sb) = (
        brep.face(a)?.surface.as_ref(),
        brep.face(b)?.surface.as_ref(),
    );
    Ok(match (sa, sb) {
        (None, None) => true,
        (Some(x), Some(y)) => a == b || x.owner().is_some_and(|o| Some(o) == y.owner()),
        _ => false,
    })
}

// ---- construction -------------------------------------------------------------------------

/// The view transformation the view-vector constructor builds: the inverse of the STEP
/// placement whose Z axis is the unit view vector and whose X reference is a unit
/// vector perpendicular to it (`MakeUnitOrthoVectors`, `SetSTEPCanonical`, `Invert`).
///
/// # Errors
///
/// `InvalidInput` (UB-169) when the view vector cannot be unitized: upstream ignores
/// that status and builds the placement from uninitialised vectors.
pub fn view_transform_for_vector(view_vector: Vec3) -> Result<Axis2Placement> {
    // get vectors perp to viewing vector
    let [unit_view, perp1, _perp2] = view_vector
        .make_unit_ortho_vectors(None)
        .map_err(|_| undefined("UB-169: view vector cannot be unitized"))?;

    // now make the ViewTransform
    let mut transform = Axis2Placement::default();
    ignore_status(transform.set_step_canonical(Vec3::new(0.0, 0.0, 0.0), unit_view, perp1))?;
    Ok(transform.invert())
}

/// The scene transform with its translation removed, and its inverse
/// (`ConstructorInit`).
fn prepare_transform(view_transform: &Axis2Placement) -> (Axis2Placement, Axis2Placement) {
    let mut vt = *view_transform;
    // zero out translation component of given ViewTransformation
    let translate = -vt.origin();
    vt.translate(translate);
    // ViewTransform invert
    let inv = vt.invert();
    (vt, inv)
}

/// The construction options of [`Hcr`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HcrOptions {
    /// `dThisApproxTol3d`: the distance tolerance of the silhouette walk.
    pub approx_tol_3d: f64,
    /// `dThisAngTolRad`: the angle tolerance of the silhouette walk.
    pub ang_tol_rad: f64,
    /// `bDisplaySeams` (default `TRUE`): seams are visible edges.
    pub display_seams: bool,
    /// `bDisplaySmoothEdges` (default `TRUE`): G1 manifold edges are visible edges.
    pub display_smooth_edges: bool,
}

/// What [`Hcr::is_point_visible`] fires rays against: an owned copy of a trimmed or
/// copied surface, or a scene face's surface.
#[derive(Debug, Clone)]
enum RaySurface {
    Face(FaceId),
    Owned(Box<Surface>),
}

/// The ray tracer state of [`Hcr::is_point_visible`] (`m_pRayTracer`, its grid).
#[derive(Debug, Clone)]
struct RaySetup {
    bbox: Extent3d,
    size: [usize; 3],
    approx_tol: f64,
    surfaces: Vec<RaySurface>,
}

/// A hidden curve removal object (`SmHCR`).
#[derive(Debug, Clone)]
pub struct Hcr {
    view_vector: Vec3,
    ray_offset_tolerance: f64,
    scene: Brep,
    view_transform: Axis2Placement,
    inv_view_transform: Axis2Placement,
    proj_bbox: Extent3d,
    sil_vertex_tree: Tree<VertexId>,
    sil_edge_tree: Tree<EdgeId>,
    sil_vertices: BTreeSet<VertexId>,
    sil_edges: BTreeSet<EdgeId>,
    back_facing_edges: BTreeSet<EdgeId>,
    visible_curves: Vec<Curve>,
    invisible_curves: Vec<Curve>,
    approx_tol_3d: f64,
    ang_tol_rad: f64,
    for_silhouettes: bool,
    display_seams: bool,
    display_smooth_edges: bool,
    ray: Option<RaySetup>,
}

impl Hcr {
    /// An HCR object looking along `view_vector` (`SmHCR(crContext, pBrep,
    /// crViewVector, ..)`).
    ///
    /// # Errors
    ///
    /// Those of [`view_transform_for_vector`] and [`Hcr::with_transform`].
    pub fn new(k: &Kernel, brep: &Brep, view_vector: Vec3, opts: HcrOptions) -> Result<Self> {
        let transform = view_transform_for_vector(view_vector)?;
        Self::with_transform(k, brep, &transform, opts)
    }

    /// An HCR object whose scene is `brep` transformed by `view_transform`
    /// (translation ignored) so that the eye looks along +Z (`SmHCR(crContext, pBrep,
    /// crViewTransformation, ..)`, `ConstructorInit`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the scene copy, transform and box (units
    /// `topology.brep_p3`, `topology.brep_p5`, `query.brep`).
    pub fn with_transform(
        k: &Kernel,
        brep: &Brep,
        view_transform: &Axis2Placement,
        opts: HcrOptions,
    ) -> Result<Self> {
        let (vt, _) = prepare_transform(view_transform);
        // copy the original Brep
        let mut scene = copy_brep(brep)?;
        scene.set_editing_enabled(true);
        // transform the copied Brep to place the eye looking up the Z axis
        ignore_status(transform_brep(&mut scene, &vt))?;
        ignore_status(find_and_set_infinite_region(&mut scene))?; // [B23]
                                                                  // its status is ignored upstream (the box then stays empty)
        let scene_box = ignored(brep_bounding_box(&scene))?.unwrap_or_else(Extent3d::new);
        Ok(Self::from_scene(k, scene, view_transform, scene_box, opts))
    }

    /// The rest of `ConstructorInit` for a scene B-rep already copied and transformed by
    /// `view_transform` (translation ignored), with `scene_box` its
    /// `SmBrep::CalculateBoundingBox`: the view vector, the ray offset tolerance, the
    /// projection box (the scene box grown by its largest size) and the empty trees
    /// and lists.
    #[must_use]
    pub fn from_scene(
        k: &Kernel,
        scene: Brep,
        view_transform: &Axis2Placement,
        scene_box: Extent3d,
        opts: HcrOptions,
    ) -> Self {
        let _ = k;
        let (vt, inv) = prepare_transform(view_transform);
        let mut proj_bbox = scene_box;
        // Ray Start tolerance based on BoundingBox size
        let ray_offset_tolerance = (proj_bbox.max().z - proj_bbox.min().z + 1.0) / 1000.0;
        // Make the bounding box big enough to contain all possible silhouettes.
        let size = proj_bbox.size();
        let max_size = size.max_dimension();
        proj_bbox.expand_absolute(max_size);
        Self {
            view_vector: Vec3::new(0.0, 0.0, 1.0),
            ray_offset_tolerance,
            scene,
            view_transform: vt,
            inv_view_transform: inv,
            proj_bbox,
            sil_vertex_tree: Tree::new_spatial(&proj_bbox),
            sil_edge_tree: Tree::new_spatial(&proj_bbox),
            sil_vertices: BTreeSet::new(),
            sil_edges: BTreeSet::new(),
            back_facing_edges: BTreeSet::new(),
            visible_curves: Vec::new(),
            invisible_curves: Vec::new(),
            approx_tol_3d: opts.approx_tol_3d,
            ang_tol_rad: opts.ang_tol_rad,
            for_silhouettes: false,
            display_seams: opts.display_seams,
            display_smooth_edges: opts.display_smooth_edges,
            ray: None,
        }
    }

    // ---- accessors --------------------------------------------------------------------

    /// The scene B-rep (`GetSceneBrep`).
    #[must_use]
    pub const fn scene_brep(&self) -> &Brep {
        &self.scene
    }
    /// The view vector in the scene (`m_vViewVector`, always `(0, 0, 1)`).
    #[must_use]
    pub const fn view_vector(&self) -> Vec3 {
        self.view_vector
    }
    /// `m_dRayOffsetTolerance`.
    #[must_use]
    pub const fn ray_offset_tolerance(&self) -> f64 {
        self.ray_offset_tolerance
    }
    /// The scene transformation without translation (`m_vViewTransform`).
    #[must_use]
    pub const fn view_transform(&self) -> &Axis2Placement {
        &self.view_transform
    }
    /// Its inverse (`m_vInvViewTransform`).
    #[must_use]
    pub const fn inv_view_transform(&self) -> &Axis2Placement {
        &self.inv_view_transform
    }
    /// The projection box (`m_vProjBBox`).
    #[must_use]
    pub const fn proj_bbox(&self) -> &Extent3d {
        &self.proj_bbox
    }
    /// The visible curve pieces, in the original orientation (`GetVisibleCurves`).
    #[must_use]
    pub fn visible_curves(&self) -> &[Curve] {
        &self.visible_curves
    }
    /// The hidden curve pieces, in the original orientation (`GetInvisibleCurves`).
    #[must_use]
    pub fn invisible_curves(&self) -> &[Curve] {
        &self.invisible_curves
    }
    /// `GetThisApproxTol3d`.
    #[must_use]
    pub const fn approx_tol_3d(&self) -> f64 {
        self.approx_tol_3d
    }
    /// `GetThisAngTolRad`.
    #[must_use]
    pub const fn ang_tol_rad(&self) -> f64 {
        self.ang_tol_rad
    }
    /// `GetDisplaySeams`.
    #[must_use]
    pub const fn display_seams(&self) -> bool {
        self.display_seams
    }
    /// `GetDisplaySmoothEdges`.
    #[must_use]
    pub const fn display_smooth_edges(&self) -> bool {
        self.display_smooth_edges
    }
    /// `SetHCRForSilhouettes`: skip non-silhouette edges in
    /// [`Hcr::compute_global_visibility`].
    pub fn set_hcr_for_silhouettes(&mut self, hcr_for_silhouettes: bool) {
        self.for_silhouettes = hcr_for_silhouettes;
    }
    /// `SetDisplaySeams`.
    pub fn set_display_seams(&mut self, display_seams: bool) {
        self.display_seams = display_seams;
    }
    /// `SetDisplaySmoothEdges`.
    pub fn set_display_smooth_edges(&mut self, display_smooth_edges: bool) {
        self.display_smooth_edges = display_smooth_edges;
    }
    /// Whether `e` is a back-facing edge found by [`Hcr::compute_silhouettes`].
    #[must_use]
    pub fn is_listed_back_facing(&self, e: EdgeId) -> bool {
        self.back_facing_edges.contains(&e)
    }
    /// Lists `e` as back facing (`m_pBackFacingEdges->Insert`, the back-facing branch
    /// of `ComputeSilhouettes`).
    pub fn add_back_facing_edge(&mut self, e: EdgeId) {
        self.back_facing_edges.insert(e);
    }
    /// The silhouette vertices found so far, ascending.
    #[must_use]
    pub fn silhouette_vertices(&self) -> Vec<VertexId> {
        self.sil_vertices.iter().copied().collect()
    }

    /// The silhouette edges found so far, in ascending id order (`GetSilhouetteEdges`).
    #[must_use]
    pub fn silhouette_edges(&self) -> Vec<EdgeId> {
        self.sil_edges.iter().copied().collect()
    }

    // ---- visibility -------------------------------------------------------------------

    /// Splits `cc` at the projected crossings of the silhouettes in front of it and
    /// classifies each interval: visible intervals get an `SM_PC_UNKNOWN` mid point,
    /// hidden ones `SM_PC_REGION` (the infinite region). `start_hint` / `end_hint`: 0
    /// none, 1 visible, 2 hidden, for the first / last interval
    /// (`ComputeCurveVisibility`).
    ///
    /// # Errors
    ///
    /// Those of [`Hcr::intersect_curve_with_silhouettes`], the curve evaluation and
    /// [`Hcr::is_point_visible`].
    pub fn compute_curve_visibility(
        &mut self,
        k: &Kernel,
        cc: &mut CurveClassification<HcrObject>,
        start_hint: u64,
        end_hint: u64,
    ) -> Result<()> {
        // classify TgtCurve into intervals divided by projected curve and vertex xsects.
        self.intersect_curve_with_silhouettes(k, cc)?;

        // for every interval
        let num_intervals = cc.size();
        for i in 0..num_intervals {
            let ivl = cc.at(i)?.interval;
            // skip zero length intervals
            if are_same(ivl.min(), ivl.max()) {
                continue;
            }

            // Classify this curve interval
            let mut visibility = false;
            // when given end hints - use them to classify the interval
            if i == 0 && start_hint != 0 {
                if start_hint == 1 {
                    visibility = true;
                }
            } else if i + 1 == num_intervals && end_hint != 0 {
                if end_hint == 1 {
                    visibility = true;
                }
            } else {
                // classify the interval by a raycast from the interval midPoint
                let t_mid = ivl.evaluate(0.534567);
                let curve = cc
                    .curve()
                    .ok_or(undefined("classification without curve"))?;
                let mid = Geometry { brep: &self.scene }
                    .curve(curve)
                    .ok_or(undefined("classification curve not resolved"))?
                    .evaluate_point(t_mid)?;
                visibility = self.is_point_visible(k, mid)?;
            }

            if visibility {
                cc.at_mut(i)?
                    .mid
                    .set_class_object(PointClassType::Unknown, None);
            } else {
                let region = infinite_region(&self.scene);
                cc.at_mut(i)?
                    .mid
                    .set_class_object(PointClassType::Region, region.map(HcrObject::Region));
            }
        }
        Ok(())
    }

    /// Computes the visible and hidden pieces of every edge and silhouette of the scene
    /// (`ComputeGlobalVisibility`).
    ///
    /// # Errors
    ///
    /// Those of [`Hcr::compute_edge_visibility`], of the curve copies and of the scene
    /// B-rep queries.
    pub fn compute_global_visibility(&mut self, k: &mut Kernel) -> Result<()> {
        // Find silhouette curves on faces
        ignore_status(self.compute_silhouettes(k))?;

        // Attributes for curves are attached to the scene edge curves (graphics only):
        // the edge list is the first scene query.
        let edges = brep_edges(&self.scene)?;

        // Disable editing now
        self.scene.set_editing_enabled(false);

        // increment and lock an unlocked mark
        let mut marked: BTreeSet<EdgeId> = BTreeSet::new();

        // Mark back-facing lines of faces parallel to vector
        let z_vector = Vec3::new(0.0, 0.0, -1.0);
        let faces = brep_faces(&self.scene)?;
        for f in faces {
            let face = self.scene.face(f)?;
            let surface = face
                .surface
                .as_ref()
                .ok_or(undefined("face without surface"))?;
            // skip faces not parallel to viewing direction
            if !surface.is_parallel_to_vector(z_vector)? {
                continue;
            }
            // This face is parallel to the view vector.
            let fu = crate::topology::face::p2::upward_faceuse(&self.scene, f)?
                .ok_or(undefined("face without face use"))?;
            let loopuses = edgeuse::p1::faceuse_loopuses(&self.scene, fu, false, None)?;
            for (jj, lu) in loopuses.into_iter().enumerate() {
                let eus = crate::topology::loopuse::edgeuses(&self.scene, lu, None, None)?;
                for eu in eus {
                    let e = edgeuse::p1::edge(&self.scene, eu)?
                        .ok_or(undefined("edge use without edge"))?;
                    if jj > 0 {
                        // Inner loops are not visible:
                        marked.insert(e);
                    } else if let Some(curve) = edge::curve(&self.scene, e)? {
                        let tol = self.scene.tolerance() / 100.0;
                        if curve.is_line(10, tol)?.is_some() {
                            let t = edge::interval(&self.scene, e)?.evaluate(0.5);
                            let (bin, _, _) = evaluate_binormal(k, &mut self.scene, eu, t)?;
                            if bin.z > -EFF_ZERO {
                                // Binormal is toward eye or perpendicular to the eye
                                marked.insert(e);
                            }
                        }
                    }
                }
            }
        }

        // For each invisible edge: copy it, trim the copy to Edge interval, and add it.
        let edges2 = brep_edges(&self.scene)?;
        let _ = edges;
        for &e in &edges2 {
            if marked.contains(&e) {
                let piece = self.edge_piece(k, e, None)?;
                self.invisible_curves.push(piece);
            }
        }

        // Now compute visibility for every edge that has not been marked as invisible.
        for &e in &edges2 {
            if self.for_silhouettes && !self.sil_edges.contains(&e) {
                continue;
            }
            if !marked.contains(&e) {
                self.compute_edge_visibility(k, e, None, false, &mut marked)?;
            }
        }
        Ok(())
    }

    /// A copy of the curve of `e`, trimmed to `interval` (the edge interval for `None`)
    /// and transformed back to the original orientation.
    fn edge_piece(&self, k: &Kernel, e: EdgeId, interval: Option<Extent1d>) -> Result<Curve> {
        let curve = edge::curve(&self.scene, e)?.ok_or(undefined("edge without curve"))?;
        let mut piece = curve.copy(k)?;
        let mut ivl = match interval {
            Some(i) => i,
            None => edge::interval(&self.scene, e)?,
        };
        piece.trim(k, &mut ivl, TrimMode::Notify)?; // may snap sIvl by tol to existing knots
        piece.transform(k, &self.inv_view_transform, None)?;
        Ok(piece)
    }

    /// Computes the visible and hidden pieces of edge `e` into the curve lists; does
    /// nothing for an edge in `marked`, and marks it (`ComputeEdgeVisibility`).
    /// `known_vertex`: an end vertex whose visibility (`vertex_is_visible`) is known.
    ///
    /// # Errors
    ///
    /// Those of the curve copies, the edge queries and
    /// [`Hcr::compute_curve_visibility`].
    pub fn compute_edge_visibility(
        &mut self,
        k: &Kernel,
        e: EdgeId,
        known_vertex: Option<VertexId>,
        vertex_is_visible: bool,
        marked: &mut BTreeSet<EdgeId>,
    ) -> Result<()> {
        // Stop if we have already touched this edge
        if marked.contains(&e) {
            return Ok(());
        }
        // Touch this edge.
        marked.insert(e);

        // If this edge is back facing, label it invisible.
        let mut visible_edge = !self.back_facing_edges.contains(&e);

        // when not displaying seams, make seams invisible except for silhouette edges
        if visible_edge && !self.display_seams && edge::is_seam(k, &mut self.scene, e)? {
            visible_edge = self.is_silhouette_edge(k, e)?;
        }
        // when not displaying G1 smooth edges, make them invisible except for silhouettes
        if visible_edge
            && !self.display_smooth_edges
            && edge::is_tangent_edge(k, &mut self.scene, e, 2.0, false)?
        {
            visible_edge = self.is_silhouette_edge(k, e)?;
        }

        if !visible_edge {
            let piece = self.edge_piece(k, e, None)?;
            self.invisible_curves.push(piece);
            return Ok(());
        }

        let mut start_hint = 0; // No Hint initially for start or end of curve
        let mut end_hint = 0;
        if let Some(known) = known_vertex {
            let prim = edgeuse::p1::primary_edgeuse(&self.scene, e)?
                .ok_or(undefined("edge without primary edge use"))?;
            let hint = if vertex_is_visible { 1 } else { 2 };
            let vertex_of = |eu| -> Result<Option<VertexId>> {
                match edgeuse::p1::vertexuse(&self.scene, eu)? {
                    Some(vu) => vertexuse::vertex(&self.scene, vu),
                    None => Err(undefined("edge use without vertex use")),
                }
            };
            // vertex is at edge start
            if vertex_of(prim)? == Some(known) {
                if edgeuse::p1::orientation(&self.scene, prim)? == OrientType::Same {
                    start_hint = hint;
                } else {
                    end_hint = hint;
                }
            }
            // vertex is at edge end
            let mate =
                edgeuse::p1::mate(&self.scene, prim)?.ok_or(undefined("edge use without mate"))?;
            if vertex_of(mate)? == Some(known) {
                if edgeuse::p1::orientation(&self.scene, mate)? == OrientType::Same {
                    start_hint = hint;
                } else {
                    end_hint = hint;
                }
            }
        }

        // CurveClassification whose intervals will eventually be classified
        if edge::curve(&self.scene, e)?.is_none() {
            return Err(undefined("edge without curve"));
        }
        let interval = edge::interval(&self.scene, e)?;
        let tol = edge::tolerance(&self.scene, e)?;
        let mut cc = new_curve_classification(k, HcrObject::Curve(e), interval, tol);

        // divide Classification at curve SceneBrep projected xsects and classify
        self.compute_curve_visibility(k, &mut cc, start_hint, end_hint)?;

        for i in 0..cc.size() {
            let civl = cc.at(i)?;
            let ivl = civl.interval;
            // skip zero length intervals
            if are_same(ivl.max(), ivl.min()) {
                continue;
            }
            let visible = civl.mid.point_class() == PointClassType::Unknown;
            // when interval is nonZero length
            if ivl.length() > EFF_ZERO_SQRT {
                let piece = self.edge_piece(k, e, Some(ivl))?;
                if visible {
                    self.visible_curves.push(piece);
                } else {
                    self.invisible_curves.push(piece);
                }
            }
        }
        Ok(())
    }

    /// Adds `e` and its vertices to the silhouette lists and trees (the silhouette
    /// branch of `ComputeSilhouettes`' edge classification).
    ///
    /// # Errors
    ///
    /// Those of the edge queries; [`SolidError::Unported`] from the curve box.
    pub fn add_silhouette_edge(&mut self, k: &Kernel, e: EdgeId) -> Result<()> {
        let v1 = edge::vertex(&self.scene, e)?;
        let v2 = edge::other_vertex(&self.scene, e, v1)?;
        let e_box = self.edge_box(k, e)?;

        // add edge to m_pSilEdges and m_pSilEdgeTree
        self.sil_edges.insert(e);
        ignore_status(self.sil_edge_tree.add_to_spatial_tree(
            &e_box,
            Some(e),
            NodeGeomType::Default,
        ))?;

        // If these are Null, it's some kind of error, but don't crash. [B90]
        for v in [v1, v2].into_iter().flatten() {
            let p = self.scene.vertex(v)?.point;
            let v_box = Extent3d::from_points(p, p);
            self.sil_vertices.insert(v);
            ignore_status(self.sil_vertex_tree.add_to_spatial_tree(
                &v_box,
                Some(v),
                NodeGeomType::Default,
            ))?;
        }
        Ok(())
    }

    /// `pEdge->GetCurve()->CalculateBoundingBox(pEdge->GetInterval(), &box)`, whose
    /// status upstream ignores (the box then stays empty).
    fn edge_box(&self, k: &Kernel, e: EdgeId) -> Result<Extent3d> {
        let curve = edge::curve(&self.scene, e)?.ok_or(undefined("edge without curve"))?;
        let ivl = edge::interval(&self.scene, e)?;
        let req = BoundingBoxRequest {
            normal: true,
            ..BoundingBoxRequest::default()
        };
        Ok(ignored(curve.calculate_bounding_box(k, &ivl, req, true))?
            .and_then(|b| b.normal)
            .unwrap_or_else(Extent3d::new))
    }

    /// Computes the silhouette curves of every non-planar scene surface, merges them
    /// into the scene as edges, and lists the silhouette and back-facing scene edges
    /// (`ComputeSilhouettes`).
    ///
    /// # Errors
    ///
    /// Those of the scene B-rep queries; [`SolidError::Unported`] from the silhouette
    /// curve walk.
    pub fn compute_silhouettes(&mut self, k: &mut Kernel) -> Result<()> {
        // Get SceneBrep Surfaces
        let surfaces = brep_surfaces(&self.scene)?;
        for f in surfaces {
            let face = self.scene.face(f)?;
            let surface = face
                .surface
                .as_ref()
                .ok_or(undefined("face without surface"))?;
            if b_spline_part(surface).is_none() {
                return Err(SolidError::null_pointer(OP));
            }
            // skip Planes - they don't have silhouettes.
            if matches!(surface.kind(), SurfaceKind::Plane(_)) {
                continue;
            }
            // pick tol to limit segment sizes in DoTrace (the partial callee does not
            // take the tolerances yet)
            let sil_tol = self.approx_tol_3d.min(self.ray_offset_tolerance / 100.0);
            let uv_domain = face.uv_domain;
            let Some((curves, uv_curves)) = ignored(surface.create_silhouette_curves(
                k,
                &uv_domain,
                self.view_vector,
                false,
                Some(sil_tol),
                Some(self.ang_tol_rad),
            ))?
            else {
                continue;
            };
            // Turn the curves into Edges in this surface's face.
            let Some(edges) = ignored(merge_curves_on_surface(
                k,
                &mut self.scene,
                f,
                self.approx_tol_3d,
                &curves,
                &uv_curves,
            ))?
            else {
                continue;
            };
            for e in edges {
                let faces = edge::faces(&self.scene, e, None)?;
                // skip edges that don't split faces
                let split = faces.len() == 2
                    && match (faces.first(), faces.get(1)) {
                        (Some(&a), Some(&b)) => same_surface(&self.scene, a, b)?,
                        _ => false,
                    };
                if !split {
                    continue;
                }
                let v1 = edge::vertex(&self.scene, e)?.ok_or(undefined("edge without vertex"))?;
                let v2 = edge::other_vertex(&self.scene, e, Some(v1))?
                    .ok_or(undefined("edge without vertex"))?;
                let p1 = self.scene.vertex(v1)?.point;
                let p2 = self.scene.vertex(v2)?.point;
                let e_box = self.edge_box(k, e)?;
                self.sil_edges.insert(e);
                ignore_status(self.sil_edge_tree.add_to_spatial_tree(
                    &e_box,
                    Some(e),
                    NodeGeomType::Default,
                ))?;
                self.sil_vertices.insert(v1);
                self.sil_vertices.insert(v2);
                ignore_status(self.sil_vertex_tree.add_to_spatial_tree(
                    &Extent3d::from_points(p1, p1),
                    Some(v1),
                    NodeGeomType::Default,
                ))?;
                ignore_status(self.sil_vertex_tree.add_to_spatial_tree(
                    &Extent3d::from_points(p2, p2),
                    Some(v2),
                    NodeGeomType::Default,
                ))?;
            }
        }

        // classify every edge in the scene (adding silhouettes may have split edges)
        let edges = brep_edges(&self.scene)?;
        for e in edges {
            if self.is_silhouette_edge(k, e)? {
                self.add_silhouette_edge(k, e)?;
            } else if self.is_back_facing_edge(k, e)? {
                self.back_facing_edges.insert(e);
            }
        }
        Ok(())
    }

    /// The edge uses, primary first, and its radial, of a manifold or spine edge.
    fn prime_and_radial(&self, e: EdgeId) -> Result<Vec<EdgeuseId>> {
        let uses = edge::edgeuses(&self.scene, e, None, None)?;
        if uses.is_empty() {
            return Err(undefined("edge without edge uses"));
        }
        Ok(uses)
    }

    /// The region of the shell of an edge use.
    fn region_of(&self, eu: EdgeuseId) -> Result<Option<RegionId>> {
        let shell =
            edgeuse::p1::shell(&self.scene, eu)?.ok_or(undefined("edge use without shell"))?;
        Ok(self.scene.shell(shell)?.region)
    }

    /// Whether `e` is a silhouette edge of its faces seen along the view vector
    /// (`IsSilhouetteEdge`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the edge use binormals (their own callees) and the
    /// infinite region; `InvalidInput` for topology upstream would dereference as null.
    /// The binormal builds a missing UV trim curve on the scene's edge use, as upstream.
    pub fn is_silhouette_edge(&mut self, k: &Kernel, e: EdgeId) -> Result<bool> {
        // low work - edge already classified
        if self.sil_edges.contains(&e) {
            return Ok(true);
        }
        // Wire edges are not silhouettes; Lamina edges are.
        if edge::is_wire(&self.scene, e)? {
            return Ok(false);
        }
        if edge::is_lamina(&self.scene, e)? {
            return Ok(true);
        }

        // Look at normals and binormals into both faces.
        let uses = self.prime_and_radial(e)?;
        let prime = *uses.first().ok_or(undefined("edge without edge uses"))?;
        let radial =
            edgeuse::p1::radial(&self.scene, prime)?.ok_or(undefined("edge use without radial"))?;
        let t = edge::interval(&self.scene, e)?.evaluate(0.5123);
        // SE(..): a failed evaluation leaves the outputs uninitialised upstream
        let (bin1, tan1, norm1) = ignored(evaluate_binormal(k, &mut self.scene, prime, t))?
            .ok_or(undefined("uninitialised binormal"))?;
        let (bin2, _, norm2) = ignored(evaluate_binormal(k, &mut self.scene, radial, t))?
            .ok_or(undefined("uninitialised binormal"))?;
        let mut norm1 = norm1;

        // If both dot products are substantial and alike w.r.t. the view vector, it is
        // not a silhouette.
        if norm1.dot(self.view_vector) * norm2.dot(self.view_vector) >= EFF_ZERO_SQRT {
            return Ok(false);
        }

        let inf = infinite_region(&self.scene);
        let inf_this = self.region_of(prime)? == inf;
        let mate =
            edgeuse::p1::mate(&self.scene, prime)?.ok_or(undefined("edge use without mate"))?;
        let inf_other = self.region_of(mate)? == inf;

        // If neither side is the infinite region then it can't be a silhouette.
        if !inf_this && !inf_other {
            return Ok(false);
        }
        // If both sides are the infinite region then it must be a silhouette.
        if inf_this && inf_other {
            return Ok(true);
        }

        let proj = tan1.cross(self.view_vector).cross(tan1);
        if proj.length_squared() < EFF_ZERO_SQ {
            return Ok(false); // curve points in direction of view
        }
        // Do an angle test to see if vector is same as one of binormals.
        let limit = 0.01 * PI / 180.0;
        for bin in [bin1, bin2] {
            match proj.angle_between(bin) {
                Ok(angle) => {
                    if angle < limit {
                        return Ok(true);
                    }
                }
                // SER in a SmBoolean function: the status reads as TRUE
                Err(err) if err.is_unported() => return Err(err),
                Err(_) => return Ok(true),
            }
        }
        // Make sNorm1 point into the solid region.
        if inf_this {
            norm1 = -norm1;
        }
        if proj.is_inside_sector(bin1, norm1, bin2) {
            return Ok(false);
        }
        Ok(true)
    }

    /// Whether `e` is a back-facing edge (`IsBackFacingEdge`).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] from the edge use binormals (their own callees); `InvalidInput` for topology upstream would
    /// dereference as null.
    pub fn is_back_facing_edge(&mut self, k: &Kernel, e: EdgeId) -> Result<bool> {
        // low work - edge already classified
        if self.sil_edges.contains(&e) {
            return Ok(true);
        }
        // Wire and Lamina edges do not count as back-facing.
        if edge::is_wire(&self.scene, e)? {
            return Ok(false);
        }
        if edge::is_lamina(&self.scene, e)? {
            return Ok(false);
        }

        let uses = self.prime_and_radial(e)?;
        let first = *uses.first().ok_or(undefined("edge without edge uses"))?;
        let first_mate =
            edgeuse::p1::mate(&self.scene, first)?.ok_or(undefined("edge use without mate"))?;
        // the scene edge's B-rep is the scene
        let inf = infinite_region(&self.scene);
        let r_first = self.region_of(first)?;
        let r_mate = self.region_of(first_mate)?;
        let prime = if r_first == inf && r_mate != inf {
            first
        } else if r_mate == inf && r_first != inf {
            first_mate
        } else {
            // neither or both edgeuses are in the infinite region
            return Ok(false);
        };

        let radial =
            edgeuse::p1::radial(&self.scene, prime)?.ok_or(undefined("edge use without radial"))?;
        let t = edge::interval(&self.scene, e)?.evaluate(0.5);
        // SE(..): a failed evaluation leaves the outputs uninitialised upstream
        let (bin1, tan1, norm1) = ignored(evaluate_binormal(k, &mut self.scene, prime, t))?
            .ok_or(undefined("uninitialised binormal"))?;
        let (bin2, _, norm2) = ignored(evaluate_binormal(k, &mut self.scene, radial, t))?
            .ok_or(undefined("uninitialised binormal"))?;

        // If both Normals point away, it's back-facing.
        if norm1.dot(self.view_vector) < 0.0 && norm2.dot(self.view_vector) < 0.0 {
            return Ok(true);
        }
        if norm1.dot(self.view_vector) * norm2.dot(self.view_vector) < EFF_ZERO_SQRT {
            let proj = tan1.cross(self.view_vector).cross(tan1);
            // Make sNorm1 point into the solid region
            let norm1 = -norm1;
            if proj.length_squared() < EFF_ZERO_SQ {
                return Ok(false);
            }
            if proj.is_inside_sector(bin1, norm1, bin2) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether the scene point is visible: a ray from in front of it along -Z hits no
    /// scene surface (`IsPointVisible`). Builds the ray tracer's surface set on the
    /// first call.
    ///
    /// # Errors
    ///
    /// Those of the scene surfaces; [`SolidError::Unported`] from the ray tracer.
    pub fn is_point_visible(&mut self, k: &Kernel, point: Point3) -> Result<bool> {
        if self.ray.is_none() {
            let surfaces = brep_surfaces(&self.scene)?;
            let num = surfaces.len();
            let size = if num < 100 {
                [10, 10, 10]
            } else if num < 1000 {
                [20, 20, 20]
            } else {
                [40, 40, 20]
            };
            let mut bbox = self.proj_bbox;
            let diag = bbox.size();
            bbox.expand_absolute(diag.length() / 100.0);
            let mut setup = RaySetup {
                bbox,
                size,
                approx_tol: self.approx_tol_3d,
                surfaces: Vec::new(),
            };
            let z_vector = Vec3::new(0.0, 0.0, -1.0);
            for f in surfaces {
                // skip surfaces parallel to viewing vector
                if self
                    .scene
                    .face(f)?
                    .surface
                    .as_ref()
                    .ok_or(undefined("face without surface"))?
                    .is_parallel_to_vector(z_vector)?
                {
                    continue;
                }
                let (domain, rectangular, natural) =
                    ignored(find_if_rectangular_trim(k, &mut self.scene, f))?.unwrap_or_default();
                let surface = self
                    .scene
                    .face(f)?
                    .surface
                    .as_ref()
                    .ok_or(undefined("face without surface"))?;
                let mut entry = RaySurface::Face(f);
                if rectangular {
                    if natural {
                        // Domain match, no trimming needed: just copy.
                        if let Some(copy) = ignored(surface.copy(k))? {
                            add_deleted_surface(&mut self.scene, &copy);
                            entry = RaySurface::Owned(Box::new(copy));
                        } else {
                            self.ray = Some(setup);
                            return Ok(false);
                        }
                    } else if let Some(bss) = b_spline_part(surface) {
                        let mut trimmed = bss.clone();
                        let mut domain = domain;
                        let _ = ignored(trimmed.trim_with_domain(&mut domain))?;
                        if let Some(copy) = ignored(trimmed.copy_and_add_analytics(k))? {
                            add_deleted_surface(&mut self.scene, &copy);
                            entry = RaySurface::Owned(Box::new(copy));
                        } else {
                            self.ray = Some(setup);
                            return Ok(false);
                        }
                    }
                }
                setup.surfaces.push(entry);
            }
            self.ray = Some(setup);
        }

        let setup = self.ray.as_ref().ok_or(undefined("ray tracer"))?;
        // The tracer borrows the surfaces, so it is rebuilt per ray: FireRay re-marks the
        // grid per call (Setup3DDDA), at O(surfaces) extra cost per ray. `Grid::new`
        // fails only for a zero voxel count, which the sizes above exclude (upstream
        // `new SmGrid` cannot fail).
        let grid = Grid::new(&setup.bbox, setup.size)?;
        let mut tracer = RayTracer::new(grid, setup.approx_tol, 1.0e-10);
        tracer.set_solver_tolerance(setup.approx_tol); // [B387]
        for s in &setup.surfaces {
            let surface = match *s {
                RaySurface::Face(f) => self
                    .scene
                    .face(f)?
                    .surface
                    .as_ref()
                    .ok_or(undefined("face without surface"))?,
                RaySurface::Owned(ref s) => &**s,
            };
            ignore_status(tracer.add_surface_to_grid(k, surface))?;
        }

        // create a ray segment [rayEnd RayStart], and rayVec = [0 0 -1]
        let mut ray_end = point;
        ray_end.z += self.ray_offset_tolerance;
        let ray_vec = Vec3::new(0.0, 0.0, -1.0);
        let max_distance = 2.0 * (self.proj_bbox.max().z - self.proj_bbox.min().z);
        let mut ray_start = ray_end;
        ray_start.z += max_distance;

        match tracer.fire_ray(ray_start, ray_vec, max_distance) {
            Ok(hit) => Ok(!hit.hits_something),
            Err(err) if err.is_unported() => Err(err),
            // signal an error and guess that the point is visible
            Err(_) => Ok(true),
        }
    }

    /// Splits `cc` at the projections along the view vector of the silhouette vertices
    /// and edges in front of its curve (`IntersectCurveWithSilhouettes`).
    ///
    /// # Errors
    ///
    /// Those of the curve box, the tree queries and the curve solves; `InvalidInput`
    /// for UB-170 (more silhouette vertices than edges in the curve's box) and for
    /// unresolved handles; [`SolidError::Unported`] where a vertex replaces another.
    pub fn intersect_curve_with_silhouettes(
        &self,
        k: &Kernel,
        cc: &mut CurveClassification<HcrObject>,
    ) -> Result<()> {
        let geom = Geometry { brep: &self.scene };
        let curve_handle = cc
            .curve()
            .ok_or(undefined("classification without curve"))?;
        let curve = geom
            .curve(curve_handle)
            .ok_or(undefined("classification curve not resolved"))?;
        let curve_zone = cc.src_zone_tol_3d();
        let ivl = cc.interval();
        let handle = CurveHandle(curve.natural_interval());

        // build bounding box for curve big enough to include all projected geometry
        let req = BoundingBoxRequest {
            normal: true,
            ..BoundingBoxRequest::default()
        };
        let mut e_box = curve
            .calculate_bounding_box(k, &ivl, req, true)?
            .normal
            .ok_or(undefined("curve box"))?;
        let mut corner = e_box.max();
        corner.z = self.proj_bbox.max().z;
        e_box.add_point(corner);
        e_box.expand_absolute(xsect_tol_3d_from_zones(k, curve_zone, curve_zone));

        // First do vertices.
        let vertices = self.sil_vertex_tree.objects_in_box(&e_box)?;
        let view = [self.view_vector];
        for v in &vertices {
            let v = v.ok_or(undefined("null silhouette vertex"))?;
            let vert_pnt = self.scene.vertex(v)?.point;
            let vert_zone = vertex::tolerance(&self.scene, v)?;
            let xsect = xsect_tol_3d_from_zones(k, vert_zone, curve_zone);
            let opts = crate::curve::curve::p1::GlobalPointSolveOptions {
                target_distance: Some(0.0),
                opt_vectors: Some(&view),
                handle: Some(handle),
                analytic_handle: None,
            };
            // Find projection of vertex on the curve along view vector.
            let sols = curve.global_point_solve(
                k,
                &ivl,
                SolverOperation::ProjectedMinimize,
                vert_pnt,
                xsect,
                SolutionRequested::All,
                &opts,
            )?;
            for sol in sols.as_slice() {
                let param = sol.start.parameters[0];
                // skip solutions outside current interval
                if !cc.interval().contains_value(param, 0.0) {
                    continue;
                }
                // skip solutions behind this curve
                let crv_pnt = curve.evaluate_point(param)?;
                if crv_pnt.z > vert_pnt.z + self.ray_offset_tolerance {
                    continue;
                }
                let mut replace = false;
                let found = ignored(cc.find_interval(k, &geom, param, BIG_DOUBLE, vert_zone))?;
                let (mut index, position) = match found {
                    Some(f) => (f.index, f.position),
                    None => (0, IntervalPosition::Outside),
                };
                // when vertex projected to existing curve interval boundary
                if position == IntervalPosition::Start || position == IntervalPosition::End {
                    if position == IntervalPosition::End {
                        index += 1;
                    }
                    let (pc, _) = cc
                        .point_class_by_index(index)
                        .map_err(|_| undefined("null point class"))?;
                    if pc.point_class() == PointClassType::Vertex {
                        let existing = ignored(pc.find_obj_point_3d(&geom))?
                            .ok_or(undefined("uninitialised vertex point"))?;
                        if existing.z >= vert_pnt.z {
                            continue; // Just ignore this one.
                        }
                        replace = true;
                    }
                }
                let mut pc = PointClassification::new(curve_zone);
                pc.set_class_object(PointClassType::Vertex, Some(HcrObject::Vertex(v)));
                pc.set_gap_3d(sol.start.solution_value);
                pc.set_src_zone_tol_3d(curve_zone);
                pc.set_pre_snap_param(sol.start.parameters[0]);
                if replace {
                    ignore_status(cc.update_point_class(k, &geom, index, &pc))?;
                } else {
                    ignore_status(cc.insert_point_class(k, &geom, &pc, param, BIG_DOUBLE, false))?;
                }
            }
        }

        // Now do Edges: the loop runs over the vertex count (UB-170).
        let edge_objects = self.sil_edge_tree.objects_in_box(&e_box)?;
        let mut sil_edges = Vec::new();
        for ii in 0..vertices.len() {
            let e = edge_objects.get(ii).copied().ok_or(undefined(
                "UB-170: silhouette edge index past the edges in the box",
            ))?;
            sil_edges.push(e);
        }
        for pe in sil_edges {
            let Some(pe) = pe else {
                continue;
            };
            let edge_zone = edge::tolerance(&self.scene, pe)?;
            let xsect = xsect_tol_3d_from_zones(k, curve_zone, edge_zone);
            // Don't intersect with self
            if HcrObject::Curve(pe) == curve_handle {
                continue;
            }
            let other =
                edge::curve(&self.scene, pe)?.ok_or(undefined("silhouette edge without curve"))?;
            let other_ivl = edge::interval(&self.scene, pe)?;
            let opts = crate::curve::curve::p9::CurveSolveOptions {
                target_distance: None,
                opt_vectors: Some(&view),
                handles: [Some(handle), Some(CurveHandle(other.natural_interval()))],
            };
            let sols = curve.global_curve_solve(
                k,
                &ivl,
                other,
                &other_ivl,
                SolverOperation::ProjectedIntersect,
                xsect,
                &opts,
                SolutionRequested::All,
            )?;
            for sol in sols.as_slice() {
                let ends = if sol.solution_type == SolutionType::RangeOfValues {
                    [Some(&sol.start), Some(&sol.end)]
                } else {
                    [Some(&sol.start), None]
                };
                for end in ends.into_iter().flatten() {
                    let p1 = curve.evaluate_point(end.parameters[0])?;
                    let p2 = other.evaluate_point(end.parameters[1])?;
                    // Don't use this intersection if it is behind the curve
                    if p1.z < p2.z + self.ray_offset_tolerance {
                        let mut pc = PointClassification::new(curve_zone);
                        pc.set_class_object(PointClassType::Edge, Some(HcrObject::Edge(pe)));
                        pc.set_gap_3d(end.solution_value);
                        pc.set_src_zone_tol_3d(curve_zone);
                        pc.set_pre_snap_param(end.parameters[0]);
                        pc.set_t_param(end.parameters[1], false);
                        if cc.interval().contains_value(end.parameters[0], 0.0) {
                            cc.insert_point_class(
                                k,
                                &geom,
                                &pc,
                                end.parameters[0],
                                BIG_DOUBLE,
                                false,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_transform_maps_view_to_z() {
        let t = view_transform_for_vector(Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let v = Vec3::new(1.0, 2.0, 3.0).unitize().unwrap();
        let z = t.transform_vector(v);
        assert!((z.z - 1.0).abs() < 1e-12);
        assert!(view_transform_for_vector(Vec3::new(0.0, 0.0, 0.0)).is_err());
    }

    #[test]
    fn from_scene_sets_tolerances() {
        let k = Kernel::default();
        let bbox = Extent3d::from_points(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0));
        let opts = HcrOptions {
            approx_tol_3d: 1e-3,
            ang_tol_rad: 0.1,
            display_seams: true,
            display_smooth_edges: true,
        };
        let h = Hcr::from_scene(&k, Brep::new(), &Axis2Placement::default(), bbox, opts);
        assert!((h.ray_offset_tolerance() - 0.004).abs() < 1e-15);
        assert_eq!(h.proj_bbox().max().z, 6.0);
        assert!(h.silhouette_edges().is_empty());
    }

    fn face_with_surface(b: &mut Brep, owner: Option<ObjectRef>) -> FaceId {
        let shell = b
            .add_shell_record(crate::topology::Shell::default())
            .unwrap();
        let f = b.add_face(shell).unwrap();
        let mut s = Surface::new(SurfaceKind::Plane(
            crate::surface::plane::Plane::from_normal(
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ),
        ));
        *s.owner_mut() = owner;
        b.face_mut(f).unwrap().surface = Some(s);
        f
    }

    #[test]
    fn new_curve_classification_converts_the_zone_tolerance() {
        use crate::geom::tol::{USE_DEFAULT, ZONE_TOL_3D};
        let k = Kernel::default();
        let make = |t: f64| {
            new_curve_classification(
                &k,
                HcrObject::Curve(EdgeId::from_index(0)),
                Extent1d::from_min_max(0.0, 1.0),
                t,
            )
        };
        // (given, source zone tolerance, tolerance of the interval's point classes)
        for (given, src, pts) in [
            (1e-4, 1e-4, 1e-4),
            (-1e-4, 1e-4, 1e-4),
            (USE_DEFAULT, ZONE_TOL_3D, ZONE_TOL_3D),
            (0.0, UNINIT_TOL, 0.0),
            (f64::NAN, UNINIT_TOL, f64::NAN),
        ] {
            let cc = make(given);
            assert_eq!(cc.src_zone_tol_3d().to_bits(), src.to_bits(), "{given}");
            let ivl = cc.at(0).unwrap();
            for pc in [&ivl.start, &ivl.mid, &ivl.end] {
                assert_eq!(pc.src_zone_tol_3d().to_bits(), pts.to_bits(), "{given}");
            }
        }
    }

    #[test]
    fn same_surface_is_object_identity() {
        let o = |n: u64| {
            Some(ObjectRef {
                type_code: 0,
                key: ObjectKey(n),
            })
        };
        let mut b = Brep::new();
        let shared1 = face_with_surface(&mut b, o(1));
        let shared2 = face_with_surface(&mut b, o(1));
        let other = face_with_surface(&mut b, o(2));
        let unowned1 = face_with_surface(&mut b, None);
        let unowned2 = face_with_surface(&mut b, None);
        assert!(same_surface(&b, shared1, shared2).unwrap());
        assert!(same_surface(&b, shared1, shared1).unwrap());
        // equal data (all are the default plane), different objects
        assert!(!same_surface(&b, shared1, other).unwrap());
        assert!(!same_surface(&b, unowned1, unowned2).unwrap());
        assert!(!same_surface(&b, unowned1, shared1).unwrap());
        assert!(same_surface(&b, unowned1, unowned1).unwrap());
    }
}
