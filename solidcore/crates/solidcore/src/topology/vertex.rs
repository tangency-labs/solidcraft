//! Vertices (`SmVertex`): the point and tolerance of a vertex, its vertex use
//! list and the topology reached through it (edges, faces, face uses, shells, regions),
//! the connectivity and classification predicates (wire, lamina, shell, loop,
//! topological and pole vertices), the tangency test of two edges at the vertex, the
//! largest cached gap and the notification step.
//!
//! Upstream: `source/SMLib/src/SmVertex.cpp`, `source/SMLib/inc/SmVertex.h`.
//!
//! | Rust                                        | Upstream                                         |
//! |---------------------------------------------|--------------------------------------------------|
//! | [`TYPE`], [`TYPE_STRING`], [`CLASS_STRING`], [`is_kind_of`] | `SmVertex::GetType`, `GetTypeString`, `GetClassString`, `IsKindOf` |
//! | [`crate::topology::Vertex`]                 | `SmVertex` (members; `Default` is `SmVertex()`, `Drop` `~SmVertex`) |
//! | [`point`], [`set_point`]                    | `SmVertex::GetPoint`, `SetPoint`                 |
//! | [`tolerance`], [`set_tolerance`], [`refresh_tolerance`] | `SmVertex::GetTolerance`, `SetTolerance`, `RefreshTolerance` (`SM_USE_OLDTOL`) |
//! | `edge_end_reach` (crate)                    | none (not upstream: a bound on the vertex's distance to its edge ends) |
//! | `cover_merged_ends` (crate)                 | none (not upstream: a point merge's vertex covers its edge ends, #21340) |
//! | [`is_exact`], [`set_exact`]                 | `SmVertex::IsExact`, `SetExact` (without `SM_TOLERANT_WATCH`) |
//! | [`poly_vertex`], [`set_poly_vertex`]        | `SmVertex::GetPolyVertex`, `SetPolyVertex`       |
//! | [`primary_vertexuse`], [`vertexuses`], [`vertexuses_with_attribute`] | `SmVertex::GetPrimaryVertexuse`, `GetVertexuses` |
//! | [`brep`]                                    | `SmVertex::GetBrep`                              |
//! | [`vertexuse_of_face`]                       | `SmVertex::GetVertexuseOfFace`                   |
//! | [`vertexuses_of_face`], [`vertexuses_of_edge`] | `SmVertex::GetVertexusesOfFace`, `GetVertexusesOfEdge` |
//! | [`edges`], [`common_edges`]                 | `SmVertex::GetEdges`, `GetCommonEdges`           |
//! | [`faces`], [`faceuses`], [`shells`], [`regions`], [`shell_in_region`] | `SmVertex::GetFaces`, `GetFaceuses`, `GetShells`, `GetRegions`, `GetShellInRegion` |
//! | [`is_connected_to`]                         | `SmVertex::IsConnectedTo`                        |
//! | [`are_edges_tangent`]                       | `SmVertex::AreEdgesTangent`                      |
//! | [`is_topological_vertex`], [`is_on_pole`]   | `SmVertex::IsTopologicalVertex`, `IsOnPole`      |
//! | [`is_on_face_seam`], [`ends_seam_edge`], [`seam_edge`], [`is_seam_edge`] | none (not upstream, below) |
//! | [`is_wire_vertex`], [`is_lamina_vertex`]    | `SmVertex::IsWireVertex`, `IsLaminaVertex`       |
//! | [`is_shell_vertex`], [`is_loop_vertex`]     | `SmVertex::IsShellVertex`, `IsLoopVertex`        |
//! | [`max_up_dim_gap_3d`], [`max_gap_3d`], [`VertexGap`], [`HeldGap`] | `SmVertex::GetMaxUpDimGap3d`, `GetMaxGap3d` (the `SmGap*` results) |
//! | [`notify`]                                  | `SmVertex::Notify`                               |
//! | [`check_pointers`]                          | `SmVertex::CheckPointers`                        |
//! | [`edge_start_vertex`], [`edge_vertex_pair`], [`edge_vertices`], [`edge_end_tangent`] | `SmEdge::GetStartVertex`, `GetVertices(SmVertex*&, SmVertex*&)`, `GetVertices(SmTArray&)`, `GetEndTangent` (CO-PORTED, `topology.edge`) |
//! | [`loopuse_vertices`], [`loop_vertices`]     | `SmLoopuse::GetVertices`, `SmLoop::GetVertices` (CO-PORTED, `topology.loopuse`) |
//! | [`corner`]                                  | `SmVertex::IsWithinXSectTol3dOfFaces`, `HasParallelFaces`, `CalcCornerFromFaces`, `IsOnFaceCorner`, `sm_AssertTestVertex1` |
//!
//! Deviations:
//! - **Object model.** A vertex is a record in the [`Brep`]'s arenas; the methods are
//!   free functions taking the B-rep and the vertex's id. Pointer arguments and results
//!   are `Option`s of ids, array outputs are returned `Vec`s, out-params and statuses
//!   are return values. The vertex use list (`m_pList`, `m_lListSize`) is
//!   [`crate::topology::Vertex::vertexuses`]. The constructor's and destructor's
//!   `SmObject::Notify` calls are not modelled.
//! - **Null pointers.** Where upstream dereferences a null pointer (an edge vertex use
//!   without edge use, an edge without primary edge use, mate or vertex use, a loop use
//!   without start, an edge without curve whose tangent is evaluated, ...) the port
//!   returns [`SolidError::InvalidInput`]. `SM_ASSERT`s, `SE` reports and the
//!   `SM_DEBUG_CODE` blocks are compiled out of the reference build and are not ported.
//!   Where upstream would put a null edge, face or region into a result list (an edge
//!   use without edge, a face use without face, a shell without region: a malformed
//!   B-rep) the port returns [`SolidError::InvalidInput`]; [`faceuses`] keeps its null
//!   entries (the edge vertex uses of a wire edge have no face use).
//! - **Seam vertices.** Upstream's `IsTopologicalVertex` keeps a vertex where a closed edge
//!   meets itself (it marks the seam) and one on a pole. The port also keeps a vertex that
//!   where its two edges on a face touch a seam of the face's surface from one side
//!   ([`is_on_face_seam`]): joined, they would meet the seam inside one edge, which the face
//!   check's seam test counts as a crossing. Upstream joins the two edges and
//!   leaves a face its own check refuses ("Face crosses Seam").
//! - **Unset tangents and cusps.** `AreEdgesTangent` and `IsTopologicalVertex` ignore a
//!   failed `GetEndTangent` (`SE`) and then read the tangent it did not write. That is
//!   the `SmVector3d` default, `SM_UNDEF_DOUBLE` in every component ([`Vec3::UNDEF`]),
//!   or in `IsTopologicalVertex` the line vector `IsLine` wrote for that edge.
//!   `AngleBetween` then sees a zero-length vector and reports `0`, so upstream both
//!   answer true (WR-256; UB-106 assumed the tangent uninitialised). As the fork
//!   (tangency-labs/smlib 7a35f33, WR-663), `IsTopologicalVertex` returns `Ok(false)` when
//!   `AngleBetween` fails. `IsTopologicalVertex` also returns
//!   the failed status of `GetEndTangent` at an edge's start (`SER`) as its `SmBoolean`,
//!   which is true, and accepts `180°` cusps (`|angle_rad ± PI| <= ang_limit_rad`)
//!   because it compares unoriented curve tangents; the port propagates a failed
//!   start-tangent evaluation as `Err` and orients the tangents through the vertex, so
//!   cusps return `Ok(false)` (WR-92).
//! - **Other uses.** `IsTopologicalVertex` counts only the vertex's edges, so upstream a
//!   vertex that also is a vertex loop in another face (a point contact inside an edge)
//!   is removable; in the port a vertex with any use that is not an edge use is not
//!   (WR-727).
//! - **Curve identity.** Each edge owns its curve, so `pCurve1 != pCurve2` in
//!   `IsTopologicalVertex` is false only when neither edge has a curve (upstream two
//!   edges may share one curve object; the difftest rejects such B-reps).
//! - **Gaps.** `GetMaxUpDimGap3d` returns pointers to the vertex uses' cached gaps and
//!   reads them as they are after the loop; [`max_up_dim_gap_3d`] remembers which
//!   cached gap each entry is and returns copies read at the end (later gap refreshes
//!   update earlier entries' siblings, as upstream). A gap to add while no gap array
//!   was passed is the null dereference upstream (`pOptGapArray->Add` on `NULL`) and
//!   returns [`SolidError::InvalidInput`]. `SmGap::operator<` compares `m_dGap3d`.
//! - **Notify.** The vertex part (passing `PostEdit`, `SplitInBrep`, `MergeInBrep` and
//!   `ChangeGeometry` to every vertex use) runs [`vertexuse::notify`] with each use's
//!   identity from the caller's `vertexuse_ref`; the `SmTopology::Notify` step runs on
//!   the caller's topology records as [`topo::notify`] does. `SetPoint` notifies with
//!   `SM_NO_GET_BREP(this)`, which is `NULL` in the reference build.
//! - **Attributes.** `GetVertexuses(list, &id)` is [`vertexuses_with_attribute`], which
//!   takes the attribute store and each use's identity from the caller.
//! - **Unported callees.** The gap evaluations reached through the vertex uses
//!   (`heal.gap`, surface units) return [`SolidError::Unported`].
//! - **Co-ports.** The small `SmEdge` and `SmLoopuse` accessors this unit reaches are
//!   ported here (table above) and recorded as CO-PORTED in `plan/modules.toml`; the
//!   loop use walk stops with [`SolidError::InvalidInput`] after visiting more edge uses
//!   than the B-rep holds (upstream loops forever on a CCW chain that does not return to
//!   its start). They have no attribute filter (callers here pass none).
//! - `IsZoneTol3dConsistent`, `GetLoopOfFace`,
//!   `GetVertexuseOfEdge`, `GetVertexNumberInBrep`, the `Has*` / `IsEdgeVertex` /
//!   `IsOnSeam` / `IsConnectedToEdge` / `IsConnectedToFace` predicates,
//!   `IsWithinXSectTol3dOfEdges`,
//!   `IsComplexCornerMember`, `RefineGeometry`, `FindAndRemoveVertexuseByEdge`,
//!   `GetMemoryUsed`, `AssertValid`, the graphics and the `Dump`s are not part of this
//!   unit.

pub mod corner;

use crate::curve::curve::{Curve, CurveEvaluate, CurveKind};
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::{AttributeStore, ObjectKey};
use crate::geom::context::Context;
use crate::geom::extent1d::Extent1d;
use crate::geom::object::{NotifyOperation, ObjectRef};
use crate::geom::tol;
use crate::heal::gap::{VertexEdgeGap, VertexFaceGap};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, PI};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::poly::PolyVertexId;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::proto_topology::p1::owning_topology;
#[cfg(test)]
use crate::topology::topology::Topology;
use crate::topology::topology::{self as topo};
use crate::topology::vertexuse;
use crate::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, LoopuseStart, RegionId, ShellId,
    VertexId, VertexuseId, VertexuseOwner,
};

const OP: &str = "topology::vertex";

/// `SmVertex_TYPE` (`TOPO_BASE_TYPE + 21`).
pub const TYPE: i64 = 16021;
/// `SmVertex::GetTypeString`.
pub const TYPE_STRING: &str = "SmVertex_TYPE";
/// `SmVertex::GetClassString`.
pub const CLASS_STRING: &str = "SmVertex";

/// `SM_CONTINUITY_ANGLE` (`SM_ANG_TOL_DEG`, degrees).
const CONTINUITY_ANGLE: f64 = tol::ANG_TOL_DEG;

/// The number of samples `IsTopologicalVertex` asks `SmCurve::IsLine` for.
const IS_LINE_SAMPLES: u64 = 5;

/// `SmVertex::IsKindOf`: [`TYPE`], then the `SmOwningTopology` chain.
#[must_use]
pub const fn is_kind_of(t: i64) -> bool {
    if t == TYPE {
        true
    } else {
        owning_topology::is_kind_of(t)
    }
}

fn null(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// Pushes `x` unless it is already in `v` (`SmTArray::AddUnique`).
fn add_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

// ---- simple access ---------------------------------------------------------------------

/// `GetPoint`: the vertex position.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn point(b: &Brep, v: VertexId) -> Result<Point3> {
    Ok(b.vertex(v)?.point)
}

/// `GetTolerance` (`SM_USE_OLDTOL`): the stored zone tolerance.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn tolerance(b: &Brep, v: VertexId) -> Result<f64> {
    Ok(b.vertex(v)?.tolerance)
}

/// `SetTolerance` (`SM_USE_OLDTOL`): sets the tolerance to `new_zone_tol_3d` (an
/// `SmZoneTol3d` value: a caller passing a double converts it with
/// [`tol::zone_tol3d`], as C++ does at the call), unless `update_only_if_larger`
/// (upstream's default `TRUE`) and it is not larger than the current one.
/// `cascade_to_boundaries` (default `TRUE`) is unused upstream.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn set_tolerance(
    b: &mut Brep,
    v: VertexId,
    new_zone_tol_3d: f64,
    update_only_if_larger: bool,
    cascade_to_boundaries: bool,
) -> Result<()> {
    // SM_REF1(bCascadeToBndries): not used here
    let _ = cascade_to_boundaries;
    let r = b.vertex_mut(v)?;

    // set tolerances - when asked skip shrinking tolerances
    if !update_only_if_larger || new_zone_tol_3d > r.tolerance {
        r.tolerance = new_zone_tol_3d;
    }
    Ok(())
}

/// `IsExact`: always true. `SM_TOLERANT_WATCH` (defined only by `NL_DataStruct.h`) is
/// not defined where the reference build compiles `SmVertex.h`, so its `SmVertex` has
/// no `m_bExact` and the inline `IsExact` returns `TRUE`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn is_exact(b: &Brep, v: VertexId) -> Result<bool> {
    b.vertex(v)?;
    Ok(true)
}

/// `SetExact`: does nothing (no `SM_TOLERANT_WATCH`, see [`is_exact`]).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn set_exact(b: &mut Brep, v: VertexId, exact: bool) -> Result<()> {
    let _ = exact;
    b.vertex(v)?;
    Ok(())
}

/// `GetPolyVertex`: the temporary 3D poly vertex back pointer.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn poly_vertex(b: &Brep, v: VertexId) -> Result<Option<PolyVertexId>> {
    Ok(b.vertex(v)?.poly_vertex)
}

/// `SetPolyVertex`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn set_poly_vertex(b: &mut Brep, v: VertexId, poly_vertex: Option<PolyVertexId>) -> Result<()> {
    b.vertex_mut(v)?.poly_vertex = poly_vertex;
    Ok(())
}

/// `GetPrimaryVertexuse`: the head of the vertex use list.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn primary_vertexuse(b: &Brep, v: VertexId) -> Result<Option<VertexuseId>> {
    Ok(b.vertex(v)?.vertexuses.first().copied())
}

/// `GetVertexuses`: the vertex uses in list order.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn vertexuses(b: &Brep, v: VertexId) -> Result<Vec<VertexuseId>> {
    Ok(b.vertex(v)?.vertexuses.clone())
}

