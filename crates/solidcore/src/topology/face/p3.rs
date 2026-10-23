//! `SmFace`, part 3: loop orientation, UV trim curve creation, the face/face
//! intersection, degeneracy and lamina tests, composite boundary curves, tolerance
//! setting and change notification.
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/inc/SmFace.h`.
//!
//! | Rust                                         | Upstream                                  |
//! |----------------------------------------------|-------------------------------------------|
//! | [`compute_loop_orientation`]                 | `SmFace::ComputeLoopOrientation`          |
//! | [`create_composites`]                        | `SmFace::CreateComposites`                |
//! | [`create_uv_trim_curves`], [`UvTrimCurveGaps`] | `SmFace::CreateUVTrimCurves`            |
//! | [`face_intersect`], [`FaceIntersectOptions`], [`FaceIntersection`], [`IntersectObject`] | `SmFace::FaceIntersect` |
//! | [`notify`]                                   | `SmFace::Notify`                          |
//! | [`set_tolerance`]                            | `SmFace::SetTolerance` (`SM_USE_OLDTOL`)  |
//! | [`is_degenerate`]                            | `SmFace::IsDegenerate`                    |
//! | [`is_lamina`]                                | `SmFace::IsLamina`                        |
//! | [`has_consecutive_lamina_edges`]             | `SmFace::HasConsecutiveLaminaEdges`       |
//! | [`has_lamina_edge`]                          | `SmFace::HasLaminaEdge` (CO-PORTED in `topology.topology_traverser`, re-exported) |
//!
//! Deviations:
//! - **Object model.** A face is a record in the [`Brep`]'s arenas; the methods are free
//!   functions taking the B-rep and the face's id. Out-parameters are return values.
//!   Where upstream dereferences null (a face without surface, an edge without curve, an
//!   edge use without vertex use or clockwise neighbour, `GetLast` of an empty array, an
//!   uninitialised `SmPoint2d`) the port returns [`SolidError::InvalidInput`]. `SER` /
//!   `NER` returns are the callee's error or [`SolidError::Upstream`] `SM_ERR`.
//! - **`SmFace::GetEdges`** with default arguments is the co-ported
//!   `topology_traverser::face_edges`; `GetEdges(.., TRUE)` (outer loop only),
//!   `GetUpwardLoopuses`, `GetUpwardEdgeuses`, `GetUpwardVertexuses` (default arguments)
//!   and `GetBrep` call `topology::face::p4`.
//! - **Statuses.** `ComputeProjectedLoopOrientation`'s status is checked as upstream (a
//!   failure falls through to the 3D walk); `sm_AreConnected` and `IsDegenerate` of a
//!   curve return values; ignored as upstream: the `Trim`, `EvaluatePoint` and
//!   `Point3DClassify` calls of `FaceIntersect`, the `NormalizedEvaluate` /
//!   `Evaluate1stDerivatives` / `PointClassify` of the quick check and the
//!   `InsertTopologyIntersections` of `IsDegenerate`, the `SetUVTrimCurve` and
//!   `sm_FixUVTrimCurves` of `CreateUVTrimCurves`; [`SolidError::Unported`] always
//!   propagates. `CreateUVTrimCurves` reads the face domain, the surface poles and the
//!   natural domain before clearing the surface owner, as upstream.
//! - **`ComputeLoopOrientation`** walks with a copy of the face surface; its last-to-first
//!   span reads `sVeryFirstUV`, which stays at the `SmVector2d` default
//!   (`SM_UNDEF_DOUBLE`, `SM_UNDEF_DOUBLE`) when no sample point dropped with a guess;
//!   the port does the same (defined upstream). A loop use of edge type without edge
//!   uses reaches `GetLast` of an empty array (UB-189) and returns
//!   [`SolidError::InvalidInput`].
//! - **`CreateUVTrimCurves`** takes the optional UV curves and their orientations as one
//!   argument (upstream requires both or neither). The surface owner is cleared and
//!   restored around the walk ([`face2::RescueSurfaceOwner`]), also on errors, and the
//!   scope is recorded on the B-rep ([`Brep::begin_surface_owner_cleared`]): a nested
//!   `smsurf_GetSurfaceCache` of the face's surface (an edge use's `EvaluateBinormal`)
//!   then builds no face trim and does not create the face's UV trim curves itself,
//!   as upstream, whose surface has no face meanwhile. A chosen
//!   candidate curve is moved out of its slot; unused candidates stay with the caller
//!   (upstream deletes them).
//! - **`FaceIntersect`** returns its outputs in [`FaceIntersection`] (the optional ones
//!   when asked for in [`FaceIntersectOptions`]). Its classifications refer to the
//!   surface/surface intersection curves by index ([`IntersectObject::Curve`]); with
//!   `want_cc_curves` (or any classification array) those curves are returned. The coincidence solutions of
//!   `IsDegenerate` name the curves by a handle carrying their natural interval; the
//!   objects are then switched to the edges, as upstream. All returned curves are owned
//!   (upstream hands out deleted ones through `pOptCC3DCrvs` without classification
//!   arrays, UB-193). Outputs are return values, so upstream's quirk that
//!   `pOptThisCCUVCrvs` / `pOptOtherCCUVCrvs` are never reset (they append to the
//!   caller's arrays) has no counterpart; in the coincident branch the failing-copy
//!   path replaces all four vectors with the intersector's.
//! - **`Notify`** takes the face surface's trimmed surface cache (if any) for
//!   `SM_NO_PRE_EDIT` (`SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, ..)` has no counterpart:
//!   B-rep surfaces carry no object cache) and the notification state of
//!   [`edge::NotifyEnv`]. `SE_MSG` for `SM_NO_UNKNOWN` is compiled out.
//! - **Unported callees** return [`SolidError::Unported`]: the curve constructor of
//!   `SmCurveClassification` (`intersect.curve_class_p4`), `SmFace::Point3DClassify`
//!   (`query.face_p1`). `SmEdgeuse::NormalizedEvaluate`
//!   and `GetOrCreateUVTrimCurve` are wired (`topology.edgeuse_p3`; their own unported
//!   callees still return it).
//! - **Bounds.** No recursion; every loop is over B-rep arrays or a fixed sample count.

use crate::curve::composite_curve::CompositeCurve;
use crate::curve::curve::p2::CRV_ON_SURF_TYPE;
use crate::curve::curve::Curve;
use crate::error::{ignore_status, ignored, EntityRef, Result, SolidError};
use crate::geom::extent1d::{Extent1d, ExtentPointType};
use crate::geom::extent2d::{Extent2d, SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN};
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::solution_array::{ObjectKind, Solution, SolutionObject, MAX_TREES};
use crate::geom::tol::xsect_tol_3d_from_zones;
use crate::geom::vector2d::{Point2, Vec2};
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::curve_class::p5::ExtractOptions;
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO, PI};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::face::p2 as qf2;
use crate::surface::surface::p2::{smos_max, TsectCurveType};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::surface::trim_srf_cache::TrimSrfCache;
use crate::surface::volume::Volume;
use crate::topology::edge::{self, NotifyEnv};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p2 as eu2;
use crate::topology::edgeuse::p3 as eu3;
use crate::topology::face::p2 as face2;
use crate::topology::loopuse as lu1;
use crate::topology::topology as topo;
use crate::topology::topology_traverser::face_edges;
use crate::topology::vertex;
use crate::topology::vertexuse as vu1;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, LoopId, LoopuseId, VertexId, VertexuseId};

pub use crate::topology::topology_traverser::has_lamina_edge;

const OP: &str = "topology::face::p3";

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

// ---- calls into other units (some not ported yet) ---------------------------------------

/// `pEU->NormalizedEvaluate(t, bUV, rPoint, pOptFirstDeriv)` (default
/// `bOkToMakeUVTrimCurve`): the point and, with `want_deriv`, the first derivative.
fn normalized_evaluate(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    t: f64,
    uv: bool,
    want_deriv: bool,
) -> Result<(Point3, Option<Vec3>)> {
    let r = eu3::normalized_evaluate(k, b, eu, t, uv, want_deriv, true)?;
    Ok((r.point, r.first_deriv))
}

/// `pEU->GetOrCreateUVTrimCurve(rpUVCurve, pdOptDistToSurf)` (default
/// `bOKToMakeUVTrimCurve`, no `pdOptDeviation`; `pdOptDistToSurf` non-null with
/// `want_dist`): the returned status and `*pdOptDistToSurf` (`-1` for an existing curve
/// without a cached gap, and when not asked for). The curve is then
/// [`eu1::uv_trim_curve`].
fn get_or_create_uv_trim_curve(
    k: &Kernel,
    b: &mut Brep,
    eu: EdgeuseId,
    want_dist: bool,
) -> Result<(Result<()>, f64)> {
    let r = eu3::get_or_create_uv_trim_curve(k, b, eu, want_dist, false, true)?;
    Ok((r.status, r.dist_to_surf))
}

/// `GetUpwardLoopuses(sLoopuses)` (default arguments).
fn upward_loopuses(b: &Brep, f: FaceId) -> Result<Vec<LoopuseId>> {
    crate::topology::face::p4::upward_loopuses(b, f, false, None)
}

/// `GetUpwardEdgeuses(sEdgeuses)` (default arguments).
fn upward_edgeuses(b: &Brep, f: FaceId) -> Result<Vec<EdgeuseId>> {
    crate::topology::face::p4::upward_edgeuses(b, f, false, None)
}

/// `GetUpwardVertexuses(sVertexuses)` (default arguments).
fn upward_vertexuses(b: &Brep, f: FaceId) -> Result<Vec<VertexuseId>> {
    crate::topology::face::p4::upward_vertexuses(b, f, false, None)
}

/// `GetEdges(sEdges, NULL, TRUE)`: the edges of the outer loop only.
fn outer_loop_edges(b: &Brep, f: FaceId) -> Result<Vec<EdgeId>> {
    crate::topology::face::p4::edges(b, f, None, true, None)
}

/// `this->GetBrep() ? this->GetBrep()->GetTolerance() : 0.0`.
fn brep_tolerance(b: &Brep, f: FaceId) -> Result<f64> {
    Ok(crate::topology::face::p4::brep(b, f)?.map_or(0.0, Brep::tolerance))
}

/// `new SmCurveClassification(pCurve, sInterval, pUVCurve, sSrcZoneTol3d)`.
fn curve_classification<O: ClassObject>(
    k: &Kernel,
    curve: O,
    interval: &Extent1d,
    uv_curve: Option<O>,
    src_zone_tol_3d: f64,
) -> Result<CurveClassification<O>> {
    // the remaining constructor arguments default: no interval storage, the curve not
    // owned and current, no surfaces, no topology intersector
    CurveClassification::with_curve(
        k,
        &crate::intersect::curve_class::ClassificationInit {
            curve: Some(curve),
            interval: *interval,
            uv_curve,
            src_zone_tol_3d,
            opt_data_size: 0,
            is_curve_owned: false,
            is_curve_current: true,
            surface1: None,
            surface2: None,
            uv_curve2: None,
            ti: None,
        },
    )
}

/// `GetSurface()->CoincidenceCheck(other, tol, rbAreCoincident, rdDist, FALSE, ..)`:
/// whether the surfaces coincide and their largest distance.
fn coincidence_check(
    k: &Kernel,
    surface: &Surface,
    other: &Surface,
    tol: f64,
) -> Result<(bool, f64)> {
    let c = surface.coincidence_check(k, other, tol, false)?;
    Ok((c.are_coincident, c.max_distance))
}

/// `pFace->Point3DClassify(sPoint, tol, TRUE, sPC)`: classifies the point into `pc`.
fn point_3d_classify(
    _k: &Kernel,
    _b: &Brep,
    _f: FaceId,
    _point: Point3,
    _tol: f64,
    _pc: &mut PointClassification<IntersectObject>,
) -> Result<()> {
    // DISPATCH GAP: SmFace::Point3DClassify is ported
    // (`query::face::p1::point_3d_classify`, query.face_p1 partial; no UNPORTED marker),
    // but it takes `&mut Brep` (for `query::face::p2::point_classify`) and this caller's
    // chain holds `&Brep` and an `IntersectObject` classification (`map_objects` converts
    // it); wire it when the B-rep can be lent mutably here. Until then every such case
    // would still defer inside it (topology.face_p2 / query.face_p2).
    Err(SolidError::unported(
        "SmFace::Point3DClassify",
        "query.face_p1",
    ))
}

// ---- small accessors ----------------------------------------------------------------------

fn surface_of(b: &Brep, f: FaceId) -> Result<&Surface> {
    face2::surface(b, f)?.ok_or_else(|| null("face without surface"))
}

fn edge_curve(b: &Brep, e: EdgeId) -> Result<&Curve> {
    b.edge(e)?
        .curve
        .as_ref()
        .ok_or_else(|| null("edge without curve"))
}

// ---- lamina tests ---------------------------------------------------------------------------

/// `IsLamina`: whether every edge of the face is lamina (`true` for a face without
/// edges).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for missing
/// topology links (those of the edge walk).
pub fn is_lamina(b: &Brep, f: FaceId) -> Result<bool> {
    let edges = face_edges(b, f)?;

    // Check each edge, returning False if any non-lamina edge is found.
    for e in edges {
        if !eu1::edge_is_lamina(b, e)? {
            return Ok(false);
        }
    }

    // arrive here when all face->Edges are lamina
    Ok(true)
}

/// `HasConsecutiveLaminaEdges`: whether two consecutive edges of the outer loop
/// (wrapping from the last to the first) are lamina.
///
/// # Errors
///
/// [`SolidError::Unported`]: `SmFace::GetEdges` of the outer loop (`topology.face_p4`).
pub fn has_consecutive_lamina_edges(b: &Brep, f: FaceId) -> Result<bool> {
    // JGU: taking advantage of GWC 603 updates. Not actually bug 603
    let edges = outer_loop_edges(b, f)?;
    let mut first_edge = false;
    let mut prior_edge = false;

    let Some(&first) = edges.first() else {
        return Ok(false);
    };

    // Check first edge
    if eu1::edge_is_lamina(b, first)? {
        first_edge = true;
        prior_edge = true;
    }

    // Check edges 1 to end
    for &e in edges.iter().skip(1) {
        if eu1::edge_is_lamina(b, e)? {
            if prior_edge {
                // Previous edge was lamina, so 2 in a row
                return Ok(true);
            }
            prior_edge = true;
        } else {
            prior_edge = false; // Reset SmBoolean, since not lamina
        }
    }

    // check first and last edge
    if first_edge && prior_edge {
        return Ok(true);
    }

    // arrive here when adjacent edges not lamina
    Ok(false)
}

// ---- tolerance and notification -----------------------------------------------------------

/// `SetTolerance`: sets the face tolerance to `new_zone_tol_3d` (only when larger with
/// `update_only_if_larger`); with `cascade_to_boundaries`, raises every face edge whose
/// tolerance is below it ([`edge::set_tolerance`] with the same flags). Upstream
/// defaults are `true`, `true`. `new_zone_tol_3d` is upstream's `SmZoneTol3d` argument:
/// a caller passing a plain double converts it first ([`crate::geom::tol::zone_tol3d`]).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of the edge walk and
/// [`edge::set_tolerance`].
pub fn set_tolerance(
    b: &mut Brep,
    f: FaceId,
    new_zone_tol_3d: f64,
    update_only_if_larger: bool,
    cascade_to_boundaries: bool,
) -> Result<()> {
    // set tolerances - when asked skip shrinking tolerances
    let face = b.face_mut(f)?;
    if !update_only_if_larger || new_zone_tol_3d > face.tolerance {
        face.tolerance = new_zone_tol_3d;
    }
    // the face's surface reads it through GetFace()
    b.sync_face_surface_tolerance(f)?;

    // when asked - make sure Edge->VertexTols at least as big as thie EdgeTol
    if cascade_to_boundaries {
        for e in face_edges(b, f)? {
            if b.edge(e)?.tolerance < new_zone_tol_3d {
                edge::set_tolerance(
                    b,
                    e,
                    new_zone_tol_3d,
                    update_only_if_larger,
                    cascade_to_boundaries,
                )?;
            }
        }
    }
    Ok(())
}

/// `Notify`: `PreEdit` resets the rectangular-trim flag to `UNSURE` and marks the
/// surface's trimmed surface cache (`trim_cache`) as modified; `PostEdit`,
/// `SplitInBrep`, `ChangeGeometry` and `Split` are passed to every edge use and vertex
/// use of the upward face use (clearing their cached UV trim curves and gaps); then the
/// notification goes to `SmTopology::Notify`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::Unported`] for the edit
/// notifications (`GetUpwardEdgeuses`, `topology.face_p4`); those of the edge use,
/// vertex use and topology notifications.
pub fn notify(
    b: &mut Brep,
    f: FaceId,
    env: &mut NotifyEnv<'_>,
    trim_cache: Option<&mut TrimSrfCache<EntityRef>>,
    operation: NotifyOperation,
    data: [Option<ObjectRef>; 3],
) -> Result<()> {
    use NotifyOperation as N;
    let [d1, d2, d3] = data;
    match operation {
        N::PreEdit => {
            // m_bRectangularTrim: UNSURE = Not yet set
            b.face_mut(f)?.rectangular_trim = face2::UNSURE;
            if let Some(sc) = trim_cache {
                sc.base_mut().face_was_modified = true;
            }
        }
        N::PostEdit | N::SplitInBrep | N::ChangeGeometry | N::Split => {
            // pass the notify call along to all Vertexuses, Edgeuses, and
            // Edgeuse->Vertexuses so that those can clean up their cached gaps
            let eus = upward_edgeuses(b, f)?;
            let vus = upward_vertexuses(b, f)?;
            for eu in eus {
                let this = (env.key_of)(EntityRef::Edgeuse(eu));
                eu2::notify(
                    b,
                    eu,
                    &mut *env.records,
                    &mut *env.attributes,
                    env.context.as_deref_mut(),
                    operation,
                    this,
                    data,
                )?;
            }
            for vu in vus {
                let this = (env.key_of)(EntityRef::Vertexuse(vu));
                vu1::notify(
                    b,
                    vu,
                    &mut *env.records,
                    &mut *env.attributes,
                    env.context.as_deref_mut(),
                    operation,
                    this,
                    data,
                )?;
            }
        }
        // SM_NO_UNKNOWN: SE_MSG only (compiled out of the release build)
        _ => {}
    }

    // Propagate notification up hierarchy
    let this = (env.key_of)(EntityRef::Face(f));
    topo::notify(
        &mut *env.records,
        &mut *env.attributes,
        env.context.as_deref_mut(),
        operation,
        this,
        d1,
        d2,
        d3,
    )
}

