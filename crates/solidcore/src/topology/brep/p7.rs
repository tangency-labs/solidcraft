//! `SmBrep`, part 7: reading a B-rep from a database file, making faces from edge uses
//! or from curves, gluing edges, copying faces between B-reps and merging the
//! self-intersection curves of a surface.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust                                         | Upstream                                     |
//! |----------------------------------------------|----------------------------------------------|
//! | [`read_from_file`]                           | `SmBrep::ReadFromFile`                       |
//! | [`make_face`], [`MakeFaceOptions`], [`NewFace`] | `SmBrep::MakeFace`                        |
//! | [`make_face_with_curves`], [`FaceCurvesInput`] | `SmBrep::MakeFaceWithCurves`               |
//! | [`glue_edges_general`]                       | `SmBrep::GlueEdgesGeneral`                   |
//! | [`copy_faces`]                               | `SmBrep::CopyFaces`                          |
//! | [`merge_self_intersection_curves`], [`SelfIntersectionCurve`] | `SmBrep::MergeSelfIntersectionCurves` |
//!
//! Deviations:
//! - **Object model.** The B-rep is a [`Brep`] of arenas; the methods are free functions
//!   taking it. Pointer arguments are `Option`s of ids (a null region is the infinite
//!   region), out-parameters are return values. Where upstream dereferences null the port
//!   returns [`SolidError::InvalidInput`] (`NER` is `SM_ERR_NULL_POINTER`).
//! - **`ReadFromFile`** takes the file's contents (the library reads no files: it must
//!   build for `wasm32-unknown-unknown`); the caller reads the file. The reader is
//!   [`BrepData::read_from_file`] (unit `topology.brep_data`), which bounds every count it
//!   reads and the nesting of its polymorphic readers (AGENTS.md section 1).
//! - **Arena records are never freed.** Upstream `delete`s of edge uses, vertex uses,
//!   shells, regions and edges unlink the record (it stays in its arena as an orphan).
//!   The B-rep's edge list is its edge arena, so `m_pEdgeListHead->Remove` has no
//!   counterpart beyond unlinking the deleted edge's uses. The owning-list edits
//!   `SmEdge::Remove(eu)`, `SmVertex::Remove(vu)`, `SmRegion::Remove(shell)` and
//!   `SmBrep::Remove(region)` (inline `SmOwningTopology` operations) edit the records'
//!   use lists and back links.
//! - **Curves and surfaces are values.** `MakeFaceWithCurves` consumes its curves and
//!   surface (upstream: "all inputs are consumed"), so "two copies, never two references"
//!   holds by construction; `MakeFace` takes the surface by value (`None` is a null
//!   pointer). `CopyFaces` copies from one B-rep into another one: upstream also allows
//!   the destination to be the source B-rep, which a shared and a mutable borrow cannot
//!   express (copy within one B-rep through a clone).
//! - **Index checks.** Loop counts that add up to more curves, orientations or edge uses
//!   than given, and self-intersection UV curve arrays shorter than the 3D curves, are
//!   [`SolidError::InvalidInput`] (upstream reads past its arrays).
//! - **Flags.** `IsDegenerate`, `IsDegenerateCurve` and `IsDegeneratePoint` return flags
//!   upstream; a Rust error of those tests (other than [`SolidError::Unported`]) counts as
//!   "not degenerate".
//! - **Gaps in other units' ports** (`plan/findings.md`): the port of
//!   `SmCurve::DeformToEndPointTargets` (unit `curve.curve_p6`) has no `pOptSurfCrvOnSurf`,
//!   so deforming a curve on surface to a gap target (upstream edits its UV curve) returns
//!   [`SolidError::Unported`] for `curve.curve_p6`; other curves, and curves on surface
//!   without targets, call the port. A curve on surface of any surface but a plain
//!   `SmBSplineSurface` (the port's `CrvOnSurf` holds one; a plane's or a surface of
//!   revolution's B-spline base would lose the kind's evaluation overrides) returns
//!   [`SolidError::Unported`] for `curve.crv_on_surf`, as at that gap's other sites.
//! - **Statuses ignored upstream** (`LiftCurve`, `DeformToEndPointTargets`, `ConvertTo2D`
//!   of the UV curves, `GetEnds`, `CopyAndAddAnalytics`, `FindRadialSector`,
//!   `SetUVTrimCurve`, `TestClosureSplitRegion`, `sm_CheckAndFixUVTrimCurves`,
//!   `RebuildUVTrimCurves`, `MakeFaceuse`) are ignored; [`SolidError::Unported`] always
//!   propagates. A failed `LiftCurve` leaves no 3D curve (a degenerate one), as upstream;
//!   a failed `GetEnds` keeps the previous end point. A failed `CopyAndAddAnalytics` is
//!   [`SolidError::InvalidInput`] (upstream then dereferences the null copy), as is
//!   `pbOptCrvOnSurf` without UV curves (upstream loops over a null array).
//! - **Unported callees** return [`SolidError::Unported`]: `SmBrep::Notify`,
//!   `SetTolerance`, `sm_CheckAndFixUVTrimCurves`, `RebuildUVTrimCurves`
//!   (`topology.brep_p2`); `MakeWireEdge`, `MakeWireEdgeVertex`
//!   (`topology.brep_p3`); `DeleteEdge` (`topology.brep_p4`); `MakeShellVertexTopology`
//!   (`topology.brep_p2`, reached through `topology.brep_p9`'s `MakeShellVertex`);
//!   `SmFace::GetBrep`, `GetEdges`
//!   (`topology.face_p4`); `SmMerge` (`boolean.merge_p2`); the curve constructor of
//!   `SmCurveClassification` (`intersect.curve_class_p4`), which
//!   `MergeSelfIntersectionCurves` reaches first. `GlueVertices` calls
//!   `topology.brep_p5` with the caller's notification environment. Its classification of a UV curve
//!   against the faces and the `SetCurve` + `MergeIntervals` that follows are two stubs
//!   (`classify_on_faces`, `set_curve_and_merge`) over a stand-in interval type; the
//!   upstream interval loop around them is ported (`GetSize() != 1` skips before the
//!   second merge, `SM_ERR` after it, `SE(SM_ERR)` for a middle not on an edge).
//!   `SmShell::IsInnerShell` (merged shells) calls `topology.shell`'s port;
//!   `MakeFaceTopology` and `TestClosureSplitRegion(faceuse)` call `topology.brep_p6`'s;
//!   `MakeTopologyFromData` and `TestClosureSplitRegion(edge)` call
//!   `topology.brep_p10`'s (`ReadFromFile` takes that unit's [`TopologyEnv`]).
//!   `GetInfiniteRegion` is the inline read [`Brep::infinite_region`], and the glue's
//!   removal of an emptied region walks `topology.brep_p1`'s region list.
//!   That `MakeFaceTopology` names the face for its surface's owner before the face
//!   exists, so it gets the key `this_face` gives the next face id (arenas only append);
//!   it builds the face on a copy of the surface, which `MakeFace` then replaces with
//!   its own (upstream: one shared surface whose owner `MakeFace` restores).
//! - **`MakeFace`** returns nothing when `MakeFaceTopology` fails; upstream still hands
//!   back the region and shell it may have made (`MakeFaceWithCurves` warns about it).
//! - **`GlueEdgesGeneral`**: the arena cannot share one curve between two edges, so
//!   upstream's `pSurvCurve == pDelCurve` short cut (no `CurveMaxDistanceBetween`) is taken
//!   only for two edges without curve (`NULL == NULL`). The glued-away edge stays in the edge arena as an orphan without edge uses;
//!   enumerations of the B-rep's edges must skip edges without edge uses (upstream removes
//!   it from `m_pEdgeListHead`).
//! - **Bounds.** No recursion; every loop is over input arrays or B-rep lists.

use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::tools::{
    find_coincident_edge, find_first_vertex_within_tol_to_pnt, CoincidentEdgeQuery,
};
use crate::kernel::Kernel;
use crate::math::{Point3, EFF_ZERO, EFF_ZERO_DEG};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::InValueType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1;
use crate::topology::brep::p10::{self, MakeTopologyOptions, TopologyEnv};
use crate::topology::brep::p6;
use crate::topology::brep_data::BrepData;
use crate::topology::database_io::FileType;
use crate::topology::edge as ed;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as fp2;
use crate::topology::topology_traverser::{self as tt, Marks};
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, RegionId, ShellId, VertexId};

const OP: &str = "topology::brep::p7";

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `SM_ZONE_TOL_3D / 10.0`: the default point tolerance of the surface degeneracy tests.
const DEGENERATE_TOL_3D: f64 = crate::geom::tol::ZONE_TOL_3D / 10.0;

