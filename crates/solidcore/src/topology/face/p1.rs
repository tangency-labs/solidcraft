//! `SmFace`, part 1: construction and destruction, loop insertion and removal, the
//! surface swap and replacement, orientation flip, UV trim curve removal, the trim
//! boundary and UV domain built from the UV trim curves, the rectangular-trim test,
//! geometry shrinking, and the seam repair of faces on periodic surfaces (`MoveSeam`,
//! `SplitAtSeam`).
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/inc/SmFace.h`.
//!
//! | Rust                                         | Upstream                                     |
//! |----------------------------------------------|----------------------------------------------|
//! | [`crate::topology::Face::new`]               | `SmFace::SmFace`                             |
//! | [`destroy`]                                  | `SmFace::~SmFace`                            |
//! | [`calculate_uv_domain_from_uv_trim_curves`], [`UvDomainFromTrim`] | `SmFace::CalculateUVDomainFromUVTrimCurves` |
//! | [`create_oriented_trim_boundary`], [`OrientedTrimBoundary`] | `SmFace::CreateOrientedTrimBoundary` |
//! | [`find_if_rectangular_trim`], [`RectangularTrim`] | `SmFace::FindIfRectangularTrim`          |
//! | [`flip_face_orientation`]                    | `SmFace::FlipFaceOrientation`                |
//! | [`set_surface`]                              | `SmFace::SetSurface`                         |
//! | [`insert_loop`]                              | `SmFace::InsertLoop`                         |
//! | [`remove_loop`]                              | `SmFace::RemoveLoop`                         |
//! | [`remove_uv_trim_curves`]                    | `SmFace::RemoveUVTrimCurves`                 |
//! | [`swap_uv`]                                  | `SmFace::SwapUV`                             |
//! | [`move_seam`], [`MoveSeamOptions`], [`MoveSeamResult`], [`SeamProblem`] | `SmFace::MoveSeam` |
//! | [`split_at_seam`], [`SeamSplit`]             | `SmFace::SplitAtSeam`                        |
//! | [`shrink_geometry`]                          | `SmFace::ShrinkGeometry`                     |
//! | [`FixSeamPlanType`]                          | `SmFixSeamPlanType` (`SmTypes.h`)            |
//!
//! Deviations:
//! - **Object model.** A face is a record in the [`Brep`]'s arenas; the methods are free
//!   functions taking the B-rep and the face's id, out-parameters are return values.
//!   Where upstream dereferences null (a face without surface or face use, a loop
//!   without loop uses, an edge use without edge, an edge without curve, a loop use
//!   without face use) the port returns [`SolidError::InvalidInput`]. The identity a
//!   face, edge or edge use is known by (a surface's or curve's owner) comes from the
//!   caller's `key_of`, as [`crate::topology::edge::NotifyEnv`]'s.
//! - **The constructor** is [`crate::topology::Face::new`] (its `SM_NO_CONSTRUCTION`
//!   notification reaches only the object layer's user callback, which the port does
//!   not have). [`destroy`] stands for the destructor: arena records are never freed,
//!   so it notifies, unlinks the face use and drops a surface the face owns.
//! - **Side-effect free getters** whose result upstream only reads later
//!   (`GetUpwardLoopuses` of `CalculateUVDomainFromUVTrimCurves`, `GetBrep` and
//!   `SmTol::GetZoneTol3d` of `MoveSeam`, `GetBrep` of `SplitAtSeam`) are called where
//!   their result is first used; the unused `sThisDomain` intersection of
//!   `CalculateUVDomainFromUVTrimCurves` is not computed.
//! - **Pointer identity.** `SetSurface` compares the new surface pointer with the
//!   current one; a surface passed by value is never the current one, so the "same
//!   surface" no-op has no counterpart. Without `delete_current` the old surface is
//!   handed back (upstream leaves it to its other owners).
//! - **Loop use lists.** `SmFaceuse::PostInsert` / `Remove` (inline `SmOwningTopology`
//!   list edits) append to / remove from [`crate::topology::Faceuse::loopuses`] and set /
//!   clear the loop use's face use. `Remove` of a loop use not on the face use's list is
//!   `SM_ERR`, as upstream's owner check.
//! - **`CreateOrientedTrimBoundary`** returns the curves and per-loop counts in
//!   [`OrientedTrimBoundary`]. A failed `SmLine::CreateLineSegment` (only possible for
//!   coincident points, which the gap test excludes) is [`SolidError::InvalidInput`]
//!   (upstream adds an uninitialised pointer). `SM_CAST_PTR(SmBSplineCurve, ..)` of a
//!   3D edge curve of another kind is `SM_ERR_NULL_POINTER` (`NER`). The mirror of a
//!   line UV trim curve in `SwapUV` is the mirror of its NURB (as upstream's
//!   `MakeNurb` and `SmBSplineCurve` copy).
//! - **`MoveSeam` / `SplitAtSeam`** take the seam classifications and the optional face
//!   properties over any [`LoopObject`]; their output arrays are returned in
//!   [`MoveSeamResult`] / [`SeamSplit`]. Statuses upstream ignores (curve
//!   `EvaluateGeometric`, `DropPoint`, `DropVectors`, `ComputeUVPoint`, `GetKnots`,
//!   `SmPeriodicExtent1d::SetMinMax` / `AddInterval`, `CopyAndAddAnalytics`, the
//!   recursive `SplitAtSeam` / `HasSeamProblem` / `MergeIntervals` of `SplitAtSeam`) are
//!   ignored; [`SolidError::Unported`] always propagates. A failed drop leaves the UV
//!   point undefined (upstream's initial `rResultUV`). An empty vertex use list in the
//!   full-period branch of `MoveSeam` is [`SolidError::InvalidInput`] (upstream reads
//!   index 0 of an empty array), and the paired vertex use loop does not underflow on
//!   an empty pairing (upstream's unsigned `GetSize() - 1`). `SplitAt` / `JoinSurface`
//!   halves that are not plain B-spline surfaces (an assert upstream, then a C cast)
//!   are [`SolidError::InvalidInput`]. A failed `JoinSurface` (unchecked upstream,
//!   which then uses a null or stale joined surface: `plan/upstream-issues.md` UB-138)
//!   returns its error. The `ShellSort` of the vertex parameters is a sort by value
//!   (equal doubles are indistinguishable). `SplitAtSeam`'s V pass after a U merge
//!   recurses on this face with the classifications of each new face, as upstream
//!   (UB-137).
//! - **`bUseJGU`** (`static constexpr FALSE` in `SplitAtSeam`) branches are dead code
//!   upstream and are not ported. The `SmTemporaryChangeValue`s of `SplitAtSeam`
//!   (`m_bEditingEnabled`, `GetDoingBooleanRef`) are not set: they guard
//!   `HasSeamProblem` (unported) and `MergeIntervals`, whose port reads neither flag
//!   (its topology edits defer to `intersect.curve_class_p2`).
//! - **`MergeIntervals`** returns handles; the port maps its new faces, edges and
//!   vertices to ids with the caller's `resolve` and returns
//!   [`SolidError::InvalidInput`] for a handle that does not resolve to an entity of
//!   the expected kind (upstream's lists hold the pointers directly). Its own status
//!   is ignored, as upstream.
//! - **`IsCrossedByEdge` / `HasNearMisses`** return a plain boolean upstream; a
//!   failure of the port other than [`SolidError::Unported`] (or, for
//!   `HasNearMisses`, [`SolidError::InvalidInput`] for a handle `geometry` cannot
//!   resolve) reads as `FALSE`.
//! - **Unported callees** return [`SolidError::Unported`]: `SmFace::Notify` and
//!   `CreateUVTrimCurves` (`topology.face_p3`); `SmFace::GetUpwardLoopuses`,
//!   `GetUpwardVertexuses`, `GetEdges`, `GetBrep` and `HasSeamProblem`
//!   (`topology.face_p4`); `SmEdgeuse::NormalizedEvaluate` and
//!   `GetOrCreateUVTrimCurve` (`topology.edgeuse_p3`); `SmBrep::ReplaceSurface`
//!   (`topology.brep_p2`).
//! - **Bounds.** `SplitAtSeam` recurses into the faces its U merge made, once per face
//!   (upstream asserts the recursion only fixes V seams); the port carries a depth and
//!   returns [`SolidError::InvalidInput`] past [`MAX_SPLIT_AT_SEAM_DEPTH`]. Every other
//!   loop is over B-rep arrays, knots or a fixed sample count.

use core::cmp::Ordering;

use crate::curve::curve::p2::evaluate_geometric;
use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::p2::BSPLINE_CURVE_TYPE;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::{Extent1d, ExtentPointType};
use crate::geom::extent2d::Extent2d;
use crate::geom::object::ObjectRef;
use crate::geom::periodic_extent1d::PeriodicExtent1d;
use crate::geom::tol::are_same_to_tol;
use crate::geom::vector2d::{Point2, Vec2};
use crate::heal::face_props::{FaceProps, LoopObject, Tri};
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::point_class::{ClassGeometry, PointClassType};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_PARAM};
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::knots::Side;
use crate::surface::surface::p2::B_SPLINE_SURFACE_TYPE;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::p7::drop_vectors_at;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as fp2;
use crate::topology::loopuse as lu1;
use crate::topology::trimming_tools as tt;
use crate::topology::vertex as vx;
use crate::topology::vertexuse as vu1;
use crate::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, VertexId, VertexuseId,
};

const OP: &str = "topology::face::p1";

/// The deepest [`split_at_seam`] recursion accepted (upstream recurses once, from a U
/// merge into the V merges of the faces it made).
pub const MAX_SPLIT_AT_SEAM_DEPTH: usize = 8;

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `SmFixSeamPlanType`: what [`move_seam`] did to a seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FixSeamPlanType {
    /// `SM_FSP_UNKNOWN`: not set.
    #[default]
    Unknown = 0,
    /// `SM_FSP_NO_CHANGES`: the face has no seam problem in this direction.
    NoChanges = 1,
    /// `SM_FSP_MOVE_OUT_OF_FACE`: the seam was moved out of the face.
    MoveOutOfFace = 2,
    /// `SM_FSP_MOVE_IN_FACE`: the seam was moved into the face (to be split there).
    MoveInFace = 3,
    /// `SM_FSP_SPLIT_FACE`: the face was split at the seam.
    SplitFace = 4,
}

// ---- unported callees ---------------------------------------------------------------------

/// `SmFace::Notify(op, ..)` (any edit notification).
fn face_notify(_b: &mut Brep, _f: FaceId) -> Result<()> {
    // UNPORTED(topology.face_p3): SmFace::Notify
    Err(SolidError::unported("SmFace::Notify", "topology.face_p3"))
}

/// `SmFace::CreateUVTrimCurves(FALSE, NULL, NULL, ..)`.
fn create_uv_trim_curves(_k: &Kernel, _b: &mut Brep, _f: FaceId) -> Result<()> {
    // UNPORTED(topology.face_p3): SmFace::CreateUVTrimCurves
    Err(SolidError::unported(
        "SmFace::CreateUVTrimCurves",
        "topology.face_p3",
    ))
}

/// `SmFace::GetUpwardLoopuses(rLoopuses)` (default arguments).
fn upward_loopuses(_b: &Brep, _f: FaceId) -> Result<Vec<LoopuseId>> {
    // UNPORTED(topology.face_p4): SmFace::GetUpwardLoopuses
    Err(SolidError::unported(
        "SmFace::GetUpwardLoopuses",
        "topology.face_p4",
    ))
}

/// `SmFace::GetUpwardVertexuses(rVertexuses)` (default arguments).
fn upward_vertexuses(_b: &Brep, _f: FaceId) -> Result<Vec<VertexuseId>> {
    // UNPORTED(topology.face_p4): SmFace::GetUpwardVertexuses
    Err(SolidError::unported(
        "SmFace::GetUpwardVertexuses",
        "topology.face_p4",
    ))
}

/// `SmFace::GetEdges(rEdges)` (default arguments).
fn face_edges(_b: &Brep, _f: FaceId) -> Result<Vec<EdgeId>> {
    // UNPORTED(topology.face_p4): SmFace::GetEdges
    Err(SolidError::unported("SmFace::GetEdges", "topology.face_p4"))
}

/// `SmFace::GetBrep() != NULL`.
fn has_brep(_b: &Brep, _f: FaceId) -> Result<bool> {
    // UNPORTED(topology.face_p4): SmFace::GetBrep
    Err(SolidError::unported("SmFace::GetBrep", "topology.face_p4"))
}

/// The seam problem of a face (`HasSeamProblem`'s enum outputs; `SM_SP_*` values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeamProblem {
    /// `eCrossedSeam`: the seams face edges cross.
    pub crossed: SurfParamType,
    /// `eMissingSeam`: the seams no seam edge represents.
    pub missing: SurfParamType,
    /// `eNearMissSeam`: the seams face edges nearly touch.
    pub near_miss: SurfParamType,
}

/// `SmFace::HasSeamProblem(rCrvClassU, rCrvClassV, eCrossed, eMissing, eNearMiss,
/// bTgtLoopsOnly)`: rebuilds the classifications and the problem.
fn has_seam_problem<O: LoopObject>(
    _k: &Kernel,
    _b: &mut Brep,
    _f: FaceId,
    _crv_class_u: &mut CurveClassification<O>,
    _crv_class_v: &mut CurveClassification<O>,
    _problem: &mut SeamProblem,
    _tgt_loops_only: bool,
) -> Result<bool> {
    // UNPORTED(topology.face_p4): SmFace::HasSeamProblem
    Err(SolidError::unported(
        "SmFace::HasSeamProblem",
        "topology.face_p4",
    ))
}

/// `pEU->NormalizedEvaluate(dT, TRUE, rUV)` (UV evaluation).
fn normalized_evaluate_uv(_b: &Brep, _eu: EdgeuseId, _t: f64) -> Result<Point3> {
    // UNPORTED(topology.edgeuse_p3): SmEdgeuse::NormalizedEvaluate
    Err(SolidError::unported(
        "SmEdgeuse::NormalizedEvaluate",
        "topology.edgeuse_p3",
    ))
}

/// `pEU->GetOrCreateUVTrimCurve(rpUVCurve)`: the edge use's UV trim curve (made when
/// missing).
fn get_or_create_uv_trim_curve(_b: &mut Brep, _eu: EdgeuseId) -> Result<Curve> {
    // UNPORTED(topology.edgeuse_p3): SmEdgeuse::GetOrCreateUVTrimCurve
    Err(SolidError::unported(
        "SmEdgeuse::GetOrCreateUVTrimCurve",
        "topology.edgeuse_p3",
    ))
}

/// `rCrvClass.IsCrossedByEdge()`: upstream returns a plain boolean, so a failure other
/// than [`SolidError::Unported`] reads as `FALSE`.
fn is_crossed_by_edge<O: LoopObject>(c: &CurveClassification<O>) -> Result<bool> {
    Ok(ignored(c.is_crossed_by_edge())?.unwrap_or(false))
}

/// `rCrvClass.HasNearMisses()` (default `bInSurfaceGaps = FALSE`): upstream returns a
/// plain boolean, so a failure reads as `FALSE`, except an unresolvable caller handle
/// ([`SolidError::InvalidInput`]) and [`SolidError::Unported`], which propagate.
fn has_near_misses<O: LoopObject>(
    k: &Kernel,
    c: &CurveClassification<O>,
    geometry: &impl ClassGeometry<O>,
) -> Result<bool> {
    match c.has_near_misses(k, geometry, false) {
        Ok(v) => Ok(v),
        Err(e) if e.is_unported() || matches!(e, SolidError::InvalidInput { .. }) => Err(e),
        Err(_) => Ok(false),
    }
}

/// `pCrvClass->MergeIntervals(&faces, &edges, &vertices, this)` (unit
/// `intersect.curve_class_p5`; the original face is a debug-only argument there): the
/// faces, edges and vertices the merge made, resolved to ids by `resolve`.
fn merge_intervals<O: LoopObject>(
    c: &mut CurveClassification<O>,
    resolve: &dyn Fn(O) -> Option<EntityRef>,
) -> Result<SeamSplit> {
    // (the merge's status is ignored, as upstream; an output the caller cannot resolve
    // is caller error and propagates)
    let Some(merged) = ignored(c.merge_intervals(false))? else {
        return Ok(SeamSplit::default());
    };
    let mut out = SeamSplit::default();
    for o in merged.faces {
        match resolve(o) {
            Some(EntityRef::Face(f)) => out.faces.push(f),
            _ => return Err(null("MergeIntervals made a face the caller cannot resolve")),
        }
    }
    for o in merged.edges {
        match resolve(o) {
            Some(EntityRef::Edge(e)) => out.edges.push(e),
            _ => {
                return Err(null(
                    "MergeIntervals made an edge the caller cannot resolve",
                ))
            }
        }
    }
    for o in merged.vertices {
        match resolve(o) {
            Some(EntityRef::Vertex(v)) => out.vertices.push(v),
            _ => {
                return Err(null(
                    "MergeIntervals made a vertex the caller cannot resolve",
                ))
            }
        }
    }
    Ok(out)
}

/// `pBrep->ReplaceSurface(this, pNewSurface, bAddToDeleteSurfaceList, rOptModifiedFaces,
/// bOptCreateUVTrimCurves)`; `MoveSeam` passes `FALSE` (delete the old surface here),
/// `NULL` (no modified-face list) and `FALSE` (no new UV trim curves).
fn replace_surface(
    _b: &mut Brep,
    _f: FaceId,
    _new_surface: Surface,
    _add_to_delete_surface_list: bool,
    _want_modified_faces: bool,
    _create_uv_trim_curves: bool,
) -> Result<()> {
    // UNPORTED(topology.brep_p2): SmBrep::ReplaceSurface
    Err(SolidError::unported(
        "SmBrep::ReplaceSurface",
        "topology.brep_p2",
    ))
}

// ---- helpers ------------------------------------------------------------------------------

/// The surface of `f`, which upstream dereferences.
fn surface_deref(b: &Brep, f: FaceId) -> Result<&Surface> {
    fp2::surface(b, f)?.ok_or_else(|| null("face without surface"))
}

/// Component `i` (`0` u, else v) of a UV point (`SmPoint2d::operator[]`).
const fn comp(p: Point2, i: usize) -> f64 {
    if i == 0 {
        p.x
    } else {
        p.y
    }
}

/// The edge of `eu`, which upstream dereferences.
fn edge_deref(b: &Brep, eu: EdgeuseId) -> Result<EdgeId> {
    eu1::edge(b, eu)?.ok_or_else(|| null("edge use without edge"))
}

/// The normal bounding box of `curve` over its natural interval
/// (`CalculateBoundingBox(GetNaturalInterval(), &box)`), as a UV extent.
fn uv_curve_box(k: &Kernel, curve: &Curve) -> Result<crate::geom::extent3d::Extent3d> {
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    curve
        .calculate_bounding_box(k, &curve.natural_interval(), req, true)?
        .normal
        .ok_or_else(|| null("bounding box not computed"))
}

// ---- destruction --------------------------------------------------------------------------

/// `~SmFace`: notifies the destruction, clears the face use and drops the surface when
/// the face (`this`) owns it.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::Unported`] for the
/// notification (`SmFace::Notify`, `topology.face_p3`).
pub fn destroy(b: &mut Brep, f: FaceId, this: ObjectRef) -> Result<()> {
    // Notify DESTRUCTION behavior
    face_notify(b, f)?;
    let face = b.face_mut(f)?;
    face.faceuse = None;

    // If there is a surface and I own it then delete it.
    if face
        .surface
        .as_ref()
        .is_some_and(|s| s.owner() == Some(this))
    {
        face.surface = None;
    }
    Ok(())
}

// ---- UV domain from the UV trim curves ----------------------------------------------------

/// The outputs of [`calculate_uv_domain_from_uv_trim_curves`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UvDomainFromTrim {
    /// `rNewFaceDomain`: the union of the UV trim curves' boxes (limited to the
    /// surface's natural domain when asked).
    pub domain: Extent2d,
    /// `*pOptMaxUVLoopGap`: the largest start-point UV loop gap of the loops (`0` when
    /// not asked for).
    pub max_uv_loop_gap: f64,
    /// `*pOpt2ndMaxUVLoopGap`: the largest second-largest gap of a loop (`0` when not
    /// asked for, or when the largest gap was not asked for).
    pub second_max_uv_loop_gap: f64,
}