// ---- composites ---------------------------------------------------------------------------

/// `CreateComposites`: one closed composite curve per loop of the face use with
/// orientation `SM_OT_SAME`, made of copies of the loop's 3D edge curves with their
/// senses (`false` for an edge use running opposite to its loop). The B-rep is mutable:
/// `CreateCurvesFromFace` may create missing UV trim curves.
///
/// # Errors
///
/// Those of [`face2::create_curves_from_face`] (including [`SolidError::Unported`] for
/// edge loops); [`SolidError::InvalidInput`] when the loop counts exceed the curves or
/// orientations returned (upstream reads past them).
pub fn create_composites(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Vec<CompositeCurve>> {
    // init output
    let mut composites = Vec::new();

    // Create List of Curves from Face Loops
    let fc = face2::create_curves_from_face(k, b, f, OrientType::Same, true, false)?;
    let mut curves = fc.curves_3d.unwrap_or_default().into_iter();

    // for every loop
    let mut count = 0usize;
    for &num_crv in &fc.curve_loops {
        let mut senses = Vec::new();
        let mut loop_curves = Vec::new();

        // for every edge in the loop
        for _ in 0..num_crv {
            let curve = curves
                .next()
                .ok_or_else(|| null("CreateComposites: fewer curves than loop edges"))?;
            let orient = fc
                .curve_orients
                .get(count)
                .copied()
                .ok_or_else(|| null("CreateComposites: fewer orientations than curves"))?;
            count += 1;

            // build Curve and Senses array
            loop_curves.push(curve);
            senses.push(orient != OrientType::Opposite);
        }

        // build CompositeCurve
        composites.push(CompositeCurve::with_curves(
            3,
            loop_curves,
            true,
            Some(&senses),
            None,
        )?);
    }

    // all done
    Ok(composites)
}

// ---- loop orientation ---------------------------------------------------------------------

/// `lNumSamples` of the 3D walk of [`compute_loop_orientation`]: 21 points per edge use.
const NUM_SAMPLES: u32 = 20;

/// `ComputeLoopOrientation`: the orientation of loop `l` on the face: `Same` (an outer,
/// counter-clockwise loop in UV), `Opposite` (inner) or `Unknown`. A vertex loop is
/// `Opposite`. When every edge use has a UV trim curve (with `do_3d_loop_computation`
/// only the existing ones; else made when missing) and consecutive ones connect, the
/// UV loop's projected orientation decides; otherwise the edge uses are sampled in 3D
/// (21 points each), dropped onto a copy of the surface and the signed UV area summed
/// (across seams). With `uv_bounds`, it is reset and receives the UV loop's samples.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for missing
/// topology, a face without surface, an edge loop without edge uses
/// (upstream reads an empty array, UB-189); those of the surface copy,
/// evaluations and solves; [`SolidError::Unported`] (from `SmEdgeuse::NormalizedEvaluate`
/// and `GetOrCreateUVTrimCurve`).
#[allow(clippy::too_many_lines)] // one upstream routine
pub fn compute_loop_orientation(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    l: LoopId,
    mut uv_bounds: Option<&mut Extent2d>,
    do_3d_loop_computation: bool,
) -> Result<OrientType> {
    // init output
    if let Some(bounds) = uv_bounds.as_deref_mut() {
        *bounds = Extent2d::new();
    }

    // get Loopuses
    let (lu1, _lu2) = lu1::loop_loopuses(b, l)?;

    // VertexLoops are InnerLoops
    if lu1::is_vertex_loopuse(b, lu1)? {
        return Ok(OrientType::Opposite);
    }

    // arrive here for EdgeLoop type Loops - Work from the Edgeuses of pLU1.
    let edgeuses = lu1::edgeuses(b, lu1, None, None)?;

    // Integrate UVArea within the loop by walking the UVTrimCurves to determine its
    // orientation. First, try it in uv space, using uv trim curves from the Edgeuses.
    let mut uv_curves: Vec<Curve> = Vec::new();
    let mut senses: Vec<bool> = Vec::new();
    let mut uv_curves_ok = true;

    // for every Loop->Edgeuse
    for (ii, &eu) in edgeuses.iter().enumerate() {
        // Don't create a UVTrimCurve if asked not to.  [B586 B592]
        let ok = if do_3d_loop_computation {
            true
        } else {
            // create missing UVTrimCurves
            get_or_create_uv_trim_curve(k, b, eu, false)?.0.is_ok()
        };
        let uv_curve = eu1::uv_trim_curve(b, eu)?.0.cloned();

        // no work - UVTrimCurve is missing - go try the 3d algorithm
        let Some(uv_curve) = uv_curve.filter(|_| ok) else {
            uv_curves_ok = false; // If a problem with any uv curve, force 3d computation.
            break;
        };

        // build UVTrimCurve data arrays
        uv_curves.push(uv_curve);
        senses.push(eu1::orientation(b, eu)? == OrientType::Same);

        // Check if UVTrimCurves are connected in UV space (not always near
        // singularities). If not, then don't use uv curves - try 2d algorithm
        if ii > 0 {
            let prev = edgeuses
                .get(ii - 1)
                .copied()
                .ok_or_else(|| null("ComputeLoopOrientation: no previous edge use"))?;
            if !face2::are_connected(k, b, prev, eu, true, false)?.connected {
                uv_curves_ok = false;
                break;
            }
        }
    }

    // Check LastEdge-to-FirstEdge UVSpace end-to-end connectedness.
    if uv_curves_ok {
        let (Some(&last), Some(&first)) = (edgeuses.last(), edgeuses.first()) else {
            return Err(null("ComputeLoopOrientation: loop use without edge uses"));
        };
        if !face2::are_connected(k, b, last, first, true, false)?.connected {
            uv_curves_ok = false;
        }
    }

    // when all the UVTrimCurves are available, connected, and form a closed loop
    if uv_curves_ok {
        // walk all UVTrimCurves to determine if this is a CCW or CW loop
        let normal = Vec3::new(0.0, 0.0, 1.0);
        match CompositeCurve::compute_projected_loop_orientation(
            k, &uv_curves, &senses, normal, uv_bounds,
        ) {
            Ok(o) if o != OrientType::Unknown => return Ok(o),
            Err(e) if e.is_unported() => return Err(e),
            // a failure or an unknown orientation falls through to the 3D walk
            _ => {}
        }
    }

    // arrive here when UVTrimCurves are not good for computation. Try 3d algorithm.

    // get temporary face->Surface copy
    let surf_copy = surface_of(b, f)?.copy(k)?;
    let face_domain = face2::uv_domain(b, f)?;
    let surf_domain = surface_of(b, f)?.natural_uv_domain();

    // Get last edgeuse endPosition (3d).
    let last = edgeuses
        .last()
        .copied()
        .ok_or_else(|| null("ComputeLoopOrientation: loop use without edge uses"))?;
    let (point, _last_tangent) = normalized_evaluate(k, b, last, 1.0, false, true)?;

    // DropPoint to Surface (why not use UVTrimCurve GuessPoint)
    let tol = face2::tolerance(b, f)?;
    let solutions = surf_copy.global_point_solve::<NoObject>(
        k,
        None,
        &face_domain,
        SolverOperation::Minimize,
        point,
        tol,
        None,
        SolutionRequested::Single,
    )?;

    let mut uv1;
    let mut uv2 = Point2::UNDEF;
    let mut have_guess = true;
    if solutions.len() == 1 {
        let s = solutions
            .get(0)
            .ok_or_else(|| null("ComputeLoopOrientation: no solution"))?;
        uv1 = Point2::new(s.start.parameters[0], s.start.parameters[1]);
    } else {
        uv1 = Point2::new(0.0, 0.0); // This will add zero to the sum.
        have_guess = false; // Don't use this as a guess.
    }

    let mut total_area = 0.0;
    // `SmPoint2d sVeryFirstUV;` default-constructs to (SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE).
    let mut very_first_uv = Point2::UNDEF;
    let mut first_uv_set = false;
    let mut param_shift = Vec2::new(0.0, 0.0);

    let closed_u = surf_copy
        .is_closed(k, &surf_domain, SurfParamType::U, None, false)?
        .0;
    let closed_v = surf_copy
        .is_closed(k, &surf_domain, SurfParamType::V, None, false)?
        .0;

    for &eu in &edgeuses {
        // For 21 sample points along the curve - integrate signed area in uv space.
        for jj in 0..=NUM_SAMPLES {
            // get the Curve Position and Tangent
            let param = f64::from(jj) / f64::from(NUM_SAMPLES);
            let (point, _tangent) = normalized_evaluate(k, b, eu, param, false, true)?;

            // DropPoint to Surface to get uv. Note, DropPoint() will step across seams.
            let drop = surf_copy.drop_point::<NoObject>(
                k,
                point,
                &face_domain,
                &DropPointOptions {
                    guess_uv: have_guess.then_some(uv1),
                    ..DropPointOptions::default()
                },
            )?;
            uv2 = drop.result_uv;

            // when there is a solution
            if drop.success {
                if !first_uv_set && have_guess {
                    very_first_uv = uv1;
                    first_uv_set = true;
                }
                face2::check_seam_crossing(
                    &mut uv1,
                    uv2,
                    &surf_domain,
                    closed_u,
                    closed_v,
                    &mut param_shift,
                );

                // Apply the correction to put the uv points into the same period as
                // they started.
                uv1 += param_shift;
                uv2 += param_shift;

                // 'Area': cross product of vectors from origin to sUV1 and to sUV2 is
                // (twice) the signed area of the triangle [ origin - sUV1 - sUV2 ].
                let this_tri_area = uv1.x * uv2.y - uv1.y * uv2.x;
                total_area += this_tri_area;

                // Shift back into the surface period, for dropping.
                uv2 -= param_shift;

                // Set up for next iter.
                uv1 = uv2;
                have_guess = true;
            } else {
                have_guess = false; // Use Global solve for next point.
            }
        }
    }

    // Catch the final-point-to-first-point span. The final point will usually be the
    // same as the first point, but could span a pole.
    uv2 += param_shift;
    let this_tri_area = uv2.x * very_first_uv.y - uv2.y * very_first_uv.x;
    total_area += this_tri_area;

    // Positive area means CCW, Negative is CW. When the integration scheme wasn't good
    // enough - return UNKNOWN
    let domain_area = face_domain.x_length() * face_domain.y_length();
    let area_ratio = 1e+8; // Pretty arbitrary: 1/10,000 of the domain in each dimension.

    Ok(if total_area > domain_area / area_ratio {
        OrientType::Same
    } else if total_area < -domain_area / area_ratio {
        OrientType::Opposite
    } else {
        OrientType::Unknown
    })
}

// ---- UV trim curves -----------------------------------------------------------------------

/// The gap statistics of [`create_uv_trim_curves`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UvTrimCurveGaps {
    /// `rdMeanCurveSurfaceGap`: average new UV trim curve/surface distance.
    pub mean_curve_surface_gap: f64,
    /// `rdMaxCurveSurfaceGap`: largest UV trim curve/surface distance.
    pub max_curve_surface_gap: f64,
    /// `rdMean3DVertexGap`: average vertex/edge use start distance.
    pub mean_3d_vertex_gap: f64,
    /// `rdMax3DVertexGap`: largest vertex/edge use start distance.
    pub max_3d_vertex_gap: f64,
    /// `rdMeanPSCurveCurveGap`: average UV gap between an edge use and its clockwise
    /// neighbour.
    pub mean_ps_curve_curve_gap: f64,
    /// `rdMaxPSCurveCurveGap`: largest such UV gap.
    pub max_ps_curve_curve_gap: f64,
}