/// `SmBrep::Remove(region)` (the inline `SmOwningTopology::Remove`): takes `region` off
/// the B-rep's region list (`topology.brep_p1`'s [`p1::regions`] order); removing the
/// list head passes the head to the next region, or `NULL` when the list empties.
///
/// # Errors
///
/// `SM_ERR` when `region` is not on the list.
fn brep_remove_region(b: &mut Brep, region: RegionId) -> Result<()> {
    let list = p1::regions(b, None)?;
    if !list.contains(&region) {
        return Err(SolidError::sm_err(OP));
    }
    let head = list.first().copied();
    let new_head = if head == Some(region) {
        list.get(1).copied()
    } else {
        head
    };
    b.region_mut(region)?.in_brep = false;
    b.set_region_list_head(new_head);
    Ok(())
}

// ---- unported callees ---------------------------------------------------------------------

/// `SmBrep::Notify(op, data1, data2, data3)` (`None` for a null datum or for the B-rep,
/// which has no identity here).
fn brep_notify(_b: &mut Brep, _op: NotifyOperation, _data: [Option<EntityRef>; 3]) -> Result<()> {
    // UNPORTED(topology.brep_p2): SmBrep::Notify
    Err(SolidError::unported("SmBrep::Notify", "topology.brep_p2"))
}

/// `SmBrep::SetTolerance(tol, bUpdateOnlyIfLarger, bCascadeToBndries)` (`SM_USE_OLDTOL`).
fn brep_set_tolerance(
    _b: &mut Brep,
    _tol: f64,
    _update_only_if_larger: bool,
    _cascade_to_boundaries: bool,
) -> Result<()> {
    // UNPORTED(topology.brep_p2): SmBrep::SetTolerance
    Err(SolidError::unported(
        "SmBrep::SetTolerance",
        "topology.brep_p2",
    ))
}

/// `sm_CheckAndFixUVTrimCurves(this)`.
fn check_and_fix_uv_trim_curves(_k: &Kernel, _b: &mut Brep) -> Result<()> {
    // UNPORTED(topology.brep_p2): sm_CheckAndFixUVTrimCurves
    Err(SolidError::unported(
        "sm_CheckAndFixUVTrimCurves",
        "topology.brep_p2",
    ))
}

/// `RebuildUVTrimCurves()`.
fn rebuild_uv_trim_curves(_k: &Kernel, _b: &mut Brep) -> Result<()> {
    // UNPORTED(topology.brep_p2): SmBrep::RebuildUVTrimCurves
    Err(SolidError::unported(
        "SmBrep::RebuildUVTrimCurves",
        "topology.brep_p2",
    ))
}

/// `MakeWireEdge(region, start, end, curve, ivl, orient, rpNewEdge)`.
fn make_wire_edge(
    _b: &mut Brep,
    _region: Option<RegionId>,
    _start: VertexId,
    _end: VertexId,
    _curve: Curve,
    _ivl: Extent1d,
    _orient: OrientType,
) -> Result<EdgeId> {
    // UNPORTED(topology.brep_p3): SmBrep::MakeWireEdge
    Err(SolidError::unported(
        "SmBrep::MakeWireEdge",
        "topology.brep_p3",
    ))
}

/// `MakeWireEdgeVertex(region, start, curve, ivl, orient, endPt, rpNewEdge, rpEndVertex)`.
fn make_wire_edge_vertex(
    _b: &mut Brep,
    _region: Option<RegionId>,
    _start: VertexId,
    _curve: Curve,
    _ivl: Extent1d,
    _orient: OrientType,
    _end_pt: Point3,
) -> Result<(EdgeId, VertexId)> {
    // UNPORTED(topology.brep_p3): SmBrep::MakeWireEdgeVertex
    Err(SolidError::unported(
        "SmBrep::MakeWireEdgeVertex",
        "topology.brep_p3",
    ))
}

/// `DeleteEdge(edge)`.
fn delete_edge(_b: &mut Brep, _e: EdgeId) -> Result<()> {
    // UNPORTED(topology.brep_p4): SmBrep::DeleteEdge
    Err(SolidError::unported(
        "SmBrep::DeleteEdge",
        "topology.brep_p4",
    ))
}

/// `GlueVertices(survivor, other)` (default `pOptWireEdgesInBrep = NULL`), with the
/// caller's notification environment.
fn glue_vertices(
    k: &mut Kernel,
    b: &mut Brep,
    v1: VertexId,
    v2: VertexId,
    env: &mut ed::NotifyEnv<'_>,
) -> Result<()> {
    super::p5::glue_with_notifications(k, b, Some(v1), Some(v2), None, Some(env))
}

/// `TestClosureSplitRegion(TRUE, edge, aNewShells, aNewRegions)` (unit
/// `topology.brep_p10`; status ignored by the caller).
fn test_closure_split_region_edge(k: &mut Kernel, b: &mut Brep, e: EdgeId) -> Result<()> {
    p10::test_closure_split_region(k, b, true, e).map(|_| ())
}

/// `MakeShellVertex(region, point, rpNewShell, rpNewVertex)`.
fn make_shell_vertex(
    b: &mut Brep,
    region: Option<RegionId>,
    point: Point3,
    env: &mut ed::NotifyEnv<'_>,
) -> Result<(ShellId, VertexId)> {
    super::p9::make_shell_vertex(b, region, point, env)
}

/// `MakeTopologyFromData(pBrepData, sAttributes, bMakeUVTrimCurves, bAddAnalytics,
/// bForceSameTol)` (unit `topology.brep_p10`) with the defaults `bCheckUVCurves =
/// FALSE`, `bAlwaysHeal = FALSE`, `bHealerIsEnabled = TRUE`. `ReadFromFile` passes an
/// empty local `sAttributes`, so the records' attribute indices refer to nothing.
fn make_topology_from_data(
    k: &mut Kernel,
    b: &mut Brep,
    data: &mut BrepData,
    env: &mut TopologyEnv<'_>,
    make_uv_trim_curves: bool,
    add_analytics: bool,
    force_same_tol: bool,
) -> Result<()> {
    // SmTArray<SmAttribute*> sAttributes; (empty)
    let saved = std::mem::take(&mut env.all_attributes);
    let r = p10::make_topology_from_data(
        k,
        b,
        data,
        env,
        MakeTopologyOptions {
            make_uv_trim_curves,
            add_analytics,
            force_same_tol,
            check_uv_trim_curves: false,
            always_heal: false,
            healer_enabled: true,
        },
    );
    env.all_attributes = saved;
    r
}

/// `SmFace::Notify(op, data1, data2, data3)` on face `f` (`None` for a null datum or for
/// the B-rep, which has no identity here).
fn face_notify(
    b: &mut Brep,
    f: FaceId,
    op: NotifyOperation,
    data: [Option<EntityRef>; 3],
) -> Result<()> {
    crate::topology::loopuse::face_notify_data(b, f, op, data)
}

/// `SmFace::SetTolerance(tol, bUpdateOnlyIfLarger, bCascadeToBndries)` (`SM_USE_OLDTOL`).
fn face_set_tolerance(
    b: &mut Brep,
    f: FaceId,
    tol: f64,
    update_only_if_larger: bool,
    cascade_to_boundaries: bool,
) -> Result<()> {
    crate::topology::face::p3::set_tolerance(
        b,
        f,
        tol,
        update_only_if_larger,
        cascade_to_boundaries,
    )
}

/// `pNewFace->CreateUVTrimCurves(bAdjustTolerances, &uvCurves, &orients, ..)`.
fn face_create_uv_trim_curves(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    adjust_tolerances: bool,
    uv: Vec<Curve>,
    orients: &[OrientType],
) -> Result<()> {
    let mut curves: Vec<Option<Curve>> = uv.into_iter().map(Some).collect();
    // the gap statistics (the NULL out-parameters) are not read
    crate::topology::face::p3::create_uv_trim_curves(
        k,
        b,
        f,
        adjust_tolerances,
        Some((&mut curves, orients)),
    )
    .map(|_| ())
}

/// `pFace->GetBrep() == this`.
fn face_in_brep(_b: &Brep, _f: FaceId) -> Result<bool> {
    // UNPORTED(topology.face_p4): SmFace::GetBrep
    Err(SolidError::unported("SmFace::GetBrep", "topology.face_p4"))
}

/// `pFace->GetEdges(sEdges)`.
fn face_edges(_b: &Brep, _f: FaceId) -> Result<Vec<EdgeId>> {
    // UNPORTED(topology.face_p4): SmFace::GetEdges
    Err(SolidError::unported("SmFace::GetEdges", "topology.face_p4"))
}

/// `SmMerge(ctx, dest, src, tol, 20 deg)`, then per face the `GetBrep() == this` check
/// (`SM_ERR`) and its `MakeFaceuse`.
fn merge_copy_faces(
    _k: &Kernel,
    _src: &Brep,
    _dest: &mut Brep,
    _faces: &[FaceId],
) -> Result<Vec<FaceId>> {
    // UNPORTED(boolean.merge_p2): SmMerge::SmMerge
    Err(SolidError::unported("SmMerge::SmMerge", "boolean.merge_p2"))
}

