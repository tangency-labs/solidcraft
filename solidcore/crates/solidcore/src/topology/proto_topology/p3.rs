//! Proto topology, part 3: finding or creating proto vertices and edges from point
//! and curve classifications, splitting proto edges and their definitions, gluing
//! coincident B-rep vertices, and replacing overlapping proto-edge groups.
//!
//! Upstream: `source/SMLib/src/SmProtoTopology.cpp`, `source/SMLib/inc/SmProtoTopology.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`ProtoTopologyManager::find_or_create_proto_vertex_pcs`] | `SmProtoTopologyManager::FindOrCreateProtoVertex(PC, PC, ..)` |
//! | [`ProtoTopologyManager::find_or_create_proto_edge`] | `SmProtoTopologyManager::FindOrCreateProtoEdge` |
//! | [`ProtoTopologyManager::edge_definition_split`] | `SmEdgeDefinition::Split` |
//! | [`ProtoTopologyManager::proto_edge_split`] | `SmProtoEdge::Split` |
//! | [`ProtoTopologyManager::split_intersecting_proto_edges`] | `SmProtoTopologyManager::SplitIntersectingProtoEdges` |
//! | [`ProtoTopologyManager::glue_coincident_brep_vertices`] | `SmProtoTopologyManager::GlueCoincidentBrepVertices` |
//! | [`ProtoTopologyManager::resolve_overlapping_proto_edges`] | `SmProtoTopologyManager::ResolveOverlappingProtoEdges` |
//!
//! Deviations:
//! - **Vertices placed by their best certified edge crossing** (not upstream, WR-410,
//!   tangency-labs/solidcore#1059, the default since its flag was retired;
//!   the SolidCraft fork's `FindOrCreateProtoEdge` change): after each end's proto vertex,
//!   [`ProtoTopologyManager::add_crossing`] with the angle at which the face pair's curve
//!   crosses the edge its end lies on (`crossing_sine`) and that curve. A later crossing
//!   with a smaller misplacement bound (gap over sine) moves the vertex only where a
//!   certified bound proves the edge stretch it moves along stays within tolerance of the
//!   other object and of the first crossing's curve; otherwise the first placement stays.
//! - **One proto vertex per crossing of an edge with a face** (not upstream, #17595). A face
//!   pair's curve ends where its own face's surface meets the other face, so the two faces at
//!   an edge with a loose tolerance give two points on that edge where it crosses a face of
//!   the other B-rep, up to tolerance / sin φ apart along it (φ the angle between the edge
//!   and the face, whatever the angle at which the curves meet the edge). The proto vertex's
//!   ball cannot take both in: the capture distance stays below half the smallest distance
//!   between two vertices of either operand, so that no ball reaches two distinct vertices
//!   (`intersect_insert_relate`), and the two points lie farther apart than vertices the
//!   body keeps distinct (0.057 on an edge of tolerance 4.0e-3 crossed at 1.3° to 1.8°; the
//!   intersection loop stayed open between them). So `FindOrCreateProtoVertex`'s lookup
//!   ([`ProtoTopologyManager::find_proto_vertex_holding`]) holds a point in a proto vertex by
//!   its ball (here: the rule of "One point or two", `docs/booleans.md`) or by its crossing: a
//!   definition on the same edge and face, with the edge between that definition's parameter and
//!   the point's proved inside the band that puts a point of the face on the edge
//!   ([`edge_piece_in_face_band`]). The band is the zone of what was classified with the edge's
//!   zone, or with the gap at which either point was taken to be on the edge where that is wider,
//!   up to the most a local tolerance may be (`LOCAL_TOLERANCE_RATIO`). Within the band the edge
//!   lies in the face, and where on that piece it passes through is not determined. An edge tangent
//!   to the face at either point (`COINCIDENT_TANGENT_SINE`, the coincidence check's angle) runs in
//!   the face there: those two points are the ends of a stretch the edge and the face share and
//!   stay two.
//! - **An edge lying in a face, off it by more than the system zone tolerance** (not
//!   upstream, #17527). Two face pairs at an edge of one B-rep that lies within its tolerance
//!   of a face of the other (a revolved tool's rim in a hole's wall, its axis 1.2e-5 off the
//!   hole's) each give their own curve along the edge, and the proto edges of the two overlap.
//!   `ResolveOverlappingProtoEdges` replaces such a group with the edge's curve classified
//!   against the face. Upstream classifies it within the manager's tolerance (half the zone
//!   tolerance), where the edge lies in the face nowhere: the group was deleted and nothing
//!   replaced it, and the loop stayed open. Here the curve is classified within the edge's own
//!   tolerance where that is the larger, as the face pairs classified it, and the group takes in
//!   every other proto edge all of whose definitions come from the same edge and face
//!   ([`ProtoTopologyManager::absorb_pieces_of_edge_in_face`]): the replacement holds every
//!   piece of the edge in the face, so a piece left out was imprinted twice and related to one
//!   edge of the other B-rep twice ("another entity is already related to the other one").
//! - **Removed proto edges** (SolidCraft fix, tangency-labs/smlib 93efd6d, UB-239).
//!   Upstream's `SplitIntersectingProtoEdges` deletes the combined-away proto edge while
//!   other proto edges (overlap lists) still name it (`CombineProtoEdge` hands its
//!   proto faces to the survivor), and later passes read it after the free. Here every
//!   reference to it is dropped before the delete ([`ProtoTopologyManager::forget_proto_edge`],
//!   part 1), here and in `ResolveOverlappingProtoEdges`. The fork does that inside
//!   `RemoveProtoEdge`; the port does it at these library deletes, so unlisting or
//!   deleting a proto edge on its own (the difftest's script ops) stays upstream's.
//!   Without it, the stale id names a deleted slot and the later passes fail with
//!   `InvalidInput`.
//! - **UB-241 in `GlueCoincidentBrepVertices`** (SolidCraft fix, tangency-labs/smlib
//!   da8d05e): use-after-free in `GlueCoincidentBrepVertices` on the `GlueVertices`
//!   path; the port stops before that call. The delete branch is defined and runs as
//!   upstream.
//! - **B-rep access.** Topology objects are the caller's handles `O`, as in
//!   [`p1`](super::p1). What these functions read from vertices and edges, and the
//!   store new curves go to, is [`ProtoBrep`] (of `topology.proto_topology_p2`, which
//!   gained `vertex_common_edges` and `face_curve_on_classify` (`SmFace::CurveOnClassify(TRUE, cc)`, implemented by the
//!   B-rep owner with `query::face::p2::curve_on_classify`) for these functions; `vertex_point`, `edge_faces` and `edge_other_vertex` are
//!   those of its supertrait [`ProtoTopology`](super::p1::proto::ProtoTopology)), which the owner of the B-reps implements. `SmTol::GetZoneTol3d` of a vertex is
//!   [`ProtoBrep::topology_tolerance`]; `SmTol::GetXSectTol3d(v1, v2)` is the
//!   sum of the two.
//! - **B-rep edits**: `SmBrep::SqueezeEdge` of `GetWhichBrep(which)` is
//!   [`ProtoBrep::brep_squeeze_edge`] (the owner of the B-reps calls
//!   `topology::brep::p3::squeeze_edge`), so
//!   [`ProtoTopologyManager::glue_coincident_brep_vertices`] takes `&mut Kernel` and
//!   the B-reps mutably.
//! - **Callees of other units** go through traits the B-reps' owner implements:
//!   `SmBrep::DeleteEdge` (`topology.brep_p4`) through [`BrepGlue::delete_edge`],
//!   `SmBrep::Point3DClassify` goes through [`ProtoBrep::brep_point_3d_classify`],
//!   which the B-reps' owner implements. `SmBrep::GlueVertices`
//!   goes through [`BrepGlue`], which the B-reps' owner implements with
//!   `topology::brep::p5` (so [`ProtoTopologyManager::glue_coincident_brep_vertices`]
//!   takes the kernel and the B-reps mutably).
//!   The intersector's `GetOtherMate` /
//!   `RemoveRelationship` are reached through [`Mates`] (ported in
//!   `boolean.topology_intersector_p1` / `_p2`; [`NoMates`] for no intersector maps). The
//!   `SmCurveClassification` constructor is part 4's ([`CurveClassification::with_curve`]). `ExtractGeometry` calls part 5 and
//!   currently reaches part 6's `Homogenize` dependency. The code
//!   after a stub is ported in full. The callees of `topology.proto_topology_p2` are
//!   its methods on [`ProtoTopologyManager`].
//! - **Deleted curves.** Functions that delete curves (through `Destruct` or the
//!   destructors) return the handles deleted, as in [`p1`](super::p1).
//! - **Undefined behaviour upstream** (dereferencing a null or deleted object, indexing
//!   an empty face array) is [`SolidError::InvalidInput`].
//! - **Out-params** become return values: the created flags and new objects of the
//!   find-or-create and split functions.
//! - `FindOrCreateProtoEdge` reads the topology intersector's `GetThisApproxTol3d`,
//!   passed as a value (`ti_approx_tol_3d`).
//! - A failing `eRet` of `GlueCoincidentBrepVertices` is returned as the error (the
//!   curves deleted before it are not reported).
//! - When `CreateProtoEdge` fails at `NER(pBrep1Topo)`, upstream returns the already
//!   allocated `rpNewPE` through the out-parameter; the port returns `None` there (the
//!   failing paths are not reachable from the callers ported so far).
//! - The `SM_DEBUG_CODE` blocks are not ported.
//! - **Circles in contact** (not upstream): `FindOrCreateProtoVertex(PC, PC)` with no curve
//!   takes a point classified onto an edge of each B-rep, where the two edges are circles in
//!   one plane that touch by the contact rule (`circle_coplanar_circle_intersect_in_contact`
//!   at `contact_tol_3d`), for their touching point (`touch_of_edges`): the surface
//!   intersection met their lens at its two crossings, which became vertices beside the
//!   touch (docs/booleans.md §6).
//! - **Vertex identity** (not upstream): `FindOrCreateProtoVertex(PC, PC)` makes no proto
//!   vertex for two classifications onto vertices farther apart than the intersection
//!   tolerance of the two (a distance upstream's `sm_FindPositionFromPCs` only reports and
//!   then covers by the new proto vertex's tolerance).
//! - **Recursion.** None.
//! - **Upper split child's interval** (WR-101): upstream's `SmProtoEdge::Split` sets
//!   the upper child's interval from this edge's interval after clamping it, so the upper
//!   child gets `[dT, dT]` instead of `[dT, old_max]`. The port's upper child keeps
//!   `[dT, old_max]`; the lower child is `[old_min, dT]`.
//! - **HANG-51.** `ResolveOverlappingProtoEdges` does not advance its index after it
//!   replaces an overlapping proto edge group, and a replacement can form a new group
//!   at the same index forever (`Solids/extrusion_old.smb`, rounded solid outset 2.5:
//!   a two-edge group re-formed thousands of times; the C++ hangs too). The port fails
//!   with `InvalidInput` once it has replaced more than [`MAX_OVERLAP_GROUPS_BASE`] +
//!   [`OVERLAP_GROUPS_PER_PROTO_EDGE`] x (proto edges at entry) groups in one call.
//!   The topology intersector's caller propagates it (it ignores only soft statuses).
//!   Across the difftest cases of the boolean, merge, topology intersector, proto
//!   topology, offset and fillet intersector units, four calls replace a group, one
//!   each.

use crate::curve::curve::p2::Curve;
use crate::curve::curve::p6::is_measurable_box;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::CurveKind;
use crate::error::{passed_on, EntityRef, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p1::{circle_coplanar_circle_intersect_in_contact, CircleCircle};
use crate::geom::global_solver::SolverOperation;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::{
    coincidence_reach_3d, contact_tol_3d, xsect_tol3d, xsect_tol_3d_from_zones, zone_tol3d,
    USE_DEFAULT,
};
use crate::intersect::curve_class::p5::ExtractOptions;
use crate::intersect::curve_class::{ClassificationInit, CurveClassification, CurveInterval};
use crate::intersect::point_class::{
    periodic_trim_witness, ClassGeometry, ClassObject, ClassObjectKind, PointClassType,
    PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{max2, min2, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::knots::Side;
use crate::query::face::p2 as face_p2;
use crate::surface::implicit::AnalyticSupport;
use crate::surface::surface::p6::COINCIDENT_TANGENT_SINE;

use super::p1::proto::{can_imprint, find_position_from_pcs, topo_dimension};
use super::p1::DeclaredContact;
use super::p1::{
    EdgeDefId, ProtoEdgeId, ProtoFaceId, ProtoTopologyManager, ProtoVertexId, VertexDefId,
};
use super::p2::ProtoBrep;

const OP: &str = "topology::proto_topology::p3";
const OP_RESOLVE_OVERLAPPING: &str =
    "topology::proto_topology::p3::resolve_overlapping_proto_edges";
/// Overlapping proto edge groups one
/// [`ProtoTopologyManager::resolve_overlapping_proto_edges`] may replace beyond
/// [`OVERLAP_GROUPS_PER_PROTO_EDGE`] per proto edge it starts with (not upstream; see
/// the module deviations, HANG-51).
pub const MAX_OVERLAP_GROUPS_BASE: usize = 64;

/// Overlapping proto edge groups one
/// [`ProtoTopologyManager::resolve_overlapping_proto_edges`] may replace per proto edge
/// it starts with, on top of [`MAX_OVERLAP_GROUPS_BASE`] (each group holds at least two
/// proto edges, so a call that does not cycle replaces groups in proportion to its
/// proto edges).
pub const OVERLAP_GROUPS_PER_PROTO_EDGE: usize = 4;
const OP_ED_SPLIT: &str = "topology::proto_topology::p3::edge_definition_split";
const OP_SPLIT_INT: &str = "topology::proto_topology::p3::split_intersecting_proto_edges";

/// The handle the curve intersections name (only the solution values are used).
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

/// A dereference of a null pointer or a deleted object upstream (undefined behaviour).
const fn dangling(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// The outcome of a status upstream ignores: the value, `None` for a failure other than
/// the errors that cannot be ignored here (unported callees, undefined behaviour).
fn ignore_soft<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::Cancelled { .. }
            | SolidError::InvalidInput { .. }
            | SolidError::PortInvariant { .. }),
        ) => Err(e),
        Err(_) => Ok(None),
    }
}

/// `SM_CAST_PTR(<kind>, obj)` for a topology kind.
fn as_kind<O: ClassObject>(o: Option<O>, kind: ClassObjectKind) -> Option<O> {
    o.filter(|o| o.class_kind() == kind)
}

/// `SM_CAST_PTR(SmTopology, obj)`.
fn as_topology<O: ClassObject>(o: Option<O>) -> Option<O> {
    o.filter(|o| {
        matches!(
            o.class_kind(),
            ClassObjectKind::Vertex
                | ClassObjectKind::Edge
                | ClassObjectKind::Face
                | ClassObjectKind::Loop
                | ClassObjectKind::Region
                | ClassObjectKind::Vertexuse
                | ClassObjectKind::Edgeuse
                | ClassObjectKind::Faceuse
                | ClassObjectKind::Loopuse
        )
    })
}

/// The curve behind `o` (a `NULL` dereference upstream for none).
fn curve_of<O: Copy>(geometry: &impl ClassGeometry<O>, o: Option<O>) -> Result<&Curve> {
    o.and_then(|o| geometry.curve(o))
        .ok_or(dangling("curve handle is null or unknown"))
}

// ---- callees not ported yet ----------------------------------------------------------