/// `CalculateUVDomainFromUVTrimCurves`: makes the face's UV trim curves and returns the
/// union of their bounding boxes, intersected with the surface's natural domain when
/// `limit_domain` (the natural domain itself without edge uses). A face without surface,
/// or whose UV trim curves cannot be made, keeps its UV domain. `want_max_gap` /
/// `want_2nd_max_gap` ask for the loop gap outputs (`pOptMaxUVLoopGap`,
/// `pOpt2ndMaxUVLoopGap`).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an edge
/// use without UV trim curve; those of the bounding boxes, the loop gaps and the domain
/// intersection (`SM_ERR_INVALID_INPUT` for disjoint domains); [`SolidError::Unported`]
/// (`CreateUVTrimCurves`, `GetUpwardLoopuses`).
pub fn calculate_uv_domain_from_uv_trim_curves(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    limit_domain: bool,
    want_max_gap: bool,
    want_2nd_max_gap: bool,
) -> Result<UvDomainFromTrim> {
    // Init output
    let mut out = UvDomainFromTrim {
        domain: fp2::uv_domain(b, f)?,
        max_uv_loop_gap: 0.0,
        second_max_uv_loop_gap: 0.0,
    };

    // Check State - no Face->Surface
    let Some(surface) = fp2::surface(b, f)? else {
        return Ok(out);
    };
    let surf_domain = surface.natural_uv_domain();

    // place a UVTrimCurve on each Face->Edgeuse
    match create_uv_trim_curves(k, b, f) {
        Ok(()) => {}
        Err(e) if e.is_unported() => return Err(e),
        // Return the default from above.
        Err(_) => return Ok(out),
    }
    let loopuses = upward_loopuses(b, f)?;

    // Build union of all face->Edgeuse UVBoundingBoxes
    // Save Loop Edgeuse_to_Edgeuse End_to_End gaps
    let mut face_domain = Extent2d::new();
    let mut box_set = false;
    for (ii, &lu) in loopuses.iter().enumerate() {
        let edgeuses = eu1::loopuse_edgeuses(b, lu)?;
        let mut this_max = 0.0;
        let mut this_2nd_max = 0.0;

        for (jj, &eu) in edgeuses.iter().enumerate() {
            let uv_box = {
                let curve = eu1::uv_trim_curve(b, eu)?
                    .0
                    .ok_or_else(|| null("edge use without UV trim curve"))?;
                uv_curve_box(k, curve)?
            };

            // Get Max Edgeuse_to_Edgeuse 2d UVSpace gap
            if want_max_gap {
                let this_gap = eu1::start_point_uv_loop_gap(k, b, eu)?;
                if this_max < this_gap {
                    this_2nd_max = this_max;
                    this_max = this_gap;
                }
            }

            // convert 3d Extent to 2d
            let uv_crv_box = Extent2d::from_extent3d(&uv_box);

            // get union of Edgeuse UVTrimCurve BoundingBoxes
            face_domain = if ii == 0 && jj == 0 {
                uv_crv_box
            } else {
                face_domain.union(&uv_crv_box)
            };
            box_set = true;
        }

        if want_max_gap && out.max_uv_loop_gap < this_max {
            out.max_uv_loop_gap = this_max;
        }
        if want_2nd_max_gap && out.second_max_uv_loop_gap < this_2nd_max {
            out.second_max_uv_loop_gap = this_2nd_max;
        }
    }

    // If we are limiting the size of the return domain,
    if limit_domain {
        face_domain = if box_set {
            face_domain.intersect(&surf_domain)?
        } else {
            surf_domain
        };
    }

    // all done - set output and return
    out.domain = face_domain;
    Ok(out)
}