/// One interval of a self-intersection curve's classification: its parameter interval
/// and the edge its middle is classified to (`None`: the middle is not on an edge). A
/// stand-in for `SmCurveClassification` (it drops the end point classes `MergeIntervals`
/// reads): wiring replaces it with `intersect.curve_class_p4`'s classification.
type ClassifiedInterval = (Extent1d, Option<EdgeId>);

/// `SmCurveClassification sCC(uvCurve, ivl, uvCurve, tol, 16, data, FALSE, TRUE,
/// &surface)` with `surface` the surface of `face`, then `CurveOnClassify(TRUE, sCC)` on
/// each of `faces`: the classification's intervals.
fn classify_on_faces(
    _b: &Brep,
    _faces: &[FaceId],
    _face: FaceId,
    _uv_curve: &Curve,
    _ivl: Extent1d,
    _tol: f64,
) -> Result<Vec<ClassifiedInterval>> {
    // UNPORTED(intersect.curve_class_p4): SmCurveClassification::SmCurveClassification
    // (then SmFace::CurveOnClassify, unit query.face_p2)
    Err(SolidError::unported(
        "SmCurveClassification::SmCurveClassification",
        "intersect.curve_class_p4",
    ))
}

/// `sCC.SetCurve(curve_3d)` and `sCC.MergeIntervals(&newFaces, NULL, NULL, face)` on a
/// classification of [`classify_on_faces`]: the faces the merge made; `intervals` become
/// the merged classification's.
fn set_curve_and_merge(
    _b: &mut Brep,
    _intervals: &mut Vec<ClassifiedInterval>,
    _curve_3d: &Curve,
    _face: FaceId,
) -> Result<Vec<FaceId>> {
    // UNPORTED(intersect.curve_class_p4): SmCurveClassification::MergeIntervals over a
    // classification of B-rep faces (the ported MergeIntervals needs the classification
    // the unported constructor makes)
    Err(SolidError::unported(
        "SmCurveClassification::MergeIntervals (B-rep classification)",
        "intersect.curve_class_p4",
    ))
}

// ---- owning-list edits ----------------------------------------------------------------------

/// `SmEdge::Remove(eu)`: unlinks the edge use from the edge.
fn edge_remove(b: &mut Brep, e: EdgeId, eu: EdgeuseId) -> Result<()> {
    let list = &mut b.edge_mut(e)?.edgeuses;
    let Some(at) = list.iter().position(|&x| x == eu) else {
        return Err(SolidError::sm_err(OP));
    };
    list.remove(at);
    b.edgeuse_mut(eu)?.edge = None;
    Ok(())
}

/// Merges `del_shell` into `keep_shell` and drops the empty region (the shell-merging
/// tails of `GlueEdgesGeneral`).
fn merge_shells(k: &mut Kernel, b: &mut Brep, mut del: ShellId, mut keep: ShellId) -> Result<()> {
    // m_pInfiniteRegion (NULL when unset; pointer equality, NULL == NULL)
    let inf = b.infinite_region();
    let keep_region = eu1::shell_region(b, keep)?;
    let del_region = eu1::shell_region(b, del)?;
    if keep_region == inf {
        // ok.
    } else if del_region == inf
        // pKeepShell->IsInnerShell(bIsInner): swap when it succeeds with TRUE (asked
        // only when neither region is the infinite one)
        || ignored(b.shell(keep)?.is_inner_shell(k, b))? == Some(true)
    {
        core::mem::swap(&mut del, &mut keep);
    }

    tt::remove_shell(k, b, del, keep)?;

    // remove it from its region, then the region when empty
    let reg = eu1::shell_region(b, del)?.ok_or_else(|| null("shell without region"))?;
    // pReg->Remove(pDelShell), then pReg->GetSize()
    crate::topology::shell::region_remove_shell(b, reg, del)?;
    if b.region(reg)?.shells.is_empty() {
        remove_emptied_region(b, reg)?;
    }
    Ok(())
}

/// The tail of the shell merge once `reg` lost its last shell: `this->Remove(pReg)`
/// (status ignored), then `m_pInfiniteRegion = m_pList`.
fn remove_emptied_region(b: &mut Brep, reg: RegionId) -> Result<()> {
    ignore_status(brep_remove_region(b, reg))?;
    // make sure infinite region points where it should: the first region of the list
    // (a stored head of None starts at the first region on the list; it is not an
    // empty list)
    let head = p1::regions(b, None)?.first().copied();
    b.set_infinite_region(head);
    Ok(())
}

// ---- ReadFromFile ---------------------------------------------------------------------------

/// `ReadFromFile`: reads one B-rep from the contents of a database file of `file_type`
/// into `b` (a B-rep as `SmBrep()` makes it: see
/// [`Brep::new_with_infinite_region`]), then checks and fixes its UV trim curves, or
/// rebuilds them with `rebuild_uv_trim_curves`. `env` names the new topology objects and
/// receives their records and the attributes of the geometry copies (its
/// `all_attributes` is not used: upstream passes an empty array).
///
/// # Errors
///
/// Those of [`BrepData::read_from_file`] and
/// [`p10::make_topology_from_data`]; [`SolidError::Unported`]
/// (`sm_CheckAndFixUVTrimCurves`, `RebuildUVTrimCurves`, and the callees of
/// `MakeTopologyFromData`).
pub fn read_from_file(
    k: &mut Kernel,
    b: &mut Brep,
    data: Vec<u8>,
    file_type: FileType,
    rebuild_uv_trim_curves_flag: bool,
    env: &mut TopologyEnv<'_>,
) -> Result<()> {
    let mut brep_data = BrepData::read_from_file(k, data, file_type)?;
    // MakeTopologyFromData(pBrepData, sAttributes, FALSE, TRUE, TRUE)
    make_topology_from_data(k, b, &mut brep_data, env, false, true, true)?;
    drop(brep_data);

    // check the UV trim curves unless they are rebuilt (statuses ignored)
    if !rebuild_uv_trim_curves_flag {
        ignore_status(check_and_fix_uv_trim_curves(k, b))?;
    }
    if rebuild_uv_trim_curves_flag {
        ignore_status(rebuild_uv_trim_curves(k, b))?;
    }
    Ok(())
}

// ---- MakeFace -------------------------------------------------------------------------------

/// The optional arguments of [`make_face`] (upstream defaults: no radial sector check, no
/// UV trim curves, none given).
#[derive(Debug, Clone, Copy, Default)]
pub struct MakeFaceOptions<'a> {
    /// `bVerifyRadialSectors`: check the face goes into the right radial sectors
    /// (non-manifold models).
    pub verify_radial_sectors: bool,
    /// `bCalculateTrims`: attach or make a UV trim curve on every edge use.
    pub calculate_trims: bool,
    /// `cpOptUVCurves`: UV trim curves to assign to the new edge uses (`None` entries
    /// are null).
    pub uv_curves: Option<&'a [Option<BSplineCurve>]>,
}

/// The topology [`make_face`] and [`make_face_with_curves`] made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NewFace {
    /// `rpNewRegion`: the region split off, if the face split one.
    pub region: Option<RegionId>,
    /// `rpNewShell`: the new shell, if any.
    pub shell: Option<ShellId>,
    /// `rpNewFace`: the new face.
    pub face: Option<FaceId>,
}

/// `MakeFace`: makes a face on `surface` (over `uv_domain`) in `region` (the infinite
/// region for `None`) from existing edge uses, `loops` giving the number of edge uses of
/// each loop (outer loop first, counter-clockwise when `orientation` is
/// [`OrientType::Same`]), and loop vertices. The face owns the surface and takes the
/// B-rep's tolerance. `this_face` names the new face for its surface's owner.
///
/// # Errors
///
/// `SM_ERR_NULL_POINTER` without surface; those of the topology build;
/// [`SolidError::Unported`] (`SmFace::SetTolerance`, which `MakeFaceTopology` reaches
/// first).
#[allow(clippy::too_many_arguments)]
pub fn make_face(
    _k: &Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    loops: &[usize],
    edgeuses: &[EdgeuseId],
    loop_vertices: &[VertexId],
    surface: Option<Surface>,
    uv_domain: &Extent2d,
    orientation: OrientType,
    opts: &MakeFaceOptions<'_>,
    this_face: &dyn Fn(FaceId) -> ObjectRef,
) -> Result<NewFace> {
    let Some(mut surface) = surface else {
        return Err(SolidError::null_pointer(OP));
    };
    // GetInfiniteRegion() (an inline field read; NULL when unset)
    let region = region.or(b.infinite_region());

    // get preexisting surface owner
    let old_owner = surface.owner();
    // the face p6 allocates next (arenas only append)
    let next_face = FaceId::from_index(
        u32::try_from(b.faces().len())
            .map_err(|_| SolidError::invalid_input(OP, "too many faces"))?,
    );
    let built = p6::make_face_topology(
        b,
        Some(surface.clone()),
        orientation,
        this_face(next_face).key,
        &p6::FaceTopologyOptions {
            region,
            edge_loops: loops,
            edgeuses,
            loop_vertices,
            verify_radial_sectors: opts.verify_radial_sectors,
            calculate_trims: opts.calculate_trims,
            uv_curves: opts.uv_curves,
        },
    );
    // restore the preexisting surface owner, whether the build fails or succeeds
    surface.set_owner(old_owner);
    let built = built?;
    let (new_region, new_shell, new_face) = (built.region, built.shell, built.face);

    b.face_mut(new_face)?.uv_domain = *uv_domain;

    // default face tolerance to Brep tolerance
    let tol = b.tolerance();
    // SetTolerance(GetTolerance()) = (tol, TRUE, TRUE): grow only, cascade
    face_set_tolerance(b, new_face, tol, true, true)?;

    // make the face->surface connections
    surface.set_owner(Some(this_face(new_face)));
    b.face_mut(new_face)?.surface = Some(surface);

    Ok(NewFace {
        region: new_region,
        shell: new_shell,
        face: Some(new_face),
    })
}