/// `CreateUVTrimCurves`: gives every edge use of the face a UV trim curve (the best of
/// `opt_uv_curves` with their orientations when given, else a new drop), closes small
/// UV gaps between consecutive edge uses away from poles, and with
/// `adjust_tolerances` raises vertex tolerances to twice their 3D gap. Returns the gap
/// statistics.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for missing
/// topology or surface; those of the curve searches, edits and evaluations;
/// [`SolidError::Unported`] (`GetUpwardLoopuses`, `GetBrep`; from `NormalizedEvaluate`
/// and `GetOrCreateUVTrimCurve`).
pub fn create_uv_trim_curves(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    adjust_tolerances: bool,
    opt_uv_curves: Option<(&mut [Option<Curve>], &[OrientType])>,
) -> Result<UvTrimCurveGaps> {
    // queried before the surface owner is cleared, as upstream: the surface is still
    // face-owned, so `GetSingularities`' approximation tolerance
    // (`SmTol::GetApproxTol3d(this)`) is the face's
    let pre = PreRescue {
        uv_domain: face2::uv_domain(b, f)?,
        srf_poles: surface_of(b, f)?
            .get_singularities_of_face(k, face2::tolerance(b, f)?, false, false)?
            .singular,
        srf_uv_domain: surface_of(b, f)?.natural_uv_domain(),
    };
    let surface = surface_of(b, f)?;
    let rescue = face2::RescueSurfaceOwner::new(surface);
    if let Some(s) = b.face_mut(f)?.surface.as_mut() {
        s.set_owner(None);
    }
    // (recorded on the B-rep: a nested `smsurf_GetSurfaceCache` of the face's surface
    // then finds no face, so it does not build the face's UV trim curves itself)
    b.begin_surface_owner_cleared(f);
    let result = create_uv_trim_curves_body(k, b, f, adjust_tolerances, opt_uv_curves, &pre);
    // SmRescueSurfaceOwner's destructor restores the owner on every return
    b.end_surface_owner_cleared(f);
    if let Some(s) = b.face_mut(f)?.surface.as_mut() {
        rescue.restore(s);
    }
    // the face's tolerance may have changed while the surface had no owner
    b.sync_face_surface_tolerance(f)?;
    result
}