// ---- oriented trim boundary ---------------------------------------------------------------

/// The outputs of [`create_oriented_trim_boundary`].
#[derive(Debug, Clone)]
pub struct OrientedTrimBoundary {
    /// `rLoops`: the number of curves of each (edge) loop.
    pub loops: Vec<usize>,
    /// `rTrimBoundary`: the curves, loop by loop (counter-clockwise for the first loop,
    /// clockwise for the others).
    pub curves: Vec<Curve>,
}

/// `CreateOrientedTrimBoundary`: copies of the face's boundary curves in loop order,
/// oriented so the face lies on their left: its UV trim curves (2D) when
/// `parameter_space`, else its trimmed 3D edge curves. With `parameter_space` and
/// `fill_singularities`, UV lines close the gaps between consecutive curves (surface
/// singularities). Only `loop_to_use` is walked when given. Missing UV trim curves are
/// made first. A copied UV trim curve is owned by its edge use (`key_of`).
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for a loop of another face; `SM_ERR_NULL_POINTER` for a 3D
/// edge curve that is not a B-spline kind; [`SolidError::InvalidInput`] where upstream
/// dereferences null; those of the copies, conversions, reversals and evaluations;
/// [`SolidError::Unported`] (`CreateUVTrimCurves`, `NormalizedEvaluate`,
/// `GetOrCreateUVTrimCurve`).
#[allow(clippy::too_many_lines)] // one upstream routine
pub fn create_oriented_trim_boundary(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    parameter_space: bool,
    fill_singularities: bool,
    loop_to_use: Option<LoopId>,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<OrientedTrimBoundary> {
    if let Some(l) = loop_to_use {
        if lu1::loop_face(b, l)? != Some(f) {
            return Err(SolidError::invalid_input(OP, "loop is not on this face"));
        }
    }

    let have_trimming_curves = match loop_to_use {
        Some(l) => tt::loop_uv_curves_present(b, l)?,
        None => tt::face_uv_curves_present(b, f)?,
    };

    // Create trimCurves for all Face->Edgeuses
    if !have_trimming_curves {
        create_uv_trim_curves(k, b, f)?;
    }

    let mut out = OrientedTrimBoundary {
        loops: Vec::new(),
        curves: Vec::new(),
    };

    // get face's loop-use array
    let loopuses = if let Some(l) = loop_to_use {
        vec![lu1::loop_loopuses(b, l)?.0]
    } else {
        let fu = fp2::upward_faceuse(b, f)?.ok_or_else(|| null("face without face use"))?;
        eu1::faceuse_loopuses(b, fu, false, None)?
    };

    // for every loop
    for lu in loopuses {
        // no work - lone vertex
        if lu1::is_vertex_loopuse(b, lu)? {
            continue;
        }

        // get array of loop's Edgeuses
        let edgeuses = eu1::loopuse_edgeuses(b, lu)?;
        let Some(&last_eu) = edgeuses.last() else {
            continue;
        };

        // the last edge's end-point (status ignored)
        let mut last_pnt = Vec3::UNDEF;
        if let Some(p) = ignored(normalized_evaluate_uv(b, last_eu, 1.0))? {
            last_pnt = p;
        }

        let mut edges_in_loop = 0usize;

        // for every edgeuse in this loop
        for &eu in &edgeuses {
            if eu1::orientation(b, eu)? == OrientType::Same {
                let uv_curve = get_or_create_uv_trim_curve(b, eu)?;
                let ivl = uv_curve.natural_interval();

                // get trim curve's start UV point
                let pnt = uv_curve.evaluate_point(ivl.min())?;

                // get dist from this start pt to last curve's end-point
                let diff = pnt - last_pnt;
                let scaled_zero = EFF_ZERO * (1.0 + pnt.max_dimension());
                if diff.length_squared() > scaled_zero && parameter_space && fill_singularities {
                    // If there is a gap create a line to fill it
                    edges_in_loop += 1;
                    out.curves.push(gap_line(last_pnt, pnt)?);
                }

                // Get UVCurve->End UVPoint
                last_pnt = uv_curve.evaluate_point(ivl.max())?;
                edges_in_loop += 1;
                if parameter_space {
                    // (status ignored upstream; only NER(pOrig))
                    let orig = ignored(get_or_create_uv_trim_curve(b, eu))?
                        .ok_or_else(|| SolidError::null_pointer(OP))?;
                    let mut bsc = orig.copy(k)?;
                    bsc.convert_to_2d(k)?;
                    let _ = bsc.set_owner(Some(key_of(EntityRef::Edgeuse(eu))), None);
                    out.curves.push(bsc);
                } else {
                    let e = edge_deref(b, eu)?;
                    out.curves.push(trimmed_edge_curve(k, b, e)?);
                }
            } else {
                let mut bsc = if parameter_space {
                    let orig = get_or_create_uv_trim_curve(b, eu)?;
                    let mut bsc = orig.copy(k)?;
                    bsc.convert_to_2d(k)?;
                    let _ = bsc.set_owner(orig.owner(), None);
                    bsc
                } else {
                    let e = edge_deref(b, eu)?;
                    let owner = edge_bspline_curve(b, e)?.owner();
                    let mut bsc = trimmed_edge_curve(k, b, e)?;
                    let _ = bsc.set_owner(owner, None);
                    bsc
                };

                let ivl = bsc.natural_interval();
                let ivl = bsc.reverse_parameterization(k, &ivl)?;
                let pnt = bsc.evaluate_point(ivl.min())?;
                let diff = pnt - last_pnt;
                let scaled_zero = EFF_ZERO * (1.0 + pnt.max_dimension());
                if diff.length_squared() > scaled_zero && parameter_space && fill_singularities {
                    // If there is a gap create a line to fill it
                    edges_in_loop += 1;
                    out.curves.push(gap_line(last_pnt, pnt)?);
                }
                last_pnt = bsc.evaluate_point(ivl.max())?;
                edges_in_loop += 1;
                out.curves.push(bsc);
            }
        }

        out.loops.push(edges_in_loop);
    }

    Ok(out)
}

/// `SmLine::CreateLineSegment(ctx, 2, start, end, pLine)`.
fn gap_line(start: Point3, end: Point3) -> Result<Curve> {
    Line::create_line_segment(2, start, end, None)
        .map(Curve::from)
        .map_err(|_| null("CreateLineSegment failed (upstream adds an uninitialised curve)"))
}

/// `SM_CAST_PTR(SmBSplineCurve, pEdge->GetCurve())`, `NER` on null.
fn edge_bspline_curve(b: &Brep, e: EdgeId) -> Result<&Curve> {
    match b.edge(e)?.curve.as_ref() {
        Some(c) if c.is_kind_of(BSPLINE_CURVE_TYPE) => Ok(c),
        _ => Err(SolidError::null_pointer(OP)),
    }
}

/// A copy of edge `e`'s B-spline curve trimmed to the edge interval.
fn trimmed_edge_curve(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Curve> {
    let orig = edge_bspline_curve(b, e)?;
    let mut bsc = orig.copy(k)?;
    let mut edge_ivl = b.edge(e)?.interval;
    bsc.trim(k, &mut edge_ivl, TrimMode::Notify)?;
    Ok(bsc)
}

// ---- rectangular trim ---------------------------------------------------------------------

/// The outputs of [`find_if_rectangular_trim`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectangularTrim {
    /// `rUVDomain`: the rectangle of the four iso UV trim curves (clamped to the natural
    /// domain); the "inside out" [`Extent2d::new`] when not rectangular.
    pub uv_domain: Extent2d,
    /// `rbRectangularTrim`: one outer loop of four iso UV trim curves.
    pub rectangular: bool,
    /// `rbNaturalTrim`: that rectangle is the surface's natural domain.
    pub natural: bool,
}

/// `FindIfRectangularTrim`: whether the face is bounded by one loop of four iso UV
/// trim curves (after filling pole gaps), and that rectangle.
///
/// # Errors
///
/// `SM_ERR` when the trim boundary cannot be made (its error is not passed on);
/// [`SolidError::InvalidInput`] for a face without surface; those of
/// [`fp2::loops`], [`lu1::loop_edges`] and the bounding boxes;
/// [`SolidError::Unported`] from [`create_oriented_trim_boundary`].
pub fn find_if_rectangular_trim(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<RectangularTrim> {
    // init return value
    let mut out = RectangularTrim {
        uv_domain: Extent2d::new(),
        rectangular: false,
        natural: false,
    };

    // naturally trimmed faces may only have 1 loop
    let face_loops = fp2::loops(b, f, false, None)?;
    let [l] = face_loops.as_slice() else {
        return Ok(out);
    };

    // naturally trimmed face outer loops with no poles have 4 edges, with poles 3 or 2.
    let n_edges = lu1::loop_edges(b, *l)?.len();
    if !(2..=4).contains(&n_edges) {
        return Ok(out);
    }

    let boundary = match create_oriented_trim_boundary(k, b, f, true, true, None, key_of) {
        Ok(r) => r,
        Err(e) if e.is_unported() => return Err(e),
        Err(_) => return Err(SolidError::sm_err(OP)),
    };

    // No work - more than 1 loop or more than 4 edges.
    if boundary.loops.len() != 1 || boundary.curves.len() != 4 {
        return Ok(out);
    }

    // for every UVTrimCurve - building a union of each UVTrimCurve's bounding box
    let mut uv_bbox = crate::geom::extent3d::Extent3d::new();
    for (ii, crv) in boundary.curves.iter().enumerate() {
        let crv_bbox = uv_curve_box(k, crv)?;
        uv_bbox = if ii == 0 {
            crv_bbox
        } else {
            uv_bbox.union(&crv_bbox)
        };

        // When any edge is not an iso-parameter line - quit.
        let size = crv_bbox.size();
        if size.x > EFF_ZERO && size.y > EFF_ZERO {
            return Ok(out);
        }
    }

    let min = Point2::new(uv_bbox.min().x, uv_bbox.min().y);
    let max = Point2::new(uv_bbox.max().x, uv_bbox.max().y);
    let nat_domain = surface_deref(b, f)?.natural_uv_domain();

    // set output (SetMinMax's status ignored)
    let _ = out
        .uv_domain
        .set_min_max(nat_domain.clamp_point(min), nat_domain.clamp_point(max));
    out.rectangular = true;
    out.natural = out.uv_domain.max_boundary_dist(&nat_domain) < EFF_ZERO_PARAM;
    Ok(out)
}

// ---- orientation, surface and loop edits --------------------------------------------------

/// `FlipFaceOrientation`: makes each loop's other loop use primary and the face's other
/// face use upward (the face becomes invalid if it was valid, and vice versa).
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without face use, mate, or loop uses (`NER`); those of
/// [`fp2::loops`]; [`SolidError::Unported`] (`SmFace::Notify`).
pub fn flip_face_orientation(b: &mut Brep, f: FaceId) -> Result<()> {
    face_notify(b, f)?;
    let top = fp2::upward_faceuse(b, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let bot = eu1::faceuse_mate(b, top)?.ok_or_else(|| SolidError::null_pointer(OP))?;

    // Flip ordering of loopuses within the loops
    for l in fp2::loops(b, f, false, None)? {
        let lu1_ = b
            .loop_(l)?
            .loopuse
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let lu2_ = b
            .loopuse(lu1_)?
            .mate
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        b.loop_mut(l)?.loopuse = Some(lu2_);
    }

    // Flip ordering of faceuses within the face and their orientation.
    b.face_mut(f)?.faceuse = Some(bot);
    b.faceuse_mut(top)?.orientation = OrientType::Opposite;
    b.faceuse_mut(bot)?.orientation = OrientType::Same;

    face_notify(b, f)?;
    Ok(())
}

/// `SetSurface`: gives the face `surface` (owned by the face, `this`) and its natural
/// domain. The previous surface is dropped with `delete_current`, else returned.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::Unported`]
/// (`SmFace::Notify`).
pub fn set_surface(
    b: &mut Brep,
    f: FaceId,
    this: ObjectRef,
    mut surface: Surface,
    delete_current: bool,
) -> Result<Option<Surface>> {
    // inform the public
    face_notify(b, f)?;

    // when asked - delete current surface
    let face = b.face_mut(f)?;
    let mut old = face.surface.take();
    if delete_current {
        old = None;
    }

    // update the Face domain, change the surface owner, save the surface
    face.uv_domain = surface.natural_uv_domain();
    surface.set_owner(Some(this));
    face.surface = Some(surface);
    Ok(old)
}

/// `SmFaceuse::PostInsert(lu)`: appends `lu` to `fu`'s loop uses.
fn faceuse_post_insert(b: &mut Brep, fu: FaceuseId, lu: LoopuseId) -> Result<()> {
    b.loopuse(lu)?;
    b.faceuse_mut(fu)?.loopuses.push(lu);
    b.loopuse_mut(lu)?.faceuse = Some(fu);
    Ok(())
}

/// `SmFaceuse::Remove(lu)`: unlinks `lu` from `fu`'s loop uses (`SM_ERR` when it is
/// not there).
fn faceuse_remove(b: &mut Brep, fu: FaceuseId, lu: LoopuseId) -> Result<()> {
    if b.loopuse(lu)?.faceuse != Some(fu) {
        return Err(SolidError::sm_err(OP));
    }
    let list = &mut b.faceuse_mut(fu)?.loopuses;
    let Some(at) = list.iter().position(|&x| x == lu) else {
        return Err(SolidError::sm_err(OP));
    };
    list.remove(at);
    b.loopuse_mut(lu)?.faceuse = None;
    Ok(())
}

/// `InsertLoop`: appends the loop's two loop uses to the face's two face uses (so an
/// outer loop can only be inserted into a face without loops).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a loop without both loop uses or a face without
/// both face uses (upstream dereferences null); [`SolidError::Unported`]
/// (`SmFace::Notify`).
pub fn insert_loop(b: &mut Brep, f: FaceId, l: LoopId) -> Result<()> {
    face_notify(b, f)?;
    let (lu_1, lu_2) = lu1::loop_loopuses(b, l)?;
    let lu_2 = lu_2.ok_or_else(|| null("loop without second loop use"))?;
    let (fu_1, fu_2) = fp2::faceuses(b, f)?;
    let fu_1 = fu_1.ok_or_else(|| null("face without face use"))?;
    let fu_2 = fu_2.ok_or_else(|| null("face use without mate"))?;
    faceuse_post_insert(b, fu_1, lu_1)?;
    faceuse_post_insert(b, fu_2, lu_2)?;
    face_notify(b, f)?;
    Ok(())
}

/// `RemoveLoop`: unlinks the loop's two loop uses from their face uses; nothing is
/// deleted.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` for a loop use without face use (`NER`); `SM_ERR` from the
/// list removal; [`SolidError::InvalidInput`] for a loop without both loop uses;
/// [`SolidError::Unported`] (`SmFace::Notify`).
pub fn remove_loop(b: &mut Brep, f: FaceId, l: LoopId) -> Result<()> {
    face_notify(b, f)?;
    let (lu_1, lu_2) = lu1::loop_loopuses(b, l)?;
    let lu_2 = lu_2.ok_or_else(|| null("loop without second loop use"))?;
    let fu_1 = lu1::faceuse(b, lu_1)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let fu_2 = lu1::faceuse(b, lu_2)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    faceuse_remove(b, fu_1, lu_1)?;
    faceuse_remove(b, fu_2, lu_2)?;
    face_notify(b, f)?;
    Ok(())
}

/// `RemoveUVTrimCurves`: deletes the UV trim curves of the upward face use's edge uses
/// (which also resets their cached gaps).
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without face use; those of [`eu1::faceuse_edgeuses`];
/// [`SolidError::Unported`] (`SmFace::Notify`).
pub fn remove_uv_trim_curves(b: &mut Brep, f: FaceId) -> Result<()> {
    face_notify(b, f)?;
    let fu = fp2::upward_faceuse(b, f)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    for eu in eu1::faceuse_edgeuses(b, fu, false, None)? {
        // status ignored upstream
        ignore_status(eu1::set_uv_trim_curve(
            b,
            eu,
            eu1::UvTrimCurveArg::Null,
            0.0,
            true,
            None,
        ))?;
    }
    Ok(())
}

/// `SwapUV`: transposes the face's UV domain, mirrors every UV trim curve about the
/// `u = v` line, swaps the surface's `u` and `v` and makes the other face use and loop
/// uses primary (reversing the face).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface or face use, a loop use
/// without loop or mate, an edge use without mate; those of the mirror and surface
/// swap; [`SolidError::Unported`] (`GetBrep`, `GetUpwardLoopuses`, `SmFace::Notify`).
pub fn swap_uv(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<()> {
    // GetBrep()->GetContext()
    has_brep(b, f)?;
    surface_deref(b, f)?;
    let x_axis = Vec3::new(1.0, 1.0, 0.0).unitize()?;
    let mut mirror = Axis2Placement::default();
    // x:[1 1 0] y:[0 0 1] z=cross(x,y):[1 -1 0] (status ignored)
    let _ = mirror.set_canonical(Point3::new(0.0, 0.0, 0.0), x_axis, Vec3::new(0.0, 0.0, 1.0));

    // Transpose the Face->UVDomain
    let d = fp2::uv_domain(b, f)?;
    let transpose = Extent2d::from_points(
        Point2::new(d.min().y, d.min().x),
        Point2::new(d.max().y, d.max().x),
    );
    fp2::set_uv_domain(b, f, &transpose)?;

    let upward = upward_loopuses(b, f)?;

    // prepare to Edit
    face_notify(b, f)?;

    let mut to_set: Vec<(EdgeuseId, Curve)> = Vec::new();
    for lu in upward {
        // Swap primary and secondary loopuses
        let l = lu1::loop_(b, lu)?.ok_or_else(|| null("loop use without loop"))?;
        let mate = lu1::other_loopuse(b, lu)?;
        b.loop_mut(l)?.loopuse = mate;

        // for every Edgeuse mated pair - mirror the UVTrimCurve
        for eu in eu1::loopuse_edgeuses(b, lu)? {
            if let Some(uv) = eu1::uv_trim_curve(b, eu)?.0 {
                // (upstream dereferences the mate only for an edge use with a curve)
                let eu_mate = eu1::mate(b, eu)?.ok_or_else(|| null("edge use without mate"))?;
                let mirrored = match uv.kind() {
                    CurveKind::BSpline(c) => c.create_mirror_curve(&mirror)?,
                    // (MakeNurb's status is ignored upstream: mirror the base as it is)
                    CurveKind::Line(c) => match c.base_with_nurb() {
                        Ok(base) => base.create_mirror_curve(&mirror)?,
                        Err(e) if e.is_unported() => return Err(e),
                        Err(_) => c.base().create_mirror_curve(&mirror)?,
                    },
                    // the conics: MakeNurb (their override) when missing, then the
                    // SmBSplineCurve mirror of that NURB (statuses ignored as upstream)
                    CurveKind::Ellipse(c) => {
                        let mut c = c.clone();
                        if c.base().gw_nurb().is_none() {
                            ignore_status(c.make_nurb(k))?;
                        }
                        c.base().create_mirror_curve(&mirror)?
                    }
                    CurveKind::Parabola(c) => {
                        let mut c = c.clone();
                        if c.base().gw_nurb().is_none() {
                            ignore_status(c.make_nurb(None))?;
                        }
                        c.base().create_mirror_curve(&mirror)?
                    }
                    CurveKind::Hyperbola(c) => {
                        let mut c = c.clone();
                        if c.base().gw_nurb().is_none() {
                            ignore_status(c.make_nurb(None))?;
                        }
                        c.base().create_mirror_curve(&mirror)?
                    }
                    // A UV trim curve is an SmBSplineCurve*: upstream's only subclasses are
                    // SmLine, SmEllipse, SmHyperbola and SmParabola (the arms above);
                    // SmHermiteCurve, SmIsoCurve, SmOffsetCurve, SmProjectedCurve,
                    // SmCompositeCurve, SmConic, SmCrvOnSurf and SmCrvInVolume derive from
                    // SmCurve and cannot be one.
                    _ => return Err(null("UV trim curve is not a B-spline curve")),
                };
                to_set.push((eu_mate, Curve::from(mirrored)));
            }
        }
    }

    // swap theSurface UV orientation
    b.face_mut(f)?
        .surface
        .as_mut()
        .ok_or_else(|| null("face without surface"))?
        .swap_uv(k)?;

    for (eu, c) in to_set {
        // status ignored upstream
        ignore_status(eu1::set_uv_trim_curve(
            b,
            eu,
            eu1::UvTrimCurveArg::New(Box::new(c)),
            0.0,
            true,
            None,
        ))?;
    }

    // Swap the Face->Faceuses swapping the upper and lower faceuses with one another
    let fu = fp2::upward_faceuse(b, f)?.ok_or_else(|| null("face without face use"))?;
    b.faceuse_mut(fu)?.orientation = OrientType::Opposite;
    let fu_mate = eu1::faceuse_mate(b, fu)?.ok_or_else(|| null("face use without mate"))?;
    b.faceuse_mut(fu_mate)?.orientation = OrientType::Same;
    b.face_mut(f)?.faceuse = Some(fu_mate);

    face_notify(b, f)?;
    Ok(())
}

// ---- shrink -------------------------------------------------------------------------------

/// `ShrinkGeometry`: shrinks the face's UV domain to its UV trim curves (snapping to the
/// surface domain within a thousandth of the new domain's diagonal), trims the surface
/// to it, removes the UV trim curves and trims each edge curve owned by an edge to that
/// edge's interval. Nothing is done for a face without surface or whose surface has no
/// owner. `key_of` names the edges and edge uses a curve's owner chain may pass.
///
/// # Errors
///
/// Those of [`calculate_uv_domain_from_uv_trim_curves`] and [`Curve::edge`];
/// [`SolidError::Unported`] (`GetEdges`, `SmFace::Notify`).
pub fn shrink_geometry(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    key_of: &dyn Fn(EntityRef) -> ObjectRef,
) -> Result<()> {
    // no work - don't work on faces when in an intermediate state
    match fp2::surface(b, f)? {
        Some(s) if s.owner().is_some() => {}
        _ => return Ok(()),
    }

    // compute face domain from boundary curves
    let face_domain = calculate_uv_domain_from_uv_trim_curves(k, b, f, true, false, false)?.domain;
    let tol = face_domain.size().length() / 1000.0;

    // get surface's current natural UVDomain
    let surf_domain = surface_deref(b, f)?.natural_uv_domain();

    // If the face domain is dTol close within surface domain, choose the surface domain
    let mut min = face_domain.min();
    let mut max = face_domain.max();
    if min.x - surf_domain.min().x < tol {
        min.x = surf_domain.min().x;
    }
    if min.y - surf_domain.min().y < tol {
        min.y = surf_domain.min().y;
    }
    if surf_domain.max().x - max.x < tol {
        max.x = surf_domain.max().x;
    }
    if surf_domain.max().y - max.y < tol {
        max.y = surf_domain.max().y;
    }

    // set the (revised) face domain
    let mut new_domain = Extent2d::from_points(min, max);
    fp2::set_uv_domain(b, f, &new_domain)?;

    // Trim the surface (status ignored)
    if let Some(s) = b.face_mut(f)?.surface.as_mut() {
        ignore_status(s.trim_with_domain(k, &mut new_domain))?;
    }

    // remove all associated UVTrimCurves (status ignored)
    ignore_status(remove_uv_trim_curves(b, f))?;

    // Now Trim all Curves to the face edges
    for e in face_edges(b, f)? {
        let Some(curve) = b.edge(e)?.curve.as_ref() else {
            return Err(null("edge without curve"));
        };
        let owner_of = |o: ObjectRef| -> Option<ObjectRef> {
            b.edgeuses()
                .ids()
                .find(|&eu| key_of(EntityRef::Edgeuse(eu)) == o)
                .and_then(|eu| b.edgeuse(eu).ok()?.edge)
                .map(|e| key_of(EntityRef::Edge(e)))
        };

        // skip curves without owners
        let Some(owner) = curve.edge(owner_of)? else {
            continue;
        };
        let Some(owner_edge) = b
            .edges()
            .ids()
            .find(|&x| key_of(EntityRef::Edge(x)) == owner)
        else {
            continue;
        };
        let mut ivl = b.edge(owner_edge)?.interval;

        // trim the curve to the used interval - status ignored
        if let Some(c) = b.edge_mut(e)?.curve.as_mut() {
            ignore_status(c.trim(k, &mut ivl, TrimMode::Notify))?;
        }
    }
    Ok(())
}

// ---- seams --------------------------------------------------------------------------------

/// Faces, edges and vertices a seam fix made (`pOptNewFaces`, `pOptNewSeamEdges`,
/// `pOptSeamVertices`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeamSplit {
    /// New faces (not including the input face).
    pub faces: Vec<FaceId>,
    /// New seam edges.
    pub edges: Vec<EdgeId>,
    /// New vertices on the seam.
    pub vertices: Vec<VertexId>,
}

/// The flags of [`move_seam`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveSeamOptions {
    /// `bOkToMoveToCenter`: the seam may move into the face (to avoid sliver faces) when
    /// it cannot move out of it.
    pub ok_to_move_to_center: bool,
    /// `bOkToSplit`: the face may be split at the seam when it cannot move out of it.
    pub ok_to_split: bool,
}

/// The outputs of [`move_seam`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveSeamResult {
    /// `eMoveSeamResult_U`.
    pub u: FixSeamPlanType,
    /// `eMoveSeamResult_V`.
    pub v: FixSeamPlanType,
    /// What the split made (with `ok_to_split`).
    pub split: SeamSplit,
}

/// Whether a seam direction value names `dir` (`== dir || == SM_SP_BOTH`).
fn names(p: SurfParamType, dir: SurfParamType) -> bool {
    p == dir || p == SurfParamType::Both
}

/// Whether a classification has a seam problem in the face (`size < 1` or a single
/// interval whose middle is not on the face means no problem).
fn class_is_quiet<O: LoopObject>(c: &CurveClassification<O>) -> bool {
    match c.intervals() {
        [] => true,
        [one] => one.mid.point_class() != PointClassType::Face,
        _ => false,
    }
}

/// The UV drop of curve point `param` of `curve` on `surface`
/// (`EvaluateGeometric` then `DropPoint` and `DropVectors`, statuses ignored).
fn sample_uv(
    k: &Kernel,
    curve: &Curve,
    surface: &Surface,
    uv_domain: &Extent2d,
    param: f64,
    pv: &mut [Vec3; 2],
) -> Result<Point2> {
    if let Some(v) = ignored(evaluate_geometric(curve, param, 1, true))? {
        if let [p, d, ..] = v.as_slice() {
            *pv = [*p, *d];
        }
    }
    let mut uv = Vec2::UNDEF;
    if let Some(r) =
        ignored(surface.drop_point::<NoObject>(k, pv[0], uv_domain, &DropPointOptions::default()))?
    {
        uv = r.result_uv;
    }
    // DropVectors of the tangent (its output is never read)
    ignore_status(drop_vectors_at(
        surface,
        uv,
        Side::Left,
        Side::Left,
        &pv[1..],
    ))?;
    Ok(uv)
}

/// The UV point of vertex use `vu`: its cached vertex/face gap's face UV, else
/// `ComputeUVPoint(.., FALSE)` (status ignored, keeping `prev`).
fn vertexuse_uv(k: &Kernel, b: &mut Brep, vu: VertexuseId, prev: Point2) -> Result<Point2> {
    if let Some(Some(gap)) = ignored(vu1::vertex_face_gap(k, b, vu, false))? {
        return Ok(gap.face_uv);
    }
    Ok(ignored(vu1::compute_uv_point(k, b, vu, false))?.unwrap_or(prev))
}

/// `MoveSeam`: on a periodic B-spline surface whose seam the face's edges cross (or
/// nearly touch), moves the seam (rebuilding the surface split at a new parameter and
/// joined the other way round) out of the face, or into it when the face spans the
/// whole period (with `ok_to_move_to_center` or `ok_to_split`); with `ok_to_split` a
/// face that still has seam problems is then split at the seam ([`split_at_seam`]).
/// `crv_class_u` / `crv_class_v` and `problem` come from `HasSeamProblem` (and may be
/// rebuilt); `props` replaces the classifications' verdicts when given. `this` is the
/// face's identity (the owner of a new surface), `resolve` maps a classification
/// object to a vertex (for the split).
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for a surface that is not a B-spline kind; `SM_ERR` when the
/// surface cannot be split; [`SolidError::InvalidInput`] as in the module docs; those
/// of the surface join, the knot query and the classification state;
/// [`SolidError::Unported`] (`GetUpwardLoopuses`, `IsCrossedByEdge`, `HasNearMisses`,
/// `GetUpwardVertexuses`, `GetBrep`, `ReplaceSurface`, `SmFace::Notify`,
/// `HasSeamProblem`, the split).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn move_seam<O: LoopObject>(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    this: ObjectRef,
    crv_class_u: &mut CurveClassification<O>,
    crv_class_v: &mut CurveClassification<O>,
    problem: SeamProblem,
    opts: MoveSeamOptions,
    props: Option<&FaceProps<O>>,
    geometry: &impl ClassGeometry<O>,
    resolve: &dyn Fn(O) -> Option<EntityRef>,
) -> Result<MoveSeamResult> {
    use SurfParamType as Sp;
    // (the seam problem is passed by value; HasSeamProblem updates this copy)
    let mut problem = problem;
    let mut out = MoveSeamResult {
        u: FixSeamPlanType::NoChanges,
        v: FixSeamPlanType::NoChanges,
        split: SeamSplit::default(),
    };

    // no work - no seam problems
    let quiet = match props {
        Some(p) => {
            matches!(p.bad_crossed_seam_dir, Sp::Neither | Sp::Unknown)
                && matches!(p.bad_missing_seam_dir, Sp::Neither | Sp::Unknown)
        }
        None => class_is_quiet(crv_class_u) && class_is_quiet(crv_class_v),
    };
    if quiet {
        return Ok(out);
    }

    // locals
    let lsmp_cnt: usize = 6;
    // (upstream reads the surface's domain before its null check)
    let surface0 = surface_deref(b, f)?;
    let uv_domain = surface0.natural_uv_domain();
    let ivl_u = uv_domain.u_interval();
    let ivl_v = uv_domain.v_interval();
    let mut periodic_u = PeriodicExtent1d::full(ivl_u.length());
    let _ = periodic_u.set_min_max(0.0, 0.0, None, true);
    let mut periodic_v = PeriodicExtent1d::full(ivl_v.length());
    let _ = periodic_v.set_min_max(0.0, 0.0, None, true);

    // check State - SmBSplineSurfaces only
    if !surface0.is_kind_of(B_SPLINE_SURFACE_TYPE)? {
        return Err(SolidError::invalid_input(
            OP,
            "MoveSeam: only SmBSplineSurface kinds are supported",
        ));
    }

    let loopuses = upward_loopuses(b, f)?;
    if loopuses.is_empty() {
        return Ok(out);
    }

    let surface_is_analytic = surface0.is_analytic()?;
    let zone_tol = crate::surface::surface::p1::zone_tol_3d(k, surface0)?;
    let mut surface = surface0.clone();
    *surface.owner_mut() = surface0.owner();

    let crosses_u = match props {
        Some(p) => names(p.bad_crossed_seam_dir, Sp::U),
        None => is_crossed_by_edge(crv_class_u)?,
    };
    let crosses_v = match props {
        Some(p) => names(p.bad_crossed_seam_dir, Sp::V),
        None => is_crossed_by_edge(crv_class_v)?,
    };
    let near_miss_u = match props {
        Some(p) => names(p.bad_near_miss_seam_dir, Sp::U),
        None => has_near_misses(k, crv_class_u, geometry)?,
    };
    let near_miss_v = match props {
        Some(p) => names(p.bad_near_miss_seam_dir, Sp::V),
        None => has_near_misses(k, crv_class_v, geometry)?,
    };
    let fix_seam_both = (crosses_u || near_miss_u) && (crosses_v || near_miss_v);

    for ii in 0..2usize {
        let (ivl, surf_param, crosses_seam, near_miss_seam) = if ii == 0 {
            (ivl_u, Sp::U, crosses_u, near_miss_u)
        } else {
            (ivl_v, Sp::V, crosses_v, near_miss_v)
        };
        let ivl_length = ivl.length();
        let mut ok_to_snap = true;

        if !(crosses_seam || near_miss_seam) {
            continue;
        }

        // skip the walk for V when both are fixed (the U walk fills both)
        if !fix_seam_both || ii == 0 {
            // sPV[0] (first sample) and sPV[1] (the walk)
            let mut pv0 = [Vec3::UNDEF; 2];
            let mut pv1 = [Vec3::UNDEF; 2];
            let first_lu = *loopuses.first().ok_or_else(|| null("no loop uses"))?;
            let first_eus = eu1::loopuse_edgeuses(b, first_lu)?;
            let eu0 = *first_eus
                .first()
                .ok_or_else(|| null("loop use without edge uses"))?;
            let e0 = edge_deref(b, eu0)?;
            let norm0 = if eu1::orientation(b, eu0)? == OrientType::Same {
                0.0
            } else {
                1.0
            };
            let mut uv_prev = {
                let edge = b.edge(e0)?;
                let curve = edge
                    .curve
                    .as_ref()
                    .ok_or_else(|| null("edge without curve"))?;
                let param = edge.interval.evaluate(norm0);
                sample_uv(k, curve, &surface, &uv_domain, param, &mut pv0)?
            };

            // init the periodic interval at the first sample
            {
                let periodic = if ii == 0 {
                    &mut periodic_u
                } else {
                    &mut periodic_v
                };
                let s = comp(uv_prev, ii) - ivl.min();
                let _ = periodic.set_min_max(s, s, None, true);
            }

            'loops: for (j0, &lu) in loopuses.iter().enumerate() {
                if periodic_full(&periodic_u, &periodic_v, ii) {
                    break;
                }
                let eus = eu1::loopuse_edgeuses(b, lu)?;
                for (j1, &eu) in eus.iter().enumerate() {
                    if periodic_full(&periodic_u, &periodic_v, ii) {
                        break 'loops;
                    }
                    let orient = eu1::orientation(b, eu)?;
                    let e = edge_deref(b, eu)?;
                    let start = usize::from(j0 + j1 == 0);
                    for kk in start..=lsmp_cnt {
                        let norm = if orient == OrientType::Same {
                            if kk == lsmp_cnt {
                                1.0
                            } else {
                                kk as f64 / lsmp_cnt as f64
                            }
                        } else if kk == 0 {
                            1.0
                        } else {
                            (lsmp_cnt - kk) as f64 / lsmp_cnt as f64
                        };
                        let uv1 = {
                            let edge = b.edge(e)?;
                            let curve = edge
                                .curve
                                .as_ref()
                                .ok_or_else(|| null("edge without curve"))?;
                            let param = edge.interval.evaluate(norm);
                            sample_uv(k, curve, &surface, &uv_domain, param, &mut pv1)?
                        };

                        // add the sampled span the short way round
                        let (lo, hi) = order(comp(uv_prev, ii), comp(uv1, ii));
                        let periodic = if ii == 0 {
                            &mut periodic_u
                        } else {
                            &mut periodic_v
                        };
                        let no_seam = hi - lo;
                        let with_seam = periodic.period() - no_seam;
                        let forward = no_seam < with_seam;
                        let _ = periodic.add_interval(lo - ivl.min(), hi - ivl.min(), forward);

                        if fix_seam_both && ii == 0 {
                            let (lo, hi) = order(uv_prev.y, uv1.y);
                            let no_seam = hi - lo;
                            // (upstream reads the U period and U minimum here)
                            let with_seam = periodic_u.period() - no_seam;
                            let forward = no_seam < with_seam;
                            let _ =
                                periodic_v.add_interval(lo - ivl.min(), hi - ivl.min(), forward);
                        }

                        uv_prev = uv1;
                    }
                }
            }
        }

        let periodic = if ii == 0 { periodic_u } else { periodic_v };
        if periodic.is_full_period() && !near_miss_seam {
            continue;
        }

        let mut new_param;
        if periodic.is_full_period() {
            // face uses all of the periodic surface: move to the vertex spacing
            let vus = upward_vertexuses(b, f)?;
            let mut params: Vec<f64> = Vec::new();
            let mut unique: Vec<f64> = Vec::new();
            let mut param_ivl = Extent1d::new();
            let mut pt_uv = uv_domain.mid();
            for &vu in &vus {
                pt_uv = vertexuse_uv(k, b, vu, pt_uv)?;
                let p = comp(pt_uv, ii);
                params.push(p);
                param_ivl.add_value(p);
                let dup = unique.iter().any(|&q| {
                    are_same_to_tol(p, q, EFF_ZERO_PARAM)
                        || are_same_to_tol(p, q + ivl_length, EFF_ZERO_PARAM)
                        || are_same_to_tol(p, q - ivl_length, EFF_ZERO_PARAM)
                });
                if !dup {
                    unique.push(p);
                }
            }
            let by_value = |a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(Ordering::Equal);
            params.sort_by(by_value);
            unique.sort_by(by_value);
            let _ = (params, param_ivl);

            let cnt = unique.len();
            let at = |i: usize| -> Result<f64> {
                unique
                    .get(i)
                    .copied()
                    .ok_or_else(|| null("MoveSeam: no vertex parameters"))
            };

            // the largest gap between parameters
            let mut max_span = (at(0)? + ivl_length) - at(cnt.wrapping_sub(1))?;
            let mut max_indx = cnt - 1;
            for jj in 0..cnt.saturating_sub(1) {
                let this_span = at(jj + 1)? - at(jj)?;
                if max_span < this_span {
                    max_span = this_span;
                    max_indx = jj;
                }
            }

            // the most spaced out parameter
            let span_of = |this: usize, next: usize| -> Result<f64> {
                Ok((at(next)? - at(this)?) + if next <= this { ivl_length } else { 0.0 })
            };
            let last_span0 = (at(0)? - at(cnt - 1)?) + ivl_length;
            let mut next_span = span_of(0, 1 % cnt)?;
            let mut max_spacing = smos_min(last_span0, next_span);
            let mut max_space_indx = 0usize;
            for jj in 1..cnt {
                let last_span = next_span;
                next_span = span_of(jj, (jj + 1) % cnt)?;
                let this_spacing = smos_min(last_span, next_span);
                if max_spacing < this_spacing {
                    max_spacing = this_spacing;
                    max_space_indx = jj;
                }
            }

            let mut use_max_point_spacing = false;
            if max_spacing > ivl_length / 10.0 {
                use_max_point_spacing = true;
                let target = at(max_space_indx)?;
                let mut paired: Vec<VertexuseId> = Vec::new();
                for &vu in &vus {
                    pt_uv = vertexuse_uv(k, b, vu, pt_uv)?;
                    if are_same_to_tol(target, comp(pt_uv, ii), EFF_ZERO_PARAM) {
                        paired.push(vu);
                    }
                }
                'pairs: for (jj, &vu_a) in paired.iter().enumerate() {
                    if jj + 1 >= paired.len() {
                        break;
                    }
                    let v1 = vu1::vertex(b, vu_a)?;
                    let v1 = v1.ok_or_else(|| null("vertex use without vertex"))?;
                    for &vu2 in paired.get(jj + 1..).unwrap_or_default() {
                        let v2 = vu1::vertex(b, vu2)?;
                        if !vx::common_edges(b, v1, v2, Some(f))?.is_empty() {
                            use_max_point_spacing = false;
                            break 'pairs;
                        }
                    }
                }
            }

            if use_max_point_spacing {
                new_param = at(max_space_indx)?;
                ok_to_snap = false;
            } else {
                let min_param = at(max_indx)?;
                let max_param = at((max_indx + 1) % cnt)?;
                new_param = if max_indx != cnt {
                    (max_param + min_param) / 2.0
                } else if (min_param - ivl.min()) > (ivl.max() - max_param) {
                    min_param - ((max_param - min_param + ivl_length) / 2.0)
                } else {
                    max_param + ((max_param - min_param + ivl_length) / 2.0)
                };
            }
        } else {
            // face does not use all of the periodic surface
            let mut unused = periodic;
            unused.complement();
            new_param = unused.mid() + ivl.min();
        }

        // snap to a knot that keeps away from the face
        if ok_to_snap {
            let far = periodic.period() / 10.0;
            let mut best: Option<usize> = None;
            let mut dist = BIG_DOUBLE;
            let mut this_ivl = periodic;
            this_ivl.set_full_period(false);
            let knots = ignored(surface.knots(surf_param, None))?
                .map(|(k, _)| k)
                .unwrap_or_default();
            let k0 = knots.first().copied().unwrap_or(0.0);
            for (jj, &knot) in knots.iter().enumerate() {
                let this_knot = knot - k0;
                let this_dist = (this_knot - new_param).abs();
                if this_dist < dist {
                    let lo = periodic.map_value(this_knot - ivl.min() - far);
                    let hi = periodic.map_value(this_knot - ivl.min() + far);
                    let _ = this_ivl.set_min_max(lo, hi, None, false);
                    if periodic.are_disjoint(&this_ivl) {
                        best = Some(jj);
                        dist = this_dist;
                    }
                }
            }
            if let Some(&knot) = best.and_then(|i| knots.get(i)) {
                new_param = knot;
            }
        }

        if ivl.classify_point(new_param, EFF_ZERO) == ExtentPointType::Inside
            && (!periodic.is_full_period() || opts.ok_to_move_to_center || opts.ok_to_split)
        {
            let (left, right) = surface.split_at(k, new_param, surf_param).map_err(|e| {
                if e.is_unported() {
                    e
                } else {
                    SolidError::sm_err(OP)
                }
            })?;
            let (SurfaceKind::BSpline(left), SurfaceKind::BSpline(right)) =
                (left.into_kind(), right.into_kind())
            else {
                return Err(null("MoveSeam: split halves are not SmBSplineSurfaces"));
            };
            let (mut left, mut right) = (left, right);
            let joined = right.join_surface(&mut left, surf_param, zone_tol)?;
            let mut joined = Surface::from(joined);
            if surface_is_analytic {
                if let SurfaceKind::BSpline(j) = joined.kind() {
                    if let Some(a) = ignored(j.copy_and_add_analytics())? {
                        if a.is_kind_of(B_SPLINE_SURFACE_TYPE)? {
                            joined = a;
                        }
                    }
                }
            }

            if ii == 0 {
                out.u = if periodic.is_full_period() {
                    FixSeamPlanType::MoveInFace
                } else {
                    FixSeamPlanType::MoveOutOfFace
                };
            } else {
                out.v = if periodic.is_full_period() {
                    FixSeamPlanType::MoveInFace
                } else {
                    FixSeamPlanType::MoveOutOfFace
                };
            }

            // replace the face surface
            surface = joined.clone();
            if has_brep(b, f)? {
                replace_surface(b, f, joined, false, false, false)?;
            } else {
                set_surface(b, f, this, joined, true)?;
            }
        }
    }

    // split the face at its seams when they are still a problem
    if opts.ok_to_split
        && (periodic_u.is_full_period() || periodic_v.is_full_period())
        && ignored(has_seam_problem(
            k,
            b,
            f,
            crv_class_u,
            crv_class_v,
            &mut problem,
            false,
        ))?
        .unwrap_or(false)
    {
        // status ignored upstream
        if let Some(s) = ignored(split_at_seam_depth(
            k,
            b,
            f,
            crv_class_u,
            crv_class_v,
            problem,
            props,
            resolve,
            0,
        ))? {
            out.split = s;
        }
    }
    Ok(out)
}