/// `GetVertexuses(list, &attribute_id)`: the vertex uses in list order that carry an
/// attribute with id `attribute_id` in `attributes`, each use identified by
/// `vertexuse_key`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn vertexuses_with_attribute(
    b: &Brep,
    v: VertexId,
    attributes: &AttributeStore,
    vertexuse_key: &dyn Fn(VertexuseId) -> ObjectKey,
    attribute_id: u64,
) -> Result<Vec<VertexuseId>> {
    Ok(b.vertex(v)?
        .vertexuses
        .iter()
        .copied()
        .filter(|&vu| {
            attributes
                .find_attribute(vertexuse_key(vu), attribute_id)
                .is_some()
        })
        .collect())
}

/// `GetBrep`: the B-rep reached through the primary vertex use's shell; `None` without
/// primary vertex use or shell (upstream reports `SE(SM_ERR)` and returns `NULL`).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::shell`].
pub fn brep(b: &Brep, v: VertexId) -> Result<Option<&Brep>> {
    let Some(vu) = primary_vertexuse(b, v)? else {
        return Ok(None);
    };
    match vertexuse::shell(b, vu)? {
        None => Ok(None),
        Some(s) => eu1::shell_brep(b, s),
    }
}

// ---- co-ported accessors of other units --------------------------------------------------

/// `SmFace::SetTolerance(tol)` (default arguments).
fn face_set_tolerance(b: &mut Brep, f: FaceId, tol: f64) -> Result<()> {
    crate::topology::face::p3::set_tolerance(b, f, tol, true, true)
}

/// `SmEdge::SetTolerance(tol)` (default arguments).
fn edge_set_tolerance(b: &mut Brep, e: EdgeId, tol: f64) -> Result<()> {
    crate::topology::edge::set_tolerance(b, e, tol, true, true)
}

/// The vertex of the vertex use at the start of edge use `eu`
/// (`pEU->GetVertexuse()->GetVertex()`, dereferencing the vertex use).
fn edgeuse_vertex(b: &Brep, eu: Option<EdgeuseId>) -> Result<Option<VertexId>> {
    let eu = eu.ok_or_else(|| null("edge without primary edge use or mate"))?;
    let vu = b
        .edgeuse(eu)?
        .vertexuse
        .ok_or_else(|| null("edge use without vertex use"))?;
    Ok(b.vertexuse(vu)?.vertex)
}

/// `SmEdge::GetVertices(rpStartVertex, rpEndVertex)` (CO-PORTED from unit
/// `topology.edge`): the vertices at the start and end of the edge's curve, through the
/// primary edge use (start) and its mate (end) when the primary edge use runs with the
/// curve, swapped otherwise.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an
/// edge without primary edge use, mate or vertex use (upstream dereferences null).
pub fn edge_vertex_pair(b: &Brep, e: EdgeId) -> Result<(Option<VertexId>, Option<VertexId>)> {
    let prim = eu1::primary_edgeuse(b, e)?.ok_or_else(|| null("edge without primary edge use"))?;
    let mate = eu1::mate(b, prim)?;
    if b.edgeuse(prim)?.orientation == OrientType::Same {
        Ok((edgeuse_vertex(b, Some(prim))?, edgeuse_vertex(b, mate)?))
    } else {
        let end = edgeuse_vertex(b, Some(prim))?;
        let start = edgeuse_vertex(b, mate)?;
        Ok((start, end))
    }
}

/// `SmEdge::GetStartVertex` (CO-PORTED from unit `topology.edge`): the vertex at the
/// start of the edge's curve.
///
/// # Errors
///
/// As [`edge_vertex_pair`].
pub fn edge_start_vertex(b: &Brep, e: EdgeId) -> Result<Option<VertexId>> {
    let prim = eu1::primary_edgeuse(b, e)?.ok_or_else(|| null("edge without primary edge use"))?;
    if b.edgeuse(prim)?.orientation == OrientType::Same {
        edgeuse_vertex(b, Some(prim))
    } else {
        edgeuse_vertex(b, eu1::mate(b, prim)?)
    }
}

/// `SmEdge::GetVertices(rVertices)` without attribute filter (CO-PORTED from unit
/// `topology.edge`): the start vertex (possibly null), then the end vertex when it is
/// another non-null vertex; empty without primary edge use.
///
/// # Errors
///
/// As [`edge_vertex_pair`] (without the missing primary edge use).
pub fn edge_vertices(b: &Brep, e: EdgeId) -> Result<Vec<Option<VertexId>>> {
    let Some(prim) = eu1::primary_edgeuse(b, e)? else {
        return Ok(Vec::new());
    };
    let mate = eu1::mate(b, prim)?;
    let (v1, v2) = if b.edgeuse(prim)?.orientation == OrientType::Same {
        (edgeuse_vertex(b, Some(prim))?, edgeuse_vertex(b, mate)?)
    } else {
        let v2 = edgeuse_vertex(b, Some(prim))?;
        (edgeuse_vertex(b, mate)?, v2)
    };
    let mut out = vec![v1];
    if v2.is_some() && v2 != v1 {
        out.push(v2);
    }
    Ok(out)
}

/// `SmEdge::GetEndTangent` (CO-PORTED from unit `topology.edge`): the first derivative
/// of the edge's curve at the start (`at_start`) or end of the edge's interval.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an
/// edge without curve (upstream dereferences null); the curve evaluation's error
/// (upstream's `SER` status).
pub fn edge_end_tangent(k: &Kernel, b: &Brep, e: EdgeId, at_start: bool) -> Result<Vec3> {
    let r = b.edge(e)?;
    let curve = r
        .curve
        .as_ref()
        .ok_or_else(|| null("GetEndTangent: edge without curve"))?;
    let ivl = r.interval;
    let param = if at_start { ivl.min() } else { ivl.max() };
    let pv = curve.evaluate(k, param, 1, true, true)?;
    pv.get(1)
        .copied()
        .ok_or_else(|| null("GetEndTangent: no derivative"))
}

/// `SmLoopuse::GetVertices` without attribute filter (CO-PORTED from unit
/// `topology.loopuse`): the vertex of a vertex loop use, else the vertices at the start
/// of the loop's edge uses in CCW order (each once; a null vertex is kept as `None`).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for a
/// vertex loop use without vertex use, an edge use without vertex use, or a CCW chain
/// that does not return to its start within the B-rep's edge use count.
pub fn loopuse_vertices(b: &Brep, lu: LoopuseId) -> Result<Vec<Option<VertexId>>> {
    let start = match b.loopuse(lu)?.start {
        // When Loopuse_TYPE - get the only Vertex
        LoopuseStart::Vertexuse(vu) => {
            let vu = vu.ok_or_else(|| null("vertex loop use without vertex use"))?;
            return Ok(vec![b.vertexuse(vu)?.vertex]);
        }
        LoopuseStart::Edgeuse(eu) => eu,
        LoopuseStart::Unknown => None,
    };
    let mut out = Vec::new();
    let mut eu = start;
    let mut visited = 0usize;
    loop {
        // for NULL terminated lists
        let Some(cur) = eu else {
            return Ok(out);
        };
        if visited > b.edgeuses().len() {
            return Err(null("loop use CCW chain does not close"));
        }
        visited += 1;
        let vu = b
            .edgeuse(cur)?
            .vertexuse
            .ok_or_else(|| null("loop edge use without vertex use"))?;
        add_unique(&mut out, b.vertexuse(vu)?.vertex);
        eu = b.edgeuse(cur)?.ccw;
        if eu == start {
            return Ok(out);
        }
    }
}

/// `SmLoop::GetVertices` (CO-PORTED from unit `topology.loopuse`): those of the loop's
/// loop use ([`loopuse_vertices`]).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a loop without loop use; those of
/// [`loopuse_vertices`].
pub fn loop_vertices(b: &Brep, l: LoopId) -> Result<Vec<Option<VertexId>>> {
    let lu = b
        .loop_(l)?
        .loopuse
        .ok_or_else(|| null("loop without loop use"))?;
    loopuse_vertices(b, lu)
}

// ---- vertex uses of a face or edge -------------------------------------------------------

/// `GetVertexuseOfFace`: `None` for no face; else the first vertex use on `face`'s
/// upward face use, or the last vertex use on `face`.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// upstream dereferences null.
pub fn vertexuse_of_face(
    b: &Brep,
    v: VertexId,
    face: Option<FaceId>,
) -> Result<Option<VertexuseId>> {
    // init return
    let mut ret = None;

    // no work - no face
    if face.is_none() {
        return Ok(ret);
    }

    // for every Vertexuse - check for a face == pFace match
    for vu in vertexuses(b, v)? {
        // skip vertices not connected to a Face
        let Some(fu) = vertexuse::faceuse(b, vu)? else {
            continue;
        };
        let this_face = b.faceuse(fu)?.face;

        if let Some(this_face) = this_face.filter(|&f| Some(f) == face) {
            // remember any found Vertexuses
            ret = Some(vu);

            // Use the first Vertexuse found on an upward Faceuse.
            if crate::topology::face::p2::upward_faceuse(b, this_face)? == Some(fu) {
                break;
            }
        }
    }

    // all done
    Ok(ret)
}

/// `GetVertexusesOfFace`: the vertex uses whose face use's face is `face` (empty for no
/// face), only those connected to `connect` when given
/// ([`vertexuse::is_connected_to`]).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::faceuse`] and
/// [`vertexuse::is_connected_to`].
pub fn vertexuses_of_face(
    b: &Brep,
    v: VertexId,
    face: Option<FaceId>,
    connect: Option<EntityRef>,
) -> Result<Vec<VertexuseId>> {
    // init return; no work - no face
    let mut out = Vec::new();
    let Some(f) = face else {
        return Ok(out);
    };

    // for every Vertexuse - check for a face == pFace match
    for vu in vertexuses(b, v)? {
        let Some(fu) = vertexuse::faceuse(b, vu)? else {
            continue;
        };
        if b.faceuse(fu)?.face == Some(f) {
            // remember any found Vertexuses that pass the ConnectTgt test
            if connect.is_none() || vertexuse::is_connected_to(b, vu, connect)? {
                out.push(vu);
            }
        }
    }
    Ok(out)
}

/// `GetVertexusesOfEdge`: the vertex uses whose edge use's edge is `edge` (empty for no
/// edge), only those connected to `connect` when given.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of
/// [`vertexuse::is_connected_to`].
pub fn vertexuses_of_edge(
    b: &Brep,
    v: VertexId,
    edge: Option<EdgeId>,
    connect: Option<EntityRef>,
) -> Result<Vec<VertexuseId>> {
    // init return; no work - no edge
    let mut out = Vec::new();
    let Some(e) = edge else {
        return Ok(out);
    };

    // for every Vertexuse - check for a Edge == pEdge match
    for vu in vertexuses(b, v)? {
        let Some(eu) = vertexuse::edgeuse(b, vu)? else {
            continue;
        };
        if b.edgeuse(eu)?.edge == Some(e) {
            // remember any found Vertexuses that pass the ConnectTgt test
            if connect.is_none() || vertexuse::is_connected_to(b, vu, connect)? {
                out.push(vu);
            }
        }
    }
    Ok(out)
}

// ---- adjacent topology ---------------------------------------------------------------------

/// `GetEdges`: the edges of the vertex's edge vertex uses, each once, in vertex use
/// order; only those whose edge use is on `face` when given.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] for an
/// edge vertex use without edge use, a loop use without face use (upstream
/// dereferences null) or an edge use without edge.
pub fn edges(b: &Brep, v: VertexId, face: Option<FaceId>) -> Result<Vec<EdgeId>> {
    let mut out = Vec::new();

    // for every vertexuse
    for vu in vertexuses(b, v)? {
        // when vertexuse is an edgeVertexuse type
        if vertexuse::is_edge_vertexuse(b, vu)? {
            let eu = vertexuse::edgeuse(b, vu)?
                .ok_or_else(|| null("GetEdges: edge vertex use without edge use"))?;
            let f = eu1::face(b, eu)?;
            let e = eu1::edge(b, eu)?;

            // add the edge to the list
            if face.is_none() || face == f {
                let e = e.ok_or_else(|| null("GetEdges: edge use without edge"))?;
                add_unique(&mut out, e);
            }
        }
    }
    Ok(out)
}