/// The face and surface values `CreateUVTrimCurves` reads before it clears the surface
/// owner.
struct PreRescue {
    uv_domain: Extent2d,
    srf_poles: u32,
    srf_uv_domain: Extent2d,
}

#[allow(clippy::too_many_lines)] // one upstream routine
fn create_uv_trim_curves_body(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    adjust_tolerances: bool,
    mut opt_uv_curves: Option<(&mut [Option<Curve>], &[OrientType])>,
    pre: &PreRescue,
) -> Result<UvTrimCurveGaps> {
    let mut mean_cs_gap = 0.0;
    let mut max_cs_gap = 0.0_f64;
    let mut mean_cc_gap = 0.0;
    let mut max_cc_gap = 0.0_f64;
    let mut mean_3d_gap = 0.0;
    let mut max_3d_gap = 0.0_f64;
    let mut total_dropped: u64 = 0;

    let PreRescue {
        uv_domain,
        srf_poles,
        srf_uv_domain,
    } = *pre;

    let loopuses = upward_loopuses(b, f)?;
    for lu in loopuses {
        let edgeuses = lu1::edgeuses(b, lu, None, None)?;
        for &eu in &edgeuses {
            total_dropped += 1;

            let Some(e) = eu1::edge(b, eu)? else {
                continue;
            };
            if eu1::uv_trim_curve(b, eu)?.0.is_some() {
                continue;
            }

            let mut found: Option<Curve> = None;
            if let Some((cands, orients)) = opt_uv_curves.as_mut() {
                let bb: &Brep = b;
                found = face2::find_best_uv_curve(k, bb, surface_of(bb, f)?, eu, cands, orients)?;

                if let Some(uv_curve) = found.take() {
                    if !adjust_tolerances || edge_curve(b, e)?.is_kind_of(CRV_ON_SURF_TYPE) {
                        // TRUE = delete existing UVTrimCurve (status ignored upstream)
                        ignore_status(eu1::set_uv_trim_curve(
                            b,
                            eu,
                            eu1::UvTrimCurveArg::New(Box::new(uv_curve)),
                            0.0,
                            true,
                            None,
                        ))?;
                        continue;
                    }

                    let r = eu2::check_and_set_uv_trim_curve(
                        k,
                        b,
                        eu,
                        uv_curve.clone(),
                        adjust_tolerances,
                        None,
                    )?;
                    if r.trim_replaced {
                        let d = r
                            .max_dist_to_surf
                            .ok_or_else(|| null("CreateUVTrimCurves: distance not set"))?;
                        mean_cs_gap += d;
                        max_cs_gap = smos_max(max_cs_gap, d);
                        continue;
                    }
                    found = Some(uv_curve);
                }
            }

            if found.is_some() {
                // TRUE = delete existing UVTrimCurve (status ignored upstream)
                ignore_status(eu1::set_uv_trim_curve(
                    b,
                    eu,
                    eu1::UvTrimCurveArg::Null,
                    0.0,
                    true,
                    None,
                ))?;
            }

            let (status, max_dist_to_surf) = get_or_create_uv_trim_curve(k, b, eu, true)?;
            status?;

            mean_cs_gap += smos_max(0.0, max_dist_to_surf); // This can come back -1.0.
            max_cs_gap = smos_max(max_cs_gap, max_dist_to_surf);
        }

        for &eu in &edgeuses {
            let cw = eu1::cw_edgeuse(b, eu)?
                .ok_or_else(|| null("CreateUVTrimCurves: edge use without CW edge use"))?;
            let vu = eu1::vertexuse(b, eu)?
                .ok_or_else(|| null("CreateUVTrimCurves: edge use without vertex use"))?;
            let v = vu1::vertex(b, vu)?
                .ok_or_else(|| null("CreateUVTrimCurves: vertex use without vertex"))?;

            let mut uv_pnt0 = normalized_evaluate(k, b, eu, 0.0, true, false)?.0;
            let mut uv_pnt1 = normalized_evaluate(k, b, cw, 1.0, true, false)?.0;

            let mut uv_gap_size = uv_pnt0.distance(uv_pnt1);

            let uv_scaled_zero = EFF_ZERO * (1.0 + uv_pnt0.max_dimension());
            let u_scaled_zero = EFF_ZERO * (1.0 + uv_pnt0.x.abs());
            let v_scaled_zero = EFF_ZERO * (1.0 + uv_pnt0.y.abs());
            let mut gap_at_pole = false;

            if uv_gap_size > uv_scaled_zero {
                let uv0 = Point2::new(uv_pnt0.x, uv_pnt0.y);
                let uv1 = Point2::new(uv_pnt1.x, uv_pnt1.y);

                let (u0, v0) =
                    srf_uv_domain.classify_point(uv0, u_scaled_zero, Some(v_scaled_zero));
                let (u1, v1) =
                    srf_uv_domain.classify_point(uv1, u_scaled_zero, Some(v_scaled_zero));
                let start = |t: ExtentPointType| {
                    matches!(t, ExtentPointType::Start | ExtentPointType::Both)
                };
                let end =
                    |t: ExtentPointType| matches!(t, ExtentPointType::End | ExtentPointType::Both);
                gap_at_pole = (srf_poles & SS_UMIN != 0 && start(u0) && start(u1))
                    || (srf_poles & SS_UMAX != 0 && end(u0) && end(u1))
                    || (srf_poles & SS_VMIN != 0 && start(v0) && start(v1))
                    || (srf_poles & SS_VMAX != 0 && end(v0) && end(v1));

                if !gap_at_pole {
                    let eu_edge = eu1::edge(b, eu)?
                        .ok_or_else(|| null("CreateUVTrimCurves: edge use without edge"))?;
                    let cw_edge = eu1::edge(b, cw)?
                        .ok_or_else(|| null("CreateUVTrimCurves: edge use without edge"))?;
                    // smos_4Max(a, b, c, d) = smos_Max(smos_Max(a, b), smos_Max(c, d))
                    let tol_3d = smos_max(
                        smos_max(brep_tolerance(b, f)?, b.vertex(v)?.tolerance),
                        smos_max(b.edge(eu_edge)?.tolerance, b.edge(cw_edge)?.tolerance),
                    );

                    // status ignored upstream
                    let fixed = ignored(face2::fix_uv_trim_curves(
                        k,
                        b,
                        eu,
                        cw,
                        v,
                        tol_3d,
                        uv_gap_size,
                        &uv_domain,
                    ))?
                    .unwrap_or(false);

                    if fixed {
                        uv_pnt0 = normalized_evaluate(k, b, eu, 0.0, true, false)?.0;
                        uv_pnt1 = normalized_evaluate(k, b, cw, 1.0, true, false)?.0;
                        uv_gap_size = uv_pnt0.distance(uv_pnt1);
                    }
                }
            }

            let pnt_3d0 = normalized_evaluate(k, b, eu, 0.0, false, false)?.0;
            let gap_3d = pnt_3d0.distance(b.vertex(v)?.point);

            mean_cc_gap += if gap_at_pole { 0.0 } else { uv_gap_size };
            max_cc_gap = smos_max(max_cc_gap, uv_gap_size);
            mean_3d_gap += gap_3d;
            max_3d_gap = smos_max(max_3d_gap, gap_3d);

            if adjust_tolerances && b.vertex(v)?.tolerance < 2.0 * gap_3d {
                vertex::set_tolerance(b, v, 2.0 * gap_3d, true, true)?;
            }
        }
    }

    let n = total_dropped as f64;
    Ok(UvTrimCurveGaps {
        mean_curve_surface_gap: mean_cs_gap / n,
        max_curve_surface_gap: max_cs_gap,
        mean_3d_vertex_gap: mean_3d_gap / n,
        max_3d_vertex_gap: max_3d_gap,
        mean_ps_curve_curve_gap: mean_cc_gap / n,
        max_ps_curve_curve_gap: max_cc_gap,
    })
}