/// `SmCurveClassification(pCurve, interval, NULL, tol)` (the remaining arguments default:
/// no interval storage, the curve not owned and current, no surfaces).
fn new_curve_classification<O: ClassObject>(
    k: &Kernel,
    curve: O,
    interval: Extent1d,
    tol: f64,
) -> Result<CurveClassification<O>> {
    CurveClassification::with_curve(
        k,
        &ClassificationInit {
            curve: Some(curve),
            interval,
            uv_curve: None,
            src_zone_tol_3d: zone_tol3d(k, tol),
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

/// The B-rep edit [`ProtoTopologyManager::glue_coincident_brep_vertices`] (and the
/// topology intersector) make on the B-reps behind the handles, implemented by their
/// owner with the mutable arenas (`topology::brep::p5`).
pub trait BrepGlue<O> {
    /// `SmBrep::GlueVertices(pV1, pV2, pOptWireEdgesInBrep)` of `brep`;
    /// `opt_wire_edges_in_brep` is `pOptWireEdgesInBrep` (`NULL` by default: every
    /// shell edge is checked).
    ///
    /// # Errors
    ///
    /// Those of `topology::brep::p5::glue_vertices`; [`SolidError::InvalidInput`] for
    /// handles that name no vertex (or edge) of `brep`.
    fn glue_vertices(
        &mut self,
        k: &mut Kernel,
        brep: O,
        v1: O,
        v2: O,
        opt_wire_edges_in_brep: Option<&[O]>,
    ) -> Result<()>;
    /// `SmBrep::DeleteEdge(pEdge)` of `brep` (`pOptSurfaceToKeep = NULL`;
    /// `topology::brep::p4::delete_edge`).
    ///
    /// # Errors
    ///
    /// Those of `topology::brep::p4::delete_edge`; [`SolidError::InvalidInput`] for a
    /// handle that names no edge of `brep`.
    fn delete_edge(&mut self, k: &mut Kernel, brep: O, edge: O) -> Result<()>;
}

/// What the manager's topology intersector (`m_pTI`) provides to
/// [`ProtoTopologyManager::glue_coincident_brep_vertices`]: `GetOtherMate` and
/// `RemoveRelationship` (implemented by
/// `boolean::topology_intersector::p2::MapMates` over the intersector's maps).
pub trait Mates<O> {
    /// `SmTopologyIntersector::GetOtherMate(pBrepEntity)`.
    fn other_mate(&self, brep_entity: O) -> Option<O>;
    /// `SmTopologyIntersector::RemoveRelationship(pBrepEntity, pOtherEntity)`.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the two are not a mated pair.
    fn remove_relationship(&mut self, brep_entity: O, other_entity: O) -> Result<()>;
}

/// An intersector without relationships: no object has a mate.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMates;

impl<O> Mates<O> for NoMates {
    fn other_mate(&self, _brep_entity: O) -> Option<O> {
        None
    }

    fn remove_relationship(&mut self, _brep_entity: O, _other_entity: O) -> Result<()> {
        Err(SolidError::topology_failed(
            "topology::proto_topology::p3::remove_relationship",
            "the two are not a mated pair",
        ))
    }
}

// ---- the manager's members ------------------------------------------------------------

impl<O: ClassObject> ProtoTopologyManager<O> {
    /// `SmProtoTopologyManager::FindOrCreateProtoVertex(rPC1, rPC2, rpNewPV,
    /// pbWasCreated)`: the proto vertex for a point two classifications agree on
    /// (found within the tolerance, else created) and whether it was created. `None`
    /// when either classification cannot be imprinted, and (not upstream) when both
    /// classify onto vertices farther apart than the two vertices' intersection
    /// tolerance.
    ///
    /// # Errors
    ///
    /// Those of `FindProtoVertex`, `CreateProtoVertex` and the vertices' points and
    /// zone tolerances; [`SolidError::Unported`]
    /// for the callees listed in the module header.
    pub fn find_or_create_proto_vertex_pcs(
        &mut self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        pc1: &mut PointClassification<O>,
        pc2: &mut PointClassification<O>,
    ) -> Result<(Option<ProtoVertexId>, bool)> {
        self.find_or_create_proto_vertex_crossing(k, geometry, pc1, pc2, None, [false; 2])
    }

    /// [`ProtoTopologyManager::find_or_create_proto_vertex_pcs`] for an end of a curve
    /// interval: `crossing` is the interval's curve and its parameter at that end. Not
    /// upstream: where the point lies on an edge, its uncertainty region reaches along the
    /// edge as far as the curve's crossing of the edge is known ([`crossing_region`]), and
    /// a proto vertex is found where the regions are not proved apart. With no `crossing`,
    /// two classifications onto edges are the crossing of those two edges; any other point
    /// is known to its tolerance alone. Not upstream (tangency-labs/solidcore#3339): `along`
    /// says, for `pc1` and `pc2`, whether the interval runs along the seam its end lies on
    /// ([`runs_along_seam`]): the curve does not cross that seam there, so it fixes no reach
    /// along it.
    ///
    /// # Errors
    ///
    /// As [`ProtoTopologyManager::find_or_create_proto_vertex_pcs`].
    pub fn find_or_create_proto_vertex_crossing(
        &mut self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        pc1: &mut PointClassification<O>,
        pc2: &mut PointClassification<O>,
        crossing: Option<(O, f64)>,
        along: [bool; 2],
    ) -> Result<(Option<ProtoVertexId>, bool)> {
        // Skip PtXSects that don't classify to Region, Face, Edge, or Vertex in both Breps
        if !can_imprint(pc1.point_class()) {
            return Ok((None, false));
        }
        if !can_imprint(pc2.point_class()) {
            return Ok((None, false));
        }

        // Not upstream (#15718): a point on one edge of a coincident line edge pair in each
        // B-rep is where two coincident pieces cross within their tolerance, no event
        if pc1.point_class() == PointClassType::Edge && pc2.point_class() == PointClassType::Edge {
            if let (Some(e1), Some(e2)) = (pc1.object(), pc2.object()) {
                if self.are_coincident_edges(e1, e2) {
                    return Ok((None, false));
                }
            }
        }

        // Not upstream: with no curve, two classifications onto edges are the crossing of
        // those two edges, and two coplanar circles a hair less than tangent cross nowhere
        // but touch at one point (docs/booleans.md §6, "Circles a hair less than tangent"):
        // a point the surface intersection found where their lens meets the other face, one
        // of the two crossings `2√(2δ r1 r2 / (r1 ± r2))` apart, is that touching point.
        if crossing.is_none() {
            touch_of_edges(k, geometry, pc1, pc2)?;
        }

        // sm_FindPositionFromPCs, status ignored; the position stays SM_UNDEF on failure
        // (passed on with `ssi_proto_topology_statuses_pass_on`, fork bcc6f01)
        let pass = k.fixes.ssi_proto_topology_statuses_pass_on;

        let found_position = ignore_soft(passed_on(
            find_position_from_pcs(k, geometry, pc1, pc2),
            pass,
        )?)?;
        // Not upstream: a proto vertex stands for one point, so it never relates two
        // vertices farther apart than their intersection tolerance (the bound upstream
        // only reports). Each classification lies within its own band of the curve's end,
        // so a crossing midway between two corners 2.8e-5 apart classifies onto both with
        // a band of 2e-5; relating them would merge two vertices that are not one. The
        // pair is not a common point: no proto vertex, as for a pair that cannot be
        // imprinted, and each body keeps its own vertex. Only vertices: their points are
        // fixed, so their distance is measured here whatever parameters the
        // classifications carry (`sm_FindPositionFromPCs`' deviation is 0 when one had
        // none at the start; it keeps upstream's meaning for the tolerances it feeds).
        // An edge's or a face's point is that of its parameters, which need not be
        // settled yet (overlapping edges are re-parametrised later by
        // `ResolveOverlappingProtoEdges`: a pair 18 mm apart there is one point), so
        // their distance proves no separation.
        if pc1.point_class() == PointClassType::Vertex
            && pc2.point_class() == PointClassType::Vertex
        {
            let tol = xsect_tol_3d_from_zones(
                k,
                pc1.obj_zone_tol_3d(k, geometry)?,
                pc2.obj_zone_tol_3d(k, geometry)?,
            );
            // (a vertex's point needs no parameters: copies marked as set read it even
            // when a caller unset them)
            let vertex_point = |pc: &PointClassification<O>| {
                let mut v = *pc;
                v.set_class_object(PointClassType::Vertex, pc.object());
                v.find_obj_point_3d(k, geometry)
            };
            let apart = vertex_point(pc1)?.distance(vertex_point(pc2)?);
            if apart.is_nan() || apart > tol {
                return Ok((None, false));
            }
        }
        let position = found_position.map_or(Vec3::UNDEF, |p| p.position);

        // The listed proto vertex that holds the point, the closest of them: not upstream,
        // one the point is not proved apart from (within the combined tolerance of the two
        // points, or their uncertainty regions: `find_proto_vertex_within`), where upstream
        // takes any whose ball (the capture distance) contains it; or (not upstream, #17595;
        // see the module's Deviations) one that stands for the same crossing of an edge with a
        // face.
        let deviation = found_position.map_or(f64::NAN, |p| p.deviation);
        let point_tol = self.proto_vertex_tolerance_of_pcs(k, geometry, deviation, pc1, pc2)?;
        let point_tol = zone_tol3d(k, point_tol);
        let limit = crate::query::face::p2::proto_vertex_capture_distance(self);
        let region = crossing_region(
            k, geometry, crossing, along, pc1, pc2, position, point_tol, limit,
        )?;
        let within =
            self.find_proto_vertex_within_if(k, position, point_tol, Some(&region), |id| {
                self.proto_vertex_incidence_conflicts(k, geometry, id, [pc1, pc2])
                    .map(|conflict| !conflict)
            })?;
        let found = self.find_proto_vertex_holding(k, geometry, position, pc1, pc2, within)?;
        // Not upstream: a point classified on a vertex is that vertex's point, whatever the
        // position found for the pair (the average of the two objects' points), so the proto
        // vertex also holds the ball of each vertex's own point: a later point on the vertex
        // (within the coincidence rule of it) is at the proto vertex, as the vertex says, though
        // it may lie farther from the average (docs/booleans.md, "One point or two").
        let mut held = Vec::new();
        for pc in [&*pc1, &*pc2] {
            if pc.point_class() == PointClassType::Vertex {
                let mut v = *pc;
                v.set_class_object(PointClassType::Vertex, pc.object());
                if let Some(at) = ignore_soft(v.find_obj_point_3d(k, geometry))? {
                    held.push(DeclaredContact::of_point(at, point_tol));
                }
            }
        }

        // when nearby PV exists
        if let Some(pv) = found {
            self.note_region(pv, region);
            for r in held {
                self.note_region(pv, r);
            }
            // AddVertexDefinition(rPC1, rPC2, pNewVD): status ignored (passed on with the
            // flag)
            ignore_soft(passed_on(
                self.add_vertex_definition_from_pcs(k, geometry, pv, pc1, pc2),
                pass,
            )?)?;
            return Ok((Some(pv), false));
        }

        // a new ProtoVertex has to be created
        let created = self.create_proto_vertex(k, geometry, pc1, pc2)?;
        if let Some(pv) = created {
            self.note_region(pv, region);
            for r in held {
                self.note_region(pv, r);
            }
        }
        Ok((created, created.is_some()))
    }

    /// `SmProtoTopologyManager::FindOrCreateProtoEdge(rCI1, rCI2, rpNewPE,
    /// pbWasCreated)`: the proto edge of a face pair's curve interval (found or
    /// created) and whether it was created. `classification1` is the classification
    /// `ci1` belongs to (its curve is the one the proto edge is made of), `classification2`
    /// that of `ci2`. The status of
    /// the non-degenerate path is always `SM_SUCCESS`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a missing curve; those of `IsDegenerate` and
    /// `FindOrCreateProtoVertex`; [`SolidError::Unported`] for the callees listed in
    /// the module header.
    pub fn find_or_create_proto_edge(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        ti_approx_tol_3d: f64,
        classification1: &CurveClassification<O>,
        ci1: &mut CurveInterval<O>,
        classification2: &CurveClassification<O>,
        ci2: &mut CurveInterval<O>,
    ) -> Result<(Option<ProtoEdgeId>, bool)> {
        let cc_curve = classification1.curve();
        let this_ivl = ci1.interval;
        let tol = ti_approx_tol_3d;

        // DD3: Check for overlapping PEs; the status is ignored (passed on with
        // `ssi_proto_topology_statuses_pass_on`, fork bcc6f01).
        let pass = k.fixes.ssi_proto_topology_statuses_pass_on;
        let overlapping: Vec<Option<ProtoEdgeId>> = ignore_soft(passed_on(
            self.check_overlapping_proto_edges(k, geometry, classification1, ci1, ci2),
            pass,
        )?)?
        .unwrap_or_default();

        // when CrvInterval is degenerate - find or create a ProtoVertex for Interval
        if curve_of(geometry, cc_curve)?.is_degenerate(k, tol, None)? {
            let start =
                can_imprint(ci1.start.point_class()) && can_imprint(ci2.start.point_class());
            let mid = can_imprint(ci1.mid.point_class()) && can_imprint(ci2.mid.point_class());
            let end = can_imprint(ci1.end.point_class()) && can_imprint(ci2.end.point_class());
            if start {
                self.find_or_create_proto_vertex_pcs(k, geometry, &mut ci1.start, &mut ci2.start)?;
            } else if mid {
                self.find_or_create_proto_vertex_pcs(k, geometry, &mut ci1.mid, &mut ci2.mid)?;
            } else if end {
                self.find_or_create_proto_vertex_pcs(k, geometry, &mut ci1.end, &mut ci2.end)?;
            }
            return Ok((None, false));
        }

        // NonDegen CrvIntervals: find or create PVs for the ends (statuses ignored)
        let mut start_pv = None;
        let mut end_pv = None;
        if can_imprint(ci1.start.point_class()) && can_imprint(ci2.start.point_class()) {
            // (the curve the interval lies on, and its parameter at this end)
            let crossing = classification1.curve().map(|c| (c, ci1.interval.min()));
            let along = [
                runs_along_seam(geometry, &ci1.start, &ci1.mid, &ci2.mid)?,
                runs_along_seam(geometry, &ci2.start, &ci2.mid, &ci1.mid)?,
            ];
            start_pv = ignore_soft(self.find_or_create_proto_vertex_crossing(
                k,
                geometry,
                &mut ci1.start,
                &mut ci2.start,
                crossing,
                along,
            ))?
            .and_then(|r| r.0);
            // not upstream (WR-410, #1059): a later crossing with a smaller gap over sine
            // moves the vertex only where certified (`add_crossing`)
            if let Some(pv) = start_pv {
                let sine = crossing_sine(
                    k,
                    &*geometry,
                    classification1,
                    ci1,
                    classification2,
                    ci2,
                    false,
                )?;
                self.add_crossing(k, &*geometry, pv, &ci1.start, &ci2.start, sine, crossing)?;
            }
        }
        if can_imprint(ci1.end.point_class()) && can_imprint(ci2.end.point_class()) {
            // (the curve the interval lies on, and its parameter at this end)
            let crossing = classification1.curve().map(|c| (c, ci1.interval.max()));
            let along = [
                runs_along_seam(geometry, &ci1.end, &ci1.mid, &ci2.mid)?,
                runs_along_seam(geometry, &ci2.end, &ci2.mid, &ci1.mid)?,
            ];
            end_pv = ignore_soft(self.find_or_create_proto_vertex_crossing(
                k,
                geometry,
                &mut ci1.end,
                &mut ci2.end,
                crossing,
                along,
            ))?
            .and_then(|r| r.0);
            // not upstream (WR-410, #1059): a later crossing with a smaller gap over sine
            // moves the vertex only where certified (`add_crossing`)
            if let Some(pv) = end_pv {
                let sine = crossing_sine(
                    k,
                    &*geometry,
                    classification1,
                    ci1,
                    classification2,
                    ci2,
                    true,
                )?;
                self.add_crossing(k, &*geometry, pv, &ci1.end, &ci2.end, sine, crossing)?;
            }
        }

        // Can't create a ProtoEdge without both PVs.
        let (Some(start_pv), Some(end_pv)) = (start_pv, end_pv) else {
            return Ok((None, false));
        };

        // quit - IntervalMid does not classify to topology in both SrcBreps
        if !can_imprint(ci1.mid.point_class()) || !can_imprint(ci2.mid.point_class()) {
            return Ok((None, false));
        }

        // quit - SrcFaces intersect at a single Vertex
        if ci1.mid.point_class() == PointClassType::Vertex
            || ci2.mid.point_class() == PointClassType::Vertex
        {
            return Ok((None, false));
        }

        // quit - degenerate interval. [B488]
        if curve_of(geometry, cc_curve)?.is_degenerate(k, tol, Some(&this_ivl))? {
            return Ok((None, false));
        }

        // See whether a ProtoEdge already exists in this location.
        let (found, is_identical) = self.find_proto_edge(
            k,
            &*geometry,
            Some(start_pv),
            Some(end_pv),
            &ci1.mid,
            &ci2.mid,
            cc_curve,
            &this_ivl,
        )?;
        let mut was_created = false;
        let mut new_pe = found;
        if new_pe.is_none() {
            // create new Protoedge and EdgeDefinition
            // The status is kept, not propagated: upstream returns SM_SUCCESS with the
            // out-parameter whatever it was (`Unported` still propagates).
            let created = ignore_soft(self.create_proto_edge(
                k,
                &mut *geometry,
                Some(start_pv),
                Some(end_pv),
                classification1,
                ci1,
                classification2,
                ci2,
            ))?;
            if let Some(pe) = created {
                was_created = true;
                let mut ids = Vec::with_capacity(overlapping.len());
                for o in &overlapping {
                    ids.push(o.ok_or(dangling("overlapping proto edge is null"))?);
                }
                self.proto_edge_mut(pe)
                    .ok_or(dangling("proto edge is deleted"))?
                    .append_overlapping_pes(&ids);
                for o in ids {
                    self.proto_edge_mut(o)
                        .ok_or(dangling("proto edge is deleted"))?
                        .add_overlapping_pe(pe);
                }
            }
            new_pe = created;
        } else if !is_identical {
            // Create new EdgeDef and add it to NewPE's EdgeDef list
            if let Some(pe) = new_pe {
                // (status ignored upstream, passed on with the flag)
                ignore_soft(passed_on(
                    self.add_edge_definition_from_intervals(
                        k,
                        &mut *geometry,
                        pe,
                        classification1,
                        ci1,
                        classification2,
                        ci2,
                    ),
                    pass,
                )?)?;
            }
        }
        Ok((new_pe, was_created))
    }

    /// `SmEdgeDefinition::Split(dParamPE, pMidPV, pTopPE, rpNewVD, rpNewED)`: splits
    /// the definition at `param_pe` after its proto edge was split. Returns the new
    /// vertex definition (added to `mid_pv`) and the new upper definition (added to
    /// `top_pe`); `(None, None)` when the parameter is not interior. This definition
    /// becomes the lower part.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted object or a missing curve;
    /// `SM_ERR_NULL_POINTER` for the `NER` checks; those of the end positions and of
    /// `SmCurve::Copy`; [`SolidError::Unported`] for the callees listed in the module
    /// header.
    pub fn edge_definition_split(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        param_pe: f64,
        mid_pv: Option<ProtoVertexId>,
        top_pe: Option<ProtoEdgeId>,
    ) -> Result<(Option<VertexDefId>, Option<EdgeDefId>)> {
        let Some(crv_pt) = self.edge_definition_split_point(k, &*geometry, ed, param_pe)? else {
            return Ok((None, None));
        };
        let tol = self
            .edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))?
            .raw_tolerance();

        // Ok, go ahead and split. Need to classify point.
        let brep1 = self.which_brep(1);
        let brep2 = self.which_brep(2);
        let mut pc1 = PointClassification::new(zone_tol3d(k, tol));
        let mut pc2 = PointClassification::new(zone_tol3d(k, tol));
        // (statuses ignored: a failed classification keeps its cleared state)
        let tol_3d = zone_tol3d(k, tol);
        if let Some(pc) = ignore_soft(passed_on(
            geometry.brep_point_3d_classify(k, brep1, crv_pt, tol_3d, true),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)? {
            pc1 = pc;
        }
        if let Some(pc) = ignore_soft(passed_on(
            geometry.brep_point_3d_classify(k, brep2, crv_pt, tol_3d, true),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)? {
            pc2 = pc;
        }

        let mid_pv = mid_pv.ok_or(dangling("mid proto vertex is null"))?;
        let (new_vd, created) = ignore_soft(passed_on(
            self.add_vertex_definition_from_pcs(k, &*geometry, mid_pv, &mut pc1, &mut pc2),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?
        .map_or((None, false), |(vd, created)| (Some(vd), created));
        let new_vd = new_vd.ok_or_else(|| {
            SolidError::topology_missing(OP_ED_SPLIT, "the vertex definition at the split")
        })?;

        // For cleanup object: don't do it if it was 'found' (vs. created).
        let clean_vd = created.then_some(new_vd);

        let new_ed = match self.copy_edge_definition(ed) {
            Ok(x) => x,
            Err(e) => {
                if let Some(vd) = clean_vd {
                    self.delete_vertex_definition(vd);
                }
                return Err(e);
            }
        };
        // The split's new EdgeDefinition starts unowned (tangency-labs/solidcore#518).
        self.edge_def_mut(new_ed)
            .ok_or(dangling("edge definition is deleted"))?
            .set_proto_edge(None);

        // Get the parameterizations set correctly before calling AddEdgeDefinition().
        let mut this_ivl = self
            .edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))?
            .xsect_interval();
        ignore_soft(passed_on(
            this_ivl.set_max(param_pe),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?;
        self.set_ed_interval(ed, this_ivl)?;
        let mut new_ivl = self
            .edge_def(new_ed)
            .ok_or(dangling("edge definition is deleted"))?
            .xsect_interval();
        ignore_soft(passed_on(
            new_ivl.set_min(param_pe),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?;
        self.set_ed_interval(new_ed, new_ivl)?;

        let top = top_pe.ok_or(dangling("top proto edge is null"))?;

        // The new definition (a copy holding `ed`'s curves) gets copies of its own, and both
        // are trimmed to their intervals, before `AddEdgeDefinition`: it reparameterizes the
        // definition onto the upper proto edge where their intervals or directions differ.
        // Upstream copies and trims after that call, so the shared curve was moved too, the
        // lower definition's trim missed it, and the whole curve, not the upper part, was
        // mapped onto the upper proto edge.
        self.copy_ed_curves(k, geometry, ed, new_ed)?;
        for which in [0, 1, 2] {
            for d in [ed, new_ed] {
                let curve = self.ed_curve(d, which)?;
                if curve.is_some() {
                    self.trim_ed_curve(k, geometry, d, curve)?;
                }
            }
        }
        ignore_soft(passed_on(
            self.add_edge_definition(k, &mut *geometry, top, new_ed),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?;
        Ok((Some(new_vd), Some(new_ed)))
    }

    /// Where [`ProtoTopologyManager::edge_definition_split`] splits `ed` at `param_pe`:
    /// the definition's point there, or `None` when the parameter is not interior to the
    /// definition or the point is within ten times its tolerance (`cbiFudge`) of either of
    /// its ends, measured to the definition's own ends rather than to the vertex
    /// definitions.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted definition, a missing proto edge or
    /// curve; those of the end positions.
    fn edge_definition_split_point(
        &self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        ed: EdgeDefId,
        param_pe: f64,
    ) -> Result<Option<Vec3>> {
        /// `cbiFudge`.
        const CBI_FUDGE: f64 = 10.0;

        let d = self
            .edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))?;

        // Check that parameter is interior to this EdgeDef.
        if !d.xsect_interval().contains_value(param_pe, -EFF_ZERO) {
            return Ok(None);
        }
        let tol = d.raw_tolerance();

        // Takes reversal into account; status ignored.
        let crv_pt = ignore_soft(passed_on(
            d.evaluate(k, geometry, param_pe, true, false),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?
        .and_then(|r| r.0)
        .unwrap_or(Vec3::UNDEF);

        // Use distance to end of this EdgeDef, not distance to our VertexDefs.
        let pe = d
            .proto_edge()
            .and_then(|pe| self.proto_edge(pe))
            .ok_or(dangling("edge definition has no proto edge"))?;
        for end_pv in [pe.start_proto_vertex(), pe.end_proto_vertex()] {
            let pt = self.edge_def_end_position_at(k, geometry, ed, end_pv)?;
            if crv_pt.distance(pt) < tol * CBI_FUDGE {
                return Ok(None);
            }
        }
        Ok(Some(crv_pt))
    }

    /// Sets an edge definition's interval.
    fn set_ed_interval(&mut self, ed: EdgeDefId, ivl: Extent1d) -> Result<()> {
        self.edge_def_mut(ed)
            .ok_or(dangling("edge definition is deleted"))?
            .set_xsect_interval(ivl);
        Ok(())
    }

    /// Sets an edge definition's 3D curve.
    fn set_ed_curve_3d(&mut self, ed: EdgeDefId, curve: Option<O>) -> Result<()> {
        self.edge_def_mut(ed)
            .ok_or(dangling("edge definition is deleted"))?
            .set_xsect_curve_3d(curve);
        Ok(())
    }

    /// An edge definition's 3D curve (`which` 0) or UV curve `which` (1, 2).
    fn ed_curve(&self, ed: EdgeDefId, which: i32) -> Result<Option<O>> {
        let d = self
            .edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))?;
        Ok(if which == 0 {
            d.xsect_curve_3d()
        } else {
            d.xsect_uv_curve(which)
        })
    }

    /// Gives `new_ed`, a copy of `ed` that holds the same curves, copies of `ed`'s 3D curve
    /// and of each UV curve it has, so that editing one definition's curves leaves the
    /// other's as they are.
    fn copy_ed_curves(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        new_ed: EdgeDefId,
    ) -> Result<()> {
        let d = self
            .edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))?;
        let c3 = d.xsect_curve_3d();
        let uvs = [d.xsect_uv_curve(1), d.xsect_uv_curve(2)];
        let copy = curve_of(&*geometry, c3)?.copy(k)?;
        let handle = geometry.insert_curve(copy).ok_or_else(|| {
            SolidError::topology_missing(OP_ED_SPLIT, "the copied 3D curve's handle")
        })?;
        self.set_ed_curve_3d(new_ed, Some(handle))?;
        for (which, uv) in [1, 2].into_iter().zip(uvs) {
            if uv.is_some() {
                let copy = curve_of(&*geometry, uv)?.copy(k)?;
                let handle = geometry.insert_curve(copy).ok_or_else(|| {
                    SolidError::topology_missing(OP_ED_SPLIT, "the copied UV curve's handle")
                })?;
                self.edge_def_mut(new_ed)
                    .ok_or(dangling("edge definition is deleted"))?
                    .set_xsect_curve_uv(which, Some(handle));
            }
        }
        Ok(())
    }

    /// `curve->Trim(pED->m_pXSectInterval)`: the status is ignored and the interval
    /// may be snapped (it is passed by reference upstream).
    fn trim_ed_curve(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        curve: Option<O>,
    ) -> Result<()> {
        let mut ivl = self
            .edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))?
            .xsect_interval();
        let crv = curve
            .and_then(|c| geometry.curve_mut(c))
            .ok_or(dangling("curve handle is null or unknown"))?;
        ignore_soft(passed_on(
            crv.trim(k, &mut ivl, TrimMode::Notify),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?;
        self.set_ed_interval(ed, ivl)
    }

    /// Where [`Self::proto_edge_split`] splits `pe` at `dt`: the point, or `None` when it
    /// does no work there (`dt` is not interior, is within the proto edge's tolerance of
    /// an end, or (not upstream) no edge definition of `pe` splits there).
    fn proto_edge_split_point(
        &self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        dt: f64,
    ) -> Result<Option<Vec3>> {
        let e = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?;

        // no work - dT is not interior to this ProtoEdge.
        if !e.xsect_interval().contains_value(dt, -EFF_ZERO) {
            return Ok(None);
        }
        let tol = e.raw_tolerance();

        let crv_pt = ignore_soft(passed_on(
            self.proto_edge_evaluate(k, geometry, pe, dt, true, false),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?
        .and_then(|r| r.0)
        .unwrap_or(Vec3::UNDEF);
        let start_pt = self.proto_edge_end_position(k, geometry, pe, true)?;
        let end_pt = self.proto_edge_end_position(k, geometry, pe, false)?;

        // no work - Split Pt is within tol of ProtoEdge end point
        if crv_pt.distance(start_pt) < tol {
            return Ok(None);
        }
        if crv_pt.distance(end_pt) < tol {
            return Ok(None);
        }

        // Not upstream: no work either when none of our EdgeDefs splits at dT. A definition
        // refuses a point within ten times its tolerance of its ends, while the test above
        // allows one; a point between the two used to split this ProtoEdge but none of its
        // definitions, leaving an upper child without definitions that the boolean's
        // pairwise split then stops on ("a proto edge has no edge definitions"). Two
        // near-tangent arcs, such as a sketch's tangent corner arcs a few round-offs
        // apart, give such points.
        let eds: Vec<EdgeDefId> = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .edge_definitions()
            .to_vec();
        let mut any_splits = false;
        for &ed in &eds {
            if self
                .edge_definition_split_point(k, geometry, ed, dt)?
                .is_some()
            {
                any_splits = true;
                break;
            }
        }
        if !any_splits {
            return Ok(None);
        }
        Ok(Some(crv_pt))
    }

    /// `SmProtoEdge::Split(dT, rpNewPV, rpNewPE)`: splits the proto edge at `dt` into
    /// a lower part (this proto edge) and a new upper part, with a proto vertex at the
    /// split point, splitting every edge definition as well. Returns the new vertex
    /// and edge; `(None, None)` when `dt` is not interior, and (not upstream) when no
    /// edge definition splits there, so a split never leaves a proto edge without
    /// definitions.
    ///
    /// The upper child keeps this edge's old interval maximum (`[dt, old_max]`) and the
    /// lower child is `[old_min, dt]` (WR-101; upstream's upper child is `[dt, dt]`,
    /// because it reads this edge's maximum after clamping it to `dt`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted object; `SM_ERR_NULL_POINTER` when no
    /// vertex is found or created; those of the end positions;
    /// [`SolidError::Unported`] for the callees listed in the module header.
    pub fn proto_edge_split(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        dt: f64,
    ) -> Result<(Option<ProtoVertexId>, Option<ProtoEdgeId>)> {
        let Some(crv_pt) = self.proto_edge_split_point(k, &*geometry, pe, dt)? else {
            return Ok((None, None));
        };

        // Split ourself first, then all of our EdgeDefs. The status is ignored.
        let (new_pv, created) = self.find_or_create_proto_vertex(k, crv_pt)?;

        // create new upper Split child ProtoEdge
        let new_pe = match self.copy_proto_edge(pe) {
            Ok(x) => x,
            Err(e) => {
                // the cleanup object deletes a created vertex
                if created {
                    self.delete_proto_vertex_object(new_pv);
                }
                return Err(e);
            }
        };

        // set Split child ProtoEdge intervals
        let mut lower = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .xsect_interval();
        // WR-101: the upper child keeps the old maximum (`[dT, old_max]`); upstream reads
        // this maximum after clamping it, giving the upper child `[dT, dT]`.
        let old_max = lower.max();
        ignore_soft(passed_on(
            lower.set_min_max(lower.min(), dt),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?;
        self.proto_edge_mut(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .set_xsect_interval(lower);
        let mut upper = self
            .proto_edge(new_pe)
            .ok_or(dangling("proto edge is deleted"))?
            .xsect_interval();
        ignore_soft(passed_on(
            upper.set_min_max(dt, old_max),
            k.fixes.ssi_proto_topology_statuses_pass_on,
        )?)?;
        self.proto_edge_mut(new_pe)
            .ok_or(dangling("proto edge is deleted"))?
            .set_xsect_interval(upper);

        // add split child ProtoEdges to pNewPV connected ProtoEdge list
        {
            let v = self
                .proto_vertex_mut(new_pv)
                .ok_or(dangling("proto vertex is deleted"))?;
            v.add_proto_edge(pe);
            v.add_proto_edge(new_pe);
        }

        // set split child ProtoEdge End ProtoVertices
        let old_end = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .end_proto_vertex();
        {
            let n = self
                .proto_edge_mut(new_pe)
                .ok_or(dangling("proto edge is deleted"))?;
            n.set_end_proto_vertex(old_end);
            n.set_start_proto_vertex(Some(new_pv));
        }
        self.proto_edge_mut(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .set_end_proto_vertex(Some(new_pv));

        // edit upper split child -> EndProtoVertex -> ProtoEdge list
        let end = old_end.ok_or(dangling("proto edge has no end proto vertex"))?;
        {
            let v = self
                .proto_vertex_mut(end)
                .ok_or(dangling("proto vertex is deleted"))?;
            v.add_proto_edge(new_pe);
            ignore_soft(passed_on(
                v.remove_proto_edge(pe),
                k.fixes.ssi_proto_topology_statuses_pass_on,
            )?)?;
        }

        // Add the new ProtoEdge to the ProtoTopoMgr ProtoTopo list
        self.add_proto_edge(new_pe);

        // Now split all of the original ProtoEdge->EdgeDefs.
        let eds: Vec<EdgeDefId> = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .edge_definitions()
            .to_vec();

        // clear new UpperSplitChild->EdgeDefinitions
        self.proto_edge_mut(new_pe)
            .ok_or(dangling("proto edge is deleted"))?
            .clear_edge_definitions();

        for ed in eds {
            ignore_soft(passed_on(
                self.edge_definition_split(k, geometry, ed, dt, Some(new_pv), Some(new_pe)),
                k.fixes.ssi_proto_topology_statuses_pass_on,
            )?)?;
        }

        // relate newPE to this_PE->CoincPFs
        let pfs: Vec<ProtoFaceId> = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .coinc_proto_faces()
            .to_vec();
        for pf in pfs {
            self.relate_coincident_pf(new_pe, pf)?;
        }

        Ok((Some(new_pv), Some(new_pe)))
    }

    /// `SmProtoTopologyManager::SplitIntersectingProtoEdges(pPE1, pPE2)`: splits two
    /// proto edges (and their definitions) at their intersections. Returns the curves
    /// deleted.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when a proto edge has no definition; [`SolidError::InvalidInput`] for a
    /// deleted object or missing curve; [`SolidError::Unported`] for the callees
    /// listed in the module header.
    pub fn split_intersecting_proto_edges(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        pe1: ProtoEdgeId,
        pe2: ProtoEdgeId,
    ) -> Result<Vec<O>> {
        let mut deleted: Vec<O> = Vec::new();
        // fork bcc6f01 (`SplitIntersectingProtoEdges`): the splits, combines and
        // removals pass on their statuses.
        let pass = k.fixes.ssi_proto_topology_statuses_pass_on;
        let e1 = self
            .proto_edge(pe1)
            .ok_or(dangling("proto edge is deleted"))?;
        let e2 = self
            .proto_edge(pe2)
            .ok_or(dangling("proto edge is deleted"))?;

        // no work - ProtoEdges already meet at a ProtoVertex.
        if e1.start_proto_vertex() == e2.start_proto_vertex()
            || e1.start_proto_vertex() == e2.end_proto_vertex()
            || e1.end_proto_vertex() == e2.start_proto_vertex()
            || e1.end_proto_vertex() == e2.end_proto_vertex()
        {
            return Ok(deleted);
        }

        // no work - ProtoEdge does not have any EdgeDefinitions
        let (Some(&first1), Some(&first2)) =
            (e1.edge_definitions().first(), e2.edge_definitions().first())
        else {
            return Err(SolidError::topology_failed(
                OP_SPLIT_INT,
                "a proto edge has no edge definitions",
            )); // [Reg_091215_simon_closed]
        };

        // no work - Skip the pair if they're known to overlap. Handled during Resolve
        if e1.overlapping_pes().contains(&pe2) {
            return Ok(deleted);
        }

        // the curves of the first EdgeDefs
        let crv1 = self
            .edge_def(first1)
            .ok_or(dangling("edge definition is deleted"))?
            .xsect_curve_3d();
        let crv2 = self
            .edge_def(first2)
            .ok_or(dangling("edge definition is deleted"))?
            .xsect_curve_3d();
        let dom1 = e1.xsect_interval();
        let dom2 = e2.xsect_interval();
        let tol = self.raw_tolerance();

        // XSect ProtoEdge curves
        let c1 = curve_of(&*geometry, crv1)?;
        let c2 = curve_of(&*geometry, crv2)?;
        // (not upstream, #14329) Two lines go to `IntersectWithLine`, whose first test
        // returns no solution when the segments' boxes are apart: answer that here, before
        // the intersection's setup. A boolean asks it for every pair of proto edges, so
        // with long outlines nearly every pair ends here.
        if let (CurveKind::Line(l1), CurveKind::Line(l2)) = (c1.kind(), c2.kind()) {
            if l1.segments_apart(&dom1, l2, &dom2, tol) {
                return Ok(deleted);
            }
        }
        k.work_counters.add_proto_edge_intersection();
        let sols = match c1.global_curve_intersect::<CurveHandle>(
            k,
            &dom1,
            c2,
            &dom2,
            tol,
            &CurveIntersectOptions::default(),
        ) {
            Ok(s) => s,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(deleted),
        };
        if sols.is_empty() {
            return Ok(deleted);
        }

        // Found an intersection. If it's interior to either ProtoEdge, split it.
        for sol in sols.as_slice() {
            // Point intersection branch
            if sol.solution_type == SolutionType::SingleValue {
                let t1 = sol.start.param(0)?;
                let t2 = sol.start.param(1)?;

                let iv1 = self
                    .proto_edge(pe1)
                    .ok_or(dangling("proto edge is deleted"))?
                    .xsect_interval();
                if iv1.contains_value(t1, -EFF_ZERO) {
                    ignore_soft(passed_on(
                        self.proto_edge_split(k, geometry, pe1, t1),
                        pass,
                    )?)?;
                }
                let iv2 = self
                    .proto_edge(pe2)
                    .ok_or(dangling("proto edge is deleted"))?
                    .xsect_interval();
                if iv2.contains_value(t2, -EFF_ZERO) {
                    ignore_soft(passed_on(
                        self.proto_edge_split(k, geometry, pe2, t2),
                        pass,
                    )?)?;
                }
            } else if sol.solution_type == SolutionType::RangeOfValues {
                // Check whether the two PEs run in the same direction or opposite.
                let same_dir = sol.start.param(1)? < sol.end.param(1)?;

                // Not upstream: an overlap whose two ends both fall where one end of either
                // proto edge refuses to split (shorter than ten times the definitions'
                // tolerance there) has no piece of that edge to combine. Combining the
                // whole edge would map the other edge's definition onto it; refuse before
                // anything is split or combined.
                let (t11, t12) = (sol.start.param(0)?, sol.end.param(0)?);
                let (t21, t22) = (sol.start.param(1)?, sol.end.param(1)?);
                if self.overlap_collapses_to_an_end(k, &*geometry, pe1, t11, t12)?
                    || self.overlap_collapses_to_an_end(k, &*geometry, pe2, t21, t22)?
                {
                    return Err(SolidError::topology_failed(
                        OP_SPLIT_INT,
                        "two proto edges overlap along less than their definitions can split",
                    ));
                }

                // T1:
                let (npv11, npe11) = ignore_soft(passed_on(
                    self.proto_edge_split(k, geometry, pe1, t11),
                    pass,
                )?)?
                .unwrap_or((None, None));
                let (npv21, npe21) = ignore_soft(passed_on(
                    self.proto_edge_split(k, geometry, pe2, t21),
                    pass,
                )?)?
                .unwrap_or((None, None));

                // Combine PVs:
                let pv1 = self.which_pv_to_combine(k, &*geometry, pe1, t11, npv11)?;
                let pv2 = self.which_pv_to_combine(k, &*geometry, pe2, t21, npv21)?;
                self.combine_and_delete_pvs(pv1, pv2, pass)?;

                // T2: first decide which pieces to split.
                let pe1_to_split = npe11.unwrap_or(pe1);
                let pe2_to_split = if same_dir { npe21.unwrap_or(pe2) } else { pe2 };

                let (npv12, npe12) = ignore_soft(passed_on(
                    self.proto_edge_split(k, geometry, pe1_to_split, t12),
                    pass,
                )?)?
                .unwrap_or((None, None));
                let (npv22, npe22) = ignore_soft(passed_on(
                    self.proto_edge_split(k, geometry, pe2_to_split, t22),
                    pass,
                )?)?
                .unwrap_or((None, None));
                let _ = npe12;

                // Combine PVs:
                let pv1 = self.which_pv_to_combine(k, &*geometry, pe1_to_split, t12, npv12)?;
                let pv2 = self.which_pv_to_combine(k, &*geometry, pe2_to_split, t22, npv22)?;
                self.combine_and_delete_pvs(pv1, pv2, pass)?;

                // Combine ProtoEdges
                let pe1_to_combine = npe11.unwrap_or(pe1);
                let pe2_to_combine = if same_dir {
                    npe21.unwrap_or(pe2)
                } else {
                    npe22.unwrap_or(pe2)
                };
                ignore_soft(passed_on(
                    self.combine_proto_edge(k, &mut *geometry, pe1_to_combine, pe2_to_combine),
                    pass,
                )?)?;

                ignore_soft(passed_on(self.remove_proto_edge(k, pe2_to_combine), pass)?)?;
                self.forget_proto_edge(pe2_to_combine);
                deleted.extend(self.delete_proto_edge_object(pe2_to_combine));
            }
        }
        Ok(deleted)
    }

    /// Whether an overlap from `ta` to `tb` along `pe` has both its ends at one end of
    /// `pe`: neither is a point [`Self::proto_edge_split`] splits at, and both lie nearer
    /// the same end of the proto edge's interval.
    fn overlap_collapses_to_an_end(
        &self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        ta: f64,
        tb: f64,
    ) -> Result<bool> {
        let iv = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .xsect_interval();
        let nearer_start = |t: f64| (t - iv.min()).abs() <= (iv.max() - t).abs();
        if nearer_start(ta) != nearer_start(tb) {
            return Ok(false);
        }
        Ok(self.proto_edge_split_point(k, geometry, pe, ta)?.is_none()
            && self.proto_edge_split_point(k, geometry, pe, tb)?.is_none())
    }

    /// The combine-and-delete step of `SplitIntersectingProtoEdges`: when both proto
    /// vertices exist and differ, combines the second into the first and deletes it.
    fn combine_and_delete_pvs(
        &mut self,
        pv1: Option<ProtoVertexId>,
        pv2: Option<ProtoVertexId>,
        pass: bool,
    ) -> Result<()> {
        if let (Some(a), Some(b)) = (pv1, pv2) {
            if a != b {
                ignore_soft(passed_on(self.combine_proto_vertex(a, b), pass)?)?;
                self.proto_vertex_mut(b)
                    .ok_or(dangling("proto vertex is deleted"))?
                    .clear_vertex_definitions();
                ignore_soft(passed_on(self.remove_proto_vertex(b), pass)?)?;
                self.delete_proto_vertex_object(b);
            }
        }
        Ok(())
    }

    /// `SmProtoTopologyManager::GlueCoincidentBrepVertices()`: glues the final
    /// vertices of the vertex definitions of each proto vertex, squeezing or deleting
    /// the short edges between them, and drops the definitions that referred to what
    /// was deleted. Returns the curves deleted. `mates` is the intersector (`m_pTI`):
    /// its `GetOtherMate` and `RemoveRelationship` drop the relationships of deleted
    /// topology ([`Mates`]).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted or unknown object; the failing status
    /// (`eRet`) of an edge deletion; [`SolidError::Unported`] for the callees listed
    /// in the module header.
    pub fn glue_coincident_brep_vertices(
        &mut self,
        k: &mut Kernel,
        brep: &mut (impl ProtoBrep<O> + BrepGlue<O>),
        mates: &mut dyn Mates<O>,
    ) -> Result<Vec<O>> {
        // statuses SMLib drops, passed on with `ssi_proto_topology_statuses_pass_on`
        // (fork bcc6f01)
        let pass = k.fixes.ssi_proto_topology_statuses_pass_on;
        let mut deleted_curves: Vec<O> = Vec::new();
        let pvs: Vec<ProtoVertexId> = self.proto_vertices().to_vec();

        // We need to check whether these are in any other ProtoVertices, ProtoEdges, or
        // ProtoFaces:
        let mut deleted_edges1: Vec<O> = Vec::new();
        let mut deleted_edges2: Vec<O> = Vec::new();
        let mut ret: Result<()> = Ok(());

        for &pv in &pvs {
            let mut vds: Vec<Option<VertexDefId>> = self
                .proto_vertex(pv)
                .ok_or(dangling("proto vertex is deleted"))?
                .vertex_definitions()
                .iter()
                .map(|&v| Some(v))
                .collect();
            let num_vds = vds.len();
            if num_vds < 2 {
                continue;
            }

            // Glue the first VD to all others.
            let vd1 = vds
                .first()
                .copied()
                .flatten()
                .ok_or(dangling("vertex definition is null"))?;

            // Keep track deleted Vertices and corresponding VD
            let mut deleted_v1: Vec<Option<O>> = vec![None; num_vds];
            let mut deleted_v2: Vec<Option<O>> = vec![None; num_vds];

            for jj in 1..num_vds {
                let vd2 = vds
                    .get(jj)
                    .copied()
                    .flatten()
                    .ok_or(dangling("vertex definition is null"))?;

                // If there is no Edge connecting the two Vertices, then Glue them.
                let final_of = |s: &Self, vd: VertexDefId, which: i32| -> Result<Option<O>> {
                    Ok(s.vertex_def(vd)
                        .ok_or(dangling("vertex definition is deleted"))?
                        .final_vertex(which))
                };
                let in_deleted = |list: &[Option<O>], v: O| list.contains(&Some(v));

                let Some(b1v1) = final_of(self, vd1, 1)? else {
                    continue;
                };
                if in_deleted(&deleted_v1, b1v1) {
                    continue;
                }
                let Some(b1v2) = final_of(self, vd2, 1)? else {
                    continue;
                };
                if in_deleted(&deleted_v1, b1v2) {
                    continue;
                }
                let Some(b2v1) = final_of(self, vd1, 2)? else {
                    continue;
                };
                if in_deleted(&deleted_v2, b2v1) {
                    continue;
                }
                let Some(b2v2) = final_of(self, vd2, 2)? else {
                    continue;
                };
                if in_deleted(&deleted_v2, b2v2) {
                    continue;
                }

                let mut dist1 = BIG_DOUBLE;
                let mut dist2 = BIG_DOUBLE;
                let mut xs1 = xsect_tol3d(k, USE_DEFAULT);
                let mut xs2 = xsect_tol3d(k, USE_DEFAULT);
                let mut was_deleted1 = false;
                let mut was_deleted2 = false;

                if in_deleted(&deleted_v1, b1v2) {
                    self.vertex_def_mut(vd2)
                        .ok_or(dangling("vertex definition is deleted"))?
                        .set_final_vertex(1, None);
                    was_deleted1 = true;
                } else {
                    dist1 = vertex_distance(brep, b1v1, b1v2)?;
                    xs1 = xsect_tol(brep, b1v1, b1v2)?;
                }
                if in_deleted(&deleted_v2, b2v2) {
                    self.vertex_def_mut(vd2)
                        .ok_or(dangling("vertex definition is deleted"))?
                        .set_final_vertex(2, None);
                    was_deleted2 = true;
                } else {
                    dist2 = vertex_distance(brep, b2v1, b2v2)?;
                    xs2 = xsect_tol(brep, b2v1, b2v2)?;
                }

                let edges1 = brep.vertex_common_edges(b1v1, b1v2)?;
                let edges2 = brep.vertex_common_edges(b2v1, b2v2)?;

                // Brep 1 then Brep 2.
                for which in [1, 2] {
                    let (v1, v2, dist, xs, edges, deleted_v, deleted_e) = if which == 1 {
                        (
                            b1v1,
                            b1v2,
                            dist1,
                            xs1,
                            &edges1,
                            &mut deleted_v1,
                            &mut deleted_edges1,
                        )
                    } else {
                        (
                            b2v1,
                            b2v2,
                            dist2,
                            xs2,
                            &edges2,
                            &mut deleted_v2,
                            &mut deleted_edges2,
                        )
                    };
                    let flag = if which == 1 {
                        &mut was_deleted1
                    } else {
                        &mut was_deleted2
                    };
                    if dist > xs {
                        continue;
                    }
                    // Not upstream (#18274): the vertex that stays is
                    // the first definition's, unless only the other is the operand's own
                    // vertex at the end of a seam edge (its definition found it on that
                    // vertex): a seam's vertex keeps its place on the seam
                    // (`resolve_vertex_definition`), and the edges of the one squeezed or
                    // glued away come to it.
                    let on_seam = |s: &Self, vd: VertexDefId, v: O| -> Result<bool> {
                        let def = s
                            .vertex_def(vd)
                            .ok_or(dangling("vertex definition is deleted"))?;
                        Ok(
                            def.brep_point_classif(which).point_class() == PointClassType::Vertex
                                && ends_seam(&*brep, v)?,
                        )
                    };
                    let keep_second = on_seam(self, vd2, v2)? && !on_seam(self, vd1, v1)?;
                    let (keep, gone) = if keep_second { (v2, v1) } else { (v1, v2) };
                    let merge_away = |s: &mut Self, deleted_v: &mut [Option<O>]| -> Result<()> {
                        set_slot(deleted_v, jj, gone);
                        if keep_second {
                            s.vertex_def_mut(vd1)
                                .ok_or(dangling("vertex definition is deleted"))?
                                .set_final_vertex(which, Some(keep));
                        }
                        s.vertex_def_mut(vd2)
                            .ok_or(dangling("vertex definition is deleted"))?
                            .set_final_vertex(which, None);
                        Ok(())
                    };
                    if edges.is_empty() {
                        if v1 != v2 {
                            ignore_soft(passed_on(
                                brep.glue_vertices(k, self.which_brep(which), keep, gone, None),
                                pass,
                            )?)?;
                            merge_away(self, deleted_v)?;
                            *flag = true;
                        }
                    } else {
                        for &edge in edges {
                            // UB-241 (SolidCraft fix, tangency-labs/smlib da8d05e): stop
                            // before the GlueVertices path; the delete branch runs as
                            // upstream.
                            let merged = deleted_v.get(jj).copied().flatten() == Some(gone);
                            let curve_handle =
                                brep.edge_curve(edge).ok_or(dangling("edge is unknown"))?;
                            let ivl = brep
                                .edge_interval(edge)
                                .ok_or(dangling("edge is unknown"))?;
                            let len = curve_of(brep, curve_handle)?
                                .approximate_length(k, &ivl, 5, false)?
                                .0;
                            if len < xs {
                                if merged && brep.edge_other_vertex(edge, Some(keep))? != Some(keep)
                                {
                                    break;
                                }
                                // If any to-be-deleted topology is mated, remove the
                                // relationship when done.
                                let edge_mate = mates
                                    .other_mate(edge)
                                    .filter(|m| m.class_kind() == ClassObjectKind::Edge);
                                let vertex_mate = mates
                                    .other_mate(gone)
                                    .filter(|m| m.class_kind() == ClassObjectKind::Vertex);
                                let other_vtx = brep.edge_other_vertex(edge, Some(keep))?;
                                let stat = if other_vtx == Some(keep) {
                                    // Can't call SqueezeEdge() in this case.
                                    brep.delete_edge(k, self.which_brep(which), edge)
                                } else {
                                    // deletes `gone`
                                    let r = brep.brep_squeeze_edge(
                                        k,
                                        self.which_brep(which),
                                        edge,
                                        keep,
                                    );
                                    merge_away(self, deleted_v)?;
                                    r
                                };
                                deleted_e.push(edge);
                                match stat {
                                    Ok(()) => {
                                        *flag = true;
                                        if let Some(m) = edge_mate {
                                            ignore_soft(passed_on(
                                                mates.remove_relationship(edge, m),
                                                pass,
                                            )?)?;
                                        }
                                        if let Some(m) = vertex_mate {
                                            ignore_soft(passed_on(
                                                mates.remove_relationship(gone, m),
                                                pass,
                                            )?)?;
                                        }
                                        let found =
                                            self.find_edge_definition_from_edge(Some(edge), which)?;
                                        if let Some(ed) = found {
                                            let d = self
                                                .edge_def_mut(ed)
                                                .ok_or(dangling("edge definition is deleted"))?;
                                            if which == 1 {
                                                // the curve may have been deleted [B673]
                                                if curve_handle == d.xsect_curve_3d() {
                                                    d.set_xsect_curve_3d(None);
                                                }
                                            } else {
                                                // IntCurve3d is a Brep1 object [B673]
                                                d.set_xsect_curve_3d(None);
                                            }
                                            d.set_xsect_curve_uv(1, None);
                                            d.set_xsect_curve_uv(2, None);
                                            // Don't remove the PE if empty.
                                            ignore_soft(passed_on(
                                                self.remove_edge_definition(k, ed, false),
                                                pass,
                                            )?)?;
                                            deleted_curves.extend(
                                                self.edge_def_mut(ed)
                                                    .ok_or(dangling("edge definition is deleted"))?
                                                    .destruct(),
                                            );
                                            deleted_curves.extend(self.delete_edge_definition(ed));
                                        }
                                    }
                                    Err(e) if e.is_unported_or_cancelled() => return Err(e),
                                    Err(e) => ret = Err(e),
                                }
                            } else {
                                // UB-241: stop before GlueVertices
                                if merged {
                                    break;
                                }
                                // coincident Vertices connected by a long Edge: Glue.
                                if v1 != v2 {
                                    ignore_soft(passed_on(
                                        brep.glue_vertices(
                                            k,
                                            self.which_brep(which),
                                            keep,
                                            gone,
                                            None,
                                        ),
                                        pass,
                                    )?)?;
                                    merge_away(self, deleted_v)?;
                                    *flag = true;
                                }
                            }
                        }
                    }
                }

                if was_deleted1 || was_deleted2 {
                    ignore_soft(passed_on(
                        self.proto_vertex_mut(pv)
                            .ok_or(dangling("proto vertex is deleted"))?
                            .remove_vertex_definition(vd2),
                        pass,
                    )?)?;
                    if let Some(slot) = vds.get_mut(jj) {
                        *slot = None;
                    }
                    self.delete_vertex_definition(vd2);
                }
            }

            // Check for stale Brep topology: we might have deleted some Final SmVerts
            // that are pointed to by other PVs.
            let del1: Vec<O> = deleted_v1.iter().flatten().copied().collect();
            let del2: Vec<O> = deleted_v2.iter().flatten().copied().collect();
            if !del1.is_empty() || !del2.is_empty() {
                for &this_pv in &pvs {
                    let defs: Vec<VertexDefId> = self
                        .proto_vertex(this_pv)
                        .ok_or(dangling("proto vertex is deleted"))?
                        .vertex_definitions()
                        .to_vec();
                    let mut keep = Vec::with_capacity(defs.len());
                    for vd in defs {
                        let d = self
                            .vertex_def(vd)
                            .ok_or(dangling("vertex definition is deleted"))?;
                        let hit = d.final_vertex(1).is_some_and(|v| del1.contains(&v))
                            || d.final_vertex(2).is_some_and(|v| del2.contains(&v));
                        if hit {
                            self.delete_vertex_definition(vd);
                        } else {
                            keep.push(vd);
                        }
                    }
                    *self
                        .proto_vertex_mut(this_pv)
                        .ok_or(dangling("proto vertex is deleted"))?
                        .vertex_definitions_mut() = keep;
                }
            }

            // Also have to check deleted SmEdges.
            if !deleted_edges1.is_empty() || !deleted_edges2.is_empty() {
                let pes: Vec<ProtoEdgeId> = self.proto_edges().to_vec();
                for &this_pe in &pes {
                    let mut mm = 0;
                    loop {
                        let list = self
                            .proto_edge(this_pe)
                            .ok_or(dangling("proto edge is deleted"))?
                            .edge_definitions();
                        let Some(&ed) = list.get(mm) else { break };
                        let d = self
                            .edge_def(ed)
                            .ok_or(dangling("edge definition is deleted"))?;
                        let hit = d.final_edge(1).is_some_and(|e| deleted_edges1.contains(&e))
                            || d.final_edge(2).is_some_and(|e| deleted_edges2.contains(&e));
                        if hit {
                            ignore_soft(passed_on(
                                self.proto_edge_mut(this_pe)
                                    .ok_or(dangling("proto edge is deleted"))?
                                    .remove_edge_definition(ed),
                                pass,
                            )?)?;
                            // No Destruct(): this EdgeDefinition was imprinted, so its
                            // curves belong to the Breps' Edges (the deleted Edge took
                            // its own along). The destructor deletes only the curves no
                            // Brep took.
                            deleted_curves.extend(self.delete_edge_definition(ed));
                        }
                        // (the index advances past the element that moved down)
                        mm += 1;
                    }
                }

                // ... and ProtoFaces have EdgeDefinitions too.
                let pfs: Vec<ProtoFaceId> = self.proto_faces().to_vec();
                for &this_pf in &pfs {
                    let coinc: Vec<ProtoEdgeId> = self
                        .proto_face(this_pf)
                        .ok_or(dangling("proto face is deleted"))?
                        .coinc_proto_edges()
                        .to_vec();
                    for this_pe in coinc {
                        let mut nn = 0;
                        loop {
                            let list = self
                                .proto_edge(this_pe)
                                .ok_or(dangling("proto edge is deleted"))?
                                .edge_definitions();
                            let Some(&ed) = list.get(nn) else { break };
                            let d = self
                                .edge_def(ed)
                                .ok_or(dangling("edge definition is deleted"))?;
                            let hit = d.final_edge(1).is_some_and(|e| deleted_edges1.contains(&e))
                                || d.final_edge(2).is_some_and(|e| deleted_edges2.contains(&e));
                            if hit {
                                ignore_soft(passed_on(
                                    self.remove_edge_definition(k, ed, false),
                                    pass,
                                )?)?;
                            }
                            nn += 1;
                        }
                    }
                }
            }
        }

        ret.map(|()| deleted_curves)
    }

    /// `SmProtoTopologyManager::ResolveOverlappingProtoEdges()`: replaces each group of
    /// overlapping proto edges with the non-overlapping proto edges of the coincident
    /// curve classified against the source faces. Returns the curves deleted.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted or null object or an edge without
    /// faces or curve; [`SolidError::Unported`] for the callees listed in the module
    /// header.
    pub fn resolve_overlapping_proto_edges(
        &mut self,
        k: &Kernel,
        geometry: &mut impl ProtoBrep<O>,
        ti_approx_tol_3d: f64,
    ) -> Result<Vec<O>> {
        // statuses SMLib drops, passed on with `ssi_proto_topology_statuses_pass_on`
        // (fork bcc6f01)
        let pass = k.fixes.ssi_proto_topology_statuses_pass_on;
        let mut deleted_curves: Vec<O> = Vec::new();
        let tol = self.raw_tolerance();

        // For each PE (at least one of the SrcTopos should be an edge.)
        let mut ii = 0usize;
        // groups replaced so far and the most allowed (not upstream: HANG-51, see the
        // module deviations)
        let mut groups_resolved = 0usize;
        let max_groups = OVERLAP_GROUPS_PER_PROTO_EDGE
            .saturating_mul(self.proto_edges().len())
            .saturating_add(MAX_OVERLAP_GROUPS_BASE);
        // repeat the index when the list was changed by removing a group
        while let Some(&pe) = self.proto_edges().get(ii) {
            let mut group = self.overlapping_pe_group(pe)?;
            let found_group = !group.is_empty();

            // skip PEs not in a Overlapping PE Group
            if !found_group {
                ii += 1;
                continue;
            }
            groups_resolved += 1;
            if groups_resolved > max_groups {
                return Err(SolidError::invalid_input(
                    OP_RESOLVE_OVERLAPPING,
                    "overlapping proto edge groups keep re-forming (HANG-51)",
                ));
            }

            // lowest topology search locals
            let mut topo1: Option<O> = None;
            let mut topo2: Option<O> = None;
            let mut dim1: u64 = 3;
            let mut dim2: u64 = 3;

            // For every Overlap PE: save lowest order SrcTopology
            for &overlap in &group {
                let eds = self
                    .proto_edge(overlap)
                    .ok_or(dangling("proto edge is deleted"))?
                    .edge_definitions();
                // Empty ProtoEdges are not a problem. Arbitrary: use the 1st EdgeDef.
                let Some(&first) = eds.first() else { continue };
                let d = self
                    .edge_def(first)
                    .ok_or(dangling("edge definition is deleted"))?;
                let t1 = as_topology(d.src_topo(1));
                let t2 = as_topology(d.src_topo(2));
                let this_dim1 = dimension(t1)?;
                let this_dim2 = dimension(t2)?;
                if this_dim1 == 0 {
                } else if this_dim1 < dim1 {
                    dim1 = this_dim1;
                    topo1 = t1;
                }
                if this_dim2 == 0 {
                } else if this_dim2 < dim2 {
                    dim2 = this_dim2;
                    topo2 = t2;
                }
            }

            // Not upstream (#17527): a group on an edge of one B-rep and a face of the other
            // is replaced by the edge's curve classified against that face, every piece of the
            // edge that lies in the face. Every other proto edge all of whose definitions lie on
            // that edge and in that face is one of those pieces again, and goes with the group.
            self.absorb_pieces_of_edge_in_face(&mut group, topo1, topo2)?;

            // PFs keep a list of coincident PEs. Check every ProtoFace: the group can be
            // coincident with more than one, and its PEs are deleted below.
            let mut replace_coin_pe = false;
            // every ProtoFace that listed a PE of the group
            let mut replace_pfs: Vec<ProtoFaceId> = Vec::new();
            let pfs: Vec<ProtoFaceId> = self.proto_faces().to_vec();
            for &pf in &pfs {
                let face = self
                    .proto_face_mut(pf)
                    .ok_or(dangling("proto face is deleted"))?;
                let list = face.coinc_proto_edges_mut();
                let size = list.len();
                list.retain(|p| !group.contains(p));
                if list.len() != size {
                    replace_coin_pe = true;
                    replace_pfs.push(pf);
                }
            }

            // Delete all OverlappingPE->EdgeDef and any PV that has no remaining PEs
            let mut overlap_pvs: Vec<ProtoVertexId> = Vec::new();
            for &overlap in &group {
                let (start, end, eds) = {
                    let e = self
                        .proto_edge(overlap)
                        .ok_or(dangling("proto edge is deleted"))?;
                    (
                        e.start_proto_vertex(),
                        e.end_proto_vertex(),
                        e.edge_definitions().to_vec(),
                    )
                };
                let start = start.ok_or(dangling("proto edge has no start proto vertex"))?;
                let end = end.ok_or(dangling("proto edge has no end proto vertex"))?;
                if !overlap_pvs.contains(&start) {
                    overlap_pvs.push(start);
                }
                if !overlap_pvs.contains(&end) {
                    overlap_pvs.push(end);
                }

                // Remove and delete all EdgeDefs from this OverlappingPE
                for ed in eds {
                    ignore_soft(passed_on(self.remove_edge_definition(k, ed, false), pass)?)?;
                    deleted_curves.extend(
                        self.edge_def_mut(ed)
                            .ok_or(dangling("edge definition is deleted"))?
                            .destruct(),
                    );
                    deleted_curves.extend(self.delete_edge_definition(ed));
                }

                // remove PE from End and Start ProtoVertex->PE list
                ignore_soft(passed_on(
                    self.proto_vertex_mut(start)
                        .ok_or(dangling("proto vertex is deleted"))?
                        .remove_proto_edge(overlap),
                    pass,
                )?)?;
                ignore_soft(passed_on(
                    self.proto_vertex_mut(end)
                        .ok_or(dangling("proto vertex is deleted"))?
                        .remove_proto_edge(overlap),
                    pass,
                )?)?;

                // remove the OverlappingPEs from this PE; clear the end vertices
                {
                    let e = self
                        .proto_edge_mut(overlap)
                        .ok_or(dangling("proto edge is deleted"))?;
                    e.clear_overlapping_pes();
                    e.set_start_proto_vertex(None);
                    e.set_end_proto_vertex(None);
                }

                // remove OverlapPE from ProtoTopoMgr, then delete it
                ignore_soft(passed_on(self.remove_proto_edge(k, overlap), pass)?)?;
                self.forget_proto_edge(overlap);
                deleted_curves.extend(self.delete_proto_edge_object(overlap));
            }

            // for every accumulated end ProtoVertex with no other PEs, delete it after
            // removing and deleting all its VertexDefs
            for &opv in &overlap_pvs {
                let v = self
                    .proto_vertex(opv)
                    .ok_or(dangling("proto vertex is deleted"))?;
                if v.num_proto_edges() == 0 {
                    let vds: Vec<VertexDefId> = v.vertex_definitions().to_vec();
                    for vd in vds {
                        // TRUE = delete ProtoVertexOwner when its count goes to zero
                        ignore_soft(passed_on(self.remove_vertex_definition(k, vd, true), pass)?)?;
                        self.delete_vertex_definition(vd);
                    }
                }
            }

            // classify the Edge->Curve against the SrcFaces
            let e1 = as_kind(topo1, ClassObjectKind::Edge);
            let e2 = as_kind(topo2, ClassObjectKind::Edge);
            let mut f1 = as_kind(topo1, ClassObjectKind::Face);
            let mut f2 = as_kind(topo2, ClassObjectKind::Face);

            let mut cc1s: Vec<CurveClassification<O>> = Vec::new();
            let mut cc2s: Vec<CurveClassification<O>> = Vec::new();

            if let (Some(edge1), Some(edge2)) = (e1, e2) {
                // SrcTopo Edge-Edge branch
                let faces1 = geometry.edge_faces(edge1)?;
                let faces2 = geometry.edge_faces(edge2)?;
                let h1 = geometry
                    .edge_curve(edge1)
                    .ok_or(dangling("edge is unknown"))?;
                let h2 = geometry
                    .edge_curve(edge2)
                    .ok_or(dangling("edge is unknown"))?;
                let ivl1 = geometry
                    .edge_interval(edge1)
                    .ok_or(dangling("edge is unknown"))?;
                let ivl2 = geometry
                    .edge_interval(edge2)
                    .ok_or(dangling("edge is unknown"))?;
                let edge_tol1 = geometry
                    .topology_tolerance(edge1)
                    .ok_or(dangling("edge is unknown"))?;
                let edge_tol2 = geometry
                    .topology_tolerance(edge2)
                    .ok_or(dangling("edge is unknown"))?;
                // Both classifications carry the first edge's curve and its source zone.
                let curve_tol = max2(tol, edge_tol1);

                // arbitrary - pick XSectFace as first Edge Face
                f1 = Some(*faces1.first().ok_or(dangling("edge has no faces"))?);
                f2 = Some(*faces2.first().ok_or(dangling("edge has no faces"))?);

                // Find the Edge->Curve coincidences; the status is ignored.
                let sols = {
                    let c1 = curve_of(&*geometry, h1)?;
                    let c2 = curve_of(&*geometry, h2)?;
                    // Prior merges can raise edge zones. Bound this new shared-interval
                    // search by each owning B-rep's zone, as in find_edge_proved_along.
                    let manager_zone = self.tolerance(k);
                    let [search_zone1, search_zone2] = [(edge1, edge_tol1), (edge2, edge_tol2)]
                        .map(|(edge, edge_zone)| {
                            let body_zone = geometry
                                .topology_entity(edge)
                                .map_or(manager_zone, |(brep, _)| brep.tolerance());
                            // Unknown zones stay unknown through the cap and floor.
                            if edge_zone.is_nan() || body_zone.is_nan() || manager_zone.is_nan() {
                                f64::NAN
                            } else {
                                edge_zone.min(body_zone).max(manager_zone)
                            }
                        });
                    // Include rounding at the trimmed curves' scale.
                    // An unavailable box contributes zero extra rounding, as in the checker.
                    let req = BoundingBoxRequest {
                        normal: true,
                        ..BoundingBoxRequest::default()
                    };
                    let mut size = 0.0;
                    for (curve, interval) in [(c1, &ivl1), (c2, &ivl2)] {
                        let boxes = curve.calculate_bounding_box(k, interval, req, true);
                        if let Some(bounds) = crate::error::ignored(boxes)?.and_then(|b| b.normal) {
                            if bounds.is_defined()
                                && bounds.is_bounded()
                                && !bounds.has_negative_volume()
                                && is_measurable_box(&bounds)
                            {
                                size = max2(size, bounds.max_dimension());
                            }
                        }
                    }
                    let coincidence_tol = coincidence_reach_3d(k, search_zone1, search_zone2, size);
                    ignore_soft(passed_on(
                        c1.global_coincidence_checker::<CurveHandle>(
                            k,
                            &ivl1,
                            c2,
                            &ivl2,
                            coincidence_tol,
                            true,
                            [None, None],
                        ),
                        pass,
                    )?)?
                    .map(|c| c.solutions)
                    .unwrap_or_default()
                };

                for sol in &sols {
                    if sol.solution_type == SolutionType::RangeOfValues {
                        // PEs expect the same curve classified against each face
                        let ivl = Extent1d::from_min_max(sol.start.param(0)?, sol.end.param(0)?);
                        let c1h = h1.ok_or(dangling("edge has no curve"))?;
                        let mut cc1 = new_curve_classification(k, c1h, ivl, curve_tol)?;
                        let mut cc2 = new_curve_classification(k, c1h, ivl, curve_tol)?;

                        // Classify the coincident curves against their faces
                        let face1 = f1.ok_or(dangling("face is null"))?;
                        let face2 = f2.ok_or(dangling("face is null"))?;
                        ignore_soft(passed_on(
                            geometry.face_curve_on_classify_mut(k, face1, true, &mut cc1),
                            pass,
                        )?)?;
                        ignore_soft(passed_on(
                            geometry.face_curve_on_classify_mut(k, face2, true, &mut cc2),
                            pass,
                        )?)?;

                        // a curve for every common interval classified in both
                        let _ = cc1.extract_geometry(
                            k,
                            &*geometry,
                            &mut cc2,
                            ExtractOptions::default(),
                        )?;

                        // Make sure that a parameter is set in each PointClass object.
                        ignore_soft(passed_on(
                            geometry.set_point_class_parameters_mut(k, &mut cc1, false),
                            pass,
                        )?)?;
                        ignore_soft(passed_on(
                            geometry.set_point_class_parameters_mut(k, &mut cc2, false),
                            pass,
                        )?)?;

                        cc1s.push(cc1);
                        cc2s.push(cc2);
                    }
                }
            } else if (e1.is_some() && f2.is_some()) || (f1.is_some() && e2.is_some()) {
                // SrcTopo Edge-Face branch: use the Edge->Curve
                let edge = e1.or(e2).ok_or(dangling("edge is null"))?;
                let handle = geometry
                    .edge_curve(edge)
                    .ok_or(dangling("edge is unknown"))?
                    .ok_or(dangling("edge has no curve"))?;
                let ivl = geometry
                    .edge_interval(edge)
                    .ok_or(dangling("edge is unknown"))?;
                // Not upstream (#17527): the edge's curve is classified within the edge's
                // own tolerance where that is the larger (see the module's Deviations).
                let edge_tol = geometry
                    .topology_tolerance(edge)
                    .ok_or(dangling("edge is unknown"))?;
                let curve_tol = max2(tol, edge_tol);
                let mut cc1 = new_curve_classification(k, handle, ivl, curve_tol)?;
                let mut cc2 = new_curve_classification(k, handle, ivl, curve_tol)?;

                // Pick a SrcFace object for the SrcEdge object
                let faces = geometry.edge_faces(edge)?;
                let first = *faces.first().ok_or(dangling("edge has no faces"))?;
                if e1.is_some() {
                    f1 = Some(first);
                } else {
                    f2 = Some(first);
                }

                // Classify Edge->Curve against both SrcFaces
                ignore_soft(passed_on(
                    geometry.face_curve_on_classify_mut(
                        k,
                        f1.ok_or(dangling("face is null"))?,
                        true,
                        &mut cc1,
                    ),
                    pass,
                )?)?;
                ignore_soft(passed_on(
                    geometry.face_curve_on_classify_mut(
                        k,
                        f2.ok_or(dangling("face is null"))?,
                        true,
                        &mut cc2,
                    ),
                    pass,
                )?)?;

                let _ = cc1.extract_geometry(k, &*geometry, &mut cc2, ExtractOptions::default())?;
                ignore_soft(passed_on(
                    geometry.set_point_class_parameters_mut(k, &mut cc1, false),
                    pass,
                )?)?;
                ignore_soft(passed_on(
                    geometry.set_point_class_parameters_mut(k, &mut cc2, false),
                    pass,
                )?)?;
                cc1s.push(cc1);
                cc2s.push(cc2);
            }

            // For each curve classification, create the new PEs
            for (cc1, cc2) in cc1s.iter_mut().zip(cc2s.iter_mut()) {
                for kk in 0..cc1.size() {
                    let mut ci1 = *cc1.at(kk)?;
                    let mut ci2 = *cc2.at(kk)?;
                    // the status is ignored
                    let result = ignore_soft(passed_on(
                        self.find_or_create_proto_edge(
                            k,
                            &mut *geometry,
                            ti_approx_tol_3d,
                            cc1,
                            &mut ci1,
                            cc2,
                            &mut ci2,
                        ),
                        pass,
                    )?)?;
                    *cc1.at_mut(kk)? = ci1;
                    *cc2.at_mut(kk)? = ci2;

                    // when any OverlappingPE was a CoinPE in a PF - add all New or Found
                    // PEs back into each of those PF->CoincPE lists
                    if let Some((Some(new_pe), _)) = result {
                        if replace_coin_pe {
                            for &pf in &replace_pfs {
                                let list = self
                                    .proto_face_mut(pf)
                                    .ok_or(dangling("proto face is deleted"))?
                                    .coinc_proto_edges_mut();
                                if !list.contains(&new_pe) {
                                    list.push(new_pe);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(deleted_curves)
    }
}

impl<O: ClassObject> ProtoTopologyManager<O> {
    /// Not upstream (#17527): adds to an overlapping proto edge `group` whose lowest source
    /// topology is an edge in one B-rep and a face in the other (`topo1`, `topo2`, either
    /// way round) every other listed proto edge that has definitions and all of whose
    /// definitions come from that edge and that face. Each such definition was classified
    /// on the edge and in the face; [`Self::resolve_overlapping_proto_edges`] replaces the
    /// group with every piece of the edge that lies in the face, so a piece left out would
    /// stand beside its own replacement, and the two would be related to one edge of the
    /// other B-rep. Overlap is found by the ends of one piece dropped on the other within the
    /// zone tolerance, so where the edge lies further than that from the face's own curve
    /// (an edge within its tolerance of the face, off it by more than the system zone
    /// tolerance) a piece can share the edge and the face with the group without being
    /// found overlapping it. Groups of any other kind are left as they are.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted proto edge or edge definition.
    fn absorb_pieces_of_edge_in_face(
        &self,
        group: &mut Vec<ProtoEdgeId>,
        topo1: Option<O>,
        topo2: Option<O>,
    ) -> Result<()> {
        let (Some(t1), Some(t2)) = (topo1, topo2) else {
            return Ok(());
        };
        let kinds = (t1.class_kind(), t2.class_kind());
        if kinds != (ClassObjectKind::Edge, ClassObjectKind::Face)
            && kinds != (ClassObjectKind::Face, ClassObjectKind::Edge)
        {
            return Ok(());
        }
        for &pe in self.proto_edges() {
            if group.contains(&pe) {
                continue;
            }
            let eds = self
                .proto_edge(pe)
                .ok_or(dangling("proto edge is deleted"))?
                .edge_definitions();
            if eds.is_empty() {
                continue;
            }
            let mut all = true;
            for &ed in eds {
                let d = self
                    .edge_def(ed)
                    .ok_or(dangling("edge definition is deleted"))?;
                if as_topology(d.src_topo(1)) != topo1 || as_topology(d.src_topo(2)) != topo2 {
                    all = false;
                    break;
                }
            }
            if all {
                group.push(pe);
            }
        }
        Ok(())
    }
}

/// The dimension `sm_TopoDimension` returns, as the `ULONG` upstream stores it
/// (`-1` wraps to a huge value).
fn dimension<O: ClassObject>(topo: Option<O>) -> Result<u64> {
    let t = topo.ok_or(dangling("source topology is null"))?;
    let d = topo_dimension(t);
    Ok(u64::try_from(d).unwrap_or(u64::MAX))
}

/// `pV1->GetPoint().DistanceBetween(pV2->GetPoint())`.
fn vertex_distance<O: Copy>(brep: &impl ProtoBrep<O>, v1: O, v2: O) -> Result<f64> {
    let p1 = brep.vertex_point(v1)?;
    let p2 = brep.vertex_point(v2)?;
    Ok(p1.distance(p2))
}

/// Not upstream: whether the vertex handle `v` names a vertex that ends a seam edge
/// ([`crate::topology::vertex::ends_seam_edge`]); `false` for a handle the adapter cannot
/// resolve.
///
/// # Errors
///
/// Those of [`crate::topology::vertex::ends_seam_edge`].
fn ends_seam<O: Copy>(brep: &impl ClassGeometry<O>, v: O) -> Result<bool> {
    match brep.topology_entity(v) {
        Some((b, EntityRef::Vertex(id))) => crate::topology::vertex::ends_seam_edge(b, id),
        _ => Ok(false),
    }
}

/// `SmTol::GetXSectTol3d(pV1, pV2)`: the sum of the two zone tolerances.
fn xsect_tol<O: Copy>(brep: &impl ProtoBrep<O>, v1: O, v2: O) -> Result<f64> {
    let t1 = brep
        .topology_tolerance(v1)
        .ok_or(dangling("vertex is unknown"))?;
    let t2 = brep
        .topology_tolerance(v2)
        .ok_or(dangling("vertex is unknown"))?;
    Ok(t1 + t2)
}

/// `sDeletedVertices[jj] = vertex`.
fn set_slot<O: Copy>(list: &mut [Option<O>], index: usize, vertex: O) {
    if let Some(slot) = list.get_mut(index) {
        *slot = Some(vertex);
    }
}

/// Most times [`edge_piece_in_face_band`] halves a part of an edge piece to prove it inside a
/// face's band (parts down to 1/1024 of the piece); a bound of the port.
const MAX_CROSSING_HALVINGS: u32 = 10;

/// The most a local tolerance may be, in zone tolerances of its body: the ratio the pointer
/// check holds an edge's and a face's tolerance to (`topology/brep/validation/pointers.rs`;
/// `docs/validity-and-tolerances.md` §5.1).
const LOCAL_TOLERANCE_RATIO: f64 = 1000.0;

impl<O: ClassObject> ProtoTopologyManager<O> {
    /// `FindProtoVertex(rPosition)` for the point `pc1`, `pc2` classify: the closest listed
    /// proto vertex that holds it. `within` is the closest one the point is not proved apart
    /// from (not upstream: [`ProtoTopologyManager::find_proto_vertex_within`], where upstream's
    /// `SmProtoVertex::ContainsPoint` takes any whose ball, the capture distance, contains
    /// it); a proto vertex also holds, not upstream (#17595; see the module's Deviations), the
    /// point of the crossing of an edge with a face it already stands for
    /// ([`Self::is_same_edge_face_crossing`]).
    ///
    /// # Errors
    ///
    /// Those of [`Self::is_same_edge_face_crossing`]; [`SolidError::InvalidInput`] for a
    /// deleted listed proto vertex.
    fn find_proto_vertex_holding(
        &self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        position: Point3,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
        within: Option<ProtoVertexId>,
    ) -> Result<Option<ProtoVertexId>> {
        let distance = |id: ProtoVertexId| -> Result<f64> {
            let pv = self
                .proto_vertex(id)
                .ok_or(dangling("proto vertex is deleted"))?;
            Ok(face_p2::proto_vertex_distance_to(pv, position))
        };
        let mut held = within;
        let mut min_dist = match within {
            Some(id) => distance(id)?,
            None => BIG_DOUBLE,
        };
        for &id in self.proto_vertices() {
            if Some(id) == within
                || !self.is_same_edge_face_crossing(k, geometry, id, pc1, pc2)?
                || self.proto_vertex_incidence_conflicts(k, geometry, id, [pc1, pc2])?
            {
                continue;
            }
            let dist = distance(id)?;
            if dist < min_dist {
                held = Some(id);
                min_dist = dist;
            }
        }
        Ok(held)
    }

    /// A shared event must keep each fixed native vertex on every opposite defining face.
    /// Check both definition orders so reach regions cannot erase a proved separation.
    pub(crate) fn proto_vertex_incidence_conflicts(
        &self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        id: ProtoVertexId,
        incoming: [&PointClassification<O>; 2],
    ) -> Result<bool> {
        let pv = self
            .proto_vertex(id)
            .ok_or(dangling("proto vertex is deleted"))?;
        for &def in pv.vertex_definitions() {
            let def = self
                .vertex_def(def)
                .ok_or(dangling("vertex definition is deleted"))?;
            let stored = [def.brep_point_classif(1), def.brep_point_classif(2)];
            for (fixed, faces) in [(incoming, stored), (stored, incoming)] {
                let [first, second] = fixed;
                let [opposite_first, opposite_second] = faces;
                for (fixed, opposite) in [(first, opposite_second), (second, opposite_first)] {
                    let Some(vertex) = fixed.vertex_object() else {
                        continue;
                    };
                    let Some(marker) = periodic_trim_witness(k, geometry, vertex)? else {
                        continue;
                    };
                    let Some(vertex_zone) = geometry.topology_tolerance(vertex) else {
                        continue;
                    };
                    let Some(vertex_face_zone) = fixed
                        .face()
                        .and_then(|face| geometry.topology_tolerance(face))
                    else {
                        continue;
                    };
                    let Some(face) = opposite.face().or_else(|| opposite.face_object()) else {
                        continue;
                    };
                    let Some(surface) = geometry
                        .face_surface(face)
                        .flatten()
                        .and_then(|s| geometry.surface(s))
                    else {
                        continue;
                    };
                    let Some(face_zone) = geometry.topology_tolerance(face) else {
                        continue;
                    };
                    let Some(object_zone) = opposite
                        .object()
                        .and_then(|o| geometry.topology_tolerance(o))
                    else {
                        continue;
                    };
                    if ![face_zone, object_zone, vertex_zone, vertex_face_zone]
                        .iter()
                        .all(|zone| zone.is_finite() && *zone >= 0.0)
                        // Raised stitched zones retain their covering-gap policy.
                        || object_zone > face_zone
                        || vertex_zone > vertex_face_zone
                    {
                        continue;
                    }
                    if AnalyticSupport::of(surface).is_some_and(|support| {
                        marker.outside_surface(k, &support, face_zone.max(object_zone))
                    }) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Not upstream (#17595; see the module's Deviations). Whether the proto vertex `pv`
    /// stands for the crossing `pc1`, `pc2` are a point of: one of them lies on an edge and
    /// the other on a face, `pv` has a definition on the same edge and face, and the edge
    /// between that definition's parameter and this point's stays inside the band that puts a
    /// point of the face on the edge ([`edge_piece_in_face_band`]; the band is derived in the
    /// body). `false` for any other pair of classes or unset parameters.
    ///
    /// # Errors
    ///
    /// Those of [`edge_piece_in_face_band`]; [`SolidError::InvalidInput`] for a deleted
    /// listed object.
    fn is_same_edge_face_crossing(
        &self,
        k: &Kernel,
        geometry: &impl ProtoBrep<O>,
        pv: ProtoVertexId,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
    ) -> Result<bool> {
        let (on_edge, on_face, edge_side) = match (pc1.point_class(), pc2.point_class()) {
            (PointClassType::Edge, PointClassType::Face) => (pc1, pc2, 1),
            (PointClassType::Face, PointClassType::Edge) => (pc2, pc1, 2),
            _ => return Ok(false),
        };
        let (Some(edge), Some(face)) = (on_edge.object(), on_face.object()) else {
            return Ok(false);
        };
        if !on_edge.are_parameters_set() || !on_face.are_parameters_set() {
            return Ok(false);
        }
        let Some(vd) = self.find_matching_vertex_definition(pv, pc1.object(), pc2.object())? else {
            return Ok(false);
        };
        let there = self
            .vertex_def(vd)
            .ok_or(dangling("vertex definition is deleted"))?;
        let piece = [there.brep_t_param(edge_side), on_edge.t_param()];
        // The band that puts a point of the face on the edge: the zone of what was classified
        // with the edge's zone, or with the gap at which either point was taken to be on the
        // edge where that is wider (a point the face's trims put there). A gap counts as the
        // edge's width only up to the most a local tolerance may be, `LOCAL_TOLERANCE_RATIO`
        // times the zone of what was classified.
        let zone = on_edge.src_zone_tol_3d();
        let width = max2(
            on_edge.obj_zone_tol_3d(k, geometry)?,
            min2(
                max2(there.gap(), on_edge.deviation()),
                LOCAL_TOLERANCE_RATIO * zone,
            ),
        );
        let band = xsect_tol_3d_from_zones(k, zone, width);
        edge_piece_in_face_band(k, geometry, edge, piece, face, on_face.uv_param(), band)
    }
}

/// Not upstream (#17595). Whether every point of `edge` between the parameters `piece` is
/// proved to lie within `band` of `face`'s surface over the face's domain. With `band` the
/// intersection tolerance that puts a point of the face on the edge, the edge lies in the face
/// there, and where between the two parameters it passes through the face is not determined. A
/// point of a part of the piece is no farther from the surface than either end of the
/// part plus the arc to that end, so the whole part is within half the sum of its two
/// ends' distances and its length ([`Curve::length_bound`], an upper bound). An end's
/// distance is that to the surface point a local solve finds from `uv_guess` (any surface
/// point bounds the distance from above). A part not proved is halved, at most
/// [`MAX_CROSSING_HALVINGS`] times, and a point found outside the band ends the proof; a
/// piece not proved is not in the band. The edge must also cross the face at both ends of the
/// piece: the sine of its tangent against the face's normal there at least
/// [`COINCIDENT_TANGENT_SINE`]. Tangent to the face at an end (the coincidence check's
/// angle), the edge runs in the face there, and the two points are the ends of a stretch the
/// two share, not one crossing.
///
/// # Errors
///
/// Those of the evaluations and the local solves; `Unported` and `Cancelled` from the
/// length bound.
fn edge_piece_in_face_band<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ProtoBrep<O>,
    edge: O,
    piece: [f64; 2],
    face: O,
    uv_guess: Point2,
    band: f64,
) -> Result<bool> {
    let curve = geometry
        .edge_curve(edge)
        .flatten()
        .and_then(|c| geometry.curve(c));
    let surface = geometry
        .face_surface(face)
        .flatten()
        .and_then(|s| geometry.surface(s));
    let (Some(curve), Some(surface), Some(domain)) =
        (curve, surface, geometry.face_uv_domain(face))
    else {
        return Ok(false);
    };

    // the distance from the edge's point at `t` to a surface point near it, and that
    // point's parameters; `None` when the solve finds none
    let distance = |t: f64, guess: Point2| -> Result<Option<(f64, Point2)>> {
        let point = curve.evaluate_point(k, t)?;
        let solved =
            surface.local_point_solve::<O>(k, &domain, SolverOperation::Minimize, point, guess)?;
        if !solved.found {
            return Ok(None);
        }
        let on = Point2::new(
            solved.solution.start.param(0)?,
            solved.solution.start.param(1)?,
        );
        Ok(Some((point.distance(surface.evaluate_point(k, on)?), on)))
    };

    let [a, b] = if piece[0] <= piece[1] {
        piece
    } else {
        [piece[1], piece[0]]
    };
    let (Some(at_a), Some(at_b)) = (distance(a, uv_guess)?, distance(b, uv_guess)?) else {
        return Ok(false);
    };

    // An edge tangent to the face at either end (within the coincidence check's angle) runs
    // in the face there: the two points are the ends of a stretch the edge and the face
    // share, not one crossing.
    let crosses = |t: f64, on: Point2| -> Result<bool> {
        let Some(tangent) = curve.evaluate(k, t, 1, true, true)?.get(1).copied() else {
            return Ok(false);
        };
        let normal = surface.evaluate_normal(k, on, Side::Right, Side::Right)?;
        let sine = (tangent.dot(normal) / tangent.length()).abs();
        // (a NaN sine, from a vanishing tangent, is no crossing)
        Ok(sine >= COINCIDENT_TANGENT_SINE)
    };
    if !crosses(a, at_a.1)? || !crosses(b, at_b.1)? {
        return Ok(false);
    }
    let mut parts = vec![(a, at_a, b, at_b, 0_u32)];
    while let Some((a, at_a, b, at_b, halvings)) = parts.pop() {
        // A length the bound does not enclose (a curve kind without one, a width under its
        // rounding) proves nothing: the part stays unproved.
        let length = match curve.length_bound(k, &Extent1d::from_min_max(a, b), band) {
            Ok(bracket) => bracket.upper,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => f64::INFINITY,
        };
        if 0.5 * (at_a.0 + at_b.0 + length) <= band {
            continue;
        }
        if halvings == MAX_CROSSING_HALVINGS {
            return Ok(false);
        }
        let mid = 0.5 * (a + b);
        let Some(at_mid) = distance(mid, at_a.1)?.filter(|at| at.0 <= band) else {
            return Ok(false);
        };
        parts.push((a, at_a, mid, at_mid, halvings + 1));
        parts.push((mid, at_mid, b, at_b, halvings + 1));
    }
    Ok(true)
}

/// Not upstream (WR-410; the SolidCraft fork's `sm_EdgeCrossingSine`): the sine of the
/// angle at which a curve interval's curve (`curve`, over `interval`) crosses the edge its
/// end classification `pc` lies on, at the interval's end (`at_end`) or start; `-1` when
/// that end is not on an edge with a curve, its parameters are unset, an evaluation
/// fails or a tangent vanishes.
///
/// # Errors
///
/// [`SolidError::Unported`] from the evaluations.
fn edge_crossing_sine<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ProtoBrep<O>,
    curve: Option<O>,
    interval: Extent1d,
    pc: &PointClassification<O>,
    at_end: bool,
) -> Result<f64> {
    let edge_curve = pc
        .edge_object()
        .and_then(|e| geometry.edge_curve(e))
        .flatten()
        .and_then(|c| geometry.curve(c));
    let curve = curve.and_then(|c| geometry.curve(c));
    let (Some(edge_curve), Some(curve)) = (edge_curve, curve) else {
        return Ok(-1.0);
    };
    if !pc.are_parameters_set() {
        return Ok(-1.0);
    }
    let mut t = pc.pre_snap_param();
    if t == BIG_DOUBLE || t == crate::math::UNDEF_DOUBLE {
        t = if at_end {
            interval.max()
        } else {
            interval.min()
        };
    }
    let tangent = |r: Result<Vec<Vec3>>| -> Result<Option<Vec3>> {
        match r {
            Ok(d) => Ok(d.get(1).copied()),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Ok(None),
        }
    };
    let (Some(c), Some(e)) = (
        tangent(curve.evaluate(k, t, 1, !at_end, false))?,
        tangent(edge_curve.evaluate(k, pc.t_param(), 1, true, false))?,
    ) else {
        return Ok(-1.0);
    };
    let len = c.length() * e.length();
    if len.is_nan() || len <= 0.0 {
        return Ok(-1.0);
    }
    Ok(c.cross(e).length() / len)
}

/// Not upstream: the unit tangent of the edge `pc` classifies onto, at its point there;
/// `None` when it is on no edge with a curve, its parameters are unset, the evaluation
/// fails or the tangent vanishes.
fn edge_direction<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ProtoBrep<O>,
    pc: &PointClassification<O>,
) -> Result<Option<Vec3>> {
    if pc.point_class() != PointClassType::Edge || !pc.are_parameters_set() {
        return Ok(None);
    }
    let Some(curve) = pc
        .edge_object()
        .and_then(|e| geometry.edge_curve(e))
        .flatten()
        .and_then(|c| geometry.curve(c))
    else {
        return Ok(None);
    };
    let d = match curve.evaluate(k, pc.t_param(), 1, true, false) {
        Ok(d) => d.get(1).copied(),
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Err(_) => None,
    };
    Ok(d.and_then(|t| {
        let len = t.length();
        (len.is_finite() && len > 0.0).then(|| t / len)
    }))
}

/// Not upstream: the uncertainty region of a point at `position`, known to `tolerance`,
/// that the classifications `pc1`, `pc2` name (docs/booleans.md, "One point or two").
/// Where the point lies on an edge and a curve fixes it there (`crossing`: the curve of
/// the interval it ends, with its parameter there; or, for two classifications onto
/// edges, the other edge), the crossing is known along that edge only to the tolerance
/// over the sine of the angle between the two, the angle enclosed over the whole reach
/// ([`Curve::crossing_reach`]): the region is that reach either way along the edge's
/// tangent, widened by the tolerance and by how far the edge strays from its tangent
/// within the reach, enclosed ([`Curve::stray_from_line`]). Where no reach is proved (the
/// curves may run along each other within it, or have no hull) or it is longer, the region
/// reaches `limit`, the capture distance: as far as proto vertices are looked for at all;
/// where the edge's stray is not enclosed either, it is the ball of `limit`. A point with
/// no curve that lies on an edge of one B-rep and in a face of the other (a face pair's
/// single point, a degenerate interval) is where that edge crosses the face's surface,
/// known along the edge only to the tolerance over the sine of that angle, which nothing
/// here encloses: its reach is `limit` (an edge 1e-9 into a plane crosses it at 2.8e-5 rad,
/// so its two crossings 1.4e-4 apart are one point with the end of the line the two touch
/// along). Of several crossings the one known worst counts. Not upstream
/// (tangency-labs/solidcore#3339): an edge `along` marks (the interval runs along that seam,
/// [`runs_along_seam`]) is not crossed there and gives no crossing. Any other point has the ball
/// of its tolerance.
#[allow(clippy::too_many_arguments)] // the point, what fixes it and how well
fn crossing_region<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ProtoBrep<O>,
    crossing: Option<(O, f64)>,
    along: [bool; 2],
    pc1: &PointClassification<O>,
    pc2: &PointClassification<O>,
    position: Vec3,
    tolerance: f64,
    limit: f64,
) -> Result<DeclaredContact> {
    let edge_of = |pc: &PointClassification<O>| {
        (pc.point_class() == PointClassType::Edge && pc.are_parameters_set())
            .then(|| pc.edge_object())
            .flatten()
            .and_then(|e| geometry.edge_curve(e))
            .flatten()
            .and_then(|c| geometry.curve(c))
            .map(|c| (c, pc.t_param()))
    };
    let (edge1, edge2) = (edge_of(pc1), edge_of(pc2));
    // (the crossing curve and its parameter, `None` for a face's surface; the edge and its
    // parameter, the edge's classification)
    let mut crossings = Vec::new();
    if let Some((curve, t)) = crossing.and_then(|(c, t)| geometry.curve(c).map(|c| (c, t))) {
        for ((edge, pc), along) in [(edge1, pc1), (edge2, pc2)].into_iter().zip(along) {
            // (not upstream, tangency-labs/solidcore#3339: an interval running along a seam
            // ends where it leaves it, not where it crosses it)
            if along {
                continue;
            }
            if let Some((e, s)) = edge {
                crossings.push((Some((curve, t)), e, s, pc));
            }
        }
    } else {
        let on_face = |pc: &PointClassification<O>| pc.point_class() == PointClassType::Face;
        match (edge1, edge2) {
            (Some((a, t)), Some((e, s))) => crossings.push((Some((a, t)), e, s, pc2)),
            (Some((e, s)), None) if on_face(pc2) => crossings.push((None, e, s, pc1)),
            (None, Some((e, s))) if on_face(pc1) => crossings.push((None, e, s, pc2)),
            _ => {}
        }
    }
    // (the edge's direction, the reach along it, the edge and its parameter)
    let mut worst: Option<(Vec3, f64, &Curve, f64)> = None;
    for (curve, edge, s, pc) in crossings {
        let Some(along) = edge_direction(k, geometry, pc)? else {
            continue;
        };
        let reach = match curve {
            Some((curve, t)) => curve
                .crossing_reach(k, t, edge, s, position, tolerance)?
                .map_or(limit, |reach| reach.min(limit)),
            None => limit,
        };
        if worst.is_none_or(|(_, old, _, _)| reach > old) {
            worst = Some((along, reach, edge, s));
        }
    }
    let Some((along, reach, edge, s)) = worst else {
        return Ok(DeclaredContact::of_point(position, tolerance));
    };
    let stray = edge.stray_from_line(k, s, along, position, reach)?;
    Ok(DeclaredContact::of_crossing(
        position,
        tolerance,
        along,
        stray.map(|stray| (reach, stray)),
        limit,
    ))
}

/// Not upstream (tangency-labs/solidcore#3339): whether an interval whose end classifies as
/// `end` and middle as `mid` runs along a seam that end lies on, inside the other B-rep's
/// face (`other_mid`): both on the same edge, one with the same face on every use. A seam bounds no material (docs/booleans.md, "Seams and
/// poles"), so the face pair that classified the run is the only one that meets it there,
/// and where the run leaves the seam is where its curve does, known to its own tolerance:
/// an intersection curve running within the tolerance of the seam does not cross it at
/// that end, and its angle to the seam fixes no reach along it ([`crossing_region`]).
///
/// # Errors
///
/// Those of the edge's use walk.
fn runs_along_seam<O: ClassObject>(
    geometry: &impl ProtoBrep<O>,
    end: &PointClassification<O>,
    mid: &PointClassification<O>,
    other_mid: &PointClassification<O>,
) -> Result<bool> {
    if other_mid.point_class() != PointClassType::Face {
        return Ok(false);
    }
    let Some(e) = end.edge_object() else {
        return Ok(false);
    };
    if mid.edge_object() != Some(e) {
        return Ok(false);
    }
    let faces = geometry.edge_use_faces(e)?;
    Ok(match faces.split_first() {
        Some((first, rest)) => {
            first.is_some() && !rest.is_empty() && rest.iter().all(|f| f == first)
        }
        None => false,
    })
}

/// Not upstream (WR-410; the SolidCraft fork's `sm_CrossingSine`): the sine of the angle
/// at which a face pair's intersection curve crosses an edge at one end (`at_end`, else
/// the start) of the curve interval pair `ci1`, `ci2`: the smaller of the two when both
/// ends lie on edges, `-1` when neither does.
///
/// # Errors
///
/// Those of [`edge_crossing_sine`].
fn crossing_sine<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ProtoBrep<O>,
    classification1: &CurveClassification<O>,
    ci1: &CurveInterval<O>,
    classification2: &CurveClassification<O>,
    ci2: &CurveInterval<O>,
    at_end: bool,
) -> Result<f64> {
    let pick = |ci: &CurveInterval<O>| if at_end { ci.end } else { ci.start };
    let sine1 = edge_crossing_sine(
        k,
        geometry,
        classification1.curve(),
        ci1.interval,
        &pick(ci1),
        at_end,
    )?;
    let sine2 = edge_crossing_sine(
        k,
        geometry,
        classification2.curve(),
        ci2.interval,
        &pick(ci2),
        at_end,
    )?;
    if sine1 < 0.0 {
        return Ok(sine2);
    }
    if sine2 < 0.0 {
        return Ok(sine1);
    }
    Ok(sine1.min(sine2))
}

/// Not upstream: moves `pc1` and `pc2`, a point classified onto an edge of each B-rep, to
/// where those two edges touch when they are circles in one plane that the contact rule
/// takes for touching (`circle_coplanar_circle_intersect_in_contact` at `contact_tol_3d`,
/// as the edge/curve intersections of the boolean read them), and the touching point lies
/// on both edges within their intersection tolerance. Any other pair is left as it is.
///
/// # Errors
///
/// Those of the circles' intersection; an unported or cancelled drop of the touching point.
fn touch_of_edges<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    pc1: &mut PointClassification<O>,
    pc2: &mut PointClassification<O>,
) -> Result<()> {
    if pc1.point_class() != PointClassType::Edge || pc2.point_class() != PointClassType::Edge {
        return Ok(());
    }
    // an edge's circle: centre, unit normal, radius
    let circle = |pc: &PointClassification<O>| -> Result<Option<(Point3, Vec3, f64)>> {
        let Some((b, EntityRef::Edge(e))) = pc.object().and_then(|o| geometry.topology_entity(o))
        else {
            return Ok(None);
        };
        let Some(curve) = b.edge(e)?.curve.as_ref() else {
            return Ok(None);
        };
        let CurveKind::Ellipse(c) = curve.kind() else {
            return Ok(None);
        };
        if !c.is_circle() {
            return Ok(None);
        }
        let place = c.position();
        Ok(Some((place.origin(), place.z_axis(), c.x_radius())))
    };
    let (Some((c1, n1, r1)), Some((c2, n2, r2))) = (circle(pc1)?, circle(pc2)?) else {
        return Ok(());
    };
    let tol = xsect_tol_3d_from_zones(
        k,
        pc1.obj_zone_tol_3d(k, geometry)?,
        pc2.obj_zone_tol_3d(k, geometry)?,
    );
    // one plane: parallel normals (the angle test of the circles' own intersection) and
    // the second centre on the first plane within the tolerance
    if n1.cross(n2).length() > EFF_ZERO_SQRT || (c2 - c1).dot(n1).abs() > tol {
        return Ok(());
    }
    let CircleCircle::Tangent(touch) =
        circle_coplanar_circle_intersect_in_contact(n1, c1, r1, c2, r2, tol, contact_tol_3d(k))?
    else {
        return Ok(());
    };
    // the touching point on both edges' arcs: dropped onto each edge's interval within
    // their tolerance (a drop that fails leaves the point where it was)
    let (mut q1, mut q2) = (*pc1, *pc2);
    for q in [&mut q1, &mut q2] {
        match q.compute_point_parameters(k, geometry, touch) {
            Ok(()) if q.deviation() <= tol => {}
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Ok(()) | Err(_) => return Ok(()),
        }
    }
    *pc1 = q1;
    *pc2 = q2;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::p1::proto::{EdgeDefinition, ProtoGeometry, ProtoTopology, VertexDefinition};
    use super::*;
    use crate::curve::line::Line;
    use crate::geom::extent2d::Extent2d;
    use crate::intersect::point_class::PointClassType;
    use crate::math::{Point2, Point3};
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::surface::surface::Surface;
    use crate::surface::volume::Volume;
    use crate::topology::Brep;

    /// An intersector without relationships has no mated pair to unlink.
    #[test]
    fn no_mates_has_no_relationship_to_remove() {
        let e = Mates::<H>::remove_relationship(&mut NoMates, vtx(0), vtx(1)).unwrap_err();
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(
            e.topology_failure(),
            Some(&crate::error::topology::TopologyFailure::Failed(
                "the two are not a mated pair"
            ))
        );
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(usize, ClassObjectKind);

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            self.1
        }
    }

    const fn vtx(i: usize) -> H {
        H(i, ClassObjectKind::Vertex)
    }

    const fn edg(i: usize) -> H {
        H(i, ClassObjectKind::Edge)
    }

    /// Vertices 0 and 1 of B-rep 1 coincide and are joined by the long edges 0 and 1
    /// (both on curve 0); vertices 2 and 3 of B-rep 2 are far apart. Records every
    /// `GlueVertices` call and refuses one of a vertex already glued away; records
    /// every `SqueezeEdge` and `DeleteEdge` (after a squeeze, `edge_other_vertex` answers
    /// vertex 0).
    struct Pool {
        curves: Vec<Curve>,
        glued: Vec<(H, H)>,
        squeezed: Vec<H>,
        deleted: Vec<H>,
    }

    impl Pool {
        fn new() -> Self {
            Self::with_length(10.0)
        }

        /// As [`Pool::new`] with edges `length` long.
        fn with_length(length: f64) -> Self {
            let line = Line::create_line_segment(
                3,
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(length, 0.0, 0.0),
                None,
            )
            .unwrap();
            Self {
                curves: vec![Curve::from(line)],
                glued: Vec::new(),
                squeezed: Vec::new(),
                deleted: Vec::new(),
            }
        }
    }

    impl ClassGeometry<H> for Pool {
        fn curve(&self, o: H) -> Option<&Curve> {
            (o.1 == ClassObjectKind::Curve)
                .then(|| self.curves.get(o.0))
                .flatten()
        }
        fn surface(&self, _o: H) -> Option<&Surface> {
            None
        }
        fn volume(&self, _o: H) -> Option<&Volume> {
            None
        }
        fn point(&self, _o: H) -> Option<Point3> {
            None
        }
        /// Vertices 4.. lie on the x axis: 4 at x = 20, then 1.5e-5 and 2.8e-5 past it.
        fn classification_vertex_point(&self, vertex: H) -> Option<Point3> {
            let dx = match vertex.0 {
                4 => 0.0,
                5 => 1.5e-5,
                6 => 2.8e-5,
                _ => return None,
            };
            Some(Point3::new(20.0 + dx, 0.0, 0.0))
        }
        fn topology_zone_tol_3d(&self, object: H) -> Option<f64> {
            (object.1 == ClassObjectKind::Vertex && object.0 >= 4).then_some(1e-5)
        }
    }

    impl ProtoGeometry<H> for Pool {
        fn curve_mut(&mut self, o: H) -> Option<&mut Curve> {
            (o.1 == ClassObjectKind::Curve)
                .then(|| self.curves.get_mut(o.0))
                .flatten()
        }
    }

    impl ProtoTopology<H> for Pool {
        fn vertex_use_faces(&self, _vertex: H) -> Result<Vec<Option<H>>> {
            Ok(Vec::new())
        }
        fn edge_use_faces(&self, _edge: H) -> Result<Vec<Option<H>>> {
            Ok(Vec::new())
        }
        fn edge_start_vertex(&self, _edge: H) -> Result<Option<H>> {
            Ok(Some(vtx(0)))
        }
        fn edge_other_vertex(&self, _edge: H, _vertex: Option<H>) -> Result<Option<H>> {
            Ok(Some(if self.squeezed.is_empty() {
                vtx(1)
            } else {
                vtx(0)
            }))
        }
        fn vertex_point(&self, vertex: H) -> Result<Point3> {
            Ok(match vertex.0 {
                2 => Point3::new(0.0, 5.0, 0.0),
                3 => Point3::new(0.0, -5.0, 0.0),
                _ => Point3::new(0.0, 0.0, 0.0),
            })
        }
        fn edge_brep(&self, _edge: H) -> Result<Option<crate::geom::attribute::ObjectKey>> {
            Ok(None)
        }
        fn edge_faces(&self, _edge: H) -> Result<Vec<H>> {
            Ok(Vec::new())
        }
    }

    impl ProtoBrep<H> for Pool {
        fn edge_interval(&self, _edge: H) -> Option<Extent1d> {
            Some(self.curves.first()?.natural_interval())
        }
        fn edge_curve(&self, _edge: H) -> Option<Option<H>> {
            Some(Some(H(0, ClassObjectKind::Curve)))
        }
        fn topology_tolerance(&self, _topo: H) -> Option<f64> {
            Some(1e-5)
        }
        fn face_uv_domain(&self, _face: H) -> Option<Extent2d> {
            None
        }
        fn set_face_uv_domain(&mut self, _face: H, _domain: Extent2d) -> Option<()> {
            None
        }
        fn face_surface(&self, _face: H) -> Option<Option<H>> {
            None
        }
        fn edge_edgeuse_of_face(&self, _edge: H, _face: H) -> Result<Option<H>> {
            Ok(None)
        }
        fn edge_is_connected_to_face(&self, _edge: H, _face: H) -> Result<bool> {
            Ok(false)
        }
        fn insert_curve(&mut self, curve: Curve) -> Option<H> {
            self.curves.push(curve);
            Some(H(self.curves.len() - 1, ClassObjectKind::Curve))
        }
        fn vertex_common_edges(&self, v1: H, v2: H) -> Result<Vec<H>> {
            Ok(if (v1, v2) == (vtx(0), vtx(1)) {
                vec![edg(0), edg(1)]
            } else {
                Vec::new()
            })
        }
        fn face_curve_on_classify(
            &self,
            _k: &Kernel,
            _face: H,
            _do_point_classify: bool,
            _cc: &mut CurveClassification<H>,
        ) -> Result<bool> {
            Ok(false)
        }
        fn face_trim_uv_domain(&mut self, _k: &Kernel, _face: H) -> Result<Extent2d> {
            Err(SolidError::invalid_input("test", "unused"))
        }
        fn face_point_classify_uv(
            &mut self,
            _k: &Kernel,
            _face: H,
            _uv: Point2,
            _source_tolerance: f64,
        ) -> Result<PointClassType> {
            Err(SolidError::invalid_input("test", "unused"))
        }
        fn brep_squeeze_edge(
            &mut self,
            _k: &mut Kernel,
            _brep: H,
            edge: H,
            _survivor: H,
        ) -> Result<()> {
            self.squeezed.push(edge);
            Ok(())
        }
        fn brep_point_3d_classify(
            &mut self,
            _k: &Kernel,
            _brep: H,
            _point: Point3,
            _tolerance: f64,
            _boundary_intersections: bool,
        ) -> Result<PointClassification<H>> {
            Err(SolidError::invalid_input("test", "unused"))
        }
        fn brep_make_edge_in_face(
            &mut self,
            _k: &Kernel,
            _brep: H,
            _face: H,
            _start: H,
            _end: H,
            _curve: Curve,
            _uv_curve: Option<Curve>,
            _interval: Extent1d,
            _orientation: OrientType,
            _tolerance: f64,
        ) -> Result<(Option<H>, Option<H>, Option<H>)> {
            Err(SolidError::invalid_input("test", "unused"))
        }
        fn edgeuse_evaluate_binormal(&mut self, _k: &Kernel, _eu: H, _param: f64) -> Result<Vec3> {
            Err(SolidError::invalid_input("test", "unused"))
        }
    }

    impl BrepGlue<H> for Pool {
        fn delete_edge(&mut self, _k: &mut Kernel, _brep: H, edge: H) -> Result<()> {
            self.deleted.push(edge);
            Ok(())
        }
        fn glue_vertices(
            &mut self,
            _k: &mut Kernel,
            _brep: H,
            v1: H,
            v2: H,
            _opt_wire_edges_in_brep: Option<&[H]>,
        ) -> Result<()> {
            let again = self.glued.iter().any(|&(_, gone)| gone == v2);
            self.glued.push((v1, v2));
            if again {
                return Err(SolidError::invalid_input(
                    "test",
                    "vertex already glued away",
                ));
            }
            Ok(())
        }
    }

    fn region_pc(o: H) -> PointClassification<H> {
        let mut pc = PointClassification::new(1e-5);
        pc.set_class_object(PointClassType::Region, Some(o));
        pc
    }

    #[test]
    fn glue_stops_once_the_second_vertex_is_glued_away() {
        // da8d05e (#371): two coincident vertices joined by two long common edges;
        // gluing for the first edge deletes the second vertex, which the second
        // edge's step glued again (upstream: a use-after-free)
        let mut k = Kernel::default();
        let mut pool = Pool::new();
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        let mut m = ProtoTopologyManager::new(&k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap();
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let r = H(50, ClassObjectKind::Region);
        for (v1, v2) in [(vtx(0), vtx(2)), (vtx(1), vtx(3))] {
            let mut vd =
                VertexDefinition::new(&k, &pool, &region_pc(r), &region_pc(r), p, 1e-5, 1e-6, None)
                    .unwrap();
            vd.set_final_vertex(1, Some(v1));
            vd.set_final_vertex(2, Some(v2));
            let id = m.insert_vertex_definition(vd).unwrap();
            m.add_vertex_definition(pv, id).unwrap();
        }

        m.glue_coincident_brep_vertices(&mut k, &mut pool, &mut NoMates)
            .unwrap();
        assert_eq!(pool.glued, vec![(vtx(0), vtx(1))]);
        // the second definition, whose B-rep 1 vertex is gone, is deleted
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 1);
    }

    #[test]
    fn glue_deletes_a_second_short_common_edge_after_the_squeeze() {
        // the delete branch runs as upstream after a squeeze;
        // boolean.api_brep crossing_creases_union_d1e-05
        let mut k = Kernel::default();
        let mut pool = Pool::with_length(1e-6);
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        let mut m = ProtoTopologyManager::new(&k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap();
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let r = H(50, ClassObjectKind::Region);
        for (v1, v2) in [(vtx(0), vtx(2)), (vtx(1), vtx(3))] {
            let mut vd =
                VertexDefinition::new(&k, &pool, &region_pc(r), &region_pc(r), p, 1e-5, 1e-6, None)
                    .unwrap();
            vd.set_final_vertex(1, Some(v1));
            vd.set_final_vertex(2, Some(v2));
            let id = m.insert_vertex_definition(vd).unwrap();
            m.add_vertex_definition(pv, id).unwrap();
        }

        m.glue_coincident_brep_vertices(&mut k, &mut pool, &mut NoMates)
            .unwrap();
        assert_eq!(pool.squeezed, vec![edg(0)]);
        assert_eq!(pool.deleted, vec![edg(1)]);
        assert!(pool.glued.is_empty());
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 1);
    }

    /// A vertex definition first placed by a grazing crossing is placed again by a later
    /// one whose misplacement bound along the edge (gap over sine) is smaller (here the
    /// same gap at a steeper angle), and not by one whose bound is larger (WR-410, #1059).
    /// Its points lie on a curve, not an edge, so no moved edge stretch needs a certificate.
    #[test]
    fn issue_1059_a_steeper_crossing_places_the_vertex_definition() {
        let k = Kernel::default();
        let pool = Pool::new();
        let curve = H(0, ClassObjectKind::Curve);
        let on_curve = |t: f64| {
            let mut pc = PointClassification::new(1e-6);
            pc.set_class_object(PointClassType::Curve, Some(curve));
            pc.set_t_param(t, false);
            pc
        };
        // two points 1e-8 apart, within the curve's zone tolerance
        let (pc1, pc2) = (on_curve(0.5), on_curve(0.5 + 1e-9));
        let mut vd = VertexDefinition::new(
            &k,
            &pool,
            &pc1,
            &pc2,
            Vec3::new(5.0, 0.0, 0.0),
            1e-6,
            0.0,
            None,
        )
        .unwrap();
        // the first crossing to arrive places it, here at a grazing angle
        assert!(!vd
            .place_by_crossing(&k, &pool, &pc1, &pc2, 0.02, None)
            .unwrap());
        // a steeper crossing fixes it more closely
        assert!(vd
            .place_by_crossing(&k, &pool, &pc1, &pc2, 0.9, None)
            .unwrap());
        // a more grazing one than that does not
        assert!(!vd
            .place_by_crossing(&k, &pool, &pc1, &pc2, 0.5, None)
            .unwrap());
    }

    /// A proto vertex relates two vertices within their intersection tolerance (two zones
    /// of 1e-5: 2e-5), and none farther apart: vertices 1.5e-5 apart make one, vertices
    /// 2.8e-5 apart (the corners of two pie slices whose arc centre misses the apex by
    /// 4.85e-5, each within 1.41e-5 of the cylinders' crossing) make none, where upstream
    /// made one of tolerance 2.8e-5 that merged the two corners.
    #[test]
    fn a_proto_vertex_never_relates_vertices_farther_apart_than_their_tolerance() {
        let k = Kernel::default();
        let pool = Pool::new();
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        let mut m = ProtoTopologyManager::new(&k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap();
        let on = |v: usize| {
            let mut pc = PointClassification::new(1e-5);
            pc.set_class_object(PointClassType::Vertex, Some(vtx(v)));
            pc
        };
        let (pv, created) = m
            .find_or_create_proto_vertex_pcs(&k, &pool, &mut on(4), &mut on(6))
            .unwrap();
        assert_eq!((pv, created), (None, false));
        assert!(m.proto_vertices().is_empty());
        let (pv, created) = m
            .find_or_create_proto_vertex_pcs(&k, &pool, &mut on(4), &mut on(5))
            .unwrap();
        assert!(pv.is_some() && created);
    }

    /// The vertices' distance is measured even when a classification starts without
    /// parameters, where `sm_FindPositionFromPCs` reports a deviation of 0: vertices
    /// 2.8e-5 apart (past the 2e-5 band) still make no proto vertex, either way round.
    #[test]
    fn vertices_without_parameters_are_measured_before_they_are_related() {
        let k = Kernel::default();
        let pool = Pool::new();
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        let mut m = ProtoTopologyManager::new(&k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap();
        let on = |v: usize, set: bool| {
            let mut pc = PointClassification::new(1e-5);
            pc.set_class_object(PointClassType::Vertex, Some(vtx(v)));
            if !set {
                pc.unset_parameters();
            }
            pc
        };
        let (mut unset, mut set) = (on(6, false), on(4, true));
        let found = find_position_from_pcs(&k, &pool, &mut unset, &mut set).unwrap();
        assert_eq!(found.deviation, 0.0);
        for (a, c) in [(false, true), (true, false)] {
            let (pv, created) = m
                .find_or_create_proto_vertex_pcs(&k, &pool, &mut on(4, a), &mut on(6, c))
                .unwrap();
            assert_eq!((pv, created), (None, false), "{a} {c}");
        }
        assert!(m.proto_vertices().is_empty());
        let (pv, created) = m
            .find_or_create_proto_vertex_pcs(&k, &pool, &mut on(4, false), &mut on(5, true))
            .unwrap();
        assert!(pv.is_some() && created);
    }

    /// A proto edge is not split at a point its definition refuses to split at: one
    /// 5e-5 from its end, farther than the proto edge's tolerance (1e-5) but within ten
    /// times the definition's. The split once went ahead without the definition, leaving
    /// an upper child with no definitions, on which the boolean stopped ("a proto edge
    /// has no edge definitions"); a join onto a wall whose sketch had two corner arcs
    /// tangent up to round-off met it.
    #[test]
    fn a_split_its_definition_refuses_leaves_the_proto_edge_whole() {
        let k = Kernel::default();
        let mut pool = Pool::new();
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        let mut m = ProtoTopologyManager::new(&k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap();
        let ivl = pool.curves[0].natural_interval();
        let start = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let end = m
            .new_proto_vertex(Point3::new(10.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(start);
        m.add_proto_vertex(end);
        let pe = m.new_proto_edge(&k, Some(start), Some(end), ivl).unwrap();
        m.add_proto_edge(pe);
        for pv in [start, end] {
            m.proto_vertex_mut(pv).unwrap().add_proto_edge(pe);
        }
        let curve = H(0, ClassObjectKind::Curve);
        let ed = EdgeDefinition::new(Some(curve), ivl, edg(0), edg(1), 1e-5, None, None, None);
        let ed = m.insert_edge_definition(ed).unwrap();
        m.add_edge_definition(&k, &mut pool, pe, ed).unwrap();

        // 5e-5 short of the end of the 10-long line
        let dt = ivl.min() + (ivl.max() - ivl.min()) * (1.0 - 5e-6);
        let split = m.proto_edge_split(&k, &mut pool, pe, dt).unwrap();
        assert_eq!(split, (None, None));
        assert_eq!(m.proto_edges(), [pe]);
        assert_eq!(m.proto_edge(pe).unwrap().edge_definitions(), [ed]);
        assert_eq!(m.proto_edge(pe).unwrap().xsect_interval(), ivl);
    }

    /// Two coincident lines overlapping along 5e-5 (`x = 0..10` and `x = 9.99995..20`)
    /// overlap within one end of each, where their definitions refuse to split. There is
    /// no piece of either to combine: combining the whole edges would put the second's
    /// definition on the first's vertices. The pairwise split refuses before changing
    /// anything, and each proto edge keeps its definition on its own vertices.
    #[test]
    fn an_overlap_within_the_no_split_zone_of_an_end_is_refused_unchanged() {
        let k = Kernel::default();
        let mut pool = Pool::new();
        let b0 = 9.99995;
        pool.curves.push(Curve::from(
            Line::create_line_segment(3, Vec3::new(b0, 0.0, 0.0), Vec3::new(20.0, 0.0, 0.0), None)
                .unwrap(),
        ));
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        let mut m = ProtoTopologyManager::new(&k, (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0).unwrap();
        let mut edge = |m: &mut ProtoTopologyManager<H>, ci: usize, from: f64, to: f64| {
            let ivl = pool.curves[ci].natural_interval();
            let s = m
                .new_proto_vertex(Point3::new(from, 0.0, 0.0), 1e-5)
                .unwrap();
            let t = m.new_proto_vertex(Point3::new(to, 0.0, 0.0), 1e-5).unwrap();
            m.add_proto_vertex(s);
            m.add_proto_vertex(t);
            let pe = m.new_proto_edge(&k, Some(s), Some(t), ivl).unwrap();
            m.add_proto_edge(pe);
            for pv in [s, t] {
                m.proto_vertex_mut(pv).unwrap().add_proto_edge(pe);
            }
            let curve = H(ci, ClassObjectKind::Curve);
            let ed = EdgeDefinition::new(
                Some(curve),
                ivl,
                edg(2 * ci),
                edg(2 * ci + 1),
                1e-5,
                None,
                None,
                None,
            );
            let ed = m.insert_edge_definition(ed).unwrap();
            m.add_edge_definition(&k, &mut pool, pe, ed).unwrap();
            (pe, ed, ivl)
        };
        let (pe1, ed1, ivl1) = edge(&mut m, 0, 0.0, 10.0);
        let (pe2, ed2, ivl2) = edge(&mut m, 1, b0, 20.0);

        assert!(m
            .split_intersecting_proto_edges(&k, &mut pool, pe1, pe2)
            .is_err());
        assert_eq!(m.proto_edges(), [pe1, pe2]);
        for (pe, ed, ivl, from, to) in [(pe1, ed1, ivl1, 0.0, 10.0), (pe2, ed2, ivl2, b0, 20.0)] {
            let p = m.proto_edge(pe).unwrap();
            assert_eq!(p.edge_definitions(), [ed]);
            assert_eq!(p.xsect_interval(), ivl);
            let at = |v: Option<ProtoVertexId>| m.proto_vertex(v.unwrap()).unwrap().position();
            assert_eq!(at(p.start_proto_vertex()), Point3::new(from, 0.0, 0.0));
            assert_eq!(at(p.end_proto_vertex()), Point3::new(to, 0.0, 0.0));
        }
    }

    /// An `n`-sided prism of circumradius 30 from `z0` up `h`.
    fn prism(k: &mut Kernel, n: usize, z0: f64, h: f64) -> Brep {
        use crate::curve::curve::Curve;
        let point = |i: usize| {
            let t = core::f64::consts::TAU * i as f64 / n as f64;
            Vec3::new(30.0 * t.cos(), 30.0 * t.sin(), z0)
        };
        let curves: Vec<Curve> = (0..n)
            .map(|i| {
                Curve::from(
                    crate::primitive::api_curves::create_line_segment(point(i), point(i + 1))
                        .unwrap(),
                )
            })
            .collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        crate::sweep::api_brep::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), h, true)
            .unwrap()
    }

    /// A Join of a prism into a prism of the same outline (z 1..2 into z 0..2: coincident
    /// sides and top) splits its proto edges where they cross. Every pair of proto edges
    /// is asked; a pair of lines whose boxes are apart now ends before any curve
    /// intersection (#14329). The intersections run stay flat as the outline grows:
    /// none here, where 558 (12 sides) and 10,008 (48 sides) ran before, growing with
    /// N². The union is the outer prism.
    #[test]
    fn a_join_of_coincident_prisms_intersects_no_far_apart_edges() {
        let mut counts = Vec::new();
        for n in [12, 48] {
            let mut k = Kernel {
                fixes: crate::kernel::UpstreamFixes::all(),
                ..Kernel::default()
            };
            let a = prism(&mut k, n, 0.0, 2.0);
            let b = prism(&mut k, n, 1.0, 1.0);
            k.work_counters.reset();
            let u = crate::boolean::api_brep::boolean_union(&mut k, a, b).unwrap();
            counts.push(k.work_counters.proto_edge_intersections());
            assert_eq!(u.faces().len(), n + 2, "{n} sides");
            assert_eq!(u.edges().len(), 3 * n, "{n} sides");
        }
        assert_eq!(counts, [0, 0]);
    }
}