/// `smos_Min(a, b)`: `a < b ? a : b` (unlike `f64::min`, a NaN `a` gives `b` and a
/// NaN `b` gives `b`).
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// Whether the periodic extent of direction `ii` covers the whole period.
fn periodic_full(u: &PeriodicExtent1d, v: &PeriodicExtent1d, ii: usize) -> bool {
    if ii == 0 {
        u.is_full_period()
    } else {
        v.is_full_period()
    }
}

/// `(min, max)` of two values, the first when they are equal.
fn order(a: f64, b: f64) -> (f64, f64) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

/// `SplitAtSeam`: inserts seam edges where the face's periodic surface has a crossed
/// or missing seam, splitting the face (first along U, then each resulting face along
/// V). `crv_class_u` / `crv_class_v` and `problem` come from `HasSeamProblem` (they are
/// rebuilt over all loops when they were made for target loops only); `props`
/// replaces the surface periodicity and missing-seam verdict when given; `resolve` maps
/// a classification object to the face, edge or vertex it names.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a face without surface or a recursion deeper than
/// [`MAX_SPLIT_AT_SEAM_DEPTH`]; those of [`Surface::is_periodic`] and
/// [`fp2::remove_suspect_split_at_seam_uv_trim_curves`]; [`SolidError::Unported`]
/// (`HasSeamProblem`; the deferrals of `MergeIntervals`' callees).
#[allow(clippy::too_many_arguments)] // one upstream routine
pub fn split_at_seam<O: LoopObject>(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    crv_class_u: &mut CurveClassification<O>,
    crv_class_v: &mut CurveClassification<O>,
    problem: SeamProblem,
    props: Option<&FaceProps<O>>,
    resolve: &dyn Fn(O) -> Option<EntityRef>,
) -> Result<SeamSplit> {
    split_at_seam_depth(
        k,
        b,
        f,
        crv_class_u,
        crv_class_v,
        problem,
        props,
        resolve,
        0,
    )
}