/// Not upstream: whether `face` surrounds the vertex on every side: the vertex has edges and
/// each is a seam of `face` ([`crate::topology::edge::face_on_both_sides`]), as a sphere's
/// pole. Such a vertex bounds no material; it is a point where the surface's
/// parametrisation degenerates (docs/booleans.md, "Seams and poles").
///
/// # Errors
///
/// Those of [`edges`] and of the edge use queries.
pub fn inside_face(b: &Brep, v: VertexId, face: FaceId) -> Result<bool> {
    let all = edges(b, v, None)?;
    if all.is_empty() {
        return Ok(false);
    }
    for e in all {
        if !crate::topology::edge::face_on_both_sides(b, e, face)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `GetCommonEdges`: the edges of [`edges`] (on `face` when given) that join this
/// vertex and `other` (closed edges included).
///
/// # Errors
///
/// Those of [`edges`] and [`edge_vertex_pair`].
pub fn common_edges(
    b: &Brep,
    v: VertexId,
    other: Option<VertexId>,
    face: Option<FaceId>,
) -> Result<Vec<EdgeId>> {
    let mut out = Vec::new();

    // For every vertex->Edge
    for e in edges(b, v, face)? {
        let (v1, v2) = edge_vertex_pair(b, e)?;

        // when an edge connects to both vertices (works for closed edges too)
        if (v1 == Some(v) && v2 == other) || (v2 == Some(v) && v1 == other) {
            out.push(e);
        }
    }
    Ok(out)
}

/// `GetFaces`: the faces of the vertex's face uses, each once.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where
/// [`vertexuse::faceuse`] dereferences null and for a face use without face.
pub fn faces(b: &Brep, v: VertexId) -> Result<Vec<FaceId>> {
    let mut out = Vec::new();

    // for every vertexuse
    for vu in vertexuses(b, v)? {
        // when vertexuse is not a ShellVertex
        if !vertexuse::is_shell_vertexuse(b, vu)? {
            if let Some(fu) = vertexuse::faceuse(b, vu)? {
                let f = b
                    .faceuse(fu)?
                    .face
                    .ok_or_else(|| null("GetFaces: face use without face"))?;
                add_unique(&mut out, f);
            }
        }
    }
    Ok(out)
}

/// `GetFaceuses`: the face use of every vertex use that is not a shell vertex use, in
/// vertex use order (`None` for one without face use, e.g. on a wire edge).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::faceuse`].
pub fn faceuses(b: &Brep, v: VertexId) -> Result<Vec<Option<FaceuseId>>> {
    let mut out = Vec::new();
    for vu in vertexuses(b, v)? {
        // when vertexuse is not a ShellVertex
        if !vertexuse::is_shell_vertexuse(b, vu)? {
            out.push(vertexuse::faceuse(b, vu)?);
        }
    }
    Ok(out)
}

/// `GetShells`: the shells of the vertex's uses, each once.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::shell`].
pub fn shells(b: &Brep, v: VertexId) -> Result<Vec<ShellId>> {
    let mut out = Vec::new();
    for vu in vertexuses(b, v)? {
        if let Some(s) = vertexuse::shell(b, vu)? {
            add_unique(&mut out, s);
        }
    }
    Ok(out)
}

/// `GetRegions`: the regions of the shells of the vertex's uses, each once.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::shell`];
/// [`SolidError::InvalidInput`] for a shell without region.
pub fn regions(b: &Brep, v: VertexId) -> Result<Vec<RegionId>> {
    let mut out = Vec::new();
    for vu in vertexuses(b, v)? {
        if let Some(s) = vertexuse::shell(b, vu)? {
            let r =
                eu1::shell_region(b, s)?.ok_or_else(|| null("GetRegions: shell without region"))?;
            add_unique(&mut out, r);
        }
    }
    Ok(out)
}

/// `GetShellInRegion`: the first shell of the vertex's uses whose region is `region`;
/// `None` when there is none (upstream `SM_ERR`).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::shell`].
pub fn shell_in_region(b: &Brep, v: VertexId, region: Option<RegionId>) -> Result<Option<ShellId>> {
    // for every vertexuse
    for vu in vertexuses(b, v)? {
        // find and return the shell that connect to the target region
        if let Some(s) = vertexuse::shell(b, vu)? {
            if eu1::shell_region(b, s)? == region {
                return Ok(Some(s));
            }
        }
    }
    Ok(None)
}

/// `IsConnectedTo`: whether the vertex connects to `target` (itself, one of its uses, an
/// edge or edge use ending at it, a face, face use, loop, loop use, shell or region
/// around it); `true` for `None` and any other kind.
///
/// # Errors
///
/// Those of the queries reaching the target kind ([`edge_vertices`], [`faces`],
/// [`loopuse_vertices`], ...).
pub fn is_connected_to(b: &Brep, v: VertexId, target: Option<EntityRef>) -> Result<bool> {
    // no work - no Topo
    let Some(target) = target else {
        return Ok(true);
    };
    match target {
        EntityRef::Vertex(x) => Ok(x == v),
        EntityRef::Vertexuse(vu) => Ok(vertexuse::vertex(b, vu)? == Some(v)),
        EntityRef::Edge(e) => Ok(edge_vertices(b, e)?.contains(&Some(v))),
        EntityRef::Edgeuse(eu) => match b.edgeuse(eu)?.vertexuse {
            Some(vu) => Ok(b.vertexuse(vu)?.vertex == Some(v)),
            None => Ok(false),
        },
        EntityRef::Face(f) => Ok(faces(b, v)?.contains(&f)),
        EntityRef::Faceuse(fu) => Ok(faceuses(b, v)?.contains(&Some(fu))),
        EntityRef::Loop(l) => Ok(loop_vertices(b, l)?.contains(&Some(v))),
        EntityRef::Loopuse(lu) => Ok(loopuse_vertices(b, lu)?.contains(&Some(v))),
        EntityRef::Shell(s) => Ok(shells(b, v)?.contains(&s)),
        EntityRef::Region(r) => Ok(regions(b, v)?.contains(&r)),
        // arrive here when cpConnectTgt is an unsupported type - return TRUE
        EntityRef::Brep(_) => Ok(true),
    }
}

// ---- predicates ------------------------------------------------------------------------------

/// `GetEndTangent` whose status upstream ignores (`SE`): the tangent, or `None` when
/// the evaluation failed (the tangent is left as it was). A missing curve is the null
/// dereference upstream; [`SolidError::Unported`] propagates.
fn end_tangent_ignoring_status(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    at_start: bool,
) -> Result<Option<Vec3>> {
    match edge_end_tangent(k, b, e, at_start) {
        Ok(t) => Ok(Some(t)),
        Err(
            err @ (SolidError::Unported { .. }
            | SolidError::InvalidInput { .. }
            | SolidError::NotFound { .. }),
        ) => Err(err),
        Err(_) => Ok(None),
    }
}

/// `AreEdgesTangent`: whether edges `e1` and `e2`, both ending at the vertex, leave it
/// in parallel directions (their end tangents, pointing through the vertex, within
/// `angle_tol_deg` degrees).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`edge_start_vertex`];
/// [`SolidError::InvalidInput`] for an edge without curve. A tangent whose evaluation
/// fails is the `SmVector3d` default ([`Vec3::UNDEF`]), as upstream.
pub fn are_edges_tangent(
    k: &Kernel,
    b: &Brep,
    v: VertexId,
    e1: EdgeId,
    e2: EdgeId,
    angle_tol_deg: f64,
) -> Result<bool> {
    // a failed GetEndTangent leaves the default SmVector3d (SM_UNDEF_DOUBLE)
    let tan1 = if edge_start_vertex(b, e1)? == Some(v) {
        end_tangent_ignoring_status(k, b, e1, true)?.unwrap_or(Vec3::UNDEF)
    } else {
        -end_tangent_ignoring_status(k, b, e1, false)?.unwrap_or(Vec3::UNDEF)
    };

    // Note that sTan1 is pointing to the inside of the curve and sTan2 will be pointing
    // to the outside of the curve. That should make them parallel.
    let tan2 = if edge_start_vertex(b, e2)? == Some(v) {
        -end_tangent_ignoring_status(k, b, e2, true)?.unwrap_or(Vec3::UNDEF)
    } else {
        end_tangent_ignoring_status(k, b, e2, false)?.unwrap_or(Vec3::UNDEF)
    };

    // AngleBetween sets the angle to 0 on failure (status ignored)
    let angle = tan1.angle_between(tan2).unwrap_or(0.0);
    Ok(angle * 180.0 / PI < angle_tol_deg)
}

/// `IsOnPole`: whether one of the vertex's uses marks a pole of its face's surface
/// ([`vertexuse::is_on_pole`], stopping at the first).
///
/// # Errors
///
/// Those of [`vertexuse::is_on_pole`].
pub fn is_on_pole(k: &Kernel, b: &mut Brep, v: VertexId) -> Result<bool> {
    let mut on_pole = false;

    // See if vertex marks a pole on one the surfaces to which it connects
    for vu in vertexuses(b, v)? {
        if on_pole {
            break;
        }
        on_pole |= vertexuse::is_on_pole(k, b, vu)?;
    }

    // all done
    Ok(on_pole)
}

/// Not upstream: whether the vertex lies on a seam of one of the faces it bounds
/// ([`vertexuse::is_on_seam_of_face`]).
///
/// # Errors
///
/// Those of [`vertexuse::is_on_seam_of_face`].
pub fn is_on_face_seam(k: &Kernel, b: &mut Brep, v: VertexId) -> Result<bool> {
    for vu in vertexuses(b, v)? {
        if vertexuse::is_on_seam_of_face(k, b, vu)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Not upstream: whether the vertex ends a seam edge ([`seam_edge`]).
///
/// # Errors
///
/// Those of [`seam_edge`].
pub fn ends_seam_edge(b: &Brep, v: VertexId) -> Result<bool> {
    Ok(seam_edge(b, v)?.is_some())
}

/// Not upstream: the first seam edge the vertex ends ([`is_seam_edge`]).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`edges`] and [`is_seam_edge`].
pub fn seam_edge(b: &Brep, v: VertexId) -> Result<Option<EdgeId>> {
    for e in edges(b, v, None)? {
        if is_seam_edge(b, e)? {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

/// Not upstream: whether one face use runs along the edge twice (the two sides of the seam
/// of the face's closed surface). Read from the topology alone, so it holds for the B-rep as
/// it stands, before any geometry is measured.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of the edge use queries.
pub fn is_seam_edge(b: &Brep, e: EdgeId) -> Result<bool> {
    let mut uses: Vec<FaceuseId> = Vec::new();
    for eu in crate::topology::edge::edgeuses(b, e, None, None)? {
        if !eu1::is_loop_edgeuse(b, eu)? {
            continue;
        }
        let Some(fu) = eu1::faceuse(b, eu)? else {
            continue;
        };
        if uses.contains(&fu) {
            return Ok(true);
        }
        uses.push(fu);
    }
    Ok(false)
}

/// `IsTopologicalVertex`: whether the vertex could be removed without changing the
/// B-rep's shape: it joins exactly two different edges, is not on a pole, the edges'
/// curves meet at the same parameter (unless both are lines) and leave the vertex
/// tangent within `SM_CONTINUITY_ANGLE`.
///
/// Edge tangents are oriented along traversal through `v`, so `180°` cusps return
/// `Ok(false)`, and a failed start-tangent evaluation (`GetEndTangent(true)`) propagates
/// its [`SolidError`] instead of returning `Ok(true)` (WR-92). A vertex with a use that
/// is not an edge use (a vertex loop in another face) is not topological (WR-727).
///
/// # Errors
///
/// Those of [`edges`], [`is_on_pole`], `SmCurve::IsLine` and [`edge_start_vertex`];
/// [`SolidError::InvalidInput`] for one edge without curve whose tangent is needed; and
/// any curve evaluation error from [`edge_end_tangent`] at an edge's start. A tangent
/// neither `IsLine` nor `GetEndTangent` wrote is the `SmVector3d` default
/// ([`Vec3::UNDEF`]), as upstream.
pub fn is_topological_vertex(k: &Kernel, b: &mut Brep, v: VertexId) -> Result<bool> {
    let sedges = edges(b, v, None)?;

    // only edges connected to exactly 2 edges are Topological
    let (Some(&edge1), Some(&edge2), 2) = (sedges.first(), sedges.get(1), sedges.len()) else {
        return Ok(false);
    };

    // skip seam vertices - those vertices can't be deleted - they are required to mark
    // the seam
    if edge1 == edge2 {
        return Ok(false);
    }

    // WR-727: joining the two edges removes every use of the vertex, so a use that is
    // not one of theirs (a vertex loop in another face, a shell vertex) keeps it
    for vu in vertexuses(b, v)? {
        if !vertexuse::is_edge_vertexuse(b, vu)? {
            return Ok(false);
        }
    }

    // skip verices marking poles
    if is_on_pole(k, b, v)? {
        return Ok(false);
    }

    // Not upstream: skip vertices on a face's seam. An edge that touches a seam needs a
    // vertex there as one that crosses it does (the face check's seam test counts a seam
    // point inside an edge as a crossing); joining the two edges would remove it.
    if is_on_face_seam(k, b, v)? {
        return Ok(false);
    }

    // remember if curves are Lines
    let line_of = |e: EdgeId| -> Result<Option<Vec3>> {
        let r = b.edge(e)?;
        match &r.curve {
            Some(c) => {
                let inside_out_line = matches!(c.kind(), CurveKind::Line(l) if l.inside_out());
                Ok(c.is_line(k, IS_LINE_SAMPLES, r.tolerance / 1000.0)?
                    .map(|(_, t)| if inside_out_line { -t } else { t }))
            }
            None => Ok(None),
        }
    };
    let line1 = line_of(edge1)?;
    let line2 = line_of(edge2)?;
    let mut tan1 = line1;
    let mut tan2 = line2;

    // pCurve1 != pCurve2: each edge owns its curve, so only two missing curves are the
    // same (see the module docs)
    let curves_differ = b.edge(edge1)?.curve.is_some() || b.edge(edge2)?.curve.is_some();

    let mut ends_at_start = None;

    // when either curve is not a line - Curve params must be the same
    if line1.is_none() || line2.is_none() {
        // Check start/end ends of both edges.
        let at_start1 = edge_start_vertex(b, edge1)? == Some(v);
        let at_start2 = edge_start_vertex(b, edge2)? == Some(v);
        ends_at_start = Some((at_start1, at_start2));

        // First check that they are at the same parameter value.
        let ivl1 = b.edge(edge1)?.interval;
        let ivl2 = b.edge(edge2)?.interval;
        let param1 = if at_start1 { ivl1.min() } else { ivl1.max() };
        let param2 = if at_start2 { ivl2.min() } else { ivl2.max() };

        // Only vertices connected to the same params are Topological
        if !tol::are_same(param1, param2) {
            return Ok(false);
        }

        // If different curves, get tangent directions at this vertex.
        if curves_differ {
            for (e, at_start, tan) in [(edge1, at_start1, &mut tan1), (edge2, at_start2, &mut tan2)]
            {
                if at_start {
                    // SER: upstream returns a failed status as the SmBoolean result
                    // (TRUE); WR-92 propagates it.
                    *tan = Some(edge_end_tangent(k, b, e, true)?);
                } else if let Some(t) = end_tangent_ignoring_status(k, b, e, false)? {
                    // SE: a failed evaluation leaves the tangent as it was
                    *tan = Some(t);
                }
            }
        }
    }

    // check the angle, to within 1 degree.
    let ang_limit_rad = CONTINUITY_ANGLE * (PI / 180.0);

    let angle_rad = if curves_differ {
        // a tangent nothing wrote is the default SmVector3d (SM_UNDEF_DOUBLE)
        let mut t1 = tan1.unwrap_or(Vec3::UNDEF);
        let mut t2 = tan2.unwrap_or(Vec3::UNDEF);
        // WR-92: orient both along traversal through `v`, so a cusp is 180° apart
        let (at_start1, at_start2) = if let Some(ends) = ends_at_start {
            ends
        } else {
            (
                edge_start_vertex(b, edge1)? == Some(v),
                edge_start_vertex(b, edge2)? == Some(v),
            )
        };
        if at_start1 {
            t1 = -t1;
        }
        if !at_start2 {
            t2 = -t2;
        }
        // AngleBetween sets the angle to 0 on failure (status ignored upstream); the
        // curves are not shown to meet G1 without it, so the vertex stays
        // (tangency-labs/smlib 7a35f33)
        match t1.angle_between(t2) {
            Err(_) => return Ok(false),
            Ok(a) => a,
        }
    } else {
        0.0
    };

    // when lines are not G1 - Vertex is not topological (upstream also let cusps pass:
    // "gwc: is this a bug? it counts cusps"; WR-92)
    if angle_rad.abs() > ang_limit_rad {
        return Ok(false);
    }

    // Passed all our tests.
    Ok(true)
}

/// `IsWireVertex`: every edge at the vertex is a wire edge (true without edges).
///
/// # Errors
///
/// Those of [`edges`].
pub fn is_wire_vertex(b: &Brep, v: VertexId) -> Result<bool> {
    for e in edges(b, v, None)? {
        if !vertexuse::edge_is_wire(b, e)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `IsLaminaVertex`: some edge at the vertex is a lamina edge.
///
/// # Errors
///
/// Those of [`edges`].
pub fn is_lamina_vertex(b: &Brep, v: VertexId) -> Result<bool> {
    for e in edges(b, v, None)? {
        if eu1::edge_is_lamina(b, e)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `IsShellVertex`: the primary vertex use connects directly to a region (false
/// without vertex uses).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn is_shell_vertex(b: &Brep, v: VertexId) -> Result<bool> {
    let Some(vu) = primary_vertexuse(b, v)? else {
        return Ok(false);
    };
    Ok(matches!(b.vertexuse(vu)?.owner, VertexuseOwner::Shell(_)))
}

/// `IsLoopVertex`: the vertex has exactly two uses and the primary one is a vertex
/// loop's (a stand-alone vertex loop of one face).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub fn is_loop_vertex(b: &Brep, v: VertexId) -> Result<bool> {
    let list = &b.vertex(v)?.vertexuses;
    if list.len() != 2 {
        return Ok(false);
    }
    let Some(&vu) = list.first() else {
        return Ok(false);
    };
    Ok(matches!(b.vertexuse(vu)?.owner, VertexuseOwner::Loopuse(_)))
}

// ---- gaps --------------------------------------------------------------------------------------

/// A vertex's up-dimension gap (an `SmGap*` result of [`max_up_dim_gap_3d`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VertexGap {
    /// A vertex use's cached vertex/edge gap.
    Edge(VertexEdgeGap),
    /// A vertex use's cached vertex/face gap.
    Face(VertexFaceGap),
}

impl VertexGap {
    /// `GetLength`: the 3D gap (`m_dGap3d`).
    #[must_use]
    pub const fn length(&self) -> f64 {
        match self {
            Self::Edge(g) => g.gap_3d,
            Self::Face(g) => g.gap_3d,
        }
    }
}

/// A gap cached on a vertex use: upstream's `SmGap*` into the vertex use `holder`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldGap {
    /// The vertex use whose cache holds the gap.
    pub holder: VertexuseId,
    /// The gap (a copy).
    pub gap: VertexGap,
}

/// Which cached gap of which vertex use (upstream's `SmGap*` into the vertex use).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GapRef {
    Edge(VertexuseId),
    Face(VertexuseId),
}

fn read_gap(b: &Brep, g: GapRef) -> Result<HeldGap> {
    Ok(match g {
        GapRef::Edge(vu) => HeldGap {
            holder: vu,
            gap: VertexGap::Edge(b.vertexuse(vu)?.vertex_edge_gap),
        },
        GapRef::Face(vu) => HeldGap {
            holder: vu,
            gap: VertexGap::Face(b.vertexuse(vu)?.vertex_face_gap),
        },
    })
}

/// `GetMaxUpDimGap3d`: the largest cached vertex/edge or vertex/face gap of the
/// vertex's uses (refreshing uninitialised caches through
/// [`vertexuse::vertex_edge_gap`] / [`vertexuse::vertex_face_gap`]), `None` without
/// gaps. With `gaps`, every gap longer than `tol` (all without `tol`) is listed there
/// (the list is cleared first). `tol` is an `SmTol3d` value (non-negative: upstream
/// stores its magnitude). Each gap names the vertex use holding it.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of the gap queries;
/// [`SolidError::InvalidInput`] for a gap to list without `gaps` (upstream dereferences
/// the null array).
pub fn max_up_dim_gap_3d(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
    gaps: Option<&mut Vec<HeldGap>>,
    tol: Option<f64>,
) -> Result<Option<HeldGap>> {
    // init output
    let mut listed: Vec<GapRef> = Vec::new();

    // iter init
    let mut max_gap: Option<GapRef> = None;

    // for every vertexuse
    for vu in vertexuses(b, v)? {
        // Vertex/Edge Gap, then Vertex/Face Gap
        for kind in [GapRef::Edge(vu), GapRef::Face(vu)] {
            let gap = match kind {
                GapRef::Edge(_) => {
                    vertexuse::vertex_edge_gap(k, b, vu, false)?.map(VertexGap::Edge)
                }
                GapRef::Face(_) => {
                    vertexuse::vertex_face_gap(k, b, vu, false)?.map(VertexGap::Face)
                }
            };
            let Some(gap) = gap else {
                continue;
            };

            // Build Gap Array
            if tol.is_none_or(|t| gap.length() > t) {
                if gaps.is_none() {
                    return Err(null("GetMaxUpDimGap3d: gap to add to a null gap array"));
                }
                listed.push(kind);
            }

            // Set Output
            let replace = match max_gap {
                None => true,
                Some(m) => read_gap(b, m)?.gap.length() < gap.length(),
            };
            if replace {
                max_gap = Some(kind);
            }
        }
    }

    // the arrays hold pointers to the cached gaps: read them as they are now
    if let Some(out) = gaps {
        out.clear();
        for g in listed {
            out.push(read_gap(b, g)?);
        }
    }

    // all done
    max_gap.map(|m| read_gap(b, m)).transpose()
}

/// `GetMaxGap3d`: [`max_up_dim_gap_3d`] (a vertex has no down-dimension gaps).
///
/// # Errors
///
/// Those of [`max_up_dim_gap_3d`].
pub fn max_gap_3d(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
    gaps: Option<&mut Vec<HeldGap>>,
    tol: Option<f64>,
) -> Result<Option<HeldGap>> {
    max_up_dim_gap_3d(k, b, v, gaps, tol)
}

// ---- maintenance ---------------------------------------------------------------------------

/// `RefreshTolerance` (`SM_USE_OLDTOL`): when `nested_refreshes` (upstream's default
/// `TRUE`), sets every connected face's and edge's tolerance to its consistent value
/// (`SmTol::GetZoneTol3d` of the object, its own tolerance with `SM_USE_OLDTOL`); then
/// the vertex's.
///
/// # Errors
///
/// Those of [`edges`], [`faces`], `SmEdge::SetTolerance` and `SmFace::SetTolerance`
/// ([`crate::topology::face::p3::set_tolerance`]).
pub fn refresh_tolerance(b: &mut Brep, v: VertexId, nested_refreshes: bool) -> Result<()> {
    // when asked
    if nested_refreshes {
        let sedges = edges(b, v, None)?;
        let sfaces = faces(b, v)?;

        // for every connected Face and Edge - Set Tol to Consistent tolerance
        for f in sfaces {
            let t = b.face(f)?.tolerance;
            face_set_tolerance(b, f, t)?;
        }
        for e in sedges {
            let t = b.edge(e)?.tolerance;
            edge_set_tolerance(b, e, t)?;
        }
    }

    // Set Vertex to consistent tolerance (SmTol::GetZoneTol3d(this) is GetTolerance)
    let t = tolerance(b, v)?;
    set_tolerance(b, v, t, true, true)
}

/// Not upstream (#18166, #21340): an upper bound on the distance from `v`'s point to the
/// ends of all its edges at `v`. Each end's distance is the evaluated end point's distance
/// plus the evaluation's error, bounded per coordinate from the curve's own data
/// ([`crate::curve::curve::Curve::point_error_bound`]: its control points, not the size of
/// the point it returns) times `√3`, each step rounded up by 4 ulps. An end with no such
/// bound, an end whose evaluation fails, and a non-finite reach add nothing, so the bound
/// is certified and covers the ends whose error is bounded; it says nothing of the
/// others. `0` for a vertex with no such end.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub(crate) fn edge_end_reach(k: &Kernel, b: &Brep, v: VertexId) -> Result<f64> {
    let up = |x: f64| x * (1.0 + 4.0 * f64::EPSILON);
    let at = b.vertex(v)?.point;
    let mut reach = 0.0_f64;
    for x in edges(b, v, None)? {
        let rec = b.edge(x)?;
        let Some(curve) = rec.curve.as_ref() else {
            continue;
        };
        let (s0, s1) = crate::topology::edge::start_end_vertices(b, x)?;
        for (end, t) in [
            (s0, rec.interval.evaluate(0.0)),
            (s1, rec.interval.evaluate(1.0)),
        ] {
            if end != Some(v) {
                continue;
            }
            let Ok(p) = curve.evaluate_point(k, t) else {
                continue;
            };
            let Some(error) = curve.point_error_bound(t) else {
                continue;
            };
            let far = up(up(at.distance(p)) + up(error * up(3.0_f64.sqrt())));
            if far.is_finite() {
                reach = reach.max(far);
            }
        }
    }
    Ok(reach)
}

/// Not upstream (#21340): after a point merge has sized `v` to the merged neighbourhood, a
/// ball of radius `v`'s tolerance, grows `v` to twice [`edge_end_reach`] when that reach is
/// within the ball, so every end it bounds lies within half the tolerance (`AssertValid`'s
/// test of an edge end; doubling is exact). Grow-only, and at most doubles the tolerance.
/// A reach beyond the ball (an end [`carry_far_edge_ends`] could not move to the vertex)
/// and an end with no bound on its evaluation's error change nothing, so no uncertified
/// figure becomes a tolerance.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id.
pub(crate) fn cover_merged_ends(k: &Kernel, b: &mut Brep, v: VertexId) -> Result<()> {
    let tol = tolerance(b, v)?;
    let reach = edge_end_reach(k, b, v)?;
    let cover = 2.0 * reach;
    if reach <= tol && cover.is_finite() {
        set_tolerance(b, v, cover, true, true)?;
    }
    Ok(())
}

/// A certified upper bound on the distance from `at` to `curve`'s point at `t`: the
/// evaluated point's distance plus `√3` times the curve's bound on its evaluation's
/// error, each step rounded up by 4 ulps ([`edge_end_reach`]'s bound of one end). `None`
/// when the evaluation fails, the curve kind has no error bound or the bound is not finite.
fn certified_end_distance(k: &Kernel, curve: &Curve, t: f64, at: Point3) -> Option<f64> {
    let up = |x: f64| x * (1.0 + 4.0 * f64::EPSILON);
    let p = curve.evaluate_point(k, t).ok()?;
    let error = curve.point_error_bound(t)?;
    let far = up(up(at.distance(p)) + up(error * up(3.0_f64.sqrt())));
    far.is_finite().then_some(far)
}

/// The B-spline form of `curve` whose points equal the curve's at every parameter, or
/// `None` for a kind that has none: the B-spline itself, or the B-spline part an
/// ellipse, hyperbola or parabola is evaluated from.
fn spline_form(curve: &Curve) -> Option<crate::nurbs::b_spline_curve::BSplineCurve> {
    match curve.kind() {
        CurveKind::BSpline(c) => Some(c.clone()),
        CurveKind::Ellipse(c) => Some(c.base().clone()),
        CurveKind::Hyperbola(c) => Some(c.base().clone()),
        CurveKind::Parabola(c) => Some(c.base().clone()),
        _ => None,
    }
}

/// Not upstream (#21511): `curve` with its point at `interval`'s start (`at_start`) or
/// end (otherwise) moved to `target` and its point at the other end of `interval` kept.
///
/// A line becomes the line from `target` to the kept end, parameterised over `interval`
/// (it stays a line; its points off `interval` are not the edge's). Any other kind with a
/// B-spline form ([`spline_form`]) has both ends of `interval` made knots the curve passes
/// through a control point at ([`BSplineCurve::make_point_knot`], exact knot insertion),
/// so moving the moved end's control point ([`BSplineCurve::move_point_at_knot`]) puts the
/// curve's end at `target`, changes its points only in the span next to that end and
/// leaves the other end's control point, the only one the curve has at that end, where
/// it was. The result is a B-spline: a moved ellipse, hyperbola or parabola is not one
/// any more. `None` for a kind with no such form (offset, curves on surfaces,
/// composite, ...), a line whose ends would meet, a degenerate interval or one off the
/// natural interval: nothing is moved then.
///
/// [`BSplineCurve::make_point_knot`]: crate::nurbs::b_spline_curve::BSplineCurve::make_point_knot
/// [`BSplineCurve::move_point_at_knot`]: crate::nurbs::b_spline_curve::BSplineCurve::move_point_at_knot
fn curve_with_moved_end(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    at_start: bool,
    target: Point3,
) -> Result<Option<Curve>> {
    let (t0, t1) = (interval.min(), interval.max());
    if !(t0 < t1) {
        return Ok(None);
    }
    if let CurveKind::Line(_) = curve.kind() {
        let kept = curve.evaluate_point(k, if at_start { t1 } else { t0 })?;
        let (p0, p1) = if at_start {
            (target, kept)
        } else {
            (kept, target)
        };
        if !(p0.distance(p1) >= crate::math::EFF_ZERO) {
            return Ok(None);
        }
        let mut line = crate::curve::line::Line::from_points(p0, p1, CurveEvaluate::dim(curve));
        line.edit_parameterization(interval)?;
        return Ok(Some(Curve::from(line)));
    }
    let Some(mut spline) = spline_form(curve) else {
        return Ok(None);
    };
    if !spline.make_point_knot(t0)? || !spline.make_point_knot(t1)? {
        return Ok(None);
    }
    let (t, from_right) = if at_start { (t0, true) } else { (t1, false) };
    if !spline.move_point_at_knot(t, from_right, target)? {
        return Ok(None);
    }
    Ok(Some(Curve::from(spline)))
}

/// Not upstream (#21511): `interval` with each end flagged in `far` (start, end) moved to
/// the parameter where `curve` passes nearest `target` (a local projection seeded at that
/// end), the curve itself kept. This is the exact edit when the surf/surf/surf point lies
/// on the edge's curve, as it does on an intersection edge whose curve is exact (a line or
/// conic of two analytic surfaces): the edge then ends at the point on its own, unchanged
/// curve, which stays a line, an ellipse, a hyperbola or a parabola. `None` when both ends
/// are flagged (a closed edge: one projection cannot tell its two ends apart), a projection
/// fails or leaves the curve's natural interval, or the new interval is not increasing.
fn retrimmed_interval(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    far: [bool; 2],
    target: Point3,
) -> Option<Extent1d> {
    use crate::curve::curve::p3::PointSolveOptions;
    use crate::geom::global_solver::SolverOperation;
    if far[0] && far[1] {
        return None;
    }
    let natural = curve.natural_interval();
    let (mut t0, mut t1) = (interval.min(), interval.max());
    let at_start = far[0];
    let seed = if at_start { t0 } else { t1 };
    let (found, foot) = curve
        .local_point_solve::<usize>(
            k,
            &natural,
            SolverOperation::Minimize,
            target,
            &PointSolveOptions::default(),
            seed,
        )
        .ok()?;
    let t = foot.start.param(0).ok()?;
    if !found || !natural.contains_value(t, 0.0) {
        return None;
    }
    if at_start {
        t0 = t;
    } else {
        t1 = t;
    }
    (t0 < t1).then(|| Extent1d::from_min_max(t0, t1))
}

/// Whether `new_curve` over `new_interval` certainly does the job [`carry_far_edge_ends`]
/// edits an edge for: each end flagged in `moved` within half its vertex's tolerance, and
/// every other end that `curve` over `interval` had within half its vertex's tolerance
/// still within it, by [`certified_end_distance`].
fn edge_end_edit_certified(
    k: &Kernel,
    b: &Brep,
    ends: [Option<VertexId>; 2],
    (curve, interval): (&Curve, &Extent1d),
    (new_curve, new_interval): (&Curve, &Extent1d),
    moved: [bool; 2],
) -> Result<bool> {
    let params = |iv: &Extent1d| [iv.evaluate(0.0), iv.evaluate(1.0)];
    let (old_t, new_t) = (params(interval), params(new_interval));
    for (i, end) in ends.iter().enumerate() {
        let Some(w) = *end else {
            continue;
        };
        let w_rec = b.vertex(w)?;
        let w_half = 0.5 * w_rec.tolerance;
        let (Some(&t_old), Some(&t_new)) = (old_t.get(i), new_t.get(i)) else {
            return Ok(false);
        };
        let after = certified_end_distance(k, new_curve, t_new, w_rec.point);
        let ok = if moved.get(i).copied().unwrap_or(false) {
            after.is_some_and(|d| d <= w_half)
        } else {
            match curve
                .evaluate_point(k, t_old)
                .map(|p| w_rec.point.distance(p))
            {
                Ok(d) if d <= w_half => after.is_some_and(|d| d <= w_half),
                _ => true,
            }
        };
        if !ok {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Not upstream (#21511): after a point merge moved `v` to its surf/surf/surf point
/// (`SmPointClassification::MergeIntoObject`'s `SetPoint(m_vSSSPoint)`), moves the ends of
/// `v`'s edges that the move left outside `v`'s ball (farther than its tolerance; the ends
/// inside are [`cover_merged_ends`]'s) to `v`'s point, so the edges keep ending at the
/// vertex that bounds them. Upstream moves only the vertex and leaves those ends behind,
/// past `AssertValid`'s edge-end test (half the vertex tolerance), and no tolerance can
/// cover them without the ball swallowing geometry the vertex does not bound.
///
/// Two edits are tried, in order, and the first certified one is kept:
/// 1. [`retrimmed_interval`]: the curve unchanged and its far end's parameter moved to
///    where the curve passes `v`'s point. Exact, and the curve keeps its class, when the
///    point lies on the curve (an intersection edge of exact surfaces).
/// 2. [`curve_with_moved_end`] at every far end (a closed edge moves both): the curve
///    itself edited to end at `v`'s point. A line stays a line; a B-spline, ellipse,
///    hyperbola or parabola becomes a B-spline, since a conic cannot end at a point off
///    it and stay that conic.
///
/// The kept edit is set with [`crate::topology::edge::set_curve`] (`SetCurve`: the change
/// is notified, which deletes the edge uses' UV trim curves as upstream deletes them when
/// the surf/surf/surf point edits an edge curve, `sm_SetXSectEdgeThroughPoint`, and the
/// spatial cache is updated), then the edge interval with
/// [`crate::topology::edge::set_interval`] when it changed. An edit is certified by
/// [`edge_end_edit_certified`]: each moved end within half `v`'s tolerance and every
/// other end of the edge that was within half its vertex's tolerance still within it.
/// When neither edit is certified, and for a curve neither edit applies to, the edge is
/// left as upstream leaves it.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of
/// [`crate::topology::edge::set_curve`] and [`crate::topology::edge::set_interval`]. A
/// failed edit of a curve leaves its edge as it is.
pub(crate) fn carry_far_edge_ends(
    k: &Kernel,
    b: &mut Brep,
    v: VertexId,
    env: &mut crate::topology::edge::NotifyEnv<'_>,
) -> Result<()> {
    use crate::topology::edge;
    let at = b.vertex(v)?.point;
    let tol = tolerance(b, v)?;
    for x in edges(b, v, None)? {
        let (s0, s1) = edge::start_end_vertices(b, x)?;
        let rec = b.edge(x)?;
        let Some(curve) = rec.curve.clone() else {
            continue;
        };
        let interval = rec.interval;
        let ends = [s0, s1];
        let params = [interval.evaluate(0.0), interval.evaluate(1.0)];
        // the ends at `v` certainly outside its ball (NaN is not outside)
        let mut far = [false; 2];
        for (i, (&end, &t)) in ends.iter().zip(params.iter()).enumerate() {
            if end != Some(v) {
                continue;
            }
            let Ok(p) = curve.evaluate_point(k, t) else {
                continue;
            };
            if at.distance(p) > tol {
                if let Some(slot) = far.get_mut(i) {
                    *slot = true;
                }
            }
        }
        if far == [false; 2] {
            continue;
        }
        let old = (&curve, &interval);
        // 1. the curve kept, its far end re-trimmed to the point on it
        let mut kept: Option<(Curve, Extent1d)> = None;
        if let Some(iv) = retrimmed_interval(k, &curve, &interval, far, at) {
            if edge_end_edit_certified(k, b, ends, old, (&curve, &iv), far)? {
                kept = Some((curve.clone(), iv));
            }
        }
        // 2. the curve edited to end at the point
        if kept.is_none() {
            let mut moved: Option<Curve> = None;
            let mut moved_ends = [false; 2];
            for (i, &is_far) in far.iter().enumerate() {
                if !is_far {
                    continue;
                }
                let at_start = i == 0;
                let current = moved.as_ref().unwrap_or(&curve);
                // a curve the edit cannot be made on is left as it is: never a refusal
                if let Ok(Some(c)) = curve_with_moved_end(k, current, &interval, at_start, at) {
                    moved = Some(c);
                    if let Some(slot) = moved_ends.get_mut(i) {
                        *slot = true;
                    }
                }
            }
            if let Some(c) = moved {
                if edge_end_edit_certified(k, b, ends, old, (&c, &interval), moved_ends)? {
                    kept = Some((c, interval));
                }
            }
        }
        let Some((new_curve, new_interval)) = kept else {
            continue;
        };
        let new_curve = {
            let mut c = new_curve;
            c.set_owner_zone_tol_3d(curve.owner_zone_tol_3d());
            let owner = (env.key_of)(EntityRef::Edge(x));
            let _ = c.set_owner(Some(owner), Some(crate::topology::brep::p2::brep_ref(b)));
            c
        };
        let options = edge::SetCurveOptions {
            delete_old_curve: true,
            ..edge::SetCurveOptions::default()
        };
        let _ = edge::set_curve(b, x, Some(new_curve), options, env, [None, None])?;
        if new_interval != interval {
            edge::set_interval(b, x, &new_interval)?;
        }
    }
    Ok(())
}

/// `Notify`: passes `PostEdit`, `SplitInBrep`, `MergeInBrep` and `ChangeGeometry` to
/// every vertex use ([`vertexuse::notify`], each under `vertexuse_ref(vu)`) so they
/// clear their cached gaps, then the notification to [`topo::notify`]
/// (`SmTopology::Notify`) on the caller's topology `records` (`this` is the vertex's
/// identity).
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; those of [`vertexuse::notify`] and
/// [`topo::notify`].
pub fn notify(
    b: &mut Brep,
    v: VertexId,
    records: &mut crate::topology::topology::NotifyRecords,
    attributes: &mut AttributeStore,
    mut context: Option<&mut Context>,
    operation: NotifyOperation,
    this: ObjectRef,
    data: [Option<ObjectRef>; 3],
    vertexuse_ref: &dyn Fn(VertexuseId) -> ObjectRef,
) -> Result<()> {
    use NotifyOperation as N;
    if matches!(
        operation,
        N::PostEdit | N::SplitInBrep | N::MergeInBrep | N::ChangeGeometry
    ) {
        // pass the notify call along to all Vertexuses so that those can clean up their
        // cached gaps
        for vu in vertexuses(b, v)? {
            vertexuse::notify(
                b,
                vu,
                records,
                attributes,
                context.as_deref_mut(),
                operation,
                vertexuse_ref(vu),
                data,
            )?;
        }
    }
    // SM_NO_UNKNOWN: SE_MSG only (compiled out of the release build)

    // Propagate notification up hierarchy
    let [d1, d2, d3] = data;
    topo::notify(records, attributes, context, operation, this, d1, d2, d3)
}

/// `SetPoint`: notifies `ChangeGeometry` ([`notify`], data `NULL`,
/// `SM_NO_GET_BREP(this)` = `NULL`, `NULL`), then moves the vertex to `new_point`.
///
/// # Errors
///
/// Those of [`notify`].
pub fn set_point(
    b: &mut Brep,
    v: VertexId,
    new_point: Point3,
    records: &mut crate::topology::topology::NotifyRecords,
    attributes: &mut AttributeStore,
    context: Option<&mut Context>,
    this: ObjectRef,
    vertexuse_ref: &dyn Fn(VertexuseId) -> ObjectRef,
) -> Result<()> {
    notify(
        b,
        v,
        records,
        attributes,
        context,
        NotifyOperation::ChangeGeometry,
        this,
        [None; 3],
        vertexuse_ref,
    )?;
    b.vertex_mut(v)?.point = new_point;
    Ok(())
}

// ---- pointer check (SmVertex::CheckPointers) ------------------------------------------------

/// `SmVertex::CheckPointers()`: whether the topology around `v` is linked consistently,
/// through each of its vertex uses in list order (stopping at the first failure): a
/// shell vertex use is the vertex's only use and its shell's sole list member; a loop
/// vertex use and the one of its mate loop use name this vertex, one loop, one face
/// and mated loop uses and face uses that point back; an edge vertex use of a wire
/// edge has mated shell edge uses of one shell; one of a loop edge has, with its mate
/// and the corner neighbour at this vertex and its mate, one loop, one face, mated
/// edge, loop and face uses that point back and consistent use types. Any other vertex
/// use or edge use type fails.
///
/// # Errors
///
/// [`SolidError::NotFound`] for a dangling id; [`SolidError::InvalidInput`] where the
/// accessors upstream calls dereference null (an edge use without edge or primary edge
/// use in `GetMate`, an edge use without vertex use or mate in
/// `GetCornerMateEdgeuse`).
pub fn check_pointers(b: &Brep, v: VertexId) -> Result<bool> {
    use crate::topology::shell::ShellMembers;
    use crate::topology::EdgeuseOwner;

    // locals
    let mut rtn = true;
    let vertexuses = vertexuses(b, v)?;
    let lu_mate = |lu: LoopuseId| -> Result<Option<LoopuseId>> { Ok(b.loopuse(lu)?.mate) };
    let lu_vertexuse = |lu: LoopuseId| -> Result<Option<VertexuseId>> {
        Ok(match b.loopuse(lu)?.start {
            LoopuseStart::Vertexuse(vu) => vu,
            _ => None,
        })
    };
    let lu_loop = |lu: LoopuseId| -> Result<Option<LoopId>> { Ok(b.loopuse(lu)?.loop_) };
    let lu_faceuse = |lu: LoopuseId| -> Result<Option<FaceuseId>> { Ok(b.loopuse(lu)?.faceuse) };
    let lu_is = |lu: LoopuseId, edge_type: bool| -> Result<bool> {
        Ok(match b.loopuse(lu)?.start {
            LoopuseStart::Edgeuse(_) => edge_type,
            LoopuseStart::Vertexuse(_) => !edge_type,
            LoopuseStart::Unknown => false,
        })
    };
    let fu_face = |fu: FaceuseId| -> Result<Option<FaceId>> { Ok(b.faceuse(fu)?.face) };
    let fu_mate = |fu: FaceuseId| -> Result<Option<FaceuseId>> { Ok(b.faceuse(fu)?.mate) };
    let fu_has = |fu: FaceuseId, lu: LoopuseId| -> Result<bool> {
        Ok(b.faceuse(fu)?.loopuses.contains(&lu))
    };
    let face_fu = |f: FaceId| -> Result<Option<FaceuseId>> { Ok(b.face(f)?.faceuse) };
    let loop_lu = |l: LoopId| -> Result<Option<LoopuseId>> { Ok(b.loop_(l)?.loopuse) };
    let vu_vertex = |vu: VertexuseId| -> Result<Option<VertexId>> { Ok(b.vertexuse(vu)?.vertex) };
    let vu_is_edge = |vu: VertexuseId| -> Result<bool> {
        Ok(matches!(b.vertexuse(vu)?.owner, VertexuseOwner::Edgeuse(_)))
    };
    let eu_is_loop = |eu: EdgeuseId| -> Result<bool> {
        Ok(matches!(b.edgeuse(eu)?.owner, EdgeuseOwner::Loopuse(_)))
    };
    let eu_is_shell = |eu: EdgeuseId| -> Result<bool> {
        Ok(matches!(b.edgeuse(eu)?.owner, EdgeuseOwner::Shell(_)))
    };
    let some_eq = |a: Option<VertexId>| a == Some(v);

    // For every Face - find all the Vertexuses
    let mut ii = 0;
    while rtn && ii < vertexuses.len() {
        let Some(&vui) = vertexuses.get(ii) else {
            break;
        };
        ii += 1;

        // switch on Vertexuse Type
        match b.vertexuse(vui)?.owner {
            VertexuseOwner::Shell(shell) => {
                // ShellVertex checks
                rtn &= shell.is_some();
                if let (true, Some(shell)) = (rtn, shell) {
                    let members = &b.shell(shell)?.members;
                    /* 1 */
                    rtn &= vertexuses.len() == 1;
                    /* 2 */
                    rtn &= *members == ShellMembers::Vertexuse(vui);
                    /* 3 (GetSize() == 1: the list of a vertex shell holds its one use) */
                    rtn &= matches!(members, ShellMembers::Vertexuse(_));
                }
            }
            VertexuseOwner::Loopuse(lu1) => {
                let lu2 = match lu1 {
                    Some(l) => lu_mate(l)?,
                    None => None,
                };
                let vuj = match lu2 {
                    Some(l) => lu_vertexuse(l)?,
                    None => None,
                };
                let v1 = b.vertexuse(vui)?.vertex;
                let v2 = match vuj {
                    Some(u) => vu_vertex(u)?,
                    None => None,
                };
                let loop1 = match lu1 {
                    Some(l) => lu_loop(l)?,
                    None => None,
                };
                let loop2 = match lu2 {
                    Some(l) => lu_loop(l)?,
                    None => None,
                };
                let fu1 = match lu1 {
                    Some(l) => lu_faceuse(l)?,
                    None => None,
                };
                let fu2 = match lu2 {
                    Some(l) => lu_faceuse(l)?,
                    None => None,
                };
                let face1 = match fu1 {
                    Some(f) => fu_face(f)?,
                    None => None,
                };
                let face2 = match fu2 {
                    Some(f) => fu_face(f)?,
                    None => None,
                };

                // LoopVertex checks
                let all = (lu1, lu2, vuj, v1, v2, loop1, loop2, fu1, fu2, face1, face2);
                rtn &= matches!(
                    all,
                    (
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_),
                        Some(_)
                    )
                );
                if let (
                    true,
                    (
                        Some(lu1),
                        Some(lu2),
                        Some(vuj),
                        Some(_),
                        Some(_),
                        Some(loop1),
                        Some(_),
                        Some(fu1),
                        Some(fu2),
                        Some(face1),
                        Some(_),
                    ),
                ) = (rtn, all)
                {
                    /*  1 */
                    rtn &= some_eq(v1) && some_eq(v2);
                    /*  2 */
                    rtn &= all.5 == all.6;
                    /*  3 */
                    rtn &= all.9 == all.10;
                    /*  4 */
                    rtn &= lu_mate(lu1)? == Some(lu2) && lu_mate(lu2)? == Some(lu1);
                    /*  5 */
                    rtn &= fu_mate(fu1)? == Some(fu2) && fu_mate(fu2)? == Some(fu1);
                    /*  6 */
                    let up = face_fu(face1)?;
                    rtn &= up == Some(fu1) || up == Some(fu2);
                    /*  7 */
                    let l = loop_lu(loop1)?;
                    rtn &= l == Some(lu1) || l == Some(lu2);
                    /*  8 */
                    rtn &= fu_has(fu1, lu1)?;
                    /*  9 */
                    rtn &= fu_has(fu2, lu2)?;
                    /* 10 */
                    rtn &= lu_vertexuse(lu1)? == Some(vui);
                    /* 11 */
                    rtn &= lu_vertexuse(lu2)? == Some(vuj);
                    /* 12 */
                    rtn &= some_eq(vu_vertex(vui)?) && some_eq(vu_vertex(vuj)?);
                    /* 13 */
                    rtn &= lu_is(lu1, false)? && lu_is(lu2, false)?;
                }
            }
            VertexuseOwner::Edgeuse(eu1_) => {
                let eu2_ = match eu1_ {
                    Some(e) => eu1::mate(b, e)?,
                    None => None,
                };
                let eu1_owner = match eu1_ {
                    Some(e) => Some(b.edgeuse(e)?.owner),
                    None => None,
                };

                // switch on Edgeuse TYpe
                match eu1_owner {
                    Some(EdgeuseOwner::Shell(_)) => {
                        // EdgeVertex Part of a Shell
                        let shell = match eu1_ {
                            Some(e) => eu1::shell(b, e)?,
                            None => None,
                        };
                        rtn &= eu1_.is_some() && eu2_.is_some() && shell.is_some();
                        if let (true, Some(e1), Some(e2), Some(_)) = (rtn, eu1_, eu2_, shell) {
                            /*  1 */
                            rtn &= shell == eu1::shell(b, e1)? && shell == eu1::shell(b, e2)?;
                            /*  2 */
                            rtn &= eu1::mate(b, e1)? == Some(e2) && eu1::mate(b, e2)? == Some(e1);
                            /*  3 */
                            rtn &= eu1::vertexuse(b, e1)? == Some(vui);
                            /*  4 */
                            rtn &= eu_is_shell(e1)? && eu_is_shell(e2)?;
                        }
                    }
                    Some(EdgeuseOwner::Loopuse(_)) => {
                        // EdgeVertex Part of an Loopuse
                        let eu3_ = match eu1_ {
                            Some(e) => eu1::corner_mate_edgeuse(b, e, Some(v))?,
                            None => None,
                        };
                        let eu4_ = match eu3_ {
                            Some(e) => eu1::mate(b, e)?,
                            None => None,
                        };
                        let eus = [eu1_, eu2_, eu3_, eu4_];
                        let mut lus = [None; 4];
                        let mut loops = [None; 4];
                        let mut fus = [None; 4];
                        let mut faces = [None; 4];
                        let mut vus = [Some(vui), None, None, None];
                        let mut vs = [None; 4];
                        for (i, eu) in eus.iter().enumerate() {
                            if let Some(e) = *eu {
                                if let Some(slot) = lus.get_mut(i) {
                                    *slot = eu1::loopuse(b, e)?;
                                }
                                if let Some(slot) = fus.get_mut(i) {
                                    *slot = eu1::faceuse(b, e)?;
                                }
                                if i > 0 {
                                    if let Some(slot) = vus.get_mut(i) {
                                        *slot = eu1::vertexuse(b, e)?;
                                    }
                                }
                            }
                        }
                        for i in 0..4 {
                            if let (Some(slot), Some(Some(l))) = (loops.get_mut(i), lus.get(i)) {
                                *slot = lu_loop(*l)?;
                            }
                            if let (Some(slot), Some(Some(f))) = (faces.get_mut(i), fus.get(i)) {
                                *slot = fu_face(*f)?;
                            }
                            if let (Some(slot), Some(Some(u))) = (vs.get_mut(i), vus.get(i)) {
                                *slot = vu_vertex(*u)?;
                            }
                        }

                        // Vertex->LoopEdge checks
                        let present = eus.iter().all(Option::is_some)
                            && lus.iter().all(Option::is_some)
                            && loops.iter().all(Option::is_some)
                            && fus.iter().all(Option::is_some)
                            && faces.iter().all(Option::is_some)
                            && vus.iter().all(Option::is_some)
                            && vs.iter().all(Option::is_some);
                        rtn &= present;
                        if let (
                            true,
                            [Some(e1), Some(e2), Some(e3), Some(e4)],
                            [Some(l1), Some(l2), Some(l3), Some(l4)],
                            [Some(f1), Some(f2), _, _],
                            [Some(loop1), _, _, _],
                            [Some(face1), _, _, _],
                            [Some(u1), Some(u2), Some(u3), Some(u4)],
                        ) = (rtn, eus, lus, fus, loops, faces, vus)
                        {
                            let [vi, _, _, vl] = vs;
                            /*  1 */
                            rtn &= some_eq(vi) && some_eq(vl);
                            /*  2 */
                            rtn &= loops.first() == loops.get(1)
                                && loops.first() == loops.get(2)
                                && loops.first() == loops.get(3);
                            /*  3 */
                            rtn &= faces.first() == faces.get(1)
                                && faces.first() == faces.get(2)
                                && faces.first() == faces.get(3);
                            /*  4 */
                            rtn &= eu1::mate(b, e1)? == Some(e2)
                                && eu1::mate(b, e2)? == Some(e1)
                                && eu1::mate(b, e3)? == Some(e4)
                                && eu1::mate(b, e4)? == Some(e3);
                            /*  5 */
                            rtn &= lu_mate(l1)? == Some(l2) && lu_mate(l2)? == Some(l1);
                            /*  6 */
                            rtn &= fu_mate(f1)? == Some(f2) && fu_mate(f2)? == Some(f1);
                            /*  7 */
                            let up = face_fu(face1)?;
                            rtn &= up == Some(f1) || up == Some(f2);
                            /*  8 */
                            let l = loop_lu(loop1)?;
                            rtn &= l == Some(l1) || l == Some(l2);
                            /*  9 */
                            rtn &= fu_has(f1, l1)?;
                            /* 10 */
                            rtn &= fu_has(f2, l2)?;
                            /* 11 */
                            rtn &= eu1::vertexuse(b, e1)? == Some(u1)
                                && eu1::vertexuse(b, e2)? == Some(u2)
                                && eu1::vertexuse(b, e3)? == Some(u3)
                                && eu1::vertexuse(b, e4)? == Some(u4);
                            /* 12 */
                            rtn &= vu_is_edge(u1)?
                                && vu_is_edge(u2)?
                                && vu_is_edge(u3)?
                                && vu_is_edge(u4)?;
                            /* 13 */
                            rtn &= eu_is_loop(e1)?
                                && eu_is_loop(e2)?
                                && eu_is_loop(e3)?
                                && eu_is_loop(e4)?;
                            /* 14 */
                            rtn &= lu_is(l1, true)?
                                && lu_is(l2, true)?
                                && lu_is(l3, true)?
                                && lu_is(l4, true)?;
                        }
                    }
                    _ => rtn = false,
                }
            }
            VertexuseOwner::Unknown => rtn = false,
        }
    }

    // all done
    Ok(rtn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::geom::extent1d::Extent1d;
    use crate::geom::tol::ZONE_TOL_3D;
    use crate::topology::{Edge, Edgeuse, EdgeuseOwner, Region, Shell, Vertex, Vertexuse};

    /// A chain of wire edges v0 - v1 - v2 along `dirs`, each from a vertex to the next,
    /// with optional line curves; plus a shell vertex v3.
    fn chain(curves: bool, d1: Vec3, d2: Vec3) -> (Brep, [VertexId; 4], [EdgeId; 2]) {
        let mut b = Brep::new();
        let r = b
            .add_region_record(Region {
                in_brep: true,
                ..Region::default()
            })
            .unwrap();
        let s = b
            .add_shell_record(Shell {
                faces: Vec::new(),
                region: Some(r),
                ..Shell::default()
            })
            .unwrap();
        let p = [Vec3::ZERO, d1, d1 + d2, Vec3::new(5.0, 5.0, 5.0)];
        let v: Vec<VertexId> = p
            .iter()
            .map(|&point| {
                b.add_vertex_record(Vertex {
                    point,
                    tolerance: ZONE_TOL_3D,
                    ..Vertex::default()
                })
                .unwrap()
            })
            .collect();
        // edge i: edge uses 2i (same, at v_i) and 2i+1 (opposite, at v_{i+1})
        let mut es = Vec::new();
        for i in 0..2u32 {
            let curve = curves.then(|| {
                let a = p[i as usize];
                let line = Line::from_points(a, p[i as usize + 1], 3);
                Curve::from(line)
            });
            let e = b
                .add_edge_record(Edge {
                    vertices: [v[i as usize], v[i as usize + 1]],
                    edgeuses: vec![
                        EdgeuseId::from_index(2 * i),
                        EdgeuseId::from_index(2 * i + 1),
                    ],
                    interval: Extent1d::from_min_max(0.0, 1.0),
                    tolerance: ZONE_TOL_3D,
                    curve,
                })
                .unwrap();
            es.push(e);
        }
        for i in 0..4u32 {
            let e = EdgeId::from_index(i / 2);
            b.add_edgeuse_record(Edgeuse {
                edge: Some(e),
                owner: EdgeuseOwner::Shell(Some(s)),
                vertexuse: Some(VertexuseId::from_index(i)),
                orientation: if i % 2 == 0 {
                    OrientType::Same
                } else {
                    OrientType::Opposite
                },
                ..Edgeuse::default()
            })
            .unwrap();
        }
        // vertex uses 0..4 of the edge uses, 4 the shell vertex use of v3
        let vu_vertex = [v[0], v[1], v[1], v[2]];
        for (i, &vx) in vu_vertex.iter().enumerate() {
            let vu = b
                .add_vertexuse_record(Vertexuse {
                    vertex: Some(vx),
                    owner: VertexuseOwner::Edgeuse(Some(EdgeuseId::from_index(i as u32))),
                    ..Vertexuse::default()
                })
                .unwrap();
            b.vertex_mut(vx).unwrap().vertexuses.push(vu);
        }
        let svu = b
            .add_vertexuse_record(Vertexuse {
                vertex: Some(v[3]),
                owner: VertexuseOwner::Shell(Some(s)),
                ..Vertexuse::default()
            })
            .unwrap();
        b.vertex_mut(v[3]).unwrap().vertexuses.push(svu);
        (b, [v[0], v[1], v[2], v[3]], [es[0], es[1]])
    }

    #[test]
    fn types_and_simple_access() {
        assert_eq!(TYPE, 16021);
        assert!(is_kind_of(TYPE));
        assert!(is_kind_of(owning_topology::TYPE));
        assert!(is_kind_of(topo::TYPE));
        assert!(!is_kind_of(vertexuse::TYPE));
        let d = Vertex::default();
        assert_eq!((d.point, d.tolerance), (Vec3::ZERO, 0.0));

        let (mut b, v, _) = chain(false, Vec3::X, Vec3::X);
        assert_eq!(point(&b, v[1]).unwrap(), Vec3::X);
        set_tolerance(&mut b, v[1], 1e-6, true, true).unwrap();
        assert_eq!(tolerance(&b, v[1]).unwrap(), ZONE_TOL_3D);
        set_tolerance(&mut b, v[1], 1e-6, false, true).unwrap();
        assert_eq!(tolerance(&b, v[1]).unwrap(), 1e-6);
        set_tolerance(&mut b, v[1], 1e-3, true, false).unwrap();
        assert_eq!(tolerance(&b, v[1]).unwrap(), 1e-3);
        set_exact(&mut b, v[1], false).unwrap();
        assert!(is_exact(&b, v[1]).unwrap());
        set_poly_vertex(&mut b, v[1], Some(PolyVertexId::from_index(3))).unwrap();
        assert_eq!(
            poly_vertex(&b, v[1]).unwrap(),
            Some(PolyVertexId::from_index(3))
        );
        assert_eq!(
            primary_vertexuse(&b, v[1]).unwrap(),
            Some(VertexuseId::from_index(1))
        );
        assert!(brep(&b, v[1]).unwrap().is_some());
        refresh_tolerance(&mut b, v[1], false).unwrap();
        // no faces: only the edge SetTolerance (topology.edge) runs
        refresh_tolerance(&mut b, v[1], true).unwrap();
    }

    #[test]
    fn adjacency() {
        let (b, v, e) = chain(false, Vec3::X, Vec3::Y);
        assert_eq!(edges(&b, v[1], None).unwrap(), vec![e[0], e[1]]);
        assert_eq!(edges(&b, v[0], None).unwrap(), vec![e[0]]);
        assert_eq!(
            common_edges(&b, v[0], Some(v[1]), None).unwrap(),
            vec![e[0]]
        );
        assert!(common_edges(&b, v[0], Some(v[2]), None).unwrap().is_empty());
        assert!(faces(&b, v[1]).unwrap().is_empty());
        assert_eq!(faceuses(&b, v[1]).unwrap(), vec![None, None]);
        assert!(faceuses(&b, v[3]).unwrap().is_empty());
        assert_eq!(shells(&b, v[1]).unwrap(), vec![ShellId::from_index(0)]);
        assert_eq!(regions(&b, v[1]).unwrap(), vec![RegionId::from_index(0)]);
        assert_eq!(
            shell_in_region(&b, v[1], Some(RegionId::from_index(0))).unwrap(),
            Some(ShellId::from_index(0))
        );
        assert_eq!(shell_in_region(&b, v[1], None).unwrap(), None);
        assert_eq!(edge_start_vertex(&b, e[1]).unwrap(), Some(v[1]));
        assert_eq!(
            edge_vertex_pair(&b, e[1]).unwrap(),
            (Some(v[1]), Some(v[2]))
        );
        assert_eq!(
            edge_vertices(&b, e[0]).unwrap(),
            vec![Some(v[0]), Some(v[1])]
        );
        assert_eq!(
            vertexuses_of_edge(&b, v[1], Some(e[1]), None).unwrap(),
            vec![VertexuseId::from_index(2)]
        );
        assert!(vertexuses_of_edge(&b, v[1], None, None).unwrap().is_empty());
        assert!(
            vertexuses_of_face(&b, v[1], Some(FaceId::from_index(0)), None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            vertexuse_of_face(&b, v[1], Some(FaceId::from_index(0))).unwrap(),
            None
        );
        assert!(is_wire_vertex(&b, v[1]).unwrap());
        assert!(!is_lamina_vertex(&b, v[1]).unwrap());
        assert!(is_shell_vertex(&b, v[3]).unwrap());
        assert!(!is_shell_vertex(&b, v[1]).unwrap());
        assert!(!is_loop_vertex(&b, v[1]).unwrap());

        let c = |t| is_connected_to(&b, v[1], t).unwrap();
        assert!(c(None));
        assert!(c(Some(EntityRef::Vertex(v[1]))));
        assert!(!c(Some(EntityRef::Vertex(v[0]))));
        assert!(c(Some(EntityRef::Edge(e[0]))));
        assert!(c(Some(EntityRef::Edgeuse(EdgeuseId::from_index(1)))));
        assert!(!c(Some(EntityRef::Edgeuse(EdgeuseId::from_index(0)))));
        assert!(c(Some(EntityRef::Vertexuse(VertexuseId::from_index(2)))));
        assert!(c(Some(EntityRef::Shell(ShellId::from_index(0)))));
        assert!(c(Some(EntityRef::Region(RegionId::from_index(0)))));
        assert!(!c(Some(EntityRef::Faceuse(FaceuseId::from_index(0)))));
    }

    #[test]
    fn tangency_and_topological_vertices() {
        let k = Kernel::default();
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::X);
        assert!(are_edges_tangent(&k, &b, v[1], e[0], e[1], 0.5).unwrap());
        assert!(is_topological_vertex(&k, &mut b, v[1]).unwrap());
        assert!(!is_topological_vertex(&k, &mut b, v[0]).unwrap());

        // a cusp (the second line doubles back) is not topological (WR-92)
        let (mut b, v, _) = chain(true, Vec3::X, -Vec3::X);
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());

        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        assert!(!are_edges_tangent(&k, &b, v[1], e[0], e[1], 0.5).unwrap());
        assert!(are_edges_tangent(&k, &b, v[1], e[0], e[1], 91.0).unwrap());
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());

        // no curves: the same (null) curve; the parameters differ (1 and 0), then match
        let (mut b, v, e) = chain(false, Vec3::X, Vec3::Y);
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());
        b.edge_mut(e[1]).unwrap().interval = Extent1d::from_min_max(1.0, 2.0);
        assert!(is_topological_vertex(&k, &mut b, v[1]).unwrap());
        assert!(matches!(
            are_edges_tangent(&k, &b, v[1], e[0], e[1], 0.5),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// WR-256: a composite curve cannot be evaluated (`SmCompositeCurve` keeps
    /// `SmCurve::Evaluate`, which returns `SM_ERR`), so `GetEndTangent` fails at the end
    /// of edge 0 and the tangent stays the `SmVector3d` default; `AngleBetween` of that
    /// zero-length vector reports `0` in `AreEdgesTangent`. `IsTopologicalVertex` does
    /// not take that angle as G1 evidence (tangency-labs/smlib 7a35f33, WR-663): the
    /// vertex is kept.
    #[test]
    fn failed_end_tangent_reads_the_default_vector() {
        use crate::curve::composite_curve::CompositeCurve;
        let k = Kernel::default();
        // edge 0 ends at v1 on [-1, 0], edge 1 (a line) starts there at 0
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        let e0 = b.edge_mut(e[0]).unwrap();
        e0.curve = Some(Curve::from(CompositeCurve::new(3)));
        e0.interval = Extent1d::from_min_max(-1.0, 0.0);
        assert!(edge_end_tangent(&k, &b, e[0], false).is_err());

        // angle 0: tangent for any positive tolerance, not for 0
        assert!(are_edges_tangent(&k, &b, v[1], e[0], e[1], 0.5).unwrap());
        assert!(are_edges_tangent(&k, &b, v[1], e[1], e[0], 0.5).unwrap());
        assert!(!are_edges_tangent(&k, &b, v[1], e[0], e[1], 0.0).unwrap());
        // the parameters agree (0 and 0), but the failed tangent gives no angle: the
        // curves are not shown to meet G1 and the vertex is kept (fork 7a35f33)
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());
    }

    // tangency-labs/smlib 7a35f33 (SmVertex::IsTopologicalVertex): without the angle
    // between the two tangents (the default zero tangent of a failed end evaluation)
    // the curves are not shown to meet G1, so the vertex is kept.
    #[test]
    fn issue_2009_vertex_without_tangent_angle_is_kept() {
        use crate::curve::composite_curve::CompositeCurve;
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        let e0 = b.edge_mut(e[0]).unwrap();
        e0.curve = Some(Curve::from(CompositeCurve::new(3)));
        e0.interval = Extent1d::from_min_max(-1.0, 0.0);
        let k = Kernel::default();
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());
    }

    #[test]
    fn gaps_and_notify() {
        let k = Kernel::default();
        let (mut b, v, _) = chain(false, Vec3::X, Vec3::X);
        // no curves, no faces: no gaps
        assert_eq!(max_gap_3d(&k, &mut b, v[1], None, None).unwrap(), None);
        // cached edge gaps on both uses of v1
        for (i, g) in [(1u32, 0.5), (2, 0.25)] {
            let vu = VertexuseId::from_index(i);
            let r = b.vertexuse_mut(vu).unwrap();
            r.vertex_edge_gap.vertexuse = Some(vu);
            r.vertex_edge_gap.gap_3d = g;
        }
        for i in 0..2 {
            b.edge_mut(EdgeId::from_index(i)).unwrap().curve =
                Some(Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3)));
        }
        let mut list = vec![HeldGap {
            holder: VertexuseId::from_index(0),
            gap: VertexGap::Edge(VertexEdgeGap::new()),
        }];
        let m = max_up_dim_gap_3d(&k, &mut b, v[1], Some(&mut list), Some(0.3)).unwrap();
        assert_eq!(
            m.map(|g| (g.holder.index(), g.gap.length())),
            Some((1, 0.5))
        );
        assert_eq!(list.len(), 1);
        assert!(matches!(
            max_up_dim_gap_3d(&k, &mut b, v[1], None, None),
            Err(SolidError::InvalidInput { .. })
        ));

        let mut records = crate::topology::topology::NotifyRecords::new();
        let mut attrs = AttributeStore::default();
        let r = |key: u64| ObjectRef {
            type_code: TYPE,
            key: ObjectKey(key),
        };
        records.insert(ObjectKey(1), Topology::new());
        let vu_ref = |vu: VertexuseId| ObjectRef {
            type_code: vertexuse::TYPE,
            key: ObjectKey(100 + u64::from(vu.index())),
        };
        set_point(
            &mut b,
            v[1],
            Vec3::new(1.0, 1.0, 0.0),
            &mut records,
            &mut attrs,
            None,
            r(1),
            &vu_ref,
        )
        .unwrap();
        assert_eq!(point(&b, v[1]).unwrap(), Vec3::new(1.0, 1.0, 0.0));
        assert!(!b
            .vertexuse(VertexuseId::from_index(1))
            .unwrap()
            .vertex_edge_gap
            .is_init());
        assert!(!b
            .vertexuse(VertexuseId::from_index(2))
            .unwrap()
            .vertex_edge_gap
            .is_init());
    }

    #[test]
    fn wr92_rejects_cusps_and_keeps_smooth_g1_and_corners() {
        use crate::nurbs::b_spline_curve::{
            BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
        };

        let k = Kernel::default();

        let dir_deg = |deg: f64| -> Vec3 {
            let rad = deg * (PI / 180.0);
            Vec3::new(rad.cos(), rad.sin(), 0.0)
        };

        // Line-chain cases: (d2, expected)
        for (d2, want) in [
            (Vec3::X, true),          // 0.0° smooth G1 continuation
            (dir_deg(0.3), true),     // 0.3° within SM_CONTINUITY_ANGLE (0.5°)
            (dir_deg(0.6), false),    // 0.6° outside SM_CONTINUITY_ANGLE
            (Vec3::Y, false),         // 90.0° right-angle corner
            (-Vec3::X, false),        // 180.0° exact cusp
            (dir_deg(179.7), false),  // 179.7° near-cusp within 0.5° of PI
            (dir_deg(-179.7), false), // -179.7° near-cusp within 0.5° of -PI
        ] {
            let (mut b, v, e) = chain(true, Vec3::X, d2);
            assert_eq!(is_topological_vertex(&k, &mut b, v[1]).unwrap(), want);
            assert_eq!(
                are_edges_tangent(&k, &b, v[1], e[0], e[1], CONTINUITY_ANGLE).unwrap(),
                want
            );
        }

        // Curved degree-2 B-splines (not lines) meeting at matching parameter 1.0
        let quad = |pts: [Point3; 3], u0: f64, u1: f64| -> Curve {
            Curve::from(
                BSplineCurve::create_canonical(&CanonicalData {
                    dim: 3,
                    degree: 2,
                    control_points: &pts,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &[3, 3],
                    knots: &[u0, u1],
                    knot_type: KnotType::Unspecified,
                    weights: None,
                    trim: None,
                })
                .unwrap(),
            )
        };

        let c0 = quad(
            [
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.5, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            ],
            0.0,
            1.0,
        );
        assert!(c0
            .is_line(&k, IS_LINE_SAMPLES, ZONE_TOL_3D / 1000.0)
            .unwrap()
            .is_none());

        for (pts1, want) in [
            // Smooth G1 continuation at (1, 0, 0) with outgoing tangent +X
            (
                [
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(1.5, 0.0, 0.0),
                    Vec3::new(2.0, 1.0, 0.0),
                ],
                true,
            ),
            // 180° curved cusp at (1, 0, 0) with outgoing tangent -X
            (
                [
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.5, 0.0, 0.0),
                    Vec3::new(0.0, -1.0, 0.0),
                ],
                false,
            ),
            // 90° curved corner at (1, 0, 0) with outgoing tangent +Y
            (
                [
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.5, 0.0),
                    Vec3::new(2.0, 1.0, 0.0),
                ],
                false,
            ),
        ] {
            let c1 = quad(pts1, 1.0, 2.0);
            assert!(c1
                .is_line(&k, IS_LINE_SAMPLES, ZONE_TOL_3D / 1000.0)
                .unwrap()
                .is_none());
            let (mut b, v, e) = chain(false, Vec3::X, pts1[2] - Vec3::X);
            let e0 = b.edge_mut(e[0]).unwrap();
            e0.curve = Some(c0.clone());
            e0.interval = Extent1d::from_min_max(0.0, 1.0);
            let e1 = b.edge_mut(e[1]).unwrap();
            e1.curve = Some(c1);
            e1.interval = Extent1d::from_min_max(1.0, 2.0);

            assert_eq!(is_topological_vertex(&k, &mut b, v[1]).unwrap(), want);
        }
    }

    #[test]
    fn wr92_reversed_and_inside_out_edges_distinguish_g1_from_cusp() {
        use crate::curve::line::BoundedLineOptions;
        use crate::nurbs::b_spline_curve::{
            BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
        };

        let k = Kernel::default();

        // 1a. Reversed second line edge (at_start1 == false, at_start2 == false):
        // smooth G1 continuation (0,0,0) -> (1,0,0) -> (2,0,0) with opposing raw tangents
        let (mut b, v, e) = chain(false, Vec3::X, Vec3::X);
        b.edge_mut(e[0]).unwrap().curve =
            Some(Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3)));
        b.edge_mut(e[1]).unwrap().vertices = [v[2], v[1]];
        b.edge_mut(e[1]).unwrap().curve = Some(Curve::from(Line::from_points(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::X,
            3,
        )));
        b.edgeuse_mut(EdgeuseId::from_index(2)).unwrap().orientation = OrientType::Opposite;
        b.edgeuse_mut(EdgeuseId::from_index(3)).unwrap().orientation = OrientType::Same;
        assert_eq!(edge_start_vertex(&b, e[1]).unwrap(), Some(v[2]));
        assert!(are_edges_tangent(&k, &b, v[1], e[0], e[1], CONTINUITY_ANGLE).unwrap());
        assert!(is_topological_vertex(&k, &mut b, v[1]).unwrap());

        // 1b. Reversed second line edge forming a 180° cusp (0,0,0) -> (1,0,0) and
        // (1,0,0) -> (0,0,0) whose raw curve tangents are parallel (+X and +X)
        let (mut b, v, e) = chain(false, Vec3::X, -Vec3::X);
        b.edge_mut(e[0]).unwrap().curve =
            Some(Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3)));
        b.edge_mut(e[1]).unwrap().vertices = [v[2], v[1]];
        b.edge_mut(e[1]).unwrap().curve =
            Some(Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3)));
        b.edgeuse_mut(EdgeuseId::from_index(2)).unwrap().orientation = OrientType::Opposite;
        b.edgeuse_mut(EdgeuseId::from_index(3)).unwrap().orientation = OrientType::Same;
        assert_eq!(edge_start_vertex(&b, e[1]).unwrap(), Some(v[2]));
        assert!(!are_edges_tangent(&k, &b, v[1], e[0], e[1], CONTINUITY_ANGLE).unwrap());
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());

        // 1c. Same reversed-edge check on curved degree-2 B-splines meeting at param 1.0
        let quad = |pts: [Point3; 3]| -> Curve {
            Curve::from(
                BSplineCurve::create_canonical(&CanonicalData {
                    dim: 3,
                    degree: 2,
                    control_points: &pts,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &[3, 3],
                    knots: &[0.0, 1.0],
                    knot_type: KnotType::Unspecified,
                    weights: None,
                    trim: None,
                })
                .unwrap(),
            )
        };
        for (pts1, want) in [
            (
                [
                    Vec3::new(2.0, 1.0, 0.0),
                    Vec3::new(1.5, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                ],
                true,
            ),
            (
                [
                    Vec3::new(0.0, -1.0, 0.0),
                    Vec3::new(0.5, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                ],
                false,
            ),
        ] {
            let (mut b, v, e) = chain(false, Vec3::X, pts1[0] - Vec3::X);
            b.edge_mut(e[0]).unwrap().curve = Some(quad([
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.5, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            ]));
            b.edge_mut(e[1]).unwrap().vertices = [v[2], v[1]];
            b.edge_mut(e[1]).unwrap().curve = Some(quad(pts1));
            b.edgeuse_mut(EdgeuseId::from_index(2)).unwrap().orientation = OrientType::Opposite;
            b.edgeuse_mut(EdgeuseId::from_index(3)).unwrap().orientation = OrientType::Same;
            assert_eq!(is_topological_vertex(&k, &mut b, v[1]).unwrap(), want);
        }

        // 2. Line::bounded with inside_out = true (IsLine returns STEP vector, while NURB
        // parameter runs opposite to STEP)
        let domain = Extent1d::from_min_max(0.0, 1.0);
        let opts = BoundedLineOptions {
            inside_out: true,
            ..BoundedLineOptions::default()
        };

        // Smooth G1 continuation: NURB [0, 1] runs from (1,0,0) to (2,0,0)
        let line_io_smooth = Line::bounded(Vec3::new(2.0, 0.0, 0.0), -Vec3::X, &domain, &opts);
        assert!(line_io_smooth.evaluate_point(0.0).approx_eq(Vec3::X));
        assert!(line_io_smooth
            .evaluate_point(1.0)
            .approx_eq(Vec3::new(2.0, 0.0, 0.0)));
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::X);
        b.edge_mut(e[1]).unwrap().curve = Some(Curve::from(line_io_smooth));
        assert!(are_edges_tangent(&k, &b, v[1], e[0], e[1], CONTINUITY_ANGLE).unwrap());
        assert!(is_topological_vertex(&k, &mut b, v[1]).unwrap());

        // 180° cusp: NURB [0, 1] runs from (1,0,0) back to (0,0,0)
        let line_io_cusp = Line::bounded(Vec3::ZERO, Vec3::X, &domain, &opts);
        assert!(line_io_cusp.evaluate_point(0.0).approx_eq(Vec3::X));
        assert!(line_io_cusp.evaluate_point(1.0).approx_eq(Vec3::ZERO));
        let (mut b, v, e) = chain(true, Vec3::X, -Vec3::X);
        b.edge_mut(e[1]).unwrap().curve = Some(Curve::from(line_io_cusp));
        assert!(!are_edges_tangent(&k, &b, v[1], e[0], e[1], CONTINUITY_ANGLE).unwrap());
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());
    }

    #[test]
    fn wr92_propagates_failed_start_tangent_evaluation() {
        use crate::curve::composite_curve::CompositeCurve;
        use crate::error::SM_ERR;

        let k = Kernel::default();

        // 1. Edge 1 starts at v[1] (at_start2 == true) at matching parameter 1.0, and
        // CompositeCurve::evaluate returns SM_ERR.
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        let e1 = b.edge_mut(e[1]).unwrap();
        e1.curve = Some(Curve::from(CompositeCurve::new(3)));
        e1.interval = Extent1d::from_min_max(1.0, 2.0);
        assert!(edge_end_tangent(&k, &b, e[1], true).is_err());
        assert!(matches!(
            is_topological_vertex(&k, &mut b, v[1]),
            Err(e) if crate::error::upstream_status(&e) == Some(SM_ERR)
        ));

        // The callers: `GetTopologicalVertices` and the boolean cleanup return the error;
        // `RemoveTopologicalEdgesAndVertices` ignores it and keeps the vertex.
        let mut k_mut = Kernel::default();
        assert!(matches!(
            crate::topology::brep::p1::topological_vertices(&k, &mut b),
            Err(e) if crate::error::upstream_status(&e) == Some(SM_ERR)
        ));
        // the cleanup visits an intersection vertex with a mate
        let mut relations = crate::boolean::merge::p1::CleanupRelations {
            vertices: vec![v[1]],
            ..Default::default()
        };
        relations.mates.vertices.record(v[1], v[3]).unwrap();
        assert!(matches!(
            crate::boolean::merge::p1::boolean_post_process(&mut k_mut, &mut b, &relations, false),
            Err(e) if crate::error::upstream_status(&e) == Some(SM_ERR)
        ));
        crate::topology::brep::p5::remove_topological_edges_and_vertices(
            &mut k_mut, &mut b, None, None,
        )
        .unwrap();
        assert!(b.vertex_list().contains(&v[1]));

        // 2. Edge 0 reversed so it starts at v[1] (at_start1 == true) at matching
        // parameter 0.0, and CompositeCurve::evaluate returns SM_ERR.
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        b.edge_mut(e[0]).unwrap().vertices = [v[1], v[0]];
        b.edge_mut(e[0]).unwrap().curve = Some(Curve::from(CompositeCurve::new(3)));
        b.edge_mut(e[0]).unwrap().interval = Extent1d::from_min_max(0.0, 1.0);
        b.edgeuse_mut(EdgeuseId::from_index(0)).unwrap().orientation = OrientType::Opposite;
        b.edgeuse_mut(EdgeuseId::from_index(1)).unwrap().orientation = OrientType::Same;
        assert_eq!(edge_start_vertex(&b, e[0]).unwrap(), Some(v[1]));
        assert!(edge_end_tangent(&k, &b, e[0], true).is_err());
        assert!(matches!(
            is_topological_vertex(&k, &mut b, v[1]),
            Err(e) if crate::error::upstream_status(&e) == Some(SM_ERR)
        ));

        // 3. When GetEndTangent(false) fails (WR-256): -Vec3::UNDEF still fails
        // AngleBetween and keeps the vertex (WR-663).
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        let e0 = b.edge_mut(e[0]).unwrap();
        e0.curve = Some(Curve::from(CompositeCurve::new(3)));
        e0.interval = Extent1d::from_min_max(-1.0, 0.0);
        assert!(!is_topological_vertex(&k, &mut b, v[1]).unwrap());
    }

    /// #21340: the chain's middle vertex v1 moved `off` from its edges' common end at
    /// `(1, 0, 0)`, with tolerance `tol`; edge 0 optionally an offset of its line (a curve
    /// kind with no bound on its evaluation's error) at `offset` in the plane of normal z.
    fn merged(tol: f64, off: Vec3, offset: Option<f64>) -> (Kernel, Brep, VertexId) {
        let (mut b, v, e) = chain(true, Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        if let Some(d) = offset {
            let rec = b.edge_mut(e[0]).unwrap();
            let line = rec.curve.take().unwrap();
            let c = crate::curve::offset_curve::OffsetCurve::new(
                3,
                line,
                Extent1d::from_min_max(0.0, 1.0),
                Vec3::new(0.0, 0.0, 1.0),
                d,
                true,
            )
            .unwrap();
            let c = Curve::from(c);
            assert!(c.point_error_bound(1.0).is_none());
            rec.curve = Some(c);
        }
        let vr = b.vertex_mut(v[1]).unwrap();
        vr.point += off;
        vr.tolerance = tol;
        (Kernel::default(), b, v[1])
    }

    /// The farthest end of `v`'s edges at it: (distance, whether every end is within half
    /// `v`'s tolerance).
    fn farthest(k: &Kernel, b: &Brep, v: VertexId) -> (f64, bool) {
        let at = b.vertex(v).unwrap().point;
        let tol = b.vertex(v).unwrap().tolerance;
        let mut far = 0.0_f64;
        for x in edges(b, v, None).unwrap() {
            let rec = b.edge(x).unwrap();
            let (s0, s1) = crate::topology::edge::start_end_vertices(b, x).unwrap();
            for (end, t) in [(s0, rec.interval.min()), (s1, rec.interval.max())] {
                if end == Some(v) {
                    let p = rec.curve.as_ref().unwrap().evaluate_point(k, t).unwrap();
                    far = far.max(at.distance(p));
                }
            }
        }
        (far, far <= 0.5 * tol)
    }

    #[test]
    fn issue_21340_merged_vertex_covers_ends_inside_its_ball() {
        // both line ends 0.3 from v1, inside the ball of radius 0.5: v1 grows to twice the
        // certified reach, just above 0.6, and covers both ends
        let (k, mut b, v) = merged(0.5, Vec3::new(0.0, 0.3, 0.0), None);
        assert!(!farthest(&k, &b, v).1);
        let reach = edge_end_reach(&k, &b, v).unwrap();
        assert!((0.3..=0.3 * (1.0 + 1e-12)).contains(&reach), "{reach}");
        cover_merged_ends(&k, &mut b, v).unwrap();
        assert_eq!(
            b.vertex(v).unwrap().tolerance.to_bits(),
            (2.0 * reach).to_bits()
        );
        let (far, covered) = farthest(&k, &b, v);
        assert!(covered && (far - 0.3).abs() < 1e-15, "{far}");

        // already larger than twice the reach: grow-only, unchanged
        let (k, mut b, v) = merged(0.7, Vec3::new(0.0, 0.3, 0.0), None);
        cover_merged_ends(&k, &mut b, v).unwrap();
        assert_eq!(b.vertex(v).unwrap().tolerance, 0.7);
    }

    #[test]
    fn issue_21340_merged_vertex_keeps_its_tolerance_past_its_ball() {
        // the ends 0.3 from v1, outside the ball of radius 0.2 (#21511's `sss_point` case):
        // the reach is not the merge's neighbourhood and v1 stays at 0.2
        let (k, mut b, v) = merged(0.2, Vec3::new(0.0, 0.3, 0.0), None);
        assert!(edge_end_reach(&k, &b, v).unwrap() > 0.2);
        cover_merged_ends(&k, &mut b, v).unwrap();
        assert_eq!(b.vertex(v).unwrap().tolerance, 0.2);
    }

    #[test]
    fn issue_21340_merged_vertex_takes_no_uncertified_end() {
        // edge 0 an offset of its line by 0.1 (no bound on its evaluation's error), its end
        // farther from v1 (moved 0.3 in z: sqrt(0.09 + 0.01)) than edge 1's line end: the
        // reach and the grown tolerance are those with edge 0 left out
        let off = Vec3::new(0.0, 0.0, 0.3);
        let (k, mut b, v) = merged(0.5, off, Some(0.1));
        assert!(farthest(&k, &b, v).0 > 0.31);
        let (_, mut without, w) = merged(0.5, off, None);
        without.edge_mut(EdgeId::from_index(0)).unwrap().curve = None;
        assert_eq!(
            edge_end_reach(&k, &b, v).unwrap().to_bits(),
            edge_end_reach(&k, &without, w).unwrap().to_bits()
        );
        cover_merged_ends(&k, &mut b, v).unwrap();
        cover_merged_ends(&k, &mut without, w).unwrap();
        let tol = b.vertex(v).unwrap().tolerance;
        assert_eq!(
            tol.to_bits(),
            without.vertex(w).unwrap().tolerance.to_bits()
        );
        assert!(tol > 0.6 && tol < 0.62, "{tol}");
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Ent(EntityRef);

    impl crate::intersect::point_class::ClassObject for Ent {
        fn class_kind(&self) -> crate::intersect::point_class::ClassObjectKind {
            use crate::intersect::point_class::ClassObjectKind as K;
            match self.0 {
                EntityRef::Vertex(_) => K::Vertex,
                EntityRef::Edge(_) => K::Edge,
                _ => K::Other,
            }
        }
    }

    /// Every end of every edge at each of `vs` within half that vertex's tolerance
    /// (`AssertValid`'s edge-end test), with the distances.
    fn ends_on_vertices(k: &Kernel, b: &Brep, vs: &[VertexId]) -> (bool, Vec<f64>) {
        let mut ok = true;
        let mut far = Vec::new();
        for &v in vs {
            let rec = b.vertex(v).unwrap();
            for x in edges(b, v, None).unwrap() {
                let e = b.edge(x).unwrap();
                let (s0, s1) = crate::topology::edge::start_end_vertices(b, x).unwrap();
                for (end, t) in [(s0, e.interval.min()), (s1, e.interval.max())] {
                    if end == Some(v) {
                        let p = e.curve.as_ref().unwrap().evaluate_point(k, t).unwrap();
                        let d = rec.point.distance(p);
                        ok &= d <= 0.5 * rec.tolerance;
                        far.push(d);
                    }
                }
            }
        }
        (ok, far)
    }

    /// #21511: a point merge onto the chain's middle vertex v1 with a surf/surf/surf
    /// point `sss` (the `topology.brep_p9` `point_merge_7_sss_point` rows' shape). Edge 1
    /// is its line, a cubic B-spline over its natural interval, one over an interior
    /// sub-interval (v2 moved onto that curve's point there) or (3) the quarter ellipse
    /// of radii 2 and 1 about the origin from `(2, 0, 0)` to `(0, 1, 0)` over its natural
    /// interval.
    fn sss_merge(edge1: u8, sss: Point3) -> (Kernel, Brep, [VertexId; 4], [EdgeId; 2]) {
        use crate::intersect::point_class::{BrepMerge, PointClassType, PointClassification};
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let mut k = Kernel::default();
        let (mut b, v, e) = chain(true, Vec3::X, Vec3::Y);
        if edge1 == 3 {
            let quarter = Extent1d::from_min_max(0.0, 90.0);
            let frame = crate::geom::axis2_placement::Axis2Placement::default();
            let c = Curve::from(
                crate::curve::ellipse::Ellipse::create_canonical(
                    &k,
                    &frame,
                    2.0,
                    1.0,
                    Some(&quarter),
                    None,
                )
                .unwrap(),
            );
            let iv = c.natural_interval();
            let (p0, p1) = (
                c.evaluate_point(&k, iv.min()).unwrap(),
                c.evaluate_point(&k, iv.max()).unwrap(),
            );
            assert!(p0.distance(Point3::new(2.0, 0.0, 0.0)) < 1e-12, "{p0:?}");
            assert!(p1.distance(Point3::new(0.0, 1.0, 0.0)) < 1e-12, "{p1:?}");
            let rec = b.edge_mut(e[1]).unwrap();
            rec.curve = Some(c);
            rec.interval = iv;
            b.edge_mut(e[0]).unwrap().curve =
                Some(Curve::from(Line::from_points(Vec3::ZERO, p0, 3)));
            b.vertex_mut(v[1]).unwrap().point = p0;
            b.vertex_mut(v[2]).unwrap().point = p1;
        } else if edge1 > 0 {
            let cp = |x: f64, y: f64| CPoint::new(x, y, 0.0, NOW);
            let (pw, knots) = if edge1 == 1 {
                let pw = vec![cp(1.0, 0.0), cp(1.4, 0.3), cp(0.8, 0.7), cp(1.0, 1.0)];
                (pw, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
            } else {
                let pw = vec![
                    cp(1.0, -0.5),
                    cp(1.3, 0.0),
                    cp(0.7, 0.4),
                    cp(1.2, 0.8),
                    cp(0.9, 1.2),
                    cp(1.0, 1.6),
                ];
                (
                    pw,
                    vec![-1.0, -1.0, -1.0, -1.0, 0.5, 1.5, 2.0, 2.0, 2.0, 2.0],
                )
            };
            let c = Curve::from(BSplineCurve::from_nurb(
                NurbCurve::new(pw, 3, knots).unwrap(),
                3,
                false,
            ));
            let (p0, p1) = (
                c.evaluate_point(&k, 0.0).unwrap(),
                c.evaluate_point(&k, 1.0).unwrap(),
            );
            b.edge_mut(e[1]).unwrap().curve = Some(c);
            // edge 0's line ends at edge 1's start (v1), v2 at edge 1's end
            b.edge_mut(e[0]).unwrap().curve =
                Some(Curve::from(Line::from_points(Vec3::ZERO, p0, 3)));
            b.vertex_mut(v[1]).unwrap().point = p0;
            b.vertex_mut(v[2]).unwrap().point = p1;
        }
        let mut pc = PointClassification::new(ZONE_TOL_3D);
        pc.set_class_object(PointClassType::Vertex, Some(Ent(EntityRef::Vertex(v[1]))));
        pc.set_sss_point(sss);
        let mut records = crate::topology::topology::NotifyRecords::new();
        let mut attributes = AttributeStore::default();
        let mut env = crate::topology::edge::NotifyEnv {
            records: &mut records,
            attributes: &mut attributes,
            context: None,
            key_of: &crate::topology::loopuse::notify_ref,
        };
        // the source point is the surf/surf/surf point, as the golden rows merge it
        pc.merge_into_brep(
            &mut k,
            sss,
            BrepMerge {
                brep: &mut b,
                target: EntityRef::Vertex(v[1]),
                handle: &Ent,
                notifications: &mut env,
                curve_ref: &|x, kind| ObjectRef {
                    key: ObjectKey(((kind as u64) << 32) | u64::from(x.index())),
                    type_code: kind,
                },
            },
        )
        .unwrap();
        (k, b, v, e)
    }

    #[test]
    fn issue_21511_sss_point_carries_its_edge_ends() {
        // the issue's moves (0.36 to 24.5 off the edges' ends), each onto a line pair, a
        // natural-interval cubic and an interior-interval cubic
        for edge1 in 0..3 {
            for sss in [
                Point3::new(0.2, 0.3, 0.0),
                Point3::new(20.0, 3.0, 4.0),
                Point3::new(10.0, -10.0, 20.0),
            ] {
                let (k, b, v, e) = sss_merge(edge1, sss);
                let before: Vec<Point3> = [v[0], v[2]]
                    .iter()
                    .map(|&w| b.vertex(w).unwrap().point)
                    .collect();
                // the vertex is at `sss`, sized to the merged points alone: its ends at the
                // old point lie outside its ball, and only a carried end can be on it
                let moved = b.vertex(v[1]).unwrap();
                assert_eq!(moved.point, sss);
                assert!(moved.tolerance <= ZONE_TOL_3D, "{}", moved.tolerance);
                let (ok, far) = ends_on_vertices(&k, &b, &[v[0], v[1], v[2]]);
                assert!(ok, "edge1 {edge1} sss {sss:?}: {far:?}");
                // the far ends stay where they were, on their own vertices
                let c0 = b.edge(e[0]).unwrap().curve.as_ref().unwrap();
                assert_eq!(c0.evaluate_point(&k, 0.0).unwrap(), before[0]);
                let c1 = b.edge(e[1]).unwrap().curve.as_ref().unwrap();
                assert!(c1.evaluate_point(&k, 1.0).unwrap().distance(before[1]) < 1e-12);
                // a line stays a line; a cubic stays a cubic B-spline
                assert!(matches!(c0.kind(), CurveKind::Line(_)));
                if edge1 == 0 {
                    assert!(matches!(c1.kind(), CurveKind::Line(_)));
                } else {
                    let CurveKind::BSpline(s) = c1.kind() else {
                        panic!("edge 1 not a B-spline")
                    };
                    assert_eq!(s.degree().unwrap(), 3);
                }
            }
        }
    }

    /// #21511: on an exact conic edge a surf/surf/surf point on the conic re-trims the
    /// edge (the ellipse kept, its start parameter moved to the point's), and one off it
    /// moves the end through a B-spline (no ellipse ends there).
    #[test]
    fn issue_21511_sss_point_on_a_conic_keeps_the_conic() {
        let on = Point3::new(2.0 * (PI / 6.0).cos(), (PI / 6.0).sin(), 0.0);
        let off = Point3::new(1.5, 0.9, 0.3);
        for (sss, stays_ellipse) in [(on, true), (off, false)] {
            let (k, b, v, e) = sss_merge(3, sss);
            assert_eq!(b.vertex(v[1]).unwrap().point, sss);
            let (ok, far) = ends_on_vertices(&k, &b, &[v[0], v[1], v[2]]);
            assert!(ok, "sss {sss:?}: {far:?}");
            let rec = b.edge(e[1]).unwrap();
            let c1 = rec.curve.as_ref().unwrap();
            // the far end stays on v2 at (0, 1, 0)
            let p1 = c1.evaluate_point(&k, rec.interval.max()).unwrap();
            assert!(p1.distance(Point3::new(0.0, 1.0, 0.0)) < 1e-12, "{p1:?}");
            if stays_ellipse {
                assert!(
                    matches!(c1.kind(), CurveKind::Ellipse(_)),
                    "{:?}",
                    c1.kind()
                );
                assert!(rec.interval.min() > c1.natural_interval().min());
            } else {
                assert!(
                    matches!(c1.kind(), CurveKind::BSpline(_)),
                    "{:?}",
                    c1.kind()
                );
            }
        }
    }
}