// ---- degeneracy ---------------------------------------------------------------------------

/// The geometry behind the edge handles of [`is_degenerate`]'s classifications.
struct EdgeGeometry<'a> {
    b: &'a Brep,
}

impl ClassGeometry<EntityRef> for EdgeGeometry<'_> {
    fn curve(&self, o: EntityRef) -> Option<&Curve> {
        match o {
            EntityRef::Edge(e) => self.b.edge(e).ok()?.curve.as_ref(),
            _ => None,
        }
    }
    fn surface(&self, o: EntityRef) -> Option<&Surface> {
        match o {
            EntityRef::Face(f) => self.b.face(f).ok()?.surface.as_ref(),
            _ => None,
        }
    }
    fn volume(&self, _o: EntityRef) -> Option<&Volume> {
        None
    }
    fn point(&self, o: EntityRef) -> Option<Point3> {
        match o {
            EntityRef::Vertex(v) => Some(self.b.vertex(v).ok()?.point),
            _ => None,
        }
    }
    // the handles are the face's B-rep entities: the topology members
    // (`SmEdge::GetTolerance`, ...) read them there
    fn topology_entity(&self, o: EntityRef) -> Option<(&Brep, EntityRef)> {
        Some((self.b, o))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<EntityRef> {
        Some(e)
    }
}