#[allow(clippy::too_many_arguments)]
fn split_at_seam_depth<O: LoopObject>(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    crv_class_u: &mut CurveClassification<O>,
    crv_class_v: &mut CurveClassification<O>,
    mut problem: SeamProblem,
    props: Option<&FaceProps<O>>,
    resolve: &dyn Fn(O) -> Option<EntityRef>,
    depth: usize,
) -> Result<SeamSplit> {
    use SurfParamType as Sp;
    if depth > MAX_SPLIT_AT_SEAM_DEPTH {
        return Err(null("SplitAtSeam: recursion too deep"));
    }
    let mut out = SeamSplit::default();

    // no work - FaceProps says no missing seam
    if let Some(p) = props {
        if matches!(p.bad_missing_seam_dir, Sp::Neither | Sp::Unknown) {
            return Ok(out);
        }
    }

    let surface = surface_deref(b, f)?;
    let uv_domain = surface.natural_uv_domain();
    let (periodic_u, periodic_v) = match props {
        // an UNSURE flag is not FALSE
        Some(p) => (p.closed_u != Tri::False, p.closed_v != Tri::False),
        // (IsPeriodic returns a flag upstream; its SER inside makes a failure TRUE)
        None => (
            ignored(surface.is_periodic(&uv_domain, Sp::U))?.unwrap_or(true),
            ignored(surface.is_periodic(&uv_domain, Sp::V))?.unwrap_or(true),
        ),
    };
    if !periodic_u && !periodic_v {
        return Ok(out);
    }

    // classify against all loops when the classifications were made for some only
    if crv_class_u.tgt_loops_only() || crv_class_v.tgt_loops_only() {
        crv_class_u.set_tgt_loops_only(false);
        crv_class_v.set_tgt_loops_only(false);
        // (result ignored upstream; HasSeamProblem returns a flag, not a status)
        ignore_status(has_seam_problem(
            k,
            b,
            f,
            crv_class_u,
            crv_class_v,
            &mut problem,
            false,
        ))?;
    }

    let any = |p: SurfParamType| matches!(p, Sp::Both | Sp::U | Sp::V);
    if !(any(problem.crossed) || any(problem.missing)) {
        return Ok(out);
    }

    let merge_u = names(problem.crossed, Sp::U) || names(problem.missing, Sp::U);
    let merge_v = names(problem.crossed, Sp::V) || names(problem.missing, Sp::V);

    let mut this_new = SeamSplit::default();
    if merge_u {
        fp2::remove_suspect_split_at_seam_uv_trim_curves(b, crv_class_u, |o| match resolve(o) {
            Some(EntityRef::Vertex(v)) => Some(v),
            _ => None,
        })?;
        // (upstream sets pBrep->m_bEditingEnabled and the context's DoingBoolean flag
        // around the merges with SmTemporaryChangeValue; neither is set here: the B-rep
        // flag is unit topology.brep_p1's and the context flag needs a mutable kernel)
        this_new = merge_intervals(crv_class_u, resolve)?;
        if !merge_v {
            out.faces.extend_from_slice(&this_new.faces);
            out.edges.extend_from_slice(&this_new.edges);
            out.vertices.extend_from_slice(&this_new.vertices);
        }
    }

    if merge_u && merge_v && !this_new.faces.is_empty() {
        let mut targets = this_new.faces.clone();
        if !targets.contains(&f) {
            targets.push(f);
        }
        for tgt in targets {
            let mut cu = CurveClassification::new(k);
            let mut cv = CurveClassification::new(k);
            let mut this_problem = SeamProblem {
                crossed: Sp::Unknown,
                missing: Sp::Unknown,
                near_miss: Sp::Unknown,
            };
            let has = ignored(has_seam_problem(
                k,
                b,
                tgt,
                &mut cu,
                &mut cv,
                &mut this_problem,
                false,
            ))?
            .unwrap_or(false);
            if has {
                let s = ignored(split_at_seam_depth(
                    k,
                    b,
                    f,
                    &mut cu,
                    &mut cv,
                    this_problem,
                    props,
                    resolve,
                    depth + 1,
                ))?
                .unwrap_or_default();
                out.faces.extend_from_slice(&s.faces);
                if tgt != f && !out.faces.contains(&tgt) {
                    out.faces.push(tgt);
                }
                out.edges.extend_from_slice(&s.edges);
                out.vertices.extend_from_slice(&s.vertices);
            }
        }
    } else if merge_v {
        fp2::remove_suspect_split_at_seam_uv_trim_curves(b, crv_class_v, |o| match resolve(o) {
            Some(EntityRef::Vertex(v)) => Some(v),
            _ => None,
        })?;
        this_new = merge_intervals(crv_class_v, resolve)?;
        out.faces.extend_from_slice(&this_new.faces);
        out.edges.extend_from_slice(&this_new.edges);
        out.vertices.extend_from_slice(&this_new.vertices);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::attribute::ObjectKey;
    use crate::intersect::point_class::{ClassObject, ClassObjectKind};
    use crate::surface::plane::Plane;
    use crate::surface::volume::Volume;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(LoopId);

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            ClassObjectKind::Loop
        }
    }

    impl LoopObject for H {
        fn from_loop(l: LoopId) -> Self {
            Self(l)
        }
    }

    /// Classification geometry for the null handles here: resolves nothing.
    struct NoGeometry;

    impl ClassGeometry<H> for NoGeometry {
        fn curve(&self, _: H) -> Option<&Curve> {
            None
        }
        fn surface(&self, _: H) -> Option<&Surface> {
            None
        }
        fn volume(&self, _: H) -> Option<&Volume> {
            None
        }
        fn point(&self, _: H) -> Option<Point3> {
            None
        }
    }

    fn me(f: FaceId) -> ObjectRef {
        ObjectRef {
            type_code: fp2::TYPE,
            key: ObjectKey(1 + u64::from(f.index())),
        }
    }

    fn key_of(e: EntityRef) -> ObjectRef {
        match e {
            EntityRef::Face(f) => me(f),
            _ => ObjectRef {
                type_code: 0,
                key: ObjectKey(0),
            },
        }
    }

    fn bare_face() -> (Brep, FaceId) {
        let mut b = Brep::new();
        let s = b.add_shell().unwrap();
        let f = b.add_face(s).unwrap();
        (b, f)
    }

    fn plane_face() -> (Brep, FaceId) {
        let (mut b, f) = bare_face();
        let mut p = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z));
        p.set_owner(Some(me(f)));
        b.face_mut(f).unwrap().surface = Some(p);
        (b, f)
    }

    #[test]
    fn plan_type_codes() {
        assert_eq!(FixSeamPlanType::Unknown as u8, 0);
        assert_eq!(FixSeamPlanType::NoChanges as u8, 1);
        assert_eq!(FixSeamPlanType::SplitFace as u8, 4);
        assert_eq!(FixSeamPlanType::default(), FixSeamPlanType::Unknown);
    }

    #[test]
    fn uv_domain_without_surface_keeps_the_face_domain() {
        let (mut b, f) = bare_face();
        let d = Extent2d::from_bounds(0.0, 1.0, 2.0, 3.0);
        fp2::set_uv_domain(&mut b, f, &d).unwrap();
        let k = Kernel::default();
        let r = calculate_uv_domain_from_uv_trim_curves(&k, &mut b, f, true, true, true).unwrap();
        assert_eq!(r.domain, d);
        assert_eq!(r.max_uv_loop_gap, 0.0);
        assert_eq!(r.second_max_uv_loop_gap, 0.0);
        // with a surface the UV trim curves must be made first (unported)
        let (mut b, f) = plane_face();
        let e = calculate_uv_domain_from_uv_trim_curves(&k, &mut b, f, true, false, false);
        assert!(e.unwrap_err().is_unported());
    }

    #[test]
    fn rectangular_trim_needs_one_loop() {
        let (mut b, f) = plane_face();
        let k = Kernel::default();
        let r = find_if_rectangular_trim(&k, &mut b, f, &key_of).unwrap();
        assert!(!r.rectangular && !r.natural);
        assert_eq!(r.uv_domain, Extent2d::new());
    }

    #[test]
    fn edits_notify_first() {
        let (mut b, f) = plane_face();
        let k = Kernel::default();
        assert!(flip_face_orientation(&mut b, f).unwrap_err().is_unported());
        assert!(remove_uv_trim_curves(&mut b, f).unwrap_err().is_unported());
        let s = b.face(f).unwrap().surface.clone().unwrap();
        assert!(set_surface(&mut b, f, me(f), s, true)
            .unwrap_err()
            .is_unported());
        assert!(swap_uv(&k, &mut b, f).unwrap_err().is_unported());
        assert!(destroy(&mut b, f, me(f)).unwrap_err().is_unported());
    }

    #[test]
    fn shrink_skips_faces_without_owned_surface() {
        let (mut b, f) = bare_face();
        let k = Kernel::default();
        shrink_geometry(&k, &mut b, f, &key_of).unwrap();
        b.face_mut(f).unwrap().surface =
            Some(Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z)));
        shrink_geometry(&k, &mut b, f, &key_of).unwrap();
        let (mut b, f) = plane_face();
        assert!(shrink_geometry(&k, &mut b, f, &key_of)
            .unwrap_err()
            .is_unported());
    }

    #[test]
    fn seams_quiet_and_non_periodic() {
        let (mut b, f) = plane_face();
        let k = Kernel::default();
        let mut cu = CurveClassification::<H>::new(&k);
        let mut cv = CurveClassification::<H>::new(&k);
        let mut p = SeamProblem {
            crossed: SurfParamType::Neither,
            missing: SurfParamType::Neither,
            near_miss: SurfParamType::Neither,
        };
        let r = move_seam(
            &k,
            &mut b,
            f,
            me(f),
            &mut cu,
            &mut cv,
            p,
            MoveSeamOptions::default(),
            None,
            &NoGeometry,
            &|_| None,
        )
        .unwrap();
        assert_eq!(r.u, FixSeamPlanType::NoChanges);
        assert_eq!(r.v, FixSeamPlanType::NoChanges);
        // a plane is periodic in neither direction
        p.crossed = SurfParamType::Both;
        let s = split_at_seam(&k, &mut b, f, &mut cu, &mut cv, p, None, &|_| None).unwrap();
        assert_eq!(s, SeamSplit::default());
        // face properties saying no missing seam end the split early
        let mut props = FaceProps::<H>::new();
        props.bad_missing_seam_dir = SurfParamType::Neither;
        let s = split_at_seam(&k, &mut b, f, &mut cu, &mut cv, p, Some(&props), &|_| None).unwrap();
        assert_eq!(s, SeamSplit::default());
        // a problem reported by the face properties reaches the unported loop walk
        props.bad_crossed_seam_dir = SurfParamType::U;
        let e = move_seam(
            &k,
            &mut b,
            f,
            me(f),
            &mut cu,
            &mut cv,
            p,
            MoveSeamOptions::default(),
            Some(&props),
            &NoGeometry,
            &|_| None,
        );
        assert!(e.unwrap_err().is_unported());
    }

    #[test]
    fn split_at_seam_depth_is_bounded() {
        let (mut b, f) = plane_face();
        let k = Kernel::default();
        let mut cu = CurveClassification::<H>::new(&k);
        let mut cv = CurveClassification::<H>::new(&k);
        let p = SeamProblem {
            crossed: SurfParamType::Both,
            missing: SurfParamType::Both,
            near_miss: SurfParamType::Neither,
        };
        let e = split_at_seam_depth(
            &k,
            &mut b,
            f,
            &mut cu,
            &mut cv,
            p,
            None,
            &|_| None,
            MAX_SPLIT_AT_SEAM_DEPTH + 1,
        );
        assert!(matches!(e, Err(SolidError::InvalidInput { .. })));
    }

    #[test]
    fn oriented_boundary_of_a_face_without_loops() {
        let (mut b, f) = plane_face();
        let k = Kernel::default();
        // no face use: upstream dereferences null
        assert!(create_oriented_trim_boundary(&k, &mut b, f, true, true, None, &key_of).is_err());
    }
}