// ---- MakeFaceWithCurves ---------------------------------------------------------------------

/// The curves of [`make_face_with_curves`]: one entry of `loops` per loop (its curve count,
/// outer loop first) over the curves in loop order, each with its orientation; 3D curves
/// (`None` entries are degenerate), UV curves (B-spline kinds), or both.
#[derive(Debug, Clone, Default)]
pub struct FaceCurvesInput {
    /// `crCurveLoops`.
    pub loops: Vec<usize>,
    /// `cpOpt3DCurves`.
    pub curves_3d: Option<Vec<Option<Curve>>>,
    /// `cpOptUVCurves`.
    pub uv_curves: Option<Vec<Curve>>,
    /// `crCurveOrientations`.
    pub orientations: Vec<OrientType>,
    /// `crLoopPoints`: the points of vertex loops.
    pub loop_points: Vec<Point3>,
}

/// The curve of `curves[i]`, which upstream reads unchecked.
fn at<T>(v: &[T], i: usize) -> Result<&T> {
    v.get(i)
        .ok_or_else(|| null("loop counts exceed the curves"))
}

/// The start of `c` as oriented by `orient` (`GetEnds`, status ignored: `prev` kept).
fn oriented_end(c: &Curve, orient: OrientType, start: bool, prev: Point3) -> Result<Point3> {
    Ok(match ignored(c.ends())? {
        Some(e) => {
            if (orient == OrientType::Same) == start {
                e.start
            } else {
                e.end
            }
        }
        None => prev,
    })
}