/// The curve objects of [`is_degenerate`]'s coincidence solutions (upstream passes the
/// edge curves): their natural intervals.
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

/// `IsDegenerate`: whether every edge of the face is degenerate or completely
/// coincident with other edges of the face (all its boundaries within tolerance of
/// other boundaries). Unless `skip_quick_check`, a point stepped `2 * tol` into the
/// face from the first non-degenerate edge that classifies inside the face proves it
/// non-degenerate. `opt_zone_tol_3d` replaces the edge (and step) tolerances. Upstream
/// defaults: `false`, `None`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an edge
/// without curve; those of the degeneracy tests; [`SolidError::Unported`] (the
/// classification of a curve, `intersect.curve_class_p4`; `NormalizedEvaluate`;
/// `InsertTopologyIntersections`). A failing coincidence check returns `false`, as
/// upstream.
#[allow(clippy::too_many_lines)] // one upstream routine
pub fn is_degenerate(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    skip_quick_check: bool,
    opt_zone_tol_3d: Option<f64>,
) -> Result<bool> {
    // return value
    let mut face_degenerate = true;

    // locals
    let mut non_degen_edgeuse: Option<EdgeuseId> = None;

    // get Face->edges
    let edges = face_edges(b, f)?;
    let mut classify: Vec<Option<CurveClassification<EntityRef>>> = Vec::new();

    // for every edge - check for degeneracy
    for &e in &edges {
        let interval = b.edge(e)?.interval;
        let edge_tol = b.edge(e)?.tolerance;
        let zone_tol = opt_zone_tol_3d.unwrap_or(edge_tol);

        // when edge is degenerate - its classification is NULL
        if edge_curve(b, e)?.is_degenerate(k, zone_tol, Some(&interval))? {
            classify.push(None);
        } else {
            classify.push(Some(curve_classification(
                k,
                EntityRef::Edge(e),
                &interval,
                None,
                edge_tol,
            )?));

            // init pNonDegenEdgeuse with 1st NonDegen Edge->Edgeuse connected to this face
            if non_degen_edgeuse.is_none() {
                non_degen_edgeuse = edge::upward_edgeuse_of_face(b, e, Some(f))?;
            }
        }
    }

    // no work - all Face->edges are degen
    let Some(non_degen_eu) = non_degen_edgeuse else {
        return Ok(true);
    };

    // 1st quick check - Step into face by 2*tol from 1st NonDegenEdge NearMidPt
    if !skip_quick_check {
        let face_zone_tol = face2::tolerance(b, f)?;
        let non_degen_edge = eu1::edge(b, non_degen_eu)?
            .ok_or_else(|| null("IsDegenerate: edge use without edge"))?;
        let edge_zone_tol = b.edge(non_degen_edge)?.tolerance;

        // evaluate pNonDegenEdgeuse OffsetIntoFace pt near EdgeMidPoint
        if let Some((uv_pt, uv_deriv)) =
            ignored(normalized_evaluate(k, b, non_degen_eu, 0.51, true, true))?
        {
            let uv_deriv = uv_deriv.unwrap_or(Vec3::UNDEF);
            // Rotate the UV derivative 90 degrees CCW: points 'inside' face.
            let mut uv_deriv = Vec3::new(-uv_deriv.y, uv_deriv.x, uv_deriv.z);

            // We have to get the magnitude of this vector in 3d.
            let uv_2d = Point2::new(uv_pt.x, uv_pt.y);
            let eval = ignored(surface_of(b, f)?.evaluate_1st_derivatives(k, uv_2d, false, false))?;
            if let Some((_srf_pt, su, sv)) = eval {
                // get 3d vector from edge pointing into face - sized to Edge 1st Deriv
                let vec_3d = su * uv_deriv.x + sv * uv_deriv.y;
                let len_3d = vec_3d.length();

                // when 3d vector length is larger than FaceZoneTol3d - resize it to
                // 2*FaceZoneTol3d and classify
                if len_3d > face_zone_tol {
                    uv_deriv *= match opt_zone_tol_3d {
                        Some(t) => 2.0 * t / len_3d,
                        None => 2.0 * face_zone_tol / len_3d,
                    };
                    let uv_test_pt = Point2::new(uv_pt.x + uv_deriv.x, uv_pt.y + uv_deriv.y);
                    let mut pc = PointClassification::<EntityRef>::new(face_zone_tol);
                    let classified = ignored(qf2::point_classify(
                        k,
                        b,
                        f,
                        uv_test_pt,
                        &mut pc,
                        qf2::PointClassifyOptions {
                            src_zone_tol_3d: edge_zone_tol,
                            do_boundary_intersections: false,
                            use_uv_classification: true,
                            ..qf2::PointClassifyOptions::default()
                        },
                    ))?;

                    // if the point classify is in the face - the face is not degenerate
                    if classified.is_some() && pc.point_class() == PointClassType::Face {
                        return Ok(false);
                    }
                }
            }
        }
    }

    // arrive here: Quick check was skipped or failed to identify the face as
    // NonDegenerate - find and mark every nonDegenerate edge/edge coincidence
    let mut coincidences = vec![false; edges.len()];

    // for every FaceEdge - check for coincidence with some other FaceEdge
    for ii in 0..edges.len() {
        let e1 = edges
            .get(ii)
            .copied()
            .ok_or_else(|| null("IsDegenerate: edge index"))?;
        let zone_tol1 = opt_zone_tol_3d.unwrap_or(b.edge(e1)?.tolerance);

        // skip degenerate edges
        if classify.get(ii).and_then(Option::as_ref).is_none() {
            continue;
        }

        // compare Edge1 with all other nonDegenerate edges for coincidence
        for jj in ii + 1..edges.len() {
            let e2 = edges
                .get(jj)
                .copied()
                .ok_or_else(|| null("IsDegenerate: edge index"))?;
            let zone_tol2 = opt_zone_tol_3d.unwrap_or(b.edge(e2)?.tolerance);
            let xsect_tol = xsect_tol_3d_from_zones(k, zone_tol1, zone_tol2);

            // skip degenerate edges or if they are both known to be coincident
            let c1 = coincidences.get(ii).copied().unwrap_or(false);
            let c2 = coincidences.get(jj).copied().unwrap_or(false);
            if classify.get(jj).and_then(Option::as_ref).is_none() || (c1 && c2) {
                continue;
            }

            // check for coincidence (the pre SM_TOLERANT_XSECT version, no snapping)
            let ivl1 = b.edge(e1)?.interval;
            let ivl2 = b.edge(e2)?.interval;
            let curve1 = edge_curve(b, e1)?;
            let curve2 = edge_curve(b, e2)?;
            let found = curve1.global_coincidence_checker(
                k,
                &ivl1,
                curve2,
                &ivl2,
                xsect_tol,
                false,
                [
                    Some(CurveHandle(curve1.natural_interval())),
                    Some(CurveHandle(curve2.natural_interval())),
                ],
            );

            // failure case - quit and return False as the do the least harm answer
            let found = match found {
                Ok(c) => c,
                Err(e) if e.is_unported() => return Err(e),
                Err(_) => return Ok(false),
            };

            // Solution objects are SmCurve pointers. For Compatibility with upcoming
            // calls switch solution objects from SmCurve to SmEdge pointers
            let solutions: Vec<Solution<EntityRef>> = found
                .solutions
                .iter()
                .map(|sol| Solution {
                    solution_type: sol.solution_type,
                    num_variables: sol.num_variables,
                    start: sol.start,
                    end: sol.end,
                    num_objects: sol.num_objects,
                    objects: {
                        let mut o = [None; MAX_TREES];
                        if let Some(x) = o.get_mut(0) {
                            *x = Some(EntityRef::Edge(e1));
                        }
                        if let Some(x) = o.get_mut(1) {
                            *x = Some(EntityRef::Edge(e2));
                        }
                        o
                    },
                    nodes: sol.nodes,
                })
                .collect();

            if !solutions.is_empty() {
                let geometry = EdgeGeometry { b };
                // insert any GlobalCoincidenceChecker() solutions into both curve
                // classifications
                let (left, right) = classify.split_at_mut(jj);
                let cc1 = left
                    .get_mut(ii)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| null("IsDegenerate: classification"))?;
                let cc2 = right
                    .first_mut()
                    .and_then(Option::as_mut)
                    .ok_or_else(|| null("IsDegenerate: classification"))?;
                // statuses ignored upstream
                ignore_status(cc1.insert_topology_intersections(k, &geometry, &solutions, 0))?;
                ignore_status(cc2.insert_topology_intersections(k, &geometry, &solutions, 1))?;

                // done with this edge once it is completely coincident
                if cc1.is_coincident(k, &geometry, &ivl1, zone_tol1)? {
                    if let Some(c) = coincidences.get_mut(ii) {
                        *c = true;
                    }
                }
                if cc2.is_coincident(k, &geometry, &ivl2, zone_tol2)? {
                    if let Some(c) = coincidences.get_mut(jj) {
                        *c = true;
                    }
                }
            }
        }

        // when CrvClassification for pEdge1 is completely coincident
        let ivl1 = b.edge(e1)?.interval;
        let cc1 = classify
            .get(ii)
            .and_then(Option::as_ref)
            .ok_or_else(|| null("IsDegenerate: classification"))?;
        if !coincidences.get(ii).copied().unwrap_or(false)
            && cc1.is_coincident(k, &EdgeGeometry { b }, &ivl1, zone_tol1)?
        {
            if let Some(c) = coincidences.get_mut(ii) {
                *c = true;
            }
        }

        // If pEdge1 is not degenerate and not completely coincident the face is not
        // degenerate
        if !coincidences.get(ii).copied().unwrap_or(false) {
            face_degenerate = false;
            break;
        }
    }

    // all done
    Ok(face_degenerate)
}