/// `MakeFaceWithCurves`: makes a face on `surface` (over `uv_domain`, `orientation` as in
/// [`make_face`]) from loops of curves: with UV curves only, each is lifted to 3D (or made
/// a curve on surface when `crv_on_surf`) and gaps larger than the B-rep tolerance are
/// closed by moving the curve ends to the gap middles; then wire edges and vertices are
/// made per curve (degenerate curves dropped, coincident ones shared) and the face is
/// made from their edge uses. `this_face` names the new face for its surface's owner.
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` without curves or loops, for a degenerate surface, or when a
/// loop does not close; `SM_ERR` for an edge shorter than the tolerance that closes;
/// [`SolidError::InvalidInput`] for loop counts exceeding the curves; those of the
/// evaluations; [`SolidError::Unported`] (see the module docs).
#[allow(clippy::too_many_lines)] // one upstream routine
pub fn make_face_with_curves(
    k: &Kernel,
    b: &mut Brep,
    region: Option<RegionId>,
    input: FaceCurvesInput,
    surface: Surface,
    uv_domain: &Extent2d,
    orientation: OrientType,
    crv_on_surf: Option<bool>,
    this_face: &dyn Fn(FaceId) -> ObjectRef,
    env: &mut ed::NotifyEnv<'_>,
) -> Result<NewFace> {
    let FaceCurvesInput {
        loops,
        curves_3d,
        mut uv_curves,
        orientations,
        loop_points,
    } = input;
    let mut out = NewFace::default();

    // check input
    if curves_3d.is_none() && uv_curves.is_none() {
        return Err(SolidError::invalid_input(OP, "no curves"));
    }
    if loops.is_empty() {
        return Err(SolidError::invalid_input(OP, "no loops"));
    }
    // GetInfiniteRegion() (an inline field read; NULL when unset)
    let region = region.or(b.infinite_region());
    // (the surface tests return flags upstream: a failed test is "no")
    if ignored(surface.is_degenerate_curve(EFF_ZERO_DEG, None, DEGENERATE_TOL_3D))?.unwrap_or(false)
    {
        return Err(SolidError::invalid_input(
            OP,
            "surface degenerates to a curve",
        ));
    }
    if ignored(surface.is_degenerate_point(DEGENERATE_TOL_3D, None))?.unwrap_or(false) {
        return Err(SolidError::invalid_input(
            OP,
            "surface degenerates to a point",
        ));
    }

    // locals
    let total: usize = loops
        .iter()
        .try_fold(0usize, |a, &n| a.checked_add(n))
        .ok_or_else(|| null("loop counts overflow"))?;
    let zone_tol = b.tolerance();
    let crv_on_surf = crv_on_surf == Some(true);

    // only UV curves: lift them to 3D, fixing gaps larger than tolerance
    let mut curves_3d: Vec<Option<Curve>> = if let Some(c) = curves_3d {
        c
    } else {
        {
            let uv = uv_curves.as_deref_mut().ok_or_else(|| null("no curves"))?;
            if total > uv.len() || total > orientations.len() {
                return Err(null("loop counts exceed the curves"));
            }
            let mut s3d: Vec<Option<Curve>> = Vec::new();
            let mut index = 0usize;
            for &n in &loops {
                // (index into s3d, curve) of the non-degenerate 3D curves of this loop
                let mut to_check: Vec<usize> = Vec::new();
                for jj in 0..n {
                    // (upstream converts the caller's curve: the copies made for the UV
                    // trim curves later are the converted ones)
                    let entry = uv
                        .get_mut(index + jj)
                        .ok_or_else(|| null("loop counts exceed the curves"))?;
                    ignore_status(entry.convert_to_2d(k))?;
                    let uv_curve = entry.clone();
                    let new3d = if crv_on_surf {
                        let base = match surface.kind() {
                            SurfaceKind::BSpline(s) => s.clone(),
                            // (a plane's or a surface of revolution's B-spline base would drop
                            // the kind's evaluation overrides, as every other kind's would)
                            _ => {
                                // UNPORTED(curve.crv_on_surf): SmCrvOnSurf over a non-B-spline
                                // surface (the Rust CrvOnSurf holds only B-spline surfaces)
                                return Err(SolidError::unported(
                                    "SmCrvOnSurf (non-B-spline surface)",
                                    "curve.crv_on_surf",
                                ));
                            }
                        };
                        let c = CrvOnSurf::new(k, uv_curve, base, Some(*uv_domain), 2)?;
                        let c = Curve::from(c);
                        // (IsDegenerate returns a flag upstream: a failed test is "no")
                        if ignored(c.is_degenerate(EFF_ZERO, None))?.unwrap_or(false) {
                            None
                        } else {
                            Some(c)
                        }
                    } else {
                        let ivl = uv_curve.natural_interval();
                        ignored(surface.lift_curve(
                            k,
                            uv_domain,
                            &uv_curve,
                            &ivl,
                            zone_tol / 4.0,
                            false,
                        ))?
                        .map(|l| Curve::from(l.curve))
                    };
                    if new3d.is_some() {
                        to_check.push(s3d.len());
                    }
                    s3d.push(new3d);
                }

                // the gap middles of gaps larger than tolerance (at each curve's start)
                let tol2 = zone_tol * zone_tol;
                let nc = to_check.len();
                let mut new_points: Vec<Option<Point3>> = vec![None; nc];
                // (sCurveStart / sCWCurveEnd keep the previous iteration's value when
                // GetEnds fails)
                let mut start = Point3::UNDEF;
                let mut cw_end = Point3::UNDEF;
                for jj in 0..nc {
                    let cw_jj = if jj == 0 { nc - 1 } else { jj - 1 };
                    let ci = *at(&to_check, jj)?;
                    let cwi = *at(&to_check, cw_jj)?;
                    // (upstream indexes the orientations by the position among the
                    // non-degenerate curves)
                    let cw_index = if jj == 0 {
                        index + n - 1
                    } else {
                        index + jj - 1
                    };
                    let o = *at(&orientations, index + jj)?;
                    let cwo = *at(&orientations, cw_index)?;
                    let c = at(&s3d, ci)?
                        .as_ref()
                        .ok_or_else(|| null("missing curve"))?;
                    let cw = at(&s3d, cwi)?
                        .as_ref()
                        .ok_or_else(|| null("missing curve"))?;
                    start = oriented_end(c, o, true, start)?;
                    cw_end = oriented_end(cw, cwo, false, cw_end)?;
                    if start.distance_squared(cw_end) > tol2 {
                        if let Some(slot) = new_points.get_mut(jj) {
                            *slot = Some((start + cw_end) / 2.0);
                        }
                    }
                }

                // deform the curves to the gap middles (statuses ignored)
                for jj in 0..nc {
                    let new_start = *at(&new_points, jj)?;
                    let new_end = *at(&new_points, if jj + 1 == nc { 0 } else { jj + 1 })?;
                    let ci = *at(&to_check, jj)?;
                    let Some(Some(curve)) = s3d.get_mut(ci) else {
                        return Err(null("missing curve"));
                    };
                    // upstream passes the surface as pOptSurfCrvOnSurf, so a curve on surface
                    // with a target edits its UV curve in place (DropPoint of the targets);
                    // the port of DeformToEndPointTargets has no such parameter and would
                    // approximate the curve instead. Without targets upstream returns at once,
                    // and other curve kinds dispatch before pOptSurfCrvOnSurf is read.
                    if matches!(curve.kind(), CurveKind::CrvOnSurf(_))
                        && (new_start.is_some() || new_end.is_some())
                    {
                        // UNPORTED(curve.curve_p6): SmCurve::DeformToEndPointTargets
                        // (pOptSurfCrvOnSurf branch)
                        return Err(SolidError::unported(
                            "SmCurve::DeformToEndPointTargets (pOptSurfCrvOnSurf)",
                            "curve.curve_p6",
                        ));
                    }
                    let deformed = ignored(curve.deform_to_end_point_targets(
                        k,
                        new_start,
                        new_end,
                        true,
                        None,
                        InValueType::Unconstrained,
                        None,
                        InValueType::Unconstrained,
                    ))?
                    .flatten();
                    if let Some(d) = deformed {
                        if let Some(slot) = s3d.get_mut(ci) {
                            *slot = Some(d);
                        }
                    }
                }
                index += n;
            }
            s3d
        }
    };
    if total > curves_3d.len() || total > orientations.len() {
        return Err(null("loop counts exceed the curves"));
    }

    // make the wire edges and vertices of every loop
    let mut sorted_loops: Vec<usize> = Vec::new();
    let mut edges: Vec<EdgeId> = Vec::new();
    let mut edge_orients: Vec<u8> = Vec::new();
    let mut curve_total = 0usize;
    for &n in &loops {
        let mut degenerate = 0usize;
        let mut vertices: Vec<VertexId> = Vec::new();
        for jj in 0..n {
            let slot = curve_total + jj;
            let Some(curve) = curves_3d.get_mut(slot).and_then(Option::take) else {
                degenerate += 1;
                continue;
            };
            let ivl = curve.natural_interval();
            let is_degen = ignored(curve.is_degenerate(EFF_ZERO, None))?.unwrap_or(false);
            let approx_len = curve.approximate_length(&ivl, 5, false)?.0;
            if is_degen || approx_len < zone_tol {
                degenerate += 1;
                continue;
            }

            let (start_param, end_param, orient, mut l_orient) =
                if *at(&orientations, slot)? == OrientType::Same {
                    (ivl.min(), ivl.max(), OrientType::Same, 0u8)
                } else {
                    (ivl.max(), ivl.min(), OrientType::Opposite, 1u8)
                };

            // find or make the start vertex
            let start_pt = curve.evaluate_point(start_param)?;
            let mut m = find_first_vertex_within_tol_to_pnt(b, &vertices, start_pt, zone_tol)?;
            if !vertices.is_empty() && !m.found {
                if m.distance < zone_tol * 100.0 {
                    m = find_first_vertex_within_tol_to_pnt(
                        b,
                        &vertices,
                        start_pt,
                        zone_tol + m.distance,
                    )?;
                }
                if !m.found {
                    return Err(SolidError::invalid_input(
                        OP,
                        "loop gap larger than tolerance",
                    ));
                }
            }
            let found_start = m.found;
            let start_vertex = if vertices.is_empty() {
                let (_shell, v) = make_shell_vertex(b, region, start_pt, env)?;
                vertices.push(v);
                // (upstream assigns the new shell only when it already has one)
                v
            } else {
                *at(&vertices, m.index)?
            };

            // the end vertex
            let end_pt = curve.evaluate_point(end_param)?;
            if end_pt.distance_squared(start_pt) < zone_tol * zone_tol && approx_len < zone_tol {
                return Err(SolidError::sm_err(OP));
            }
            let mut me = find_first_vertex_within_tol_to_pnt(b, &vertices, end_pt, zone_tol)?;
            if jj + 1 == n && !me.found {
                if me.distance < zone_tol * 100.0 {
                    me = find_first_vertex_within_tol_to_pnt(
                        b,
                        &vertices,
                        end_pt,
                        zone_tol + me.distance,
                    )?;
                }
                if !me.found {
                    return Err(SolidError::invalid_input(OP, "loop does not close"));
                }
            }

            if me.found {
                let end_vertex = *at(&vertices, me.index)?;
                if found_start {
                    let query = CoincidentEdgeQuery {
                        start_vertex,
                        end_vertex,
                        curve: &curve,
                        interval: ivl,
                        tolerance: zone_tol,
                    };
                    if let Some(c) = find_coincident_edge(b, &query, &edges, 0)? {
                        if c.max_deviation < zone_tol * 1.0e-3 {
                            // share the coincident edge (the curve is dropped)
                            let coin = *at(&edges, c.index)?;
                            edges.push(coin);
                            if !c.same_orientation {
                                l_orient = 1 - l_orient;
                            }
                            let coin_orient = *at(&edge_orients, c.index)?;
                            edge_orients.push(if coin_orient == l_orient {
                                coin_orient
                            } else {
                                1 - coin_orient
                            });
                            continue;
                        }
                    }
                }
                let e = make_wire_edge(b, region, start_vertex, end_vertex, curve, ivl, orient)?;
                edges.push(e);
                edge_orients.push(l_orient);
            } else {
                let (e, v) =
                    make_wire_edge_vertex(b, region, start_vertex, curve, ivl, orient, end_pt)?;
                edges.push(e);
                edge_orients.push(l_orient);
                vertices.push(v);
            }
        }
        curve_total += n;
        if n > degenerate {
            sorted_loops.push(n - degenerate);
        }
    }

    // vertex loops
    let mut loop_vertices: Vec<VertexId> = Vec::new();
    for &p in &loop_points {
        let (_shell, v) = make_shell_vertex(b, region, p, env)?;
        loop_vertices.push(v);
    }

    // the edge use of each edge with the wanted orientation
    let mut face_edgeuses: Vec<EdgeuseId> = Vec::new();
    for (i, &e) in edges.iter().enumerate() {
        let o = *at(&edge_orients, i)?;
        for eu in ed::edgeuses(b, e, None, None)? {
            let eo = eu1::orientation(b, eu)?;
            if (eo == OrientType::Same && o == 0) || (eo == OrientType::Opposite && o == 1) {
                face_edgeuses.push(eu);
                break;
            }
        }
    }

    let made = make_face(
        k,
        b,
        region,
        &sorted_loops,
        &face_edgeuses,
        &loop_vertices,
        Some(surface),
        uv_domain,
        orientation,
        &MakeFaceOptions::default(),
        this_face,
    )?;
    out.region = made.region;
    if made.shell.is_some() && out.shell.is_some() {
        out.shell = made.shell;
    }
    out.face = made.face;

    // with curves on surface: make the UV trim curves from the given UV curves
    if crv_on_surf {
        let face = out.face.ok_or_else(|| null("no new face"))?;
        // (upstream loops over cpOptUVCurves->GetSize(): a null array is dereferenced)
        let uv: Vec<Curve> = uv_curves
            .ok_or_else(|| null("crv_on_surf without UV curves (upstream dereferences null)"))?;
        // CreateUVTrimCurves(FALSE, &sNewUVCurves, &crCurveOrientations, ..)
        let r = face_create_uv_trim_curves(k, b, face, false, uv, &orientations);
        match r {
            Err(e) if e.is_unported() => return Err(e),
            Err(SolidError::Upstream { code, .. }) if code == crate::error::SM_ERR => {
                // delete the edge uses of the face's edges
                for e in face_edges(b, face)? {
                    for eu in ed::edgeuses(b, e, None, None)? {
                        edge_remove(b, e, eu)?;
                    }
                }
                return Err(SolidError::sm_err(OP));
            }
            Ok(()) | Err(_) => {}
        }
    }
    Ok(out)
}

// ---- GlueEdgesGeneral -----------------------------------------------------------------------

/// `GlueEdgesGeneral`: merges the edge uses of `to_delete` into `surviving` (edges with
/// common vertices; `relative_orientation` says how their curves run), drops the deleted
/// edge, widens the surviving edge's tolerance by `edge_gap`, merges the shells that now
/// touch and, with `do_region_nesting`, places the edge uses in their radial sectors and
/// tests for split regions. A wire `to_delete` is deleted. Nothing happens without both
/// edges or for one edge.
///
/// # Errors
///
/// `SM_ERR` when the edges do not share both vertices; those of the radial sector, edge
/// use and shell edits; [`SolidError::Unported`] (`SmBrep::Notify` first; see the module
/// docs).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // one upstream routine
pub fn glue_edges_general(
    k: &mut Kernel,
    b: &mut Brep,
    surviving: Option<EdgeId>,
    relative_orientation: OrientType,
    edge_gap: f64,
    do_region_nesting: bool,
    to_delete: Option<EdgeId>,
    inside_faceuses: Option<&[FaceuseId]>,
    env: &mut ed::NotifyEnv<'_>,
) -> Result<()> {
    let (Some(surv), Some(del)) = (surviving, to_delete) else {
        return Ok(());
    };
    if surv == del {
        return Ok(());
    }

    // Notify(SM_NO_PRE_EDIT, this, NULL, NULL)
    brep_notify(b, NotifyOperation::PreEdit, [None, None, None])?;

    // a wire edge is just deleted
    if ed::is_wire(b, del)? {
        delete_edge(b, del)?;
        return Ok(());
    }

    let surv_ref = (env.key_of)(EntityRef::Edge(surv));
    ed::notify(
        b,
        surv,
        env,
        NotifyOperation::PreEdit,
        // (upstream passes (pSurvivingEdge, this, NULL); the B-rep has no identity here
        // and PreEdit does not read it)
        [Some(surv_ref), None, None],
    )?;
    // Notify(SM_NO_MERGE_IN_BREP, pSurvivingEdge, pEdgeToDelete, this)
    brep_notify(
        b,
        NotifyOperation::MergeInBrep,
        [
            Some(EntityRef::Edge(surv)),
            Some(EntityRef::Edge(del)),
            None,
        ],
    )?;
    for f in ed::faces(b, del, None)? {
        // SmFace::Notify(SM_NO_PRE_EDIT, pFace, this, NULL)
        face_notify(
            b,
            f,
            NotifyOperation::PreEdit,
            [Some(EntityRef::Face(f)), None, None],
        )?;
    }

    // the same geometry: the same curve over the same interval, or curves within a
    // thousandth of the edge tolerance whose middles agree
    let mut same_geometry = false;
    if relative_orientation == OrientType::Same {
        let surv_ivl = ed::interval(b, surv)?;
        let del_ivl = ed::interval(b, del)?;
        if crate::geom::tol::are_same(surv_ivl.min(), del_ivl.min())
            && crate::geom::tol::are_same(surv_ivl.max(), del_ivl.max())
        {
            match (&b.edge(surv)?.curve, &b.edge(del)?.curve) {
                // when curves are the same (upstream's pointer test; in the arena only two
                // edges without curve)
                (None, None) => same_geometry = true,
                // test the geometry
                (Some(sc), Some(dc)) => {
                    let surv_tol = b.edge(surv)?.tolerance;
                    let tol = surv_tol / 100.0;
                    let dn = dc.natural_interval();
                    let md = sc.curve_max_distance_between(
                        k,
                        &sc.natural_interval(),
                        dc,
                        dn.min(),
                        dn.max(),
                        4,
                        Some(tol),
                    )?;
                    if md.distance < surv_tol / 1000.00 {
                        let t_mid = surv_ivl.evaluate(0.45678);
                        let sp = sc.evaluate_point(t_mid)?;
                        let dp = dc.evaluate_point(t_mid)?;
                        let tolerance = EFF_ZERO * (1.0 + sp.max_dimension());
                        if sp.distance(dp) < tolerance {
                            same_geometry = true;
                        }
                    }
                }
                // (upstream dereferences the null curve)
                _ => return Err(null("edge without curve")),
            }
        }
    }

    // both vertices must be shared
    let sv_st = ed::start_vertex(b, surv)?;
    let sv_end = ed::other_vertex(b, surv, sv_st)?;
    let dv_st = ed::start_vertex(b, del)?;
    let dv_end = ed::other_vertex(b, del, dv_st)?;
    let st_glued = sv_st == dv_st || sv_st == dv_end;
    let end_glued = sv_end == dv_st || sv_end == dv_end;
    if !st_glued || !end_glued {
        return Err(SolidError::sm_err(OP));
    }

    let mut surv_wire = ed::is_wire(b, surv)?;
    let to_delete_eus = ed::edgeuses(b, del, None, None)?;
    let mut test_split: Vec<FaceuseId> = Vec::new();

    // every mated pair, from the end of the list
    let mut jj = to_delete_eus.len();
    while jj >= 2 {
        let deu1 = *at(&to_delete_eus, jj - 2)?;
        let deu2 = *at(&to_delete_eus, jj - 1)?;

        // the radial sector the pair goes into
        let mut sector_eu: Option<EdgeuseId> = None;
        let mut sector_fu: Option<FaceuseId> = None;
        if do_region_nesting {
            if let Some(s) = ignored(ed::find_radial_sector(
                k,
                b,
                surv,
                deu1,
                None,
                OrientType::Same,
                inside_faceuses,
            ))? {
                sector_eu = s.radial_edgeuse;
                sector_fu = s.coincident_faceuse;
            }
        }
        let mut sector = match sector_eu {
            Some(s) => s,
            None => eu1::primary_edgeuse(b, surv)?.ok_or_else(|| null("edge without edge use"))?,
        };

        // coincident faces: take the side whose regions match
        if sector_fu.is_some() {
            let reg = |b: &Brep, eu: Option<EdgeuseId>| -> Result<Option<RegionId>> {
                let eu = eu.ok_or_else(|| null("missing edge use"))?;
                match eu1::shell(b, eu)? {
                    Some(s) => eu1::shell_region(b, s),
                    None => Err(null("edge use without shell")),
                }
            };
            let mate = eu1::mate(b, sector)?;
            let r_del1 = reg(b, Some(deu1))?;
            let r_del2 = reg(b, Some(deu2))?;
            let r_sector = reg(b, Some(sector))?;
            let r_mate = reg(b, mate)?;
            let r_sec_rad = reg(b, eu1::radial(b, sector)?)?;
            let mate_rad = match mate {
                Some(m) => eu1::radial(b, m)?,
                None => None,
            };
            let r_mate_rad = reg(b, mate_rad)?;
            let (m1, m2, s1, s2) = if eu1::orientation(b, deu1)? == eu1::orientation(b, sector)? {
                (
                    r_del1 == r_sec_rad,
                    r_del2 == r_sector,
                    r_del1 == r_mate,
                    r_del2 == r_mate_rad,
                )
            } else {
                (
                    r_del1 == r_sector,
                    r_del2 == r_sec_rad,
                    r_del1 == r_mate_rad,
                    r_del2 == r_mate,
                )
            };
            let switch = if m1 && m2 {
                Some(false)
            } else if s1 && s2 {
                Some(true)
            } else if m1 || m2 {
                Some(false)
            } else if s1 || s2 {
                Some(true)
            } else {
                None
            };
            if switch == Some(true) {
                sector = mate_rad.ok_or_else(|| null("mate without radial"))?;
            }
        }

        // (sbCheckDEU1 is FALSE upstream: the region check of the pair is dead code)

        // different geometry: the pair's UV trim curve no longer fits (status ignored)
        if !same_geometry {
            ignore_status(eu1::set_uv_trim_curve(
                b,
                deu1,
                eu1::UvTrimCurveArg::Null,
                0.0,
                true,
                None,
            ))?;
        }

        edge_remove(b, del, deu1)?;
        edge_remove(b, del, deu2)?;

        // a face use to test for a split region
        let shell_of = |b: &Brep, eu: EdgeuseId| eu1::shell(b, eu);
        let mut temp_fu: Option<FaceuseId> = None;
        let sector_mate = eu1::mate(b, sector)?.ok_or_else(|| null("edge use without mate"))?;
        if eu1::is_loop_edgeuse(b, sector)? && shell_of(b, sector)? == shell_of(b, sector_mate)? {
            temp_fu = eu1::faceuse(b, sector)?;
        } else if eu1::is_loop_edgeuse(b, deu1)? && shell_of(b, deu1)? == shell_of(b, deu2)? {
            temp_fu = eu1::faceuse(b, deu1)?;
        }
        if let Some(fu) = temp_fu {
            if !test_split.contains(&fu) {
                test_split.push(fu);
            }
        }

        if relative_orientation == OrientType::Opposite {
            let o1 = eu1::orientation(b, deu1)?;
            let o2 = eu1::orientation(b, deu2)?;
            eu1::set_orientation(b, deu1, o2)?;
            eu1::set_orientation(b, deu2, o1)?;
        }

        let surv_eus = ed::edgeuses(b, surv, None, None)?;
        ed::add_oriented_eu_pair(b, surv, deu1, deu2, Some(sector))?;

        // the surviving edge was a wire: drop its wire edge uses and their vertex uses
        if surv_wire {
            for eu in surv_eus {
                let vu =
                    eu1::vertexuse(b, eu)?.ok_or_else(|| null("edge use without vertex use"))?;
                edge_remove(b, surv, eu)?;
                let v = b
                    .vertexuse(vu)?
                    .vertex
                    .ok_or_else(|| null("vertex use without vertex"))?;
                let list = &mut b.vertex_mut(v)?.vertexuses;
                if let Some(at) = list.iter().position(|&x| x == vu) {
                    list.remove(at);
                }
                b.vertexuse_mut(vu)?.vertex = None;
            }
            surv_wire = false;
        }

        // widen the surviving edge's tolerance to enclose the gap
        let del_tol = b.edge(del)?.tolerance;
        let surv_tol = b.edge(surv)?.tolerance;
        // (smos_Max: a > b ? a : b)
        let max_tol = if del_tol > surv_tol {
            del_tol
        } else {
            surv_tol
        };
        let mut radius = edge_gap + max_tol;
        if radius < edge_gap * 2.0 {
            radius = edge_gap * 2.0;
        }
        if radius > surv_tol * 1.1 {
            ed::set_tolerance(b, surv, radius, true, true)?;
        }

        jj -= 2;
    }

    // merge the shells the surviving edge's edge uses now connect
    for eu in ed::edgeuses(b, surv, None, None)? {
        let radial = eu1::radial(b, eu)?.ok_or_else(|| null("edge use without radial"))?;
        let del_shell = eu1::shell(b, eu)?.ok_or_else(|| null("edge use without shell"))?;
        let keep_shell = eu1::shell(b, radial)?.ok_or_else(|| null("edge use without shell"))?;
        if del_shell != keep_shell {
            merge_shells(k, b, del_shell, keep_shell)?;
        }
    }

    if do_region_nesting {
        let mut marks = Marks::default();
        let mut new_region = None;
        for fu in test_split {
            // status ignored; each call first resets rpNewRegion to NULL
            let attempt = p6::test_closure_split_region_attempt(k, b, &mut marks, true, fu)?;
            new_region = attempt.region;
        }
        if ed::is_spine(b, surv)? && new_region.is_none() {
            ignore_status(test_closure_split_region_edge(k, b, surv))?;
        }
    }
    Ok(())
}

// ---- CopyFaces ------------------------------------------------------------------------------

/// `CopyFaces`: copies `faces` of `src` into `dest`: with `use_merge` through the merge
/// (keeping the topology between copied faces), else face by face from copies of their
/// curves and surface (a face that cannot be copied makes the result `SM_ERR` but the
/// others are still copied). The destination's tolerance grows to the source's (it is
/// never lowered). Returns the
/// new faces of `dest`, and whether every face was copied.
///
/// # Errors
///
/// `SM_ERR` for a face of another B-rep; [`SolidError::Unported`] (`SmBrep::SetTolerance`
/// first; see the module docs).
pub fn copy_faces(
    k: &Kernel,
    src: &mut Brep,
    faces: &[FaceId],
    dest: &mut Brep,
    use_merge: bool,
    this_face: &dyn Fn(FaceId) -> ObjectRef,
    env: &mut ed::NotifyEnv<'_>,
) -> Result<(Vec<FaceId>, bool)> {
    // pDestinationBrep->SetTolerance(GetTolerance()) = (tol, TRUE, TRUE): grow only
    brep_set_tolerance(dest, src.tolerance(), true, true)?;
    let mut new_faces = Vec::new();
    let mut all_ok = true;
    // (upstream reads the destination's infinite region before both branches; an
    // inline field read, NULL when unset)
    let inf = dest.infinite_region();

    if use_merge {
        // (the merge object is made before the face loop; MakeFaceuse's status is
        // ignored)
        if let Some(made) = ignored(merge_copy_faces(k, src, dest, faces))? {
            new_faces = made;
        }
        return Ok((new_faces, all_ok));
    }

    for &f in faces {
        if !face_in_brep(src, f)? {
            return Err(SolidError::sm_err(OP));
        }
        let Some(curves) = ignored(fp2::create_curves_from_face(
            k,
            src,
            f,
            OrientType::Same,
            true,
            true,
        ))?
        else {
            all_ok = false;
            continue;
        };
        let s1 = fp2::surface(src, f)?.ok_or_else(|| null("face without surface"))?;
        // (status ignored upstream, which then dereferences the null copy)
        let s2 = ignored(s1.copy_and_add_analytics(k))?
            .ok_or_else(|| null("CopyAndAddAnalytics failed (upstream dereferences null)"))?;
        let dom = s2.natural_uv_domain();
        let input = FaceCurvesInput {
            loops: curves
                .curve_loops
                .iter()
                .map(|&n| usize::try_from(n).map_err(|_| null("loop count")))
                .collect::<Result<Vec<_>>>()?,
            curves_3d: curves.curves_3d.map(|v| v.into_iter().map(Some).collect()),
            uv_curves: curves.uv_curves,
            orientations: curves.curve_orients,
            loop_points: Vec::new(),
        };
        match make_face_with_curves(
            k,
            dest,
            inf,
            input,
            s2,
            &dom,
            OrientType::Same,
            None,
            this_face,
            env,
        ) {
            Ok(made) => {
                if let Some(nf) = made.face {
                    new_faces.push(nf);
                }
                // SmFace::Notify(SM_NO_COPY, pNewFace, pDestinationBrep, this)
                face_notify(
                    src,
                    f,
                    NotifyOperation::Copy,
                    [made.face.map(EntityRef::Face), None, None],
                )?;
            }
            Err(e) if e.is_unported() => return Err(e),
            Err(_) => all_ok = false,
        }
    }
    Ok((new_faces, all_ok))
}

// ---- MergeSelfIntersectionCurves ------------------------------------------------------------

/// One self-intersection curve of a surface: its 3D curve and its UV curves on the two
/// sheets that meet there.
#[derive(Debug, Clone)]
pub struct SelfIntersectionCurve {
    /// `cr3DCurves[k]`.
    pub curve_3d: Curve,
    /// `crUVCurves1[k]`.
    pub uv_curve_1: Curve,
    /// `crUVCurves2[k]`.
    pub uv_curve_2: Curve,
}

/// `MergeSelfIntersectionCurves`: imprints each self-intersection curve of the surface of
/// `face` on the face (through both UV curves) and glues the two edges made for it,
/// giving non-manifold edges with four face uses. Returns the glued edges.
///
/// # Errors
///
/// `SM_ERR` when the second classification does not give one interval; those of the
/// classifications, vertex and edge gluing; [`SolidError::Unported`] (the curve
/// classification, `GlueVertices`, `GlueEdgesGeneral`'s callees).
pub fn merge_self_intersection_curves(
    k: &mut Kernel,
    b: &mut Brep,
    face: Option<FaceId>,
    tol_3d: f64,
    curves: &[SelfIntersectionCurve],
    env: &mut ed::NotifyEnv<'_>,
) -> Result<Vec<EdgeId>> {
    let mut new_edges = Vec::new();
    let mut all_faces: Vec<FaceId> = face.into_iter().collect();
    let face = match face {
        Some(f) => f,
        None if curves.is_empty() => return Ok(new_edges),
        None => return Err(null("surface owner is not a face")),
    };
    for c in curves {
        let tol = tol_3d;
        let mut cc1 = classify_on_faces(
            b,
            &all_faces,
            face,
            &c.uv_curve_1,
            c.uv_curve_1.natural_interval(),
            tol,
        )?;
        let new_faces = set_curve_and_merge(b, &mut cc1, &c.curve_3d, face)?;
        all_faces.extend(new_faces);
        for &(ivl, mid_edge) in &cc1 {
            let Some(e1) = mid_edge else {
                continue;
            };
            let mut cc2 = classify_on_faces(b, &all_faces, face, &c.uv_curve_2, ivl, tol)?;
            if cc2.len() != 1 {
                continue;
            }
            let new_faces = set_curve_and_merge(b, &mut cc2, &c.curve_3d, face)?;
            all_faces.extend(new_faces);
            let [(_, mid2)] = cc2.as_slice() else {
                return Err(SolidError::sm_err(OP));
            };
            // (SE(SM_ERR): the error is signalled, not returned)
            let Some(e2) = *mid2 else {
                continue;
            };
            let st1 = ed::start_vertex(b, e1)?;
            let end1 = ed::other_vertex(b, e1, st1)?;
            let st2 = ed::start_vertex(b, e2)?;
            let end2 = ed::other_vertex(b, e2, st2)?;
            let v = |x: Option<VertexId>| x.ok_or_else(|| null("edge without vertex"));
            if st1 != st2 {
                glue_vertices(k, b, v(st1)?, v(st2)?, env)?;
            }
            if st1 != end1 && end1 != end2 {
                glue_vertices(k, b, v(end1)?, v(end2)?, env)?;
            }
            if e1 != e2 {
                glue_edges_general(
                    k,
                    b,
                    Some(e1),
                    OrientType::Same,
                    tol,
                    false,
                    Some(e2),
                    None,
                    env,
                )?;
            }
            new_edges.push(e1);
        }
    }
    Ok(new_edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::attribute::{AttributeStore, ObjectKey};
    use crate::geom::vector2d::Point2;
    use crate::math::Vec3;
    use crate::surface::plane::Plane;
    use crate::topology::Region;

    fn key_of(_e: EntityRef) -> ObjectRef {
        ObjectRef {
            type_code: 0,
            key: ObjectKey(0),
        }
    }

    fn on_list() -> Region {
        Region {
            in_brep: true,
            ..Region::default()
        }
    }

    #[test]
    fn emptied_region_resets_the_infinite_region_to_the_list_head() {
        // the region is not on the list (Remove fails, ignored): the head stays, and a
        // stored head of None is the first region on the list
        let mut b = Brep::new();
        let r0 = b.add_region_record(on_list()).unwrap();
        let r1 = b.add_region_record(Region::default()).unwrap();
        assert_eq!(b.region_list_head(), None);
        remove_emptied_region(&mut b, r1).unwrap();
        assert_eq!(b.infinite_region(), Some(r0));
        assert_eq!(p1::regions(&b, None).unwrap(), vec![r0]);

        // removing the head passes it on; removing the last region empties the list
        let mut b = Brep::new();
        let r0 = b.add_region_record(on_list()).unwrap();
        let r1 = b.add_region_record(on_list()).unwrap();
        b.set_infinite_region(Some(r0));
        remove_emptied_region(&mut b, r0).unwrap();
        assert_eq!(b.infinite_region(), Some(r1));
        assert_eq!(b.region_list_head(), Some(r1));
        remove_emptied_region(&mut b, r1).unwrap();
        assert_eq!(b.infinite_region(), None);
        assert!(p1::regions(&b, None).unwrap().is_empty());

        // removing a region behind the head keeps the head
        let mut b = Brep::new();
        let r0 = b.add_region_record(on_list()).unwrap();
        let r1 = b.add_region_record(on_list()).unwrap();
        remove_emptied_region(&mut b, r1).unwrap();
        assert_eq!(b.infinite_region(), Some(r0));
        assert_eq!(p1::regions(&b, None).unwrap(), vec![r0]);
    }

    fn face_ref(f: FaceId) -> ObjectRef {
        ObjectRef {
            type_code: fp2::TYPE,
            key: ObjectKey(1 + u64::from(f.index())),
        }
    }

    fn plane() -> Surface {
        Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z))
    }

    fn dom() -> Extent2d {
        Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))
    }

    #[test]
    fn glue_no_ops() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        let v0 = b.add_vertex(Point3::new(0.0, 0.0, 0.0)).unwrap();
        let v1 = b.add_vertex(Point3::new(1.0, 0.0, 0.0)).unwrap();
        let e = b.add_edge(v0, v1).unwrap();
        let mut records = std::collections::BTreeMap::new();
        let mut attrs = AttributeStore::default();
        let mut env = ed::NotifyEnv {
            records: &mut records,
            attributes: &mut attrs,
            context: None,
            key_of: &key_of,
        };
        for (s, d) in [
            (None, Some(e)),
            (Some(e), None),
            (None, None),
            (Some(e), Some(e)),
        ] {
            glue_edges_general(
                &mut k,
                &mut b,
                s,
                OrientType::Same,
                0.0,
                true,
                d,
                None,
                &mut env,
            )
            .unwrap();
        }
        let e2 = b.add_edge(v0, v1).unwrap();
        let r = glue_edges_general(
            &mut k,
            &mut b,
            Some(e),
            OrientType::Same,
            0.0,
            true,
            Some(e2),
            None,
            &mut env,
        );
        assert!(r.unwrap_err().is_unported());
    }

    #[test]
    fn make_face_needs_a_surface() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let opts = MakeFaceOptions::default();
        let e = make_face(
            &k,
            &mut b,
            None,
            &[],
            &[],
            &[],
            None,
            &dom(),
            OrientType::Same,
            &opts,
            &face_ref,
        );
        assert!(matches!(
            e,
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
        let e = make_face(
            &k,
            &mut b,
            None,
            &[],
            &[],
            &[],
            Some(plane()),
            &dom(),
            OrientType::Same,
            &opts,
            &face_ref,
        );
        // no infinite region: MakeFaceTopology's NULL region
        assert!(matches!(
            e,
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR_NULL_POINTER,
                ..
            })
        ));
        let inf = b
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        b.set_infinite_region_record(Some(inf));
        let e = make_face(
            &k,
            &mut b,
            None,
            &[],
            &[],
            &[],
            Some(plane()),
            &dom(),
            OrientType::Same,
            &opts,
            &face_ref,
        );
        // the infinite region: MakeFaceTopology reaches SmFace::SetTolerance (unported)
        assert!(e.unwrap_err().is_unported());
    }

    #[test]
    fn face_with_curves_input_errors() {
        let k = Kernel::default();
        let mut b = Brep::new();
        let r = b
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        let mut records = std::collections::BTreeMap::new();
        let mut attrs = AttributeStore::default();
        let mut env = ed::NotifyEnv {
            records: &mut records,
            attributes: &mut attrs,
            context: None,
            key_of: &key_of,
        };
        let mut run = |b: &mut Brep, input: FaceCurvesInput, region| {
            make_face_with_curves(
                &k,
                b,
                region,
                input,
                plane(),
                &dom(),
                OrientType::Same,
                None,
                &face_ref,
                &mut env,
            )
        };
        // no curves
        let e = run(
            &mut b,
            FaceCurvesInput {
                loops: vec![1],
                ..Default::default()
            },
            Some(r),
        );
        assert!(matches!(e, Err(SolidError::InvalidInput { .. })));
        // no loops
        let input = FaceCurvesInput {
            curves_3d: Some(Vec::new()),
            ..Default::default()
        };
        assert!(matches!(
            run(&mut b, input, Some(r)),
            Err(SolidError::InvalidInput { .. })
        ));
        // a NULL region is the infinite region: a degenerate curve only reaches
        // unported topology
        b.set_infinite_region_record(Some(r));
        let input = FaceCurvesInput {
            loops: vec![1],
            curves_3d: Some(vec![None]),
            orientations: vec![OrientType::Same],
            ..Default::default()
        };
        assert!(run(&mut b, input.clone(), None).unwrap_err().is_unported());
        // a degenerate curve only: the face topology is unported
        assert!(run(&mut b, input, Some(r)).unwrap_err().is_unported());
        // more loop entries than curves
        let input = FaceCurvesInput {
            loops: vec![3],
            curves_3d: Some(vec![None]),
            orientations: vec![OrientType::Same],
            ..Default::default()
        };
        assert!(matches!(
            run(&mut b, input, Some(r)),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    #[test]
    fn merge_self_without_curves() {
        let mut k = Kernel::default();
        let mut b = Brep::new();
        let s = b.add_shell().unwrap();
        let f = b.add_face(s).unwrap();
        let mut records = std::collections::BTreeMap::new();
        let mut attrs = AttributeStore::default();
        let mut env = ed::NotifyEnv {
            records: &mut records,
            attributes: &mut attrs,
            context: None,
            key_of: &key_of,
        };
        assert!(
            merge_self_intersection_curves(&mut k, &mut b, Some(f), 1e-5, &[], &mut env)
                .unwrap()
                .is_empty()
        );
        assert!(
            merge_self_intersection_curves(&mut k, &mut b, None, 1e-5, &[], &mut env)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn read_errors_and_deferral() {
        let mut k = Kernel::default();
        let mut b = Brep::new_with_infinite_region().unwrap();
        let mut store = AttributeStore::new();
        let mut records = std::collections::BTreeMap::new();
        let mut env = TopologyEnv {
            all_attributes: &[],
            attributes: &mut store,
            key_of: &key_of,
            records: &mut records,
            next_key: 1,
        };
        assert!(
            read_from_file(&mut k, &mut b, Vec::new(), FileType::Ascii, false, &mut env).is_err()
        );
        let mut src = Brep::new();
        let mut dest = Brep::new();
        let mut records = std::collections::BTreeMap::new();
        let mut attrs = AttributeStore::default();
        let mut env = ed::NotifyEnv {
            records: &mut records,
            attributes: &mut attrs,
            context: None,
            key_of: &key_of,
        };
        assert!(
            copy_faces(&k, &mut src, &[], &mut dest, false, &face_ref, &mut env)
                .unwrap_err()
                .is_unported()
        );
    }
}