// ---- face/face intersection ---------------------------------------------------------------

/// The objects [`face_intersect`]'s classifications refer to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntersectObject {
    /// The surface/surface intersection curve at this index
    /// ([`FaceIntersection::cc_curves_3d`]).
    Curve(usize),
    /// The UV curve of intersection curve `.0` on this face (`.1 == false`) or the
    /// other face.
    UvCurve(usize, bool),
    /// The surface of a face.
    Surface(FaceId),
    /// The 3D curve of an edge.
    EdgeCurve(EdgeId),
    /// The UV trim curve stored on an edge use.
    Trim(EdgeuseId),
    /// A topology entity of the B-rep.
    Topology(EntityRef),
}

impl ClassObject for IntersectObject {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve(_) | Self::UvCurve(..) | Self::EdgeCurve(_) | Self::Trim(_) => {
                ClassObjectKind::Curve
            }
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Topology(e) => e.class_kind(),
        }
    }
}

/// The options of [`face_intersect`]: which optional outputs are wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // upstream's optional output pointers
pub struct FaceIntersectOptions {
    /// `pOptThisUVCurves`.
    pub want_this_uv_curves: bool,
    /// `pOptOtherUVCurves`.
    pub want_other_uv_curves: bool,
    /// `pOpt3DPoints`.
    pub want_points: bool,
    /// `pOptThisCrvClasses`.
    pub want_this_classes: bool,
    /// `pOptOtherCrvClasses`.
    pub want_other_classes: bool,
    /// `pOptCC3DCrvs`, `pOptThisCCUVCrvs`, `pOptOtherCCUVCrvs`. The curves are returned
    /// also when either classification array is requested (the classifications index
    /// them).
    pub want_cc_curves: bool,
    /// `pbOptCoincidenceFlag`: whether the surfaces are known to be coincident (`None`:
    /// run the coincidence check).
    pub coincidence: Option<bool>,
}

/// The outputs of [`face_intersect`].
#[derive(Debug, Clone, Default)]
pub struct FaceIntersection {
    /// `r3DCurves`: the intersection curves lying in both faces.
    pub curves_3d: Vec<Curve>,
    /// `pOptThisUVCurves`: their UV curves on this face (when asked).
    pub this_uv_curves: Option<Vec<Option<Curve>>>,
    /// `pOptOtherUVCurves`: their UV curves on the other face (when asked).
    pub other_uv_curves: Option<Vec<Option<Curve>>>,
    /// `pOpt3DPoints`: single-point intersections (when asked).
    pub points_3d: Option<Vec<Point3>>,
    /// `pOptThisCrvClasses`: this face's classification of every surface/surface curve
    /// that yielded curves (when asked).
    pub this_classes: Option<Vec<CurveClassification<IntersectObject>>>,
    /// `pOptOtherCrvClasses`: the other face's classifications (when asked).
    pub other_classes: Option<Vec<CurveClassification<IntersectObject>>>,
    /// `pOptCC3DCrvs`: the surface/surface intersection curves (when asked).
    pub cc_curves_3d: Option<Vec<Curve>>,
    /// `pOptThisCCUVCrvs`: their UV curves on this surface (when asked).
    pub cc_this_uv_curves: Option<Vec<Option<Curve>>>,
    /// `pOptOtherCCUVCrvs`: their UV curves on the other surface (when asked).
    pub cc_other_uv_curves: Option<Vec<Option<Curve>>>,
}

/// The classification objects of [`face_intersect`] (also used by
/// `topology::brep::p7`'s `MergeSelfIntersectionCurves`).
pub(crate) struct IntersectObjects<'a> {
    /// The B-rep of the faces.
    pub(crate) b: &'a Brep,
    /// The curves behind [`IntersectObject::Curve`].
    pub(crate) curves_3d: &'a [Curve],
    /// The curves behind [`IntersectObject::UvCurve`] `(i, false)`.
    pub(crate) uv1: &'a [Option<Curve>],
    /// The curves behind [`IntersectObject::UvCurve`] `(i, true)`.
    pub(crate) uv2: &'a [Option<Curve>],
}

impl qf2::CurveOnClassifyObjects<IntersectObject> for IntersectObjects<'_> {
    fn face(&self, f: FaceId) -> IntersectObject {
        IntersectObject::Topology(EntityRef::Face(f))
    }
    fn vertex(&self, v: VertexId) -> IntersectObject {
        IntersectObject::Topology(EntityRef::Vertex(v))
    }
    fn edge(&self, e: EdgeId) -> IntersectObject {
        IntersectObject::Topology(EntityRef::Edge(e))
    }
    fn vertexuse(&self, vu: VertexuseId) -> IntersectObject {
        IntersectObject::Topology(EntityRef::Vertexuse(vu))
    }
    fn edgeuse(&self, eu: EdgeuseId) -> IntersectObject {
        IntersectObject::Topology(EntityRef::Edgeuse(eu))
    }
    fn edge_curve(&self, e: EdgeId) -> IntersectObject {
        IntersectObject::EdgeCurve(e)
    }
    fn trim_curve(&self, eu: EdgeuseId) -> IntersectObject {
        IntersectObject::Trim(eu)
    }
    fn face_surface(&self, f: FaceId) -> Option<IntersectObject> {
        Some(IntersectObject::Surface(f))
    }
    fn loop_id(&self, tgt_loop: IntersectObject) -> Option<LoopId> {
        match tgt_loop {
            IntersectObject::Topology(EntityRef::Loop(l)) => Some(l),
            _ => None,
        }
    }
}

impl ClassGeometry<IntersectObject> for IntersectObjects<'_> {
    fn classification_vertex_point(&self, o: IntersectObject) -> Option<Point3> {
        match o {
            IntersectObject::Topology(EntityRef::Vertex(v)) => {
                self.b.vertex(v).ok().map(|v| v.point)
            }
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: IntersectObject) -> Option<f64> {
        match o {
            IntersectObject::Topology(EntityRef::Vertex(v)) => {
                self.b.vertex(v).ok().map(|v| v.tolerance)
            }
            IntersectObject::Topology(EntityRef::Face(f)) => {
                self.b.face(f).ok().map(|f| f.tolerance)
            }
            _ => None,
        }
    }
    fn curve(&self, o: IntersectObject) -> Option<&Curve> {
        match o {
            IntersectObject::Curve(i) => self.curves_3d.get(i),
            IntersectObject::UvCurve(i, false) => self.uv1.get(i)?.as_ref(),
            IntersectObject::UvCurve(i, true) => self.uv2.get(i)?.as_ref(),
            IntersectObject::EdgeCurve(e) => self.b.edge(e).ok()?.curve.as_ref(),
            IntersectObject::Trim(eu) => eu1::uv_trim_curve(self.b, eu).ok()?.0,
            IntersectObject::Surface(_) | IntersectObject::Topology(_) => None,
        }
    }
    fn surface(&self, o: IntersectObject) -> Option<&Surface> {
        match o {
            IntersectObject::Surface(f) => self.b.face(f).ok()?.surface.as_ref(),
            _ => None,
        }
    }
    fn volume(&self, _o: IntersectObject) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: IntersectObject) -> Option<Point3> {
        None
    }
}

/// `FaceIntersect`: the curves (and, when asked, points) where face `f` meets
/// `other`. Coincident surfaces (by `options.coincidence`, else
/// `SmSurface::CoincidenceCheck` within `approx_tol_3d`) give copies of both faces'
/// non-degenerate edge curves trimmed to their edges; otherwise the surfaces are
/// intersected (`GlobalSurfaceIntersect` over the face domains, angle tolerance
/// `crv_ang_tol_deg`). Each intersection curve is classified on both faces and the
/// pieces inside both are kept; touching points inside or on both faces are points.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for a face
/// without surface; those of the intersection, copies and classifications;
/// [`SolidError::Unported`] (`Point3DClassify`, the curve classification
/// constructor).
#[allow(clippy::too_many_lines)] // one upstream routine
pub fn face_intersect(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    other: FaceId,
    approx_tol_3d: f64,
    crv_ang_tol_deg: f64,
    options: &FaceIntersectOptions,
) -> Result<FaceIntersection> {
    let mut out = FaceIntersection {
        this_uv_curves: options.want_this_uv_curves.then(Vec::new),
        other_uv_curves: options.want_other_uv_curves.then(Vec::new),
        points_3d: options.want_points.then(Vec::new),
        this_classes: options.want_this_classes.then(Vec::new),
        other_classes: options.want_other_classes.then(Vec::new),
        ..FaceIntersection::default()
    };

    let use_surface_edges = [true, true];
    let ang_tol_rad = crv_ang_tol_deg * PI / 180.0;
    let other_zone_tol = face2::tolerance(b, other)?;
    let this_zone_tol = face2::tolerance(b, f)?;
    let zone_tol = smos_max(this_zone_tol, other_zone_tol);

    let mut curves_3d: Vec<Curve> = Vec::new();
    let mut uv_curves1: Vec<Option<Curve>> = Vec::new();
    let mut uv_curves2: Vec<Option<Curve>> = Vec::new();
    let mut curve_types: Vec<TsectCurveType> = Vec::new();

    let (are_coincident, coinc_dist) = if let Some(c) = options.coincidence {
        (c, 0.0)
    } else {
        // run the Coincidence checker
        coincidence_check(k, surface_of(b, f)?, surface_of(b, other)?, approx_tol_3d)?
    };

    let mut have_curves = false;
    if are_coincident && coinc_dist < approx_tol_3d {
        have_curves = true;
        'faces: for face in [f, other] {
            for e in face_edges(b, face)? {
                let edge_crv = edge_curve(b, e)?;
                if edge_crv.is_degenerate(k, EFF_ZERO, None)? {
                    continue;
                }
                let mut new_crv = match edge_crv.copy(k) {
                    Ok(c) => c,
                    Err(e) if e.is_unported() => return Err(e),
                    Err(_) => {
                        have_curves = false;
                        break 'faces;
                    }
                };
                let mut interval = b.edge(e)?.interval;
                // may snap sIvl by tol to existing knots (status ignored)
                ignored(new_crv.trim(k, &mut interval, TrimMode::Notify))?;
                curves_3d.push(new_crv);
                uv_curves1.push(None);
                uv_curves2.push(None);
                curve_types.push(TsectCurveType::RegionBoundary);
            }
        }
    }

    if !have_curves {
        let r = surface_of(b, f)?.global_surface_intersect(
            k,
            &face2::uv_domain(b, f)?,
            surface_of(b, other)?,
            &face2::uv_domain(b, other)?,
            use_surface_edges,
            Some(approx_tol_3d),
            Some(ang_tol_rad),
        )?;
        curves_3d = r.curves_3d;
        uv_curves1 = r.surface1_uv_curves;
        uv_curves2 = r.surface2_uv_curves;
        curve_types = r.curve_types;
    }

    for (ii, curve) in curves_3d.iter().enumerate() {
        let curve_type = curve_types
            .get(ii)
            .copied()
            .ok_or_else(|| null("FaceIntersect: fewer curve types than curves"))?;
        if curve_type == TsectCurveType::Touching {
            let Some(points) = out.points_3d.as_mut() else {
                continue;
            };
            // status ignored upstream
            let point = ignored(curve.evaluate_point(k, 0.5))?.unwrap_or(Vec3::UNDEF);
            let mut pc1 = PointClassification::<IntersectObject>::new(this_zone_tol);
            let mut pc2 = PointClassification::<IntersectObject>::new(other_zone_tol);
            // statuses ignored upstream
            ignore_status(point_3d_classify(k, b, f, point, this_zone_tol, &mut pc1))?;
            ignore_status(point_3d_classify(
                k,
                b,
                other,
                point,
                other_zone_tol,
                &mut pc2,
            ))?;
            let (pct1, pct2) = (pc1.point_class(), pc2.point_class());
            let on = |t: PointClassType| {
                matches!(
                    t,
                    PointClassType::Face | PointClassType::Edge | PointClassType::Vertex
                )
            };
            if on(pct1) && on(pct2) {
                points.push(point);
            }
            continue;
        }

        let uv1 = uv_curves1
            .get(ii)
            .and_then(Option::as_ref)
            .map(|_| IntersectObject::UvCurve(ii, false));
        let uv2 = uv_curves2
            .get(ii)
            .and_then(Option::as_ref)
            .map(|_| IntersectObject::UvCurve(ii, true));
        let ivl = curve.natural_interval();
        let mut cc1 = curve_classification(k, IntersectObject::Curve(ii), &ivl, uv1, zone_tol)?;
        let mut cc2 = curve_classification(k, IntersectObject::Curve(ii), &ivl, uv2, zone_tol)?;

        let objects = IntersectObjects {
            b,
            curves_3d: &curves_3d,
            uv1: &uv_curves1,
            uv2: &uv_curves2,
        };
        qf2::curve_on_classify(k, b, f, &objects, true, &mut cc1, false, false, true)?;
        qf2::curve_on_classify(k, b, other, &objects, true, &mut cc2, false, false, true)?;

        let ex = cc1.extract_geometry(
            k,
            &objects,
            &mut cc2,
            ExtractOptions {
                uv_curves1: true,
                uv_curves2: true,
                points_3d: true,
            },
        )?;
        let found = !ex.curves_3d.is_empty();
        out.curves_3d.extend(ex.curves_3d);
        if let Some(v) = out.this_uv_curves.as_mut() {
            v.extend(ex.uv_curves1.into_iter().map(Some));
        }
        if let Some(v) = out.other_uv_curves.as_mut() {
            v.extend(ex.uv_curves2.into_iter().map(Some));
        }
        if let Some(v) = out.points_3d.as_mut() {
            v.extend(ex.points_3d);
        }
        if found {
            if let Some(v) = out.this_classes.as_mut() {
                v.push(cc1);
            }
            if let Some(v) = out.other_classes.as_mut() {
                v.push(cc2);
            }
        }
    }

    // The classifications refer to these curves by index, so they are returned whenever
    // classifications are requested too (upstream keeps them alive through the
    // classifications; `pOptCC3DCrvs` is then only the caller's cleanup list).
    if options.want_cc_curves || options.want_this_classes || options.want_other_classes {
        out.cc_curves_3d = Some(curves_3d);
        out.cc_this_uv_curves = Some(uv_curves1);
        out.cc_other_uv_curves = Some(uv_curves2);
    }
    Ok(out)
}
