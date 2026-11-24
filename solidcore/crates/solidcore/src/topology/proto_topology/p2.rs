//! Proto topology, part 2: creating, finding, combining and resolving proto vertices,
//! proto edges and their definitions, the imprint of vertex and edge definitions into
//! both B-reps, and the updates of the proto topology after an imprint split an edge
//! or a face.
//!
//! Upstream: `source/SMLib/src/SmProtoTopology.cpp`, `source/SMLib/inc/SmProtoTopology.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`ProtoBrep`] | the B-rep members these functions read through `SmEdge*` / `SmFace*` / `SmVertex*` (`GetInterval`, `GetCurve`, `GetTolerance`, `GetUVDomain`, `SetUVDomain`, `GetSurface`, `GetEdgeuseOfFace`, `IsConnectedToFace`), `SmCurve::Copy`'s allocation, and the B-rep edits of `SmPointClassification::MergeIntoObject` ([`ProtoBrep::merge_point_class_into_brep`]) |
//! | [`ProtoTopologyManager::imprint_vertex_definition`] | `SmVertexDefinition::Imprint` |
//! | [`ProtoTopologyManager::imprint_edge_definition`] | `SmEdgeDefinition::Imprint` |
//! | [`ProtoTopologyManager::resolve_proto_vertex`] | `SmProtoVertex::Resolve` |
//! | [`ProtoTopologyManager::add_vertex_definition_from_pcs`] | `SmProtoVertex::AddVertexDefinition(SmPointClassification&, SmPointClassification&, ..)` |
//! | [`ProtoTopologyManager::find_matching_edge_definition`] | `SmProtoEdge::FindMatchingEdgeDefinition` |
//! | [`ProtoTopologyManager::add_edge_definition`] | `SmProtoEdge::AddEdgeDefinition(SmEdgeDefinition*)` |
//! | [`ProtoTopologyManager::add_edge_definition_from_intervals`] | `SmProtoEdge::AddEdgeDefinition(const SmCurveInterval&, const SmCurveInterval&)` |
//! | [`ProtoTopologyManager::combine_proto_edge`] | `SmProtoEdge::CombineProtoEdge` |
//! | [`ProtoTopologyManager::resolve_proto_edge`] | `SmProtoEdge::Resolve` |
//! | [`ProtoTopologyManager::create_vertex_definition`] | `SmProtoTopologyManager::CreateVertexDefinition` |
//! | [`ProtoTopologyManager::create_proto_vertex`] | `SmProtoTopologyManager::CreateProtoVertex` |
//! | [`ProtoTopologyManager::find_or_create_proto_vertex`] | `SmProtoTopologyManager::FindOrCreateProtoVertex` |
//! | [`ProtoTopologyManager::find_proto_edge`] | `SmProtoTopologyManager::FindProtoEdge` |
//! | [`ProtoTopologyManager::create_proto_edge`] | `SmProtoTopologyManager::CreateProtoEdge` |
//! | [`ProtoTopologyManager::remove_edge_definition`] | `SmProtoTopologyManager::RemoveEdgeDefinition` |
//! | [`ProtoTopologyManager::update_edges`] | `SmProtoTopologyManager::UpdateEdges` |
//! | [`ProtoTopologyManager::update_faces`] | `SmProtoTopologyManager::UpdateFaces` |
//! | [`ProtoTopologyManager::can_combine_vertex_definitions`] | `SmProtoTopologyManager::CanCombineVertexDefinitions` |
//! | [`ProtoBrep::brep_make_edge_in_face`], [`make_edge_in_face_in_context`] | `SmBrep::MakeEdgeInFace` of `SmEdgeDefinition::Imprint` on `GetWhichBrep(i)` |
//!
//! Deviations:
//! - **B-rep access.** The topology objects are the caller's handles `O`, as in
//!   [`p1`](super::p1). The stored members of edges, faces and vertices these functions
//!   read or write, the two connectivity walks of `UpdateFaces`, and the allocation of
//!   curve copies go through [`ProtoBrep`], which the owner of the B-reps implements
//!   (upstream reaches them through the pointers). `SmTol::GetZoneTol3d` of a vertex,
//!   edge or face is [`ProtoBrep::topology_tolerance`]; of any other object
//!   [`object_zone_tol_3d`].
//! - **Imprinting into a face.** `SmBrep::MakeEdgeInFace` of `SmEdgeDefinition::Imprint`
//!   is [`ProtoBrep::brep_make_edge_in_face`]: the owner runs part 5's arena entry point
//!   on the B-rep behind its handles (most through [`make_edge_in_face_in_context`]),
//!   which checks that B-rep's own `m_bEditingEnabled`. The curves it consumes are
//!   passed by value (the definition's 3D curve is cloned out of the owner's store,
//!   where its handle stays valid for the later `Copy` for B-rep 2, as upstream's
//!   pointer does); `SM_CAST_PTR(SmBSplineCurve, m_pXSectUVCurve<i>)` passes the UV
//!   curve only when it is of a B-spline kind.
//! - **Deleted curves.** Functions that delete curves (through `Destruct` or the
//!   destructors) return the handles deleted, as in [`p1`](super::p1).
//! - **Undefined behaviour upstream** is [`SolidError::InvalidInput`]: dereferencing a
//!   null or deleted object (plan/upstream-issues.md UB-117, UB-118), and the reads of an
//!   uninitialised position, gap or tangent after an ignored failure (of
//!   `sm_FindPositionFromPCs` in `CreateVertexDefinition` and `CreateProtoVertex`, UB-115;
//!   of the mid tangents in `AddEdgeDefinition`, UB-116).
//! - **Leaks.** On the error returns of `CreateProtoEdge`, `AddEdgeDefinition` (from
//!   intervals) and `Imprint` upstream leaks the proto edge or curve copies it made; the
//!   checks are done before anything is made, so nothing is leaked.
//! - **Out-params** become return values: `AddVertexDefinition`'s definition and
//!   created flag, `FindOrCreateProtoVertex`'s created flag, `FindProtoEdge`'s
//!   identical flag, `CanCombineVertexDefinitions`' choice (its `SM_ERR_NULL_POINTER`
//!   return leaves the choice 0). `FindMatchingEdgeDefinition`'s `rbRev` is always
//!   `FALSE` upstream and dropped.
//! - **Curve intervals** are passed with the classification they belong to (upstream
//!   reaches it through the interval's back pointer; `SmCurveInterval::GetCurve` is the
//!   classification's curve).
//! - **Not upstream: no split on a vertex** (tangency-labs/solidcore#480).
//!   `SmProtoEdge::Resolve` splits a proto edge that runs from a proto vertex back to
//!   itself along an open curve longer than `2 * m_dTol` by adding a vertex definition
//!   that copies the first one's classifications. `CanCombineVertexDefinitions` has
//!   just merged definitions up to `5 * (tol1 + tol2)` apart into that proto vertex,
//!   keeping the one on a vertex. When the copied definition is on a vertex in either
//!   B-rep, both ends land on that one vertex there (moved twice, to their midpoint)
//!   while the other B-rep gets a short edge between two vertices; the merge then
//!   fails in `MakeFaceuse` (a union whose coplanar faces' rims cross 4e-5 to 2e-4 mm
//!   from a vertex, `SM_ERR`), or returns a body whose loops gap by the curve's length
//!   (a box corner inside a sphere by less than the summed tolerances). So
//!   [`ProtoTopologyManager::resolve_proto_edge`] treats such a proto edge as
//!   degenerate instead, as upstream does for one shorter than `2 * m_dTol`, when the
//!   whole curve stays within its ends' reach of the definition (a certain hull of the
//!   curve, `curve_piece_within`); a curve that leaves it is refused. The `SM_ERR` trigger
//!   is confirmed on the C++ build (WR-214); the failing body is Rust-measured.
//! - **Not upstream (WR-306, #1058).** [`ProtoTopologyManager::resolve_proto_edge`] also
//!   treats a proto edge from a proto vertex back to itself as degenerate when its curve
//!   ends find one vertex definition and every definition's curve provably lies within that
//!   definition's tolerance of its position (a certified hull, `curve_piece_within`), and
//!   marks the proto vertex as having absorbed it; the Boolean's `ImprintIntersections`
//!   then fits that vertex to its edges. The SMLib fork's fix bounded the curve's length by
//!   twice the proto vertex's containment radius instead, which is at least the capture
//!   distance (0.01 in the Boolean), so it absorbed real closed curves (a circle of radius
//!   1e-3) and replaced upstream's `2 * m_dTol` test instead of adding to it.
//! - **Not upstream (WR-579, #1517).** [`ProtoTopologyManager::resolve_proto_edge`]
//!   splits a proto edge from a proto vertex back to itself whose curve ends find two of
//!   its definitions at each other definition of that proto vertex lying on the curve,
//!   then resolves the proto vertex (the SMLib fork's tangency-labs/smlib@9eec159);
//!   upstream keeps the edge whole past that vertex.
//! - **Not upstream (WR-580, #1519; the SMLib fork's tangency-labs/smlib@81cd61a).**
//!   [`ProtoTopologyManager::imprint_edge_definition`] takes an edge Brep2's face already
//!   has between the definition's two vertices along its curve as the definition's edge
//!   there instead of making a second one; upstream makes it, and the merge leaves
//!   coincident edges. The fork finds the edge with `SmTools::FindCoincidentEdge` (30
//!   samples); here the two whole curves must be proved within tolerance on control points
//!   (`curve_runs_along`).
//! - **Not upstream ("One edge or two", docs/booleans.md; own design).**
//!   [`ProtoTopologyManager::imprint_edge_definition`] first takes an edge the face of
//!   either B-rep already has between the definition's two vertices when the two are
//!   proved one curve by enclosure, each within the two's tolerance plus the edge's own
//!   ([`ProtoBrep::face_edge_proved_along`], [`crate::curve::proximity`]); WR-580's
//!   control-point bound above is the fallback in Brep2.
//! - **Not upstream (WR-619, #1794; own design).**
//!   [`ProtoTopologyManager::imprint_edge_definition`] makes no edge for a definition whose ends
//!   are one proto vertex and one vertex in each B-rep, along a speck within that vertex's
//!   tolerance and the definition's own (an enclosure over the curve's control points, never
//!   samples), when another definition of its proto edge is
//!   imprinted between two distinct vertices in each B-rep (`collapsed_beside_sibling`);
//!   upstream makes a closed edge on that vertex.
//! - **Not upstream: exactly placed definitions combine within their tolerances.**
//!   `CanCombineVertexDefinitions` (`sdTolFactor`) and `CanCombineEdgeDefinitions` combine
//!   two connected definitions within 5 times their summed tolerances. When both
//!   definitions are exactly placed (`exactly_placed`: every source object a vertex, an edge
//!   on a line or a face on a plane, and an edge definition's curve a line) the radius is
//!   the sum itself ([`EXACT_TOL_FACTOR`]): their positions are exact to rounding, and two
//!   of them farther apart are distinct points, so the larger radius deleted a real
//!   crossing (a crossing 4.8e-5 from a vertex definition with 1e-5 tolerances, the
//!   flange-corner case; a line 7.5e-5 from another, `wr_614`). Definitions an inexact
//!   solve placed keep upstream's factor.
//! - **Not upstream: ends fitted to proto vertices (WR-452, WR-453, #435, #410).**
//!   [`ProtoTopologyManager::imprint_edge_definition`] first fits each end of the definition's
//!   interval to its proto vertex where `imprint_ends`'s checks pass (the SMLib fork's
//!   thread-groove fixes, tangency-labs/smlib b43876f, and the continuation a8caccd; the
//!   flag `imprint_extends_edge_to_proto_vertex` was retired by #14246).
//! - **Not upstream: one trim interval** (WR-767, tangency-labs/solidcore#3214).
//!   [`ProtoTopologyManager::create_proto_edge`] trims the 3D curve and its uv curves to the
//!   one interval none of their trims snaps (`exact_pieces::common_trim_interval`); upstream
//!   trims them in turn with one interval, and the edge can end past its 3D curve.
//! - **Not upstream: a proto edge combined the other way keeps its ends** (#18546).
//!   [`ProtoTopologyManager::combine_proto_edge`] compares `other`'s ends with `pe`'s start
//!   with start and end with end, as upstream, unless `other` runs from `pe`'s end to its
//!   start: then no end moves. Upstream moves both ends: it unlists `other` at each, points
//!   its start at `pe`'s start and its end at `pe`'s end, which no longer list it, and
//!   lists `pe` there once more. The `RemoveProtoEdge` that follows in
//!   `SplitIntersectingProtoEdges` then fails at both ends (an overlap running the other
//!   way); upstream drops that status, passing it on (as the fork does) would make it
//!   the boolean's error. An end that is genuinely another proto vertex still moves as
//!   upstream.
//! - `UpdateEdges` trims a definition at the split point; its alternative of splitting
//!   the proto edge (`SmProtoEdge::Split`) sits behind `static constexpr SmBoolean
//!   cbiDoSplit = FALSE` and is not ported.
//! - The `SM_DEBUG_CODE` blocks (dumps, drawing) are not ported; release `SM_ASSERT`s
//!   are expression statements whose only effect is the dereference they make.
//! - **Proto faces in `UpdateFaces`.** Not sorted between the split face's children:
//!   nothing reads that sorting (see [`ProtoTopologyManager::update_faces`]), so neither
//!   its edge use binormals (`SmEdgeuse::EvaluateBinormal`) nor its classifications of
//!   coincident proto edges are made.
//! - **Recursion.** None.

use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::p2::BSPLINE_CURVE_TYPE;
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::boolean::BooleanFailure;
use crate::error::topology::TopologyFailure;
use crate::error::EntityRef;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::global_solver::{SolutionRequested, SolverOperation};
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::geom::tol::{xsect_tol3d, zone_tol3d, ScaledZero};
use crate::intersect::curve_class::p3::curve_runs_along;
use crate::intersect::curve_class::p6::MergeContext;
use crate::intersect::curve_class::{CurveClassification, CurveInterval};
use crate::intersect::point_class::{
    object_zone_tol_3d, ClassObject, ClassObjectKind, MergeOutputs, PointClassType,
    PointClassification,
};
use crate::kernel::Kernel;
use crate::math::{max2, max3, max4, min2, Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::surface::surface::exact_pieces::{common_trim_interval, trim_unsnapped};
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::brep::p5::{make_edge_in_face_in, EdgeInFace};
use crate::topology::VertexId;

mod imprint_ends;

use super::p1::proto::{
    are_connected, can_imprint, find_position_from_pcs, topo_dimension, ED_TOL_FACTOR,
};
use super::p1::{
    EdgeDefId, EdgeDefinition, ProtoEdgeId, ProtoGeometry, ProtoTopology, ProtoTopologyManager,
    ProtoVertex, ProtoVertexId, VertexDefId, VertexDefinition,
};

const OP: &str = "topology::proto_topology::p2";
const OP_IMPRINT_VD: &str =
    "topology::proto_topology::p2::ProtoTopologyManager::imprint_vertex_definition";
const OP_IMPRINT_ED: &str =
    "topology::proto_topology::p2::ProtoTopologyManager::imprint_edge_definition";
const OP_ADD_VD: &str =
    "topology::proto_topology::p2::ProtoTopologyManager::add_vertex_definition_from_pcs";
const OP_CREATE_PE: &str = "topology::proto_topology::p2::ProtoTopologyManager::create_proto_edge";
const OP_UPDATE_EDGES: &str = "topology::proto_topology::p2::ProtoTopologyManager::update_edges";
const OP_COMBINE_VD: &str =
    "topology::proto_topology::p2::ProtoTopologyManager::can_combine_vertex_definitions";
const OP_RESOLVE_PE: &str =
    "topology::proto_topology::p2::ProtoTopologyManager::resolve_proto_edge";
const OP_RESOLVE_PV: &str =
    "topology::proto_topology::p2::ProtoTopologyManager::resolve_proto_vertex";

/// `sdTolFactor` of `CanCombineVertexDefinitions`: the distance allowed between two
/// combinable vertex definitions, in units of their summed tolerances. Upstream's, kept
/// for definitions an inexact solve placed (`exactly_placed`).
const VD_TOL_FACTOR: f64 = 5.0;

/// Not upstream: the distance allowed between two combinable definitions (vertex or edge)
/// that are both exactly placed (`exactly_placed`), in units of their summed tolerances.
/// Their positions are closed-form intersections of linear objects, exact to rounding, so
/// two of them are one point (one curve) exactly when their tolerance balls (tubes) meet:
/// farther apart than the sum of their tolerances they are distinct by the kernel's own
/// identity radius, the intersection tolerance (`xsect_tol_3d_from_zones`), and combining
/// them deletes a real crossing (`docs/validity-and-tolerances.md` §5.1).
pub(crate) const EXACT_TOL_FACTOR: f64 = 1.0;

/// The B-rep side of the proto topology: the members of the caller's edges, faces and
/// vertices these functions read or write, and the store new curves go to.
///
/// Every method answers for a live handle of the kind it names; `None` is an unknown
/// handle or one of another kind (a dereference of a bad pointer upstream).
pub trait ProtoBrep<O>: ProtoGeometry<O> + ProtoTopology<O> {
    /// `SmEdge::GetInterval`.
    fn edge_interval(&self, edge: O) -> Option<Extent1d>;
    /// `SmEdge::GetCurve`: the handle of the edge's curve (`Some(None)` for none).
    fn edge_curve(&self, edge: O) -> Option<Option<O>>;
    /// `GetTolerance` of a vertex, edge or face (the stored zone tolerance).
    fn topology_tolerance(&self, topo: O) -> Option<f64>;
    /// `SmFace::GetUVDomain`.
    fn face_uv_domain(&self, face: O) -> Option<Extent2d>;
    /// Trim-derived UV domain with native's ignored ordinary status. Retains the
    /// initialized or partial domain on failure.
    ///
    /// # Errors
    ///
    /// Invalid or stale owning-face handles and genuine unported callees.
    fn face_trim_uv_domain(&mut self, k: &Kernel, face: O) -> Result<Extent2d>;
    /// The actual (possibly partial) UV point classification with the native
    /// `PointClassify(uv, tolerance, TRUE, TRUE, pc)` flags and ignored ordinary status.
    ///
    /// # Errors
    ///
    /// Invalid or stale owning-face handles and genuine unported callees.
    fn face_point_classify_uv(
        &mut self,
        k: &Kernel,
        face: O,
        uv: Point2,
        source_tolerance: f64,
    ) -> Result<PointClassType>;
    /// `SmFace::SetUVDomain`.
    fn set_face_uv_domain(&mut self, face: O, domain: Extent2d) -> Option<()>;
    /// `SmFace::GetSurface`: the handle of the face's surface (`Some(None)` for none).
    fn face_surface(&self, face: O) -> Option<Option<O>>;
    /// `SmEdge::GetEdgeuseOfFace`: the edge's use bounding `face`, if any.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn edge_edgeuse_of_face(&self, edge: O, face: O) -> Result<Option<O>>;
    /// `SmEdge::IsConnectedToFace`.
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn edge_is_connected_to_face(&self, edge: O, face: O) -> Result<bool>;
    /// Stores a new curve (an `SmCurve::Copy`) and returns its handle; `None` when the
    /// store is full.
    fn insert_curve(&mut self, curve: Curve) -> Option<O>;
    /// `SmVertex::GetCommonEdges` (read by `topology.proto_topology_p3`).
    ///
    /// # Errors
    ///
    /// Those of the implementation's topology walk.
    fn vertex_common_edges(&self, v1: O, v2: O) -> Result<Vec<O>>;
    /// Not upstream (docs/booleans.md, "One edge or two"): the edge of `face` between
    /// `start` and `end` that is proved to be `curve` over `interval`: the two within
    /// `tolerance` plus the edge's of each other over their whole lengths
    /// ([`crate::geom::tools::find_edge_proved_along`]). `crossing` is the face whose
    /// surface cuts `face` in `curve`, when the curve is such an intersection: a seam of
    /// `face` is the curve only within the curve's reach ("Seams and poles"). An owner
    /// with no B-rep behind its handles (the default body) has no such edge.
    ///
    /// # Errors
    ///
    /// Those of the search.
    #[allow(clippy::too_many_arguments)] // the search's in-params
    fn face_edge_proved_along(
        &self,
        _k: &Kernel,
        _face: O,
        _start: O,
        _end: O,
        _curve: &Curve,
        _interval: Extent1d,
        _tolerance: f64,
        _crossing: Option<O>,
    ) -> Result<Option<O>> {
        Ok(None)
    }
    /// Not upstream (#16937; read by the topology intersector's `edges_coincide`):
    /// `SmTools::FindCoincidentEdge` over `face`'s edges (`SmFace::GetEdges`) for an edge
    /// between `start` and `end` whose geometry lies along `curve` over `interval` within
    /// `tolerance`. Returns the first such edge and its sampled maximum deviation.
    ///
    /// # Errors
    ///
    /// Those of the search; an owner with no B-rep behind its handles (the default body)
    /// returns [`SolidError::Unported`] under a `harness:` label.
    #[allow(clippy::too_many_arguments)] // FindCoincidentEdge's in-params
    fn face_coincident_edge(
        &self,
        _k: &Kernel,
        _face: O,
        _start: O,
        _end: O,
        _curve: &Curve,
        _interval: Extent1d,
        _tolerance: f64,
    ) -> Result<Option<(O, f64)>> {
        Err(SolidError::unported(
            "SmTools::FindCoincidentEdge (no B-rep behind the handles)",
            "harness:proto-brep-coincident-edge",
        ))
    }
    /// Not upstream (WR-580, #1519): `SmEdge::SetTolerance(dTol, bUpdateOnlyIfLarger =
    /// TRUE, bCascadeToBndries = TRUE)` on `edge`, read when an imprint takes an edge the face
    /// already has as its definition's edge: the edge's tolerance (and its vertices') rises
    /// to the bound the two curves were proved within, as the edge it replaces would have
    /// carried it.
    ///
    /// # Errors
    ///
    /// Those of the setter; an owner with no B-rep behind its handles (the default body)
    /// returns [`SolidError::Unported`] under a `harness:` label.
    fn raise_edge_tolerance(&mut self, _k: &Kernel, _edge: O, _tolerance: f64) -> Result<()> {
        Err(SolidError::unported(
            "SmEdge::SetTolerance (no B-rep behind the handles)",
            "harness:proto-brep-edge-tolerance",
        ))
    }
    /// `SmBrep::SqueezeEdge(pEdge, pSurvivingVertex)` on the B-rep `brep` (called by
    /// `topology.proto_topology_p3` on `GetWhichBrep`): the owner of a real B-rep
    /// implements it with `topology::brep::p3::squeeze_edge`.
    ///
    /// # Errors
    ///
    /// Those of the edit; an owner with no B-rep behind its handles returns
    /// [`SolidError::Unported`] under a `harness:` label.
    fn brep_squeeze_edge(&mut self, k: &mut Kernel, brep: O, edge: O, survivor: O) -> Result<()>;
    /// `SmFace::CurveOnClassify(bDoPointClassify, cc)` with upstream's other arguments at
    /// their defaults (`FALSE`), read by `topology.proto_topology_p3`: the B-rep owner
    /// implements it with `query::face::p2::curve_on_classify`. Returns
    /// `pbOptModifiedCurve`.
    ///
    /// # Errors
    ///
    /// Those of `curve_on_classify`.
    fn face_curve_on_classify(
        &self,
        k: &Kernel,
        face: O,
        do_point_classify: bool,
        cc: &mut CurveClassification<O>,
    ) -> Result<bool>;
    /// Mutable owning-query successor. Legacy providers retain their read-only behavior;
    /// actual providers can retain lazy UV/gap updates on both success and failure.
    /// # Errors
    /// The original face classification error.
    fn face_curve_on_classify_mut(
        &mut self,
        k: &Kernel,
        face: O,
        do_point_classify: bool,
        cc: &mut CurveClassification<O>,
    ) -> Result<bool> {
        self.face_curve_on_classify(k, face, do_point_classify, cc)
    }

    /// `pCurveClassification->SetPointClassParameters(bSetMids)` of
    /// `ResolveOverlappingProtoEdges`. Upstream's point solve of a face's surface makes
    /// the face's trimmed surface cache (`smsurf_GetSurfaceCache`), so an owner that can
    /// lend its faces' B-reps overrides this with
    /// [`CurveClassification::set_point_class_parameters_mut`]; the default is the
    /// read-only [`CurveClassification::set_point_class_parameters`].
    ///
    /// # Errors
    ///
    /// Those of [`CurveClassification::set_point_class_parameters`].
    fn set_point_class_parameters_mut(
        &mut self,
        k: &Kernel,
        cc: &mut CurveClassification<O>,
        set_mids: bool,
    ) -> Result<()>
    where
        Self: Sized,
        O: ClassObject,
    {
        cc.set_point_class_parameters(k, &*self, set_mids)
    }

    /// `SmBrep::Point3DClassify(point, tolerance, bDoBoundaryIntersections, sPC)` of the
    /// B-rep `brep` names, read by `topology.proto_topology_p3`: the B-rep owner
    /// implements it with `query::brep::BrepQuery::point_3d_classify` on its B-rep and
    /// maps the classification's objects to its handles.
    ///
    /// # Errors
    ///
    /// Those of `point_3d_classify`; [`SolidError::InvalidInput`] for a handle that
    /// names no B-rep or an object without a handle.
    fn brep_point_3d_classify(
        &mut self,
        k: &Kernel,
        brep: O,
        point: Point3,
        tolerance: f64,
        boundary_intersections: bool,
    ) -> Result<PointClassification<O>>;
    /// `pBrep->MakeEdgeInFace(pFace, pStartVtx, pEndVtx, pCurve, pTempUV, ivl, eOrient,
    /// dTol, rpNewEdge, rpNewLoop, rpNewFace)` of `SmEdgeDefinition::Imprint` on the
    /// B-rep `brep` (`GetWhichBrep`): the owner of real B-reps runs
    /// [`make_edge_in_face_in`] on the B-rep behind the handles, whose own
    /// `m_bEditingEnabled` it checks (most owners through
    /// [`make_edge_in_face_in_context`]). `curve` and `uv_curve` are consumed. Returns
    /// the new edge, loop and face, each `None` when not made.
    ///
    /// # Errors
    ///
    /// Those of the edit; [`SolidError::InvalidInput`] for a face or vertex that is not
    /// in `brep`; an owner with no B-rep behind its handles returns
    /// [`SolidError::Unported`] under a `harness:` label.
    fn brep_make_edge_in_face(
        &mut self,
        k: &Kernel,
        brep: O,
        face: O,
        start: O,
        end: O,
        curve: Curve,
        uv_curve: Option<Curve>,
        interval: Extent1d,
        orientation: OrientType,
        tolerance: f64,
    ) -> Result<(Option<O>, Option<O>, Option<O>)>;
    /// `SmEdgeuse::EvaluateBinormal(dParam, FALSE, rPt, rBinormal)` of the edge use
    /// `edgeuse` (read by `UpdateFaces`): the owner of real B-reps runs
    /// [`evaluate_binormal`](crate::topology::edgeuse::p3::evaluate_binormal) on the
    /// B-rep behind the handle (which may build the edge use's UV trim curve, as
    /// upstream) and returns the binormal.
    ///
    /// # Errors
    ///
    /// Those of `evaluate_binormal`; [`SolidError::InvalidInput`] for a handle that
    /// names no edge use; an owner with no B-rep behind its handles returns
    /// [`SolidError::Unported`] under a `harness:` label.
    fn edgeuse_evaluate_binormal(&mut self, k: &Kernel, edgeuse: O, param: f64) -> Result<Vec3>;
    /// `rPC.MergeIntoObject(srcPoint, &newEdges, &newVertices)` of
    /// `SmVertexDefinition::Imprint`: the classification's point merged into the B-rep
    /// behind its object, which upstream edits through the object's pointer (a new
    /// shell vertex, vertex loop or edge split, or the existing vertex moved by
    /// `SmVertex::SetPoint`). The owner of mutable B-reps implements it with
    /// [`merge_into_object_mut`](crate::intersect::curve_class::p6::merge_into_object_mut);
    /// the default has no mutable owning context and returns
    /// [`PointClassification::merge_into_object`]'s result.
    ///
    /// # Errors
    ///
    /// Those of the merge.
    fn merge_point_class_into_brep(
        &mut self,
        k: &mut Kernel,
        pc: &mut PointClassification<O>,
        src_point: Point3,
    ) -> Result<MergeOutputs<O>>
    where
        O: ClassObject,
    {
        pc.merge_into_object(k, src_point)
    }
}

/// The handle the curve point solves name (only the solution count and values are
/// used).
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

/// A dereference of a null pointer or a deleted object upstream, or a read of an
/// uninitialised value (undefined behaviour).
const fn dangling(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// A status upstream ignores, except the errors that cannot be ignored here: unported
/// callees and undefined behaviour upstream.
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

/// Whether every coordinate of `p` is finite.
fn finite_point(p: Point3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

/// Not upstream (#16637): whether `x` lies within `tol` of the segment from `a` to `b`,
/// up to the rounding of placing a point on that segment and of this test.
///
/// The test computes the closest point `a + t·(b - a)` (`t` clamped to `[0, 1]`) and
/// accepts a distance `d` to it of at most `tol + γ_32·S`, with
/// `S = |x| + |a| + |b|`. The `γ_32·S` term is what makes every point the kernel places
/// on the segment pass for any `tol >= 0` (`find_position_from_pcs`: `a`, `b`, or
/// `(1 - f)·a + f·b` with `f` the first gap's share of the two, in `[0, 1]` when
/// neither gap is negative):
/// - That point is within `γ_3·(|a| + |b|)` of the segment: per coordinate, the
///   rounding of `1 - f`, two products and a sum is at most `γ_3·max(|a_i|, |b_i|)`.
/// - The computed `t` is off the exact projection's by at most `γ_13·|x - a| / |b - a|`
///   (the subtractions, the two dot products and the division), and clamping only
///   shrinks that. The computed closest point is within `γ_3·(2|a| + |b|)` of the
///   segment point at the computed `t`, and the distance has a relative error of
///   `γ_4`.
/// - So the computed `d` is at most `(1 + γ_4)·(γ_3 + γ_13 + 2γ_3)·S <= γ_26·S`, and
///   `γ_32` covers the rounding of `S`, of the product and of the sum with `tol`.
///
/// Underflow is not counted: it adds at most about 1.5e-154 (a segment whose squared
/// length underflows is measured from `a`). Accepting a little more than `tol` never
/// breaks a bound: the caller covers each accepted point by its own measured distance.
/// A non-finite distance or bound (an overflow) refuses.
fn within_tolerance_of_segment(x: Point3, a: Point3, b: Point3, tol: f64) -> bool {
    let ab = b - a;
    let len2 = ab.dot(ab);
    let t = if len2 > 0.0 {
        ((x - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = x.distance(a + t * ab);
    let bound = tol + gamma(32) * (x.length() + a.length() + b.length());
    bound.is_finite() && d <= bound
}

/// Not upstream (WR-306, #18312): whether a ball of radius `old_tol` at `before`, grown to
/// radius `grown` at `centre`, reaches a vertex at `at` with tolerance `tol` that the ball
/// before kept clear of: the two balls are certainly apart before (`old_tol + tol < D`,
/// `D` the exact distance from `before`) and may touch after (`D' <= grown + tol`, `D'`
/// from `centre`). A ball that does not grow reaches nothing new; nor does one that was
/// within a rounding of the vertex before (that overlap is not the growth's, and is left
/// as upstream leaves it).
///
/// A computed distance `d` is within `γ_4·D` of the exact one (the inputs are stored
/// points). A computed sum `s` is at least `(1 - u)` times the exact one, and the rounded
/// product `s·(1 + γ_8)` (the factor's own rounding included) is then at least `(1 + γ_4)`
/// times the exact sum. So `old·(1 + γ_8) < d` gives `old_exact < D`, and
/// `D' <= reach_exact` gives `d' <= reach·(1 + γ_8)`: the test may refuse a growth that
/// misses by a rounding, and of balls it finds apart before it never passes one that
/// reaches. A distance that overflows to infinity stands for what it proves, an exact
/// distance of at least `√MAX·(1 - γ_4)` (the squares' sum overflowed, or a difference
/// did): clear of a smaller reach, possibly reached by a larger one. A NaN distance or
/// bound reaches nothing.
fn grown_into(
    before: Point3,
    old_tol: f64,
    centre: Point3,
    grown: f64,
    at: Point3,
    tol: f64,
) -> bool {
    let margin = 1.0 + gamma(8);
    let overflowed = f64::MAX.sqrt() * (1.0 - gamma(4));
    let at_least = |d: f64| if d == f64::INFINITY { overflowed } else { d };
    let reach = (grown + tol) * margin;
    let after = centre.distance(at);
    grown.partial_cmp(&old_tol) == Some(core::cmp::Ordering::Greater)
        && (old_tol + tol) * margin < at_least(before.distance(at))
        && (after <= reach || (after == f64::INFINITY && overflowed <= reach))
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

/// `SM_CAST_PTR(<kind>, obj)` for a topology kind.
fn as_kind<O: ClassObject>(o: Option<O>, kind: ClassObjectKind) -> Option<O> {
    o.filter(|o| o.class_kind() == kind)
}

/// `SmTol::GetZoneTol3d(const SmObject*)`: a vertex's, edge's or face's own tolerance,
/// otherwise [`object_zone_tol_3d`].
fn zone_tol_of<O: ClassObject>(
    k: &Kernel,
    brep: &impl ProtoBrep<O>,
    object: Option<O>,
) -> Result<f64> {
    match object {
        Some(o)
            if matches!(
                o.class_kind(),
                ClassObjectKind::Vertex | ClassObjectKind::Edge | ClassObjectKind::Face
            ) =>
        {
            brep.topology_tolerance(o)
                .ok_or(dangling("topology handle is unknown"))
        }
        _ => object_zone_tol_3d(k, brep, object),
    }
}

/// `SmTol::GetXSectTol3d(const SmObject*, const SmObject*)`: the sum of the two zone
/// tolerances.
fn xsect_tol_of<O: ClassObject>(
    k: &Kernel,
    brep: &impl ProtoBrep<O>,
    obj1: Option<O>,
    obj2: Option<O>,
) -> Result<f64> {
    let t1 = zone_tol_of(k, brep, obj1)?;
    let t2 = zone_tol_of(k, brep, obj2)?;
    Ok(xsect_tol3d(k, t1 + t2))
}

/// Not upstream (WR-619, condition 3 of `collapsed_beside_sibling`): when the whole of
/// `curve` over `interval` lies within `tol` of `vertex` (the curve is a speck on that
/// vertex; a real closed curve on it leaves it), returns a certified upper bound of its
/// distance from `vertex` together with the curve's start and end points.
///
/// Decided on an enclosure, never on samples: the curve over `interval` lies in the convex
/// hull of the points `Curve::hull_points_over` gives (a line's ends, a B-spline's control
/// points of the knot spans that meet `interval`), and the distance from `vertex` is convex,
/// so its largest value over those points is its largest value over the curve. Every hull
/// point must be within `tol` (the tolerance comparison itself, like every capture of an end
/// by its vertex, is on the computed distance: the ends of a speck between two corners
/// `tol` apart sit on that bound). The returned gap widens each point's distance by its
/// coordinates' error `e` (at most `2 e` farther) and by the rounding of the difference
/// (`γ_1`) and the norm (`γ_3`), so it bounds the whole curve; `ImprintIntersections` grows
/// the vertex's tolerance to cover twice it. A curve of another kind proves nothing: `None`,
/// and the definition is imprinted.
///
/// # Errors
///
/// Those of the end evaluations.
fn speck_on_vertex(
    k: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    vertex: Point3,
    tol: f64,
) -> Result<Option<(f64, Point3, Point3)>> {
    let Some(hull) = curve.hull_points_over(&interval) else {
        return Ok(None);
    };
    let mut max_gap = 0.0_f64;
    for (q, e) in hull {
        let dist = q.distance(vertex);
        // a NaN distance is refused too
        if dist.is_nan() || dist > tol {
            return Ok(None);
        }
        max_gap = max2(max_gap, (dist + 2.0 * e) * (1.0 + gamma(6)));
    }
    let start_pt = curve.evaluate_point(k, interval.min())?;
    let end_pt = curve.evaluate_point(k, interval.max())?;
    Ok(Some((max_gap, start_pt, end_pt)))
}

/// The curve behind `o` (a `NULL` dereference upstream for none).
fn curve_of<O: Copy>(brep: &impl ProtoBrep<O>, o: Option<O>) -> Result<&Curve> {
    o.and_then(|o| brep.curve(o))
        .ok_or(dangling("curve handle is null or unknown"))
}

/// Not upstream: whether every point a definition takes from the topology object `o` is a
/// closed-form intersection of linear objects, exact to rounding: `o` is a vertex, an edge
/// on a line or a face on a plane.
pub(crate) fn exactly_placed<O: ClassObject>(brep: &impl ProtoBrep<O>, o: Option<O>) -> bool {
    let Some(o) = o else {
        return false;
    };
    match o.class_kind() {
        ClassObjectKind::Vertex => true,
        ClassObjectKind::Edge => brep
            .edge_curve(o)
            .flatten()
            .and_then(|c| brep.curve(c))
            .is_some_and(|c| matches!(c.kind(), CurveKind::Line(_))),
        ClassObjectKind::Face => brep
            .face_surface(o)
            .flatten()
            .and_then(|s| brep.surface(s))
            .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_))),
        _ => false,
    }
}

/// `pEdge->GetCurve()` dereferenced.
fn edge_curve_of<O: Copy>(brep: &impl ProtoBrep<O>, edge: O) -> Result<O> {
    brep.edge_curve(edge)
        .ok_or(dangling("edge handle is unknown"))?
        .ok_or(dangling("edge has no curve"))
}

/// `SmCurve::Trim(rDomain)` (`domain` may be snapped); its status, which upstream
/// ignores, is passed on as the fork does (bcc6f01).
fn trim_curve<O: Copy>(
    k: &Kernel,
    brep: &mut impl ProtoBrep<O>,
    curve: Option<O>,
    domain: &mut Extent1d,
) -> Result<()> {
    if let Some(c) = curve {
        let crv = brep
            .curve_mut(c)
            .ok_or(dangling("curve handle is unknown"))?;
        // fork bcc6f01 (`UpdateEdges`, `SER(..->Trim(sDomain))`): passed on
        crv.trim(k, domain, TrimMode::Notify)?;
    }
    Ok(())
}

/// `SmTol::GetZoneTol3d_ForXSectResult(cpObj1, cpObj2)`: the larger zone tolerance.
fn zone_tol_for_xsect_result<O: ClassObject>(
    k: &Kernel,
    brep: &impl ProtoBrep<O>,
    obj1: O,
    obj2: O,
) -> Result<f64> {
    Ok(max2(
        zone_tol_of(k, brep, Some(obj1))?,
        zone_tol_of(k, brep, Some(obj2))?,
    ))
}

/// `GlobalPointSolve(crIvl, SM_SO_INTERSECT, crPt, dTol, &dTol, NULL, SM_SR_SINGLE,
/// rSols)` of the curve `curve`: the solution values, or `None` when the solve
/// failed.
fn edge_point_solve<O: ClassObject>(
    k: &Kernel,
    brep: &impl ProtoBrep<O>,
    curve: O,
    interval: &Extent1d,
    point: Point3,
    tol: f64,
) -> Result<Option<Vec<f64>>> {
    let crv = curve_of(brep, Some(curve))?;
    let opts = GlobalPointSolveOptions {
        target_distance: Some(tol),
        handle: Some(CurveHandle(crv.natural_interval())),
        ..GlobalPointSolveOptions::default()
    };
    let sols = ignore_soft(crv.global_point_solve(
        k,
        interval,
        SolverOperation::Intersect,
        point,
        tol,
        SolutionRequested::Single,
        &opts,
    ))?;
    Ok(sols.map(|s| {
        (0..s.len())
            .filter_map(|i| s.get(i).map(|x| x.start.solution_value))
            .collect()
    }))
}

// ---- callees in units not ported yet -------------------------------------------------------

/// `SM_CAST_PTR(SmBSplineCurve, pUVCurve)` of a definition's UV curve: a copy of the
/// curve when it is of a B-spline kind, `None` for none or another kind.
fn bspline_uv_curve<O: Copy>(brep: &impl ProtoBrep<O>, uv: Option<O>) -> Result<Option<Curve>> {
    match uv {
        None => Ok(None),
        Some(o) => {
            let c = brep
                .curve(o)
                .ok_or(dangling("UV curve handle is unknown"))?;
            Ok(c.is_kind_of(BSPLINE_CURVE_TYPE).then(|| c.clone()))
        }
    }
}

/// [`ProtoBrep::brep_make_edge_in_face`] for an owner that reaches its B-reps through a
/// [`MergeContext`] (`MergeIntervalBrep::merge_context` of the face handle): runs
/// [`make_edge_in_face_in`] on the context's B-rep, which checks that B-rep's own
/// `m_bEditingEnabled`, notifying through the context and naming the new edge's curve
/// with its `curve_ref`. `start` and `end` are the vertices in that B-rep.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a context whose target is not a face; those of
/// [`make_edge_in_face_in`].
pub fn make_edge_in_face_in_context<O>(
    k: &Kernel,
    ctx: MergeContext<'_, O>,
    start: VertexId,
    end: VertexId,
    curve: Curve,
    uv_curve: Option<Curve>,
    interval: Extent1d,
    orientation: OrientType,
    tolerance: f64,
) -> Result<(Option<O>, Option<O>, Option<O>)> {
    let mut ctx = ctx;
    let EntityRef::Face(face) = ctx.target else {
        return Err(SolidError::invalid_input(
            OP,
            "MakeEdgeInFace target is not a face",
        ));
    };
    let mut out = EdgeInFace::default();
    make_edge_in_face_in(
        k,
        ctx.brep,
        Some(face),
        Some(start),
        Some(end),
        curve,
        uv_curve,
        interval,
        orientation,
        tolerance,
        &mut out,
        &mut ctx.notifications,
        ctx.curve_ref,
    )?;
    Ok((
        out.edge.map(|e| (ctx.handle)(EntityRef::Edge(e))),
        out.loop_.map(|l| (ctx.handle)(EntityRef::Loop(l))),
        out.face.map(|f| (ctx.handle)(EntityRef::Face(f))),
    ))
}

/// `sm_ClassifyParamOnFaces`: first matching face interior, or zero.
fn classify_param_on_faces<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl ProtoBrep<O>,
    uv: Point2,
    zone_tol_3d: f64,
    face1: O,
    domain1: &Extent2d,
    face2: O,
    domain2: &Extent2d,
) -> Result<i32> {
    let in_face1 = domain1.contains_point(uv, EFF_ZERO);
    let in_face2 = domain2.contains_point(uv, EFF_ZERO);
    if in_face1 && !in_face2 {
        return Ok(1);
    }
    if !in_face1 && in_face2 {
        return Ok(2);
    }
    let tolerance = if zone_tol_3d > EFF_ZERO {
        zone_tol_3d
    } else {
        zone_tol_for_xsect_result(k, brep, face1, face2)?
    };
    k.work_counters.add_split_face_classification();
    if brep.face_point_classify_uv(k, face1, uv, tolerance)? == PointClassType::Face {
        return Ok(1);
    }
    if brep.face_point_classify_uv(k, face2, uv, tolerance)? == PointClassType::Face {
        return Ok(2);
    }
    Ok(0)
}

/// `SmSurface::DropPoint(pt, GetNaturalUVDomain(), NULL, .., SM_SO_NORMALIZE)`: the
/// drop point when it succeeded.
fn surface_drop_point<O: Copy>(
    k: &Kernel,
    brep: &impl ProtoBrep<O>,
    surface: Option<O>,
    point: Point3,
) -> Result<Option<Point2>> {
    let srf = surface
        .and_then(|o| brep.surface(o))
        .ok_or(dangling("surface handle is null or unknown"))?;
    let domain = srf.natural_uv_domain_with_nurb(k)?;
    let opts = crate::surface::surface::p5::DropPointOptions {
        operation: SolverOperation::Normalize, // Don't want off-boundary drops.
        ..crate::surface::surface::p5::DropPointOptions::default()
    };
    let r = srf.drop_point::<crate::surface::surface::p4::NoObject>(k, point, &domain, &opts)?;
    Ok(r.success.then_some(r.result_uv))
}

impl<O: ClassObject> ProtoTopologyManager<O> {
    fn vd_ref(&self, vd: VertexDefId) -> Result<&VertexDefinition<O>> {
        self.vertex_def(vd)
            .ok_or(dangling("vertex definition is deleted"))
    }

    fn vd_mut(&mut self, vd: VertexDefId) -> Result<&mut VertexDefinition<O>> {
        self.vertex_def_mut(vd)
            .ok_or(dangling("vertex definition is deleted"))
    }

    fn ed_ref(&self, ed: EdgeDefId) -> Result<&EdgeDefinition<O>> {
        self.edge_def(ed)
            .ok_or(dangling("edge definition is deleted"))
    }

    fn ed_mut(&mut self, ed: EdgeDefId) -> Result<&mut EdgeDefinition<O>> {
        self.edge_def_mut(ed)
            .ok_or(dangling("edge definition is deleted"))
    }

    fn pv_live(&self, pv: Option<ProtoVertexId>) -> Result<ProtoVertexId> {
        pv.filter(|&pv| self.proto_vertex(pv).is_some())
            .ok_or(dangling("proto vertex is null or deleted"))
    }

    fn pe_live(&self, pe: Option<ProtoEdgeId>) -> Result<ProtoEdgeId> {
        pe.filter(|&pe| self.proto_edge(pe).is_some())
            .ok_or(dangling("proto edge is null or deleted"))
    }

    /// Deletes the edge definition `ed` after `Destruct` (`pED->Destruct(); delete
    /// pED;`): the curves deleted.
    fn destruct_and_delete(&mut self, ed: EdgeDefId) -> Vec<O> {
        let mut deleted = self
            .edge_def_mut(ed)
            .map(EdgeDefinition::destruct)
            .unwrap_or_default();
        deleted.extend(self.delete_edge_definition(ed));
        deleted
    }

    // ---- SmVertexDefinition ----------------------------------------------------------

    /// Imprints the vertex definition `vd` into both B-reps (`Imprint`): each
    /// classification merges the position into its object (creating or reusing a
    /// vertex, which becomes the definition's final vertex), then the proto topology is
    /// updated for an edge that was split. The classification's object is restored
    /// afterwards (for B-rep 2 only when the update succeeded, as upstream).
    ///
    /// # Errors
    ///
    /// Those of [`ProtoBrep::merge_point_class_into_brep`]; `SM_ERR` when a merge
    /// made no single vertex and the classification is not on a vertex; those of
    /// [`ProtoTopologyManager::update_edges`]; [`SolidError::InvalidInput`] for a
    /// definition without a proto vertex (upstream reaches the manager through it).
    pub fn imprint_vertex_definition(
        &mut self,
        k: &mut Kernel,
        brep: &mut impl ProtoBrep<O>,
        vd: VertexDefId,
    ) -> Result<()> {
        for which in [1, 2] {
            // If we're on an Edge, save that Edge for checking splits, after merge.
            let v = self.vd_ref(vd)?;
            let this_edge = v.brep_point_classif(which).edge_object();

            // MergeIntoObject() resets the PointClass' Object to the newly-created Vertex
            // (if it creates one). Save the original topology and restore it after.
            let pc_type = v.brep_point_classif(which).point_class();
            let pc_object = v.brep_point_classif(which).object();
            let position = v.position();

            let v = self.vd_mut(vd)?;
            let outputs =
                brep.merge_point_class_into_brep(k, v.brep_point_classif_mut(which), position)?;

            if outputs.new_vertices.len() == 1 {
                v.set_final_vertex(which, outputs.new_vertices.first().copied());
            } else if v.src_topo_type(which) == PointClassType::Vertex {
                let vertex = v.brep_point_classif(which).vertex_object();
                v.set_final_vertex(which, vertex);
            } else {
                return Err(SolidError::topology(
                    OP_IMPRINT_VD,
                    TopologyFailure::MergeGaveNoVertex {
                        new_vertices: outputs.new_vertices.len(),
                    },
                ));
            }

            if which == 1 {
                // Restore the PointClass' original object.
                v.brep_point_classif_mut(which)
                    .set_class_object(pc_type, pc_object);
            }

            // the manager is reached through the proto vertex
            self.pv_live(self.vd_ref(vd)?.proto_vertex())?;

            // imprint Vertex into the Brep - commonly splits edge
            self.update_edges(k, brep, which, this_edge, &outputs.new_edges, Some(vd))?;

            if which == 2 {
                // Restore the PointClass' original object.
                self.vd_mut(vd)?
                    .brep_point_classif_mut(which)
                    .set_class_object(pc_type, pc_object);
            }
        }
        Ok(())
    }

    // ---- SmEdgeDefinition ------------------------------------------------------------

    /// Imprints the edge definition `ed` into both B-reps (`Imprint`; the vertex
    /// definitions at both proto vertices must have been imprinted): on a face source a
    /// new edge is made in the face (splitting it, after which the proto topology on it
    /// is updated), on an edge source that edge is the final edge.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when a proto vertex has no final vertex for the
    /// definition; those of [`ProtoTopologyManager::proto_vertex_final_vertex`];
    /// those of [`ProtoBrep::brep_make_edge_in_face`] for a face source;
    /// [`SolidError::InvalidInput`] for a definition without a proto edge, or a proto
    /// edge without both proto vertices.
    pub fn imprint_edge_definition(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
    ) -> Result<()> {
        self.imprint_edge_definition_reporting_edits(k, brep, ed).0
    }

    /// [`ProtoTopologyManager::imprint_edge_definition`], also answering whether the
    /// failure may have left a B-rep half edited (UB-242). A failure without an edit (no
    /// final vertex, or a check before the first `MakeEdgeInFace`) leaves both B-reps as
    /// they were. So does a `MakeEdgeInFace` that stops at its `FindVertexSector` checks
    /// ([`crate::topology::brep::p5::is_no_vertex_sector`]): they precede every edit of
    /// its B-rep, and an edge the first `MakeEdgeInFace` already made in Brep1 is whole,
    /// so upstream's continuation is defined and the port goes on with it.
    pub fn imprint_edge_definition_reporting_edits(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
    ) -> (Result<()>, bool) {
        let mut edited = false;
        let result = self.imprint_edge_definition_inner(k, brep, ed, &mut edited);
        (result, edited)
    }

    /// Not upstream (WR-619): when the
    /// definition `ed`, with proto vertices `pvs` and final vertices `vertices` (start, end in
    /// B-reps 1 and 2), is a collapsed speck, returns a bound of each B-rep vertex's maximum
    /// distance to the speck and the speck's start and end points. All must hold, else `None`:
    /// its ends are one proto vertex; they are one vertex in each B-rep; its curve is a speck
    /// on those vertices ([`speck_on_vertex`] in each B-rep, within both that vertex's
    /// tolerance and the definition's own, the tolerance its edge would have); and another
    /// definition of its proto edge is imprinted between two distinct final vertices in each
    /// B-rep (the proto edge is not lost; the speck carries no geometry its vertex does not
    /// have).
    ///
    /// # Errors
    ///
    /// Those of the curve evaluations and of
    /// [`ProtoTopologyManager::proto_vertex_final_vertex`].
    fn collapsed_beside_sibling(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        ed: EdgeDefId,
        pvs: (ProtoVertexId, ProtoVertexId),
        vertices: [(O, O); 2],
    ) -> Result<Option<([f64; 2], Point3, Point3)>> {
        let [(start1, end1), (start2, end2)] = vertices;
        if pvs.0 != pvs.1 || start1 != end1 || start2 != end2 {
            return Ok(None);
        }
        let def = self.ed_ref(ed)?;
        let curve = curve_of(brep, def.xsect_curve_3d())?;
        let interval = def.xsect_interval();
        let mut gaps = [0.0_f64; 2];
        let mut ends = (Point3::ZERO, Point3::ZERO);
        for (slot, v) in gaps.iter_mut().zip([start1, start2]) {
            let (Some(point), Some(tol)) = (
                brep.classification_vertex_point(v),
                brep.topology_tolerance(v),
            ) else {
                return Ok(None);
            };
            let Some((gap, p0, p1)) =
                speck_on_vertex(k, curve, interval, point, min2(tol, def.raw_tolerance()))?
            else {
                return Ok(None);
            };
            *slot = gap;
            ends = (p0, p1);
        }
        let Some(pe) = def.proto_edge() else {
            return Ok(None);
        };
        let siblings = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .edge_definitions()
            .to_vec();
        for other in siblings.into_iter().filter(|&o| o != ed) {
            let start_pv = self.pv_live(self.edge_def_start_proto_vertex(other)?)?;
            let end_pv = self.pv_live(self.edge_def_end_proto_vertex(other)?)?;
            let mut distinct = true;
            for which in [1, 2] {
                let s = self.proto_vertex_final_vertex(k, brep, start_pv, which, other, true)?;
                let e = self.proto_vertex_final_vertex(k, brep, end_pv, which, other, false)?;
                distinct &= matches!((s, e), (Some(s), Some(e)) if s != e);
            }
            if distinct {
                return Ok(Some((gaps, ends.0, ends.1)));
            }
        }
        Ok(None)
    }

    /// Not upstream (docs/booleans.md, "One edge or two"): the edge that the face of B-rep
    /// `which` the definition `ed` lies in already has between `start` and `end`, the
    /// definition's two vertices there, and that is proved to be the definition's curve:
    /// each within the two's tolerance (the larger of the face's and the definition's, plus
    /// the edge's) of the other over its whole length, or in a vertex's tolerance of it
    /// ([`ProtoBrep::face_edge_proved_along`]). `None` for a definition on no face there,
    /// one between a vertex and itself, and where no edge is proved.
    ///
    /// # Errors
    ///
    /// Those of the search; [`SolidError::InvalidInput`] for an unknown face handle.
    fn face_edge_proved_to_be(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        ed: EdgeDefId,
        which: i32,
        start: O,
        end: O,
    ) -> Result<Option<O>> {
        let d = self.ed_ref(ed)?;
        if d.src_topo_type(which) != PointClassType::Face || start == end {
            return Ok(None);
        }
        let (Some(curve), Some(face)) = (
            d.xsect_curve_3d(),
            as_kind(d.src_topo(which), ClassObjectKind::Face),
        ) else {
            return Ok(None);
        };
        let face_tol = brep
            .topology_tolerance(face)
            .ok_or(dangling("face handle is unknown"))?;
        // the face whose surface cuts this one in the curve, when the other side is a face
        let other = 3 - which;
        let crossing = (d.src_topo_type(other) == PointClassType::Face)
            .then(|| as_kind(d.src_topo(other), ClassObjectKind::Face))
            .flatten();
        brep.face_edge_proved_along(
            k,
            face,
            start,
            end,
            curve_of(brep, Some(curve))?,
            d.xsect_interval(),
            max2(face_tol, d.raw_tolerance()),
            crossing,
        )
    }

    /// Not upstream (#16640): whether the definition `ed` is an edge-to-edge one whose ends
    /// are one vertex in each B-rep (`vertices`: start and end in B-rep 1, then B-rep 2)
    /// although its curve ends farther apart than a valid closed edge's can ([`ends_are_a_stretch`]).
    ///
    /// # Errors
    ///
    /// Those of the curve evaluations; [`SolidError::InvalidInput`] for a non-finite
    /// tolerance.
    fn is_unbounded_edge_stretch(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        ed: EdgeDefId,
        vertices: [(O, O); 2],
    ) -> Result<bool> {
        let [(start1, end1), (start2, end2)] = vertices;
        if start1 != end1 || start2 != end2 {
            return Ok(false);
        }
        let d = self.ed_ref(ed)?;
        if d.src_topo_type(1) != PointClassType::Edge || d.src_topo_type(2) != PointClassType::Edge
        {
            return Ok(false);
        }
        let (Some(tol1), Some(tol2)) = (
            brep.topology_tolerance(start1),
            brep.topology_tolerance(start2),
        ) else {
            return Ok(false);
        };
        let gap = d
            .end_position(k, brep, true)?
            .distance(d.end_position(k, brep, false)?);
        ends_are_a_stretch(gap, d.raw_tolerance(), [tol1, tol2])
    }

    /// Not upstream (WR-580, #1519): the edge Brep2's face of definition `ed` already has
    /// between `start` and `end` (the definition's vertices in Brep2) along the definition's
    /// curve, which is then the definition's edge in Brep2, with the bound the two curves
    /// were proved within; `None` when the face has none.
    ///
    /// An edge qualifies when it is an edge of the face joining the two vertices
    /// (`SmVertex::GetCommonEdges`, `SmEdge::IsConnectedToFace`) and its curve over its
    /// interval and the definition's curve over its interval are proved within the larger of
    /// the face's and the definition's tolerance of each other as wholes
    /// ([`curve_runs_along`]: a bound on control points, never samples; for two coplanar
    /// circle arcs, within that tolerance plus the coordinates' resolution, `√3 ulp` of their
    /// magnitude, #18065). The first such edge is taken.
    ///
    /// # Errors
    ///
    /// Those of the topology walks and of the bound's evaluations; a dangling handle.
    fn along_face_edge(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        ed: EdgeDefId,
        start: O,
        end: O,
    ) -> Result<Option<(O, f64)>> {
        let d = self.ed_ref(ed)?;
        let Some(curve) = d.xsect_curve_3d() else {
            return Ok(None);
        };
        let curve = curve_of(brep, Some(curve))?;
        let interval = d.xsect_interval();
        let face = as_kind(d.src_topo(2), ClassObjectKind::Face)
            .ok_or(dangling("the definition's second source is not a face"))?;
        let face_tol = brep
            .topology_tolerance(face)
            .ok_or(dangling("face handle is unknown"))?;
        let tol = max2(face_tol, d.raw_tolerance());
        for edge in brep.vertex_common_edges(start, end)? {
            if !brep.edge_is_connected_to_face(edge, face)? {
                continue;
            }
            let edge_curve = curve_of(brep, Some(edge_curve_of(brep, edge)?))?;
            let edge_interval = brep
                .edge_interval(edge)
                .ok_or(dangling("edge handle is unknown"))?;
            if curve_runs_along(k, curve, &interval, edge_curve, &edge_interval, tol)? {
                return Ok(Some((edge, tol)));
            }
        }
        Ok(None)
    }

    /// Not upstream (#17098): the edge of B-rep `which` that the face-sourced definition `ed`
    /// stands for already, when there is one. One intersection curve can be found twice: as
    /// an edge of B-rep `which` against a face of the other B-rep, and as this definition, the
    /// face `face` of B-rep `which` against an edge of the other. Imprinted again, the face
    /// had two edges between the same vertices along one curve, and the other B-rep's edge,
    /// related to the first by the other definition, was then asked for the second as well:
    /// "another entity is already related to the other one".
    ///
    /// So when another definition of a proto edge between this definition's two proto
    /// vertices is that mirror pair, sourced in B-rep `which` on an edge that runs between
    /// this definition's two final vertices there (`start` and `end`, distinct) and bounds
    /// `face`, and in the other B-rep on a face that this definition's edge there bounds, and
    /// the edge is proved within `along_tol` plus its own tolerance of this definition's curve
    /// over the whole of both ([`Curve::proved_within`], a certified bound, never samples),
    /// that edge is this definition's edge. Only that pair is one curve found twice: beside
    /// a definition sourced on an edge in both B-reps, this one is a curve of its own, even
    /// within tolerance of it (the two rims of a sheet shelled to `2 * tolerance` keep both
    /// their edges). `along_tol` is the tolerance the edge would be made with (the face's or
    /// the definition's, the larger), as WR-580 takes it in Brep2.
    ///
    /// # Errors
    ///
    /// Those of the topology walks and of the bound; [`SolidError::InvalidInput`] for a
    /// deleted definition, proto edge or proto vertex, or a missing curve.
    #[allow(clippy::too_many_arguments)] // the definition, its side, face, vertices and tolerance
    fn sibling_edge_in_face(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        ed: EdgeDefId,
        which: i32,
        face: O,
        (start, end): (O, O),
        along_tol: f64,
    ) -> Result<Option<O>> {
        if start == end {
            return Ok(None);
        }
        let ends = (
            self.pv_live(self.edge_def_start_proto_vertex(ed)?)?,
            self.pv_live(self.edge_def_end_proto_vertex(ed)?)?,
        );
        let d = self.ed_ref(ed)?;
        // this definition's source in the other B-rep, which must be an edge
        let across = 3 - which;
        if d.src_topo_type(across) != PointClassType::Edge {
            return Ok(None);
        }
        let Some(own_edge) = as_kind(d.src_topo(across), ClassObjectKind::Edge) else {
            return Ok(None);
        };
        let curve = curve_of(brep, d.xsect_curve_3d())?;
        let interval = d.xsect_interval();
        let proto_edges = self
            .proto_vertex(ends.0)
            .ok_or(dangling("proto vertex is deleted"))?
            .proto_edges()
            .to_vec();
        for pe in proto_edges {
            let siblings = self
                .proto_edge(pe)
                .ok_or(dangling("proto edge is deleted"))?
                .edge_definitions()
                .to_vec();
            for other in siblings.into_iter().filter(|&o| o != ed) {
                let o = self.ed_ref(other)?;
                if o.src_topo_type(which) != PointClassType::Edge
                    || o.src_topo_type(across) != PointClassType::Face
                {
                    continue;
                }
                let Some(other_face) = as_kind(o.src_topo(across), ClassObjectKind::Face) else {
                    continue;
                };
                let Some(edge) = as_kind(o.src_topo(which), ClassObjectKind::Edge) else {
                    continue;
                };
                let other_ends = (
                    self.pv_live(self.edge_def_start_proto_vertex(other)?)?,
                    self.pv_live(self.edge_def_end_proto_vertex(other)?)?,
                );
                if other_ends != ends && other_ends != (ends.1, ends.0) {
                    continue;
                }
                let s = brep.edge_start_vertex(edge)?;
                let e = brep.edge_other_vertex(edge, s)?;
                let between =
                    (s, e) == (Some(start), Some(end)) || (s, e) == (Some(end), Some(start));
                if !between
                    || !brep.edge_is_connected_to_face(edge, face)?
                    || !brep.edge_is_connected_to_face(own_edge, other_face)?
                {
                    continue;
                }
                let edge_curve = brep
                    .edge_curve(edge)
                    .ok_or(dangling("edge handle is unknown"))?;
                let edge_curve = curve_of(brep, edge_curve)?;
                let edge_interval = brep
                    .edge_interval(edge)
                    .ok_or(dangling("edge handle is unknown"))?;
                let edge_tol = brep
                    .topology_tolerance(edge)
                    .ok_or(dangling("edge handle is unknown"))?;
                if curve.proved_within(
                    k,
                    &interval,
                    edge_curve,
                    &edge_interval,
                    along_tol + edge_tol,
                )? {
                    return Ok(Some(edge));
                }
            }
        }
        Ok(None)
    }

    /// The body of [`ProtoTopologyManager::imprint_edge_definition`]; `edited` is set
    /// before the first B-rep edit.
    fn imprint_edge_definition_inner(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        ed: EdgeDefId,
        edited: &mut bool,
    ) -> Result<()> {
        // Make sure both SmVertices are in place before proceeding.
        let start_pv = self.edge_def_start_proto_vertex(ed)?;
        let end_pv = self.edge_def_end_proto_vertex(ed)?;
        let start_pv = self.pv_live(start_pv)?;
        let end_pv = self.pv_live(end_pv)?;

        let null = || SolidError::topology(OP_IMPRINT_ED, TopologyFailure::NoFinalVertexAtEnd);
        let start_vtx1 = self
            .proto_vertex_final_vertex(k, brep, start_pv, 1, ed, true)?
            .ok_or_else(null)?;
        let end_vtx1 = self
            .proto_vertex_final_vertex(k, brep, end_pv, 1, ed, false)?
            .ok_or_else(null)?;
        let start_vtx2 = self
            .proto_vertex_final_vertex(k, brep, start_pv, 2, ed, true)?
            .ok_or_else(null)?;
        let end_vtx2 = self
            .proto_vertex_final_vertex(k, brep, end_pv, 2, ed, false)?
            .ok_or_else(null)?;
        let brep1 = self.which_brep(1);
        let brep2 = self.which_brep(2);

        // Not upstream (WR-619, #1794; own design): a definition that would be a closed
        // edge on one vertex in each B-rep along a speck within that vertex's tolerance and its
        // own, beside a sibling definition imprinted between distinct vertices, makes no edge;
        // each vertex's tolerance covers the skipped speck's span in `ImprintIntersections`.
        if let Some(([gap1, gap2], p0, p1)) = self.collapsed_beside_sibling(
            k,
            &*brep,
            ed,
            (start_pv, end_pv),
            [(start_vtx1, end_vtx1), (start_vtx2, end_vtx2)],
        )? {
            self.note_skipped_speck_vertex(start_vtx1, gap1, p0, p1);
            self.note_skipped_speck_vertex(start_vtx2, gap2, p0, p1);
            return Ok(());
        }

        // Not upstream (#16640): an edge-to-edge definition whose curve runs from one point
        // to another farther apart than every tolerance involved, yet whose two ends are one
        // vertex in each B-rep, is a stretch of coincidence that no vertex bounds at its far
        // end. Two edges that cross at a shallow angle (a tangent arc that grazes a line)
        // stay within tolerance of each other on both sides of the crossing; the crossing is
        // split, but the stretch past it ends in the middle of both edges, which would
        // otherwise be related whole to each other and to the edges the crossing already
        // related. Such a definition is not imprinted: its edges keep the mates their own
        // definitions give them, so one relation holds per entity.
        if self.is_unbounded_edge_stretch(
            k,
            &*brep,
            ed,
            [(start_vtx1, end_vtx1), (start_vtx2, end_vtx2)],
        )? {
            return Ok(());
        }

        // not upstream (WR-452, WR-453, #435, #410): fit the ends to the proto vertices
        self.fit_edge_def_ends_to_proto_vertices(
            k,
            brep,
            ed,
            start_pv,
            end_pv,
            [(start_vtx1, start_vtx2), (end_vtx1, end_vtx2)],
        )?;

        let d = self.ed_ref(ed)?;
        let type1 = d.src_topo_type(1);
        let type2 = d.src_topo_type(2);

        // First do Brep 1.
        let mut new_e1 = None;
        let mut new_f1 = None;
        let mut old_f1 = None;

        // Not upstream (docs/booleans.md, "One edge or two"): an edge Brep1's face already
        // has between the two vertices that is proved to be the curve is this edge there.
        let mut along_edge1 =
            self.face_edge_proved_to_be(k, &*brep, ed, 1, start_vtx1, end_vtx1)?;

        // Not upstream (#17098): a face that already has this curve as the edge another
        // definition between the same proto vertices is sourced on keeps that edge, when the
        // search above found none.
        if along_edge1.is_none() && type1 == PointClassType::Face {
            let d = self.ed_ref(ed)?;
            if let Some(face) = as_kind(d.src_topo(1), ClassObjectKind::Face) {
                let face_tol = brep
                    .topology_tolerance(face)
                    .ok_or(dangling("face handle is unknown"))?;
                let along_tol = max2(face_tol, d.raw_tolerance());
                along_edge1 = self.sibling_edge_in_face(
                    k,
                    &*brep,
                    ed,
                    1,
                    face,
                    (start_vtx1, end_vtx1),
                    along_tol,
                )?;
            }
        }
        let d = self.ed_ref(ed)?;

        if let Some(edge) = along_edge1 {
            self.ed_mut(ed)?.set_final_edge(1, Some(edge));
        } else if type1 == PointClassType::Face {
            old_f1 = as_kind(d.src_topo(1), ClassObjectKind::Face);
            let face = old_f1.ok_or_else(null)?;
            let face_tol = brep
                .topology_tolerance(face)
                .ok_or(dangling("face handle is unknown"))?;
            let this_tol = max2(face_tol, d.raw_tolerance());
            let temp_uv = bspline_uv_curve(brep, d.xsect_uv_curve(1))?;
            let curve = curve_of(brep, d.xsect_curve_3d())?.clone();
            let interval = d.xsect_interval();

            // Note, input curves are consumed:
            *edited = true;
            match brep.brep_make_edge_in_face(
                k,
                brep1,
                face,
                start_vtx1,
                end_vtx1,
                curve,
                temp_uv,
                interval,
                OrientType::Same,
                this_tol,
            ) {
                Ok((e, _l, f)) => {
                    new_e1 = e;
                    new_f1 = f;
                }
                Err(e) => {
                    if crate::topology::brep::p5::is_no_vertex_sector(&e) {
                        // MakeEdgeInFace stopped before its first edit
                        *edited = false;
                    }
                    if !e.is_unported_or_cancelled() {
                        // Get rid of our curves, which were not consumed.
                        let _ = self.ed_mut(ed)?.destruct();
                    }
                    return Err(e);
                }
            }
            self.ed_mut(ed)?.set_final_edge(1, new_e1);
        } else if type1 == PointClassType::Edge {
            let e = as_kind(d.src_topo(1), ClassObjectKind::Edge);
            self.ed_mut(ed)?.set_final_edge(1, e);
        }

        // Not upstream (WR-580, #1519; the fork's tangency-labs/smlib@81cd61a): in Brep2 the
        // face may already have an edge between this edge's two vertices that lies along the
        // curve within tolerance (the curve runs along the face's own boundary there). That
        // edge is this edge in Brep2: imprinted again, the face had two coincident edges, and
        // the merge put a copy of the boundary edge beside this one in Brep1 (`SmFace` tests
        // 33 and 34). The fork takes it on `SmTools::FindCoincidentEdge`'s 30 samples; here
        // only an enclosure of the two whole curves takes it.
        //
        // Not upstream (docs/booleans.md, "One edge or two"): first an edge proved to be the
        // curve within the two's tolerance plus the edge's own (each of the other over its
        // whole length, `face_edge_proved_to_be`), which keeps its tolerance; else WR-580's
        // control-point bound (`along_face_edge`), whose edge then carries the bound it
        // proved, as the edge it stands for would have (`this_tol` below): relating it to
        // Brep1's edge only raises both to the larger of their own tolerances.
        let mut along_edge2 = self
            .face_edge_proved_to_be(k, &*brep, ed, 2, start_vtx2, end_vtx2)?
            .map(|edge| (edge, None));
        if along_edge2.is_none() && type2 == PointClassType::Face && start_vtx2 != end_vtx2 {
            along_edge2 = self
                .along_face_edge(k, brep, ed, start_vtx2, end_vtx2)?
                .map(|(edge, bound)| (edge, Some(bound)));
        }

        // Not upstream (#17098): as in Brep1, the edge another definition between the same
        // proto vertices is sourced on, when the search above found none.
        if along_edge2.is_none() && type2 == PointClassType::Face {
            let d = self.ed_ref(ed)?;
            if let Some(face2) = as_kind(d.src_topo(2), ClassObjectKind::Face) {
                let face_tol = brep
                    .topology_tolerance(face2)
                    .ok_or(dangling("face handle is unknown"))?;
                let along_tol = max2(face_tol, d.raw_tolerance());
                along_edge2 = self
                    .sibling_edge_in_face(
                        k,
                        &*brep,
                        ed,
                        2,
                        face2,
                        (start_vtx2, end_vtx2),
                        along_tol,
                    )?
                    .map(|edge| (edge, None));
            }
        }

        // Now do Brep 2.
        let d = self.ed_ref(ed)?;
        if let Some((edge, bound)) = along_edge2 {
            if let Some(bound) = bound {
                *edited = true;
                brep.raise_edge_tolerance(k, edge, bound)?;
            }
            self.ed_mut(ed)?.set_final_edge(2, Some(edge));
        } else if type2 == PointClassType::Face {
            // m_pXSectCurve3d went into Brep1. Make a copy for the other Brep (status
            // ignored).
            let copy = ignore_soft(curve_of(brep, d.xsect_curve_3d())?.copy(k))?;

            let old_f2 = as_kind(d.src_topo(2), ClassObjectKind::Face).ok_or_else(null)?;
            let face_tol = brep
                .topology_tolerance(old_f2)
                .ok_or(dangling("face handle is unknown"))?;
            let this_tol = max2(face_tol, d.raw_tolerance());

            let temp_uv = bspline_uv_curve(brep, d.xsect_uv_curve(2))?;
            let interval = d.xsect_interval();
            // a failed copy is a null curve, dereferenced in MakeEdgeInFace
            let copy = copy.ok_or(dangling("copy of the 3D curve failed"))?;
            *edited = true;
            let made = brep.brep_make_edge_in_face(
                k,
                brep2,
                old_f2,
                start_vtx2,
                end_vtx2,
                copy,
                temp_uv,
                interval,
                OrientType::Same,
                this_tol,
            );
            let (new_e2, new_f2) = match made {
                Ok((e, _l, f)) => (e, f),
                Err(e) => {
                    if crate::topology::brep::p5::is_no_vertex_sector(&e) {
                        // MakeEdgeInFace stopped before its first edit of Brep2, and
                        // Brep1's new edge is whole
                        *edited = false;
                    }
                    if !e.is_unported_or_cancelled() {
                        // Brep1 keeps the curves it consumed; the copy is dropped.
                        let d = self.ed_mut(ed)?;
                        d.set_xsect_curve_3d(None);
                        d.set_xsect_curve_uv(1, None);
                    }
                    return Err(e);
                }
            };
            self.ed_mut(ed)?.set_final_edge(2, new_e2);

            // If a Face was split, move the proto topology now on the new Face.
            if let Some(new_f2) = new_f2 {
                let other_edge = as_kind(self.ed_ref(ed)?.src_topo(1), ClassObjectKind::Edge);
                self.update_faces(k, brep, 2, old_f2, new_f2, new_e2, other_edge, Some(ed))?;
            }
        } else if type2 == PointClassType::Edge {
            let e = as_kind(d.src_topo(2), ClassObjectKind::Edge);
            self.ed_mut(ed)?.set_final_edge(2, e);
        }

        // Deferred:
        if let (Some(new_f1), Some(old_f1)) = (new_f1, old_f1) {
            let other_edge = as_kind(self.ed_ref(ed)?.src_topo(2), ClassObjectKind::Edge);
            self.update_faces(k, brep, 1, old_f1, new_f1, new_e1, other_edge, Some(ed))?;
        }
        Ok(())
    }

    // ---- SmProtoVertex ---------------------------------------------------------------

    /// Resolves the proto vertex `pv` (`Resolve`): resolves each vertex definition,
    /// then deletes, of every pair that can be combined, the redundant one
    /// ([`ProtoTopologyManager::can_combine_vertex_definitions`]). Not upstream (WR-306):
    /// before the delete, the one that stays takes over the hull points of the self-loops
    /// the deleted one absorbed and grows to cover them
    /// ([`VertexDefinition::take_over_hull`]). Not upstream (#18312): a definition's move,
    /// a hull's hand-over or the proto vertex's placement that would grow a tolerance into
    /// another vertex definition is refused ([`Self::refuse_growth_overlap`]).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects; [`SolidError::Unported`] from
    /// the definitions' resolution and comparisons; [`SolidError::Failed`] when a hull
    /// point lies at no finite distance from the definition taking it over; the refusal
    /// of a growth into another vertex definition.
    pub fn resolve_proto_vertex(
        &mut self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pv: ProtoVertexId,
    ) -> Result<()> {
        let pv = self.pv_live(Some(pv))?;
        let mut vds: Vec<Option<VertexDefId>> = self
            .proto_vertex(pv)
            .map(|v| v.vertex_definitions().iter().copied().map(Some).collect())
            .unwrap_or_default();

        // `SER( pVD1->Resolve(..) )` and `SER( CanCombineVertexDefinitions(..) )` (fork
        // bcc6f01; upstream ignores both statuses); the definitions deleted so far are
        // unlisted before the error is returned.
        for ii in 0..vds.len() {
            let Some(vd1) = vds.get(ii).copied().flatten() else {
                continue;
            };
            let ret = match self.resolve_vertex_definition(k, brep, vd1) {
                Ok(r) => r,
                Err(e) => {
                    vds.retain(Option::is_some);
                    self.write_vertex_definitions(pv, &vds)?;
                    return Err(e);
                }
            };
            let Some(slot) = vds.get_mut(ii) else {
                continue;
            };
            if ret == 1 {
                // Don't use RemoveVertexDefinition(), that removes it from the array.
                self.delete_vertex_definition(vd1);
                *slot = None;
            }
        }
        vds.retain(Option::is_some);
        self.write_vertex_definitions(pv, &vds)?;
        let num_vds = vds.len();

        // No further checks for a single VertexDef.
        if num_vds < 2 {
            return Ok(());
        }

        // Pairwise:
        // Not upstream (#16637): the computed points of the definitions combined away, and
        // whether every one of them could be computed (gathered before each delete, never
        // an error: a point that cannot be computed only stops the move below).
        let mut absorbed: Vec<Point3> = Vec::new();
        // each absorbed definition's tolerance and its range in `absorbed`
        let mut absorbed_defs: Vec<(f64, core::ops::Range<usize>)> = Vec::new();
        let mut absorbed_any = false;
        let mut absorbed_bounded = true;
        for ii in 0..num_vds - 1 {
            let Some(vd1) = vds.get(ii).copied().flatten() else {
                continue;
            };
            let mut jj = ii + 1;
            while jj < num_vds {
                let Some(vd2) = vds.get(jj).copied().flatten() else {
                    jj += 1;
                    continue;
                };
                let combined = self
                    .vertex_combine_factor(brep, vd1, vd2)
                    .and_then(|factor| {
                        self.can_combine_vertex_definitions_within(k, brep, vd1, vd2, factor)
                    })
                    // Not upstream (WR-306): the definition that stays takes over the hull
                    // of a self-loop the deleted one absorbed, and covers it
                    .and_then(|which| {
                        match which {
                            1 => self.hand_over_hull(k, vd1, vd2)?,
                            2 => self.hand_over_hull(k, vd2, vd1)?,
                            _ => {}
                        }
                        Ok(which)
                    });
                let delete_which = match combined {
                    Ok(w) => w,
                    Err(e) => {
                        // the definitions left are listed at `pv`, whatever the statuses: a
                        // hull hand-over's refusal (#18312) comes here with them ignored too
                        vds.retain(Option::is_some);
                        let listed = self.write_vertex_definitions(pv, &vds);
                        // Not upstream (#16637): on every error path, the points of those
                        // combined away so far stay covered where `pv` is
                        if absorbed_any {
                            let (ends, _, ends_bounded) =
                                self.attached_curve_ends(k, brep, pv, &absorbed, &absorbed_defs);
                            absorbed.extend(ends);
                            self.cover_in_place(
                                k,
                                pv,
                                &absorbed,
                                absorbed_bounded && ends_bounded,
                            )?;
                        }
                        listed?;
                        return Err(e);
                    }
                };
                if delete_which == 1 {
                    let (points, bounded) = self.absorbed_points_of(k, brep, vd1);
                    let tol = self.vd_ref(vd1).map_or(f64::NAN, |d| d.tolerance(k));
                    absorbed_defs.push((tol, absorbed.len()..absorbed.len() + points.len()));
                    absorbed.extend(points);
                    absorbed_any = true;
                    absorbed_bounded = absorbed_bounded && bounded;
                    self.delete_vertex_definition(vd1);
                    if let Some(s) = vds.get_mut(ii) {
                        *s = None;
                    }
                    // We're done with PV[ii].
                    jj = num_vds + 1;
                } else if delete_which == 2 {
                    let (points, bounded) = self.absorbed_points_of(k, brep, vd2);
                    let tol = self.vd_ref(vd2).map_or(f64::NAN, |d| d.tolerance(k));
                    absorbed_defs.push((tol, absorbed.len()..absorbed.len() + points.len()));
                    absorbed.extend(points);
                    absorbed_any = true;
                    absorbed_bounded = absorbed_bounded && bounded;
                    self.delete_vertex_definition(vd2);
                    if let Some(s) = vds.get_mut(jj) {
                        *s = None;
                    }
                }
                jj += 1;
            }
        }

        vds.retain(Option::is_some);
        self.write_vertex_definitions(pv, &vds)?;
        if absorbed_any {
            let (ends, survivor_ends, ends_bounded) =
                self.attached_curve_ends(k, brep, pv, &absorbed, &absorbed_defs);
            absorbed.extend(ends);
            self.place_at_survivors(
                k,
                brep,
                pv,
                &absorbed,
                &survivor_ends,
                absorbed_bounded && ends_bounded,
            )?;
        }
        Ok(())
    }

    /// Not upstream (#16637): the points a vertex definition (one about to be combined
    /// away, or one that remains when the proto vertex moves) holds, computed now: its
    /// position and, for each classification to an object that has points (all but
    /// unknown, nothing and region, whose point is the position itself), the object point
    /// at its parameters (`FindObjPoint3d`). Returns them and whether every one was
    /// computed, is finite and is consistent with the definition. The classifications'
    /// stored gaps (`m_dGap3d`) are not used: several writers set them to something other
    /// than the distance at the stored parameters.
    ///
    /// Only finite points are returned. The object points are returned only when they are
    /// consistent with the definition's position, the way the kernel places a definition
    /// (`find_position_from_pcs`):
    /// - With one object point, the position is that point, so the point must lie within
    ///   the definition's tolerance of it (the distance rounded up by `γ_4`, as
    ///   `attached_curve_ends` attributes ends).
    /// - With two, the position is the first, the second or a weighted point between
    ///   them, however far apart they are (that deviation goes into the gap, not the
    ///   tolerance). So the position must lie within the definition's tolerance of the
    ///   segment between them ([`within_tolerance_of_segment`]).
    ///
    /// Object points that fail this are not points the definition stands for, so covering
    /// them would only widen the tolerance by that inconsistency. The two writers that
    /// place a position from its object points, `find_position_from_pcs` and
    /// `resolve_vertex_definition` (`pt1 + frac * vec` with `frac` clamped into `[0, 1]`,
    /// on the segment within the same `γ_3` rounding), never build such a definition
    /// (argued). The self-loop split in `resolve_proto_edge` sets a position to a
    /// curve end and then re-projects the parameters from it, so its distance to the
    /// segment is bounded only by the smaller gap; there the claim rests only on the
    /// measured goldens count (4 rejections), not on an argument. Inconsistent object
    /// points, and an object point that cannot be computed (parameters not set, an
    /// evaluation error of any kind, a non-finite result), make the result unbounded
    /// instead of an error, so the vertex does not move and a resolution that
    /// combines definitions never fails here. That includes [`SolidError::Unported`]
    /// (an adapter that cannot resolve a topology handle): these evaluations are
    /// measurements upstream does not make, so they must not fail a resolution upstream
    /// completes; without them the vertex stays where upstream puts it. Point
    /// evaluation never polls the cancel token, so [`SolidError::Cancelled`] does not
    /// arise here; the resolution's callers poll it.
    fn absorbed_points_of(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        vd: VertexDefId,
    ) -> (Vec<Point3>, bool) {
        let Ok(d) = self.vd_ref(vd) else {
            return (Vec::new(), false);
        };
        let position = d.position();
        let tol = d.tolerance(k);
        let mut bounded = finite_point(position);
        // only finite points are recorded: a non-finite one has no distance to cover
        let mut points = Vec::new();
        if bounded {
            points.push(position);
        }
        let mut objs = Vec::new();
        for which in [1, 2] {
            let pc = d.brep_point_classif(which);
            if matches!(
                pc.point_class(),
                PointClassType::Unknown | PointClassType::Nothing | PointClassType::Region
            ) {
                continue;
            }
            match pc.find_obj_point_3d(k, brep) {
                Ok(p) if finite_point(p) => objs.push(p),
                _ => bounded = false,
            }
        }
        let consistent = match objs.as_slice() {
            [] => true,
            [p] => p.distance(position) * (1.0 + gamma(4)) <= tol,
            [a, b] => within_tolerance_of_segment(position, *a, *b, tol),
            _ => false,
        };
        if consistent {
            points.extend(objs);
        } else {
            bounded = false;
        }
        (points, bounded)
    }

    /// Not upstream (#16637): the ends at `pv` of the intersection curves on the proto
    /// edges that start or end at `pv` that trace to a definition combined away here. A
    /// definition stores the position it computed and its object points, not the point it
    /// classified (a curve end), so that end can lie a little off them (3.6e-13 in the
    /// #16637 cases).
    ///
    /// Each end is evaluated now and attributed by measured distance only:
    /// - It traces to an absorbed definition when it lies within that definition's
    ///   tolerance of one of its computed points (`absorbed`, ranges in `absorbed_defs`,
    ///   distances rounded up by `γ_4`). Such an end is returned, to be covered.
    /// - Otherwise it traces to a definition that remains at `pv` when it lies within that
    ///   definition's tolerance of one of its computed points. It stays that definition's
    ///   end, as upstream has it. It is returned second, to be covered only if the vertex
    ///   moves off upstream's position ([`Self::place_at_survivors`]): a move can take the
    ///   vertex farther from it than the tolerance reaches.
    /// - An end that traces to neither, or that cannot be evaluated, makes the result
    ///   unbounded (the vertex then does not move). This is never an error, for the
    ///   reasons [`Self::absorbed_points_of`] gives.
    fn attached_curve_ends(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pv: ProtoVertexId,
        absorbed: &[Point3],
        absorbed_defs: &[(f64, core::ops::Range<usize>)],
    ) -> (Vec<Point3>, Vec<Point3>, bool) {
        let Some(v) = self.proto_vertex(pv) else {
            return (Vec::new(), Vec::new(), false);
        };
        let within = |end: Point3, points: &[Point3], tol: f64| {
            points
                .iter()
                .any(|&h| h.distance(end) * (1.0 + gamma(4)) <= tol)
        };
        let mut survivors = Vec::new();
        for &vd in v.vertex_definitions() {
            let tol = self.vd_ref(vd).map_or(f64::NAN, |d| d.tolerance(k));
            survivors.push((tol, self.absorbed_points_of(k, brep, vd).0));
        }
        // a closed proto edge is listed twice
        let mut pes = v.proto_edges().to_vec();
        pes.sort_unstable();
        pes.dedup();
        let mut points = Vec::new();
        let mut survivor_ends = Vec::new();
        let mut bounded = true;
        for pe in pes {
            let Some(e) = self.proto_edge(pe) else {
                bounded = false;
                continue;
            };
            let at_start = e.start_proto_vertex() == Some(pv);
            let at_end = e.end_proto_vertex() == Some(pv);
            for &ed in e.edge_definitions() {
                let Ok(d) = self.ed_ref(ed) else {
                    bounded = false;
                    continue;
                };
                let Ok(curve) = curve_of(brep, d.xsect_curve_3d()) else {
                    bounded = false;
                    continue;
                };
                let ivl = d.xsect_interval();
                for (attached, t) in [(at_start, ivl.min()), (at_end, ivl.max())] {
                    if !attached {
                        continue;
                    }
                    let end = match curve.evaluate_point(k, t) {
                        Ok(p) if finite_point(p) => p,
                        _ => {
                            bounded = false;
                            continue;
                        }
                    };
                    if absorbed_defs.iter().any(|(tol, r)| {
                        absorbed
                            .get(r.clone())
                            .is_some_and(|h| within(end, h, *tol))
                    }) {
                        points.push(end);
                    } else if survivors.iter().any(|(tol, h)| within(end, h, *tol)) {
                        survivor_ends.push(end);
                    } else {
                        bounded = false;
                    }
                }
            }
        }
        (points, survivor_ends, bounded)
    }

    /// Not upstream (#16637): records `absorbed` (the computed points of definitions combined
    /// away, `absorbed_points_of`) at `pv` and covers every point it has absorbed where it
    /// is, without moving it ([`ProtoVertex::cover_absorbed`]), unless that grows it into
    /// another vertex definition (#18312, [`Self::set_grown_proto_vertex`]).
    fn cover_in_place(
        &mut self,
        k: &Kernel,
        pv: ProtoVertexId,
        absorbed: &[Point3],
        bounded: bool,
    ) -> Result<()> {
        let mut grown = self
            .proto_vertex(pv)
            .ok_or(dangling("proto vertex is deleted"))?
            .clone();
        grown.absorb(absorbed, bounded);
        grown.cover_absorbed();
        self.set_grown_proto_vertex(k, pv, grown)
    }

    /// Not upstream (#18312): writes `grown`, the proto vertex `pv` placed or covering what
    /// it absorbed (#16637), unless its grown tolerance reaches, from where `grown` is, a
    /// vertex definition of another proto vertex that `pv`'s tolerance kept clear of from
    /// where `pv` was ([`Self::growth_reaches_another`]; the placement is ours, not
    /// upstream's, so "clear before" is judged before it; `pv`'s own definitions are what it
    /// stands for).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when `pv` is deleted; the refusal
    /// ([`Self::refuse_growth_overlap`]) of a growth into another definition, with `pv`
    /// unchanged.
    fn set_grown_proto_vertex(
        &mut self,
        k: &Kernel,
        pv: ProtoVertexId,
        grown: ProtoVertex,
    ) -> Result<()> {
        let v = self
            .proto_vertex(pv)
            .ok_or(dangling("proto vertex is deleted"))?;
        if self.growth_reaches_another(
            k,
            v.position(),
            v.tolerance(k),
            grown.position(),
            grown.tolerance(k),
            None,
            v.vertex_definitions(),
        )? {
            return Err(self.refuse_growth_overlap(grown.tolerance(k)));
        }
        *self
            .proto_vertex_mut(pv)
            .ok_or(dangling("proto vertex is deleted"))? = grown;
        Ok(())
    }

    /// Not upstream (#16637): after `resolve_proto_vertex` combined definitions, records
    /// `absorbed` at `pv` and places `pv` at the average of the definitions that remain.
    /// Upstream leaves the proto vertex at the average of all its definitions, absorbed
    /// ones included: a point no remaining definition is at, which an edge end was then
    /// continued to (out of its face's domain at a pole).
    ///
    /// The move takes `pv` off the point upstream's tolerance was set around, so it also
    /// records the computed points of every remaining definition (`absorbed_points_of`:
    /// its position and object points) and `survivor_ends`, the attached curve ends that
    /// trace to a remaining definition (`attached_curve_ends`). The write then widens the
    /// tolerance to reach every point recorded, in this or an earlier resolution
    /// ([`ProtoVertex::set_position`]).
    ///
    /// The move needs every one of those points. If one could not be computed (`bounded`
    /// false, now or in an earlier resolution, or a remaining definition's point), or if
    /// the average is not finite, `pv` stays at upstream's position. It then covers only
    /// the absorbed points there.
    ///
    /// Not upstream (#18312): the placed vertex is written only when its grown tolerance
    /// reaches no vertex definition of another proto vertex that its tolerance before kept
    /// clear of ([`Self::set_grown_proto_vertex`]); refused otherwise, with `pv` unchanged.
    fn place_at_survivors(
        &mut self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pv: ProtoVertexId,
        absorbed: &[Point3],
        survivor_ends: &[Point3],
        bounded: bool,
    ) -> Result<()> {
        let mut v = self
            .proto_vertex(pv)
            .ok_or(dangling("proto vertex is deleted"))?
            .clone();
        v.absorb(absorbed, bounded);
        let unbounded = v.absorbed_unbounded();
        let live = v.vertex_definitions().to_vec();
        let mut position = None;
        let mut held = survivor_ends.to_vec();
        if !unbounded && !live.is_empty() {
            let mut sum = Point3::new(0.0, 0.0, 0.0);
            let mut all_bounded = true;
            for &vd in &live {
                sum += self.vd_ref(vd)?.position();
                let (points, bounded) = self.absorbed_points_of(k, brep, vd);
                held.extend(points);
                all_bounded = all_bounded && bounded;
            }
            let average = sum / live.len() as f64;
            position = (all_bounded && finite_point(average)).then_some(average);
        }
        // a move: the remaining definitions' points are covered from the new position. With
        // no move the position is the one upstream's tolerance was set around, which then
        // stands for them (as when the vertex does not move at all).
        if position.is_some_and(|p| p != v.position()) {
            v.absorb(&held, true);
        }
        match position {
            Some(p) => v.set_position(p),
            None => v.cover_absorbed(),
        }
        self.set_grown_proto_vertex(k, pv, v)
    }

    /// Not upstream (WR-306): the hull points the definition `from`, about to be combined
    /// away, recorded for the self-loops it absorbed go to `to`, which grows to cover them
    /// where it is ([`VertexDefinition::take_over_hull`]). Nothing changes when `from` holds
    /// none. Not upstream (#18312): nor when the growth would reach another vertex
    /// definition `to`'s tolerance kept clear of ([`Self::growth_reaches_another`]); the
    /// combine is refused then.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a deleted definition; [`SolidError::Failed`] when
    /// a point lies at no finite distance from `to`, and the refusal
    /// ([`Self::refuse_growth_overlap`]) of a growth into another definition (`to` is
    /// unchanged in both).
    fn hand_over_hull(&mut self, k: &Kernel, from: VertexDefId, to: VertexDefId) -> Result<()> {
        let hull = self.vd_ref(from)?.absorbed_hull().to_vec();
        if hull.is_empty() {
            return Ok(());
        }
        let current = self.vd_ref(to)?;
        let mut grown = current.clone();
        grown.take_over_hull(k, &hull)?;
        if self.growth_reaches_another(
            k,
            current.position(),
            current.tolerance(k),
            grown.position(),
            grown.tolerance(k),
            current.proto_vertex(),
            &[to, from],
        )? {
            return Err(self.refuse_growth_overlap(grown.tolerance(k)));
        }
        *self.vd_mut(to)? = grown;
        Ok(())
    }

    /// Sets the vertex definitions listed at `pv`.
    fn write_vertex_definitions(
        &mut self,
        pv: ProtoVertexId,
        vds: &[Option<VertexDefId>],
    ) -> Result<()> {
        let v = self
            .proto_vertex_mut(pv)
            .ok_or(dangling("proto vertex is deleted"))?;
        *v.vertex_definitions_mut() = vds.iter().flatten().copied().collect();
        Ok(())
    }

    /// Not upstream: the combine radius of two vertex definitions in units of their summed
    /// tolerances: [`EXACT_TOL_FACTOR`] when both are exactly placed (each source object a
    /// vertex, an edge on a line or a face on a plane), upstream's `VD_TOL_FACTOR`
    /// otherwise.
    fn vertex_combine_factor(
        &self,
        brep: &impl ProtoBrep<O>,
        vd1: VertexDefId,
        vd2: VertexDefId,
    ) -> Result<f64> {
        let exact = |vd: VertexDefId| -> Result<bool> {
            let d = self.vd_ref(vd)?;
            Ok(exactly_placed(brep, d.src_topo(1)) && exactly_placed(brep, d.src_topo(2)))
        };
        Ok(if exact(vd1)? && exact(vd2)? {
            EXACT_TOL_FACTOR
        } else {
            VD_TOL_FACTOR
        })
    }

    /// Not upstream: the combine radius of two edge definitions of one proto edge in units
    /// of their summed tolerances: [`EXACT_TOL_FACTOR`] when both are exactly placed (each
    /// source object a vertex, an edge on a line or a face on a plane, and the 3D curve a
    /// line), upstream's factor 5 otherwise.
    fn edge_combine_factor(
        &self,
        brep: &impl ProtoBrep<O>,
        ed1: EdgeDefId,
        ed2: EdgeDefId,
    ) -> Result<f64> {
        let exact = |ed: EdgeDefId| -> Result<bool> {
            let d = self.ed_ref(ed)?;
            Ok(exactly_placed(brep, d.src_topo(1))
                && exactly_placed(brep, d.src_topo(2))
                && matches!(
                    curve_of(brep, d.xsect_curve_3d())?.kind(),
                    CurveKind::Line(_)
                ))
        };
        Ok(if exact(ed1)? && exact(ed2)? {
            EXACT_TOL_FACTOR
        } else {
            ED_TOL_FACTOR
        })
    }

    /// Finds or creates a vertex definition of the classifications `pc1`, `pc2` at
    /// the proto vertex `pv` (`AddVertexDefinition(rPC1, rPC2, ..)`): an existing
    /// definition on the same two topology objects, or a new one
    /// ([`ProtoTopologyManager::create_vertex_definition`], which may compute the
    /// classifications' parameters) listed at `pv`, whose position becomes the average
    /// of its definitions' and whose tolerance grows to the objects' zone tolerances.
    /// Returns the definition and whether it was created.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when either classification is not on a topology object;
    /// those of [`ProtoTopologyManager::create_vertex_definition`].
    pub fn add_vertex_definition_from_pcs(
        &mut self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pv: ProtoVertexId,
        pc1: &mut PointClassification<O>,
        pc2: &mut PointClassification<O>,
    ) -> Result<(VertexDefId, bool)> {
        let off = || SolidError::topology(OP_ADD_VD, TopologyFailure::ClassificationOffTopology);
        let topo1 = as_topology(pc1.object()).ok_or_else(off)?;
        let topo2 = as_topology(pc2.object()).ok_or_else(off)?;

        // no work - VertexDefintion with input Tgt SrcTopos already exists
        if let Some(vd) = self.find_matching_vertex_definition(pv, Some(topo1), Some(topo2))? {
            return Ok((vd, false));
        }

        // create Vertex definition for input PointClassifications
        let vd = self.create_vertex_definition(k, brep, pc1, pc2)?;

        // add VertexDef to ProtoVertex list
        self.vd_mut(vd)?.set_proto_vertex(Some(pv));
        let this = self
            .proto_vertex_mut(pv)
            .ok_or(dangling("proto vertex is deleted"))?;
        this.vertex_definitions_mut().push(vd);

        // ProtoVertex position = Avg(VertexDef positions)
        let vds = this.vertex_definitions().to_vec();
        let mut avg = Point3::new(0.0, 0.0, 0.0);
        for &x in &vds {
            avg += self.vd_ref(x)?.position();
        }
        let position = avg / vds.len() as f64;

        // ProtoVertex ZoneTol3d
        let tol1 = zone_tol_of(k, brep, pc1.object())?;
        let tol2 = zone_tol_of(k, brep, pc2.object())?;
        let this = self
            .proto_vertex_mut(pv)
            .ok_or(dangling("proto vertex is deleted"))?;
        // (also covers the points of definitions combined away here earlier, #16637)
        this.set_position(position);
        let tol = max3(this.raw_tolerance(), tol1, tol2);
        this.set_raw_tolerance(tol);
        Ok((vd, true))
    }

    // ---- SmProtoEdge -----------------------------------------------------------------

    /// The definition of the proto edge `pe` identical to the proposed one of the
    /// interval pair `ci1`, `ci2` (`FindMatchingEdgeDefinition`): the same shape
    /// between the proto edge's ends and the same source objects as the intervals'
    /// midpoints. `cc1` is the classification `ci1` belongs to (its curve is the
    /// intersection curve).
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::is_same_edge_definition`];
    /// [`SolidError::InvalidInput`] for a deleted proto edge.
    pub fn find_matching_edge_definition(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        cc1: &CurveClassification<O>,
        ci1: &CurveInterval<O>,
        ci2: &CurveInterval<O>,
    ) -> Result<Option<EdgeDefId>> {
        let e = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?;
        let xsect_crv = cc1.curve();
        let ivl = ci1.interval;
        let (start, end) = (e.start_proto_vertex(), e.end_proto_vertex());

        for &this_ed in e.edge_definitions() {
            let (same, identical) = self.is_same_edge_definition(
                k, brep, this_ed, start, end, &ci1.mid, &ci2.mid, xsect_crv, &ivl,
            )?;
            if same && identical {
                return Ok(Some(this_ed));
            }
        }
        Ok(None)
    }

    /// Lists the edge definition `ed` at the proto edge `pe` (`AddEdgeDefinition`):
    /// the first definition sets the proto edge's interval; a later one is
    /// reparameterised to it (reversed when its mid tangent opposes the proto edge's)
    /// when needed. A definition owned by another proto edge is unlisted there.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects or a failing mid evaluation
    /// (upstream then reads an uninitialised tangent); [`SolidError::Unported`] from
    /// the evaluations and the reparameterisation.
    pub fn add_edge_definition(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        ed: EdgeDefId,
    ) -> Result<()> {
        let pe = self.pe_live(Some(pe))?;
        let d = self.ed_ref(ed)?;
        let ivl = d.xsect_interval();
        let owning_pe = d.proto_edge();
        let e = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?;

        if e.num_edge_definitions() == 0 {
            // set ProtoEdge->Interval = EdgeDefinition->Interval
            self.proto_edge_mut(pe)
                .ok_or(dangling("proto edge is deleted"))?
                .set_xsect_interval(ivl);
        } else {
            let pe_ivl = e.xsect_interval();
            let uninit = || dangling("mid tangent is uninitialised (evaluation failed)");
            // statuses passed on as the fork does (bcc6f01)
            let pe_tan = self
                .proto_edge_evaluate_mid(k, brep, pe, false, true)?
                .1
                .ok_or_else(uninit)?;
            let ed_tan = self
                .ed_ref(ed)?
                .evaluate_mid(k, brep, false, true)?
                .1
                .ok_or_else(uninit)?;

            // remember when ProtoEdge->Tan is opposite to EdgeDef->Tan
            let reverse = pe_tan.dot(ed_tan) < 0.0;

            // when EdgeDef parameter opposes ProtoEdges or Ivls don't match - reparameterize
            if reverse || !ivl.are_same(&pe_ivl) {
                self.ed_mut(ed)?.reparameterize(k, brep, &pe_ivl, reverse)?;
            }
        }

        // EdgeDef->Owner is not ProtoEdge - remove EdgeDef from its owner.
        if let Some(owner) = owning_pe {
            if owner != pe {
                let o = self
                    .proto_edge_mut(owner)
                    .ok_or(dangling("owning proto edge is deleted"))?;
                o.remove_edge_definition(ed)?;
            }
        }

        // add EdgeDef to this ProtoEdge
        self.proto_edge_mut(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .edge_definitions_mut()
            .push(ed);
        self.ed_mut(ed)?.set_proto_edge(Some(pe));
        Ok(())
    }

    /// Adds the definition of the interval pair `ci1`, `ci2` to the proto edge `pe`
    /// unless an identical one exists (`AddEdgeDefinition(crCI1, crCI2)`): a trimmed
    /// copy of the intersection curve over `ci1`'s interval, on the intervals' midpoint
    /// objects, with the larger of the classifications' source tolerances. `cc1`,
    /// `cc2` are the classifications the intervals belong to.
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::find_matching_edge_definition`] and of the
    /// curve copy (`SM_ERR_NULL_POINTER` without a copy);
    /// [`SolidError::InvalidInput`] for a midpoint object that is not topology (its
    /// type is read) or a missing curve.
    pub fn add_edge_definition_from_intervals(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        cc1: &CurveClassification<O>,
        ci1: &CurveInterval<O>,
        cc2: &CurveClassification<O>,
        ci2: &CurveInterval<O>,
    ) -> Result<()> {
        // no work - identical EdgeDefinition already exists
        if self
            .find_matching_edge_definition(k, brep, pe, cc1, ci1, ci2)?
            .is_some()
        {
            return Ok(());
        }

        let mut ivl = ci1.interval;
        let topo1 = as_topology(ci1.mid.object());
        let topo2 = as_topology(ci2.mid.object());
        let tol = max2(cc1.src_zone_tol_3d(), cc2.src_zone_tol_3d());

        // copy and trim the XSectCurve3d to place in new EdgeDefinition
        let mut copy = curve_of(brep, cc1.curve())?.copy(k)?;
        // statuses passed on as the fork does (bcc6f01)
        copy.trim(k, &mut ivl, TrimMode::Notify)?;

        // the new definition reads both objects' types
        let topo1 = topo1.ok_or(dangling("source object is not topology"))?;
        let topo2 = topo2.ok_or(dangling("source object is not topology"))?;
        let c3 = brep
            .insert_curve(copy)
            .ok_or(dangling("curve store is full"))?;
        let new_ed = self.insert_edge_definition(EdgeDefinition::new(
            Some(c3),
            ivl,
            topo1,
            topo2,
            tol,
            None,
            None,
            Some(pe),
        ))?;
        // status ignored upstream; passed on as the fork does (bcc6f01)
        self.add_edge_definition(k, brep, pe, new_ed)?;
        Ok(())
    }

    /// Merges the proto edge `other` into `pe` (`CombineProtoEdge`): the larger
    /// tolerance, `other`'s overlapping proto edges, `other`'s ends moved to `pe`'s
    /// (listing `pe` at `pe`'s ends), `other`'s edge definitions added to `pe` (all
    /// of them, from a copy of the list), and `other`'s coincident proto faces moved
    /// to `pe`. An `other` from `pe`'s end to its start, the other way between the same
    /// two proto vertices, already ends at `pe`'s proto vertices: no end moves (not
    /// upstream, which compares start with start; see the file's deviations).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted or null objects that are dereferenced;
    /// those of [`ProtoTopologyManager::add_edge_definition`] that cannot be ignored.
    pub fn combine_proto_edge(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        other: ProtoEdgeId,
    ) -> Result<()> {
        // If they're the same, do nothing.
        if other == pe {
            return Ok(());
        }
        // fork bcc6f01 (`CombineProtoEdge` returns the status of its edits)
        let pe = self.pe_live(Some(pe))?;
        let other = self.pe_live(Some(other))?;
        let o = self
            .proto_edge(other)
            .ok_or(dangling("proto edge is deleted"))?
            .clone();
        let this = self
            .proto_edge_mut(pe)
            .ok_or(dangling("proto edge is deleted"))?;
        this.set_raw_tolerance(max2(this.raw_tolerance(), o.raw_tolerance()));
        this.append_overlapping_pes(o.overlapping_pes());

        let (this_start, this_end) = (this.start_proto_vertex(), this.end_proto_vertex());
        let (other_start, other_end) = (o.start_proto_vertex(), o.end_proto_vertex());
        // not upstream: running the other way between `pe`'s two proto vertices, `other`
        // keeps its ends, where it is listed, and `pe` is not listed there again
        let backwards = other_start.is_some()
            && other_end.is_some()
            && other_start == this_end
            && other_end == this_start;
        if other_start != this_start && !backwards {
            self.proto_edge_mut(other)
                .ok_or(dangling("proto edge is deleted"))?
                .set_start_proto_vertex(this_start);
            let os = self.pv_live(other_start)?;
            let removed = self
                .proto_vertex_mut(os)
                .ok_or(dangling("proto vertex is deleted"))?
                .remove_proto_edge(other);
            removed?;
            let ts = self.pv_live(this_start)?;
            self.proto_vertex_mut(ts)
                .ok_or(dangling("proto vertex is deleted"))?
                .add_proto_edge(pe);
        }

        if other_end != this_end && !backwards {
            self.proto_edge_mut(other)
                .ok_or(dangling("proto edge is deleted"))?
                .set_end_proto_vertex(this_end);
            let oe = self.pv_live(other_end)?;
            let removed = self
                .proto_vertex_mut(oe)
                .ok_or(dangling("proto vertex is deleted"))?
                .remove_proto_edge(other);
            removed?;
            let te = self.pv_live(this_end)?;
            self.proto_vertex_mut(te)
                .ok_or(dangling("proto vertex is deleted"))?
                .add_proto_edge(pe);
        }

        // Add in other PE's EdgeDefinitions, and remove them from pOtherPE.
        // AddEdgeDefinition already removes the definition from its previous owner, so
        // walk a copy of the list.
        let other_eds = self
            .proto_edge(other)
            .ok_or(dangling("proto edge is deleted"))?
            .edge_definitions()
            .to_vec();
        for ed in other_eds {
            self.add_edge_definition(k, brep, pe, ed)?;
            let _ = self
                .proto_edge_mut(other)
                .ok_or(dangling("proto edge is deleted"))?
                .remove_edge_definition(ed);
        }

        // Take over pOtherPE's coincident ProtoFaces. The caller deletes pOtherPE right
        // after this call, and RemoveProtoEdge does not update ProtoFaces, so a
        // ProtoFace would otherwise keep pOtherPE in its coincident proto edges.
        let other_pfs = self
            .proto_edge(other)
            .ok_or(dangling("proto edge is deleted"))?
            .coinc_proto_faces()
            .to_vec();
        for pf in other_pfs {
            let pf_edges = self
                .proto_face_mut(pf)
                .ok_or(dangling("proto face is deleted"))?
                .coinc_proto_edges_mut();
            if let Some(idx) = pf_edges.iter().position(|&x| x == other) {
                pf_edges.remove(idx);
            }
            self.relate_coincident_pf(pe, pf)?;
        }
        self.proto_edge_mut(other)
            .ok_or(dangling("proto edge is deleted"))?
            .coinc_proto_faces_mut()
            .clear();
        Ok(())
    }

    /// Not upstream (WR-306, #1058): whether the proto edge from `start` back to itself,
    /// with first curve `int_curve` and definitions `eds`, is a speck its vertex holds.
    /// That needs both of `int_curve`'s ends to find one vertex definition of `start`,
    /// and every definition's curve, over its own interval, to lie within that
    /// definition's tolerance of its position, decided on a certified hull of the curve
    /// ([`curve_piece_within`]). That is what is proven; the vertex made from that
    /// definition standing for the whole curve is argued: `ImprintIntersections` then fits
    /// it to its edges and grows its tolerance to cover them. A curve with no certain
    /// hull, or one reaching past the tolerance, is not absorbed: the capture distance,
    /// which only says how far apart definitions are merged, never decides it. Returns that
    /// definition with the hull points of every definition's curve, which it records
    /// ([`VertexDefinition::absorb_hull`]) so that a later move keeps them covered.
    fn curve_inside_one_vertex(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        start: Option<ProtoVertexId>,
        int_curve: &Curve,
        eds: &[EdgeDefId],
    ) -> Result<Option<AbsorbedCurve>> {
        // A null or deleted proto vertex holds nothing: upstream's short branch raises
        // no error for it, so neither does this test.
        let Some(start) = start.filter(|&pv| self.proto_vertex(pv).is_some()) else {
            return Ok(None);
        };
        let ends = int_curve.ends(k)?;
        let Some(vd0) = self.closest_vertex_definition(start, ends.start)? else {
            return Ok(None);
        };
        if self.closest_vertex_definition(start, ends.end)? != Some(vd0) {
            return Ok(None);
        }
        let v0 = self.vd_ref(vd0)?;
        let (centre, reach) = (v0.position(), v0.tolerance(k));
        let mut hull = Vec::new();
        for &ed in eds {
            let d = self.ed_ref(ed)?;
            let ivl = d.xsect_interval();
            let curve = curve_of(brep, d.xsect_curve_3d())?;
            if !curve_piece_within(curve, &ivl, centre, reach) {
                return Ok(None);
            }
            // (`curve_piece_within` holds only with a certain hull)
            hull.extend(curve.piece_hull_points(&ivl).into_iter().flatten());
        }
        Ok(Some((vd0, hull)))
    }

    /// Not upstream (WR-306): the refusal of a split that would move a definition holding
    /// an absorbed curve where its tolerance cannot cover that curve without reaching
    /// another vertex definition, of its own proto vertex or of any other listed one (or
    /// to no finite distance from it: `needed` is `None`), kept
    /// for the Boolean to return as the other self-loop refusals are.
    fn refuse_moving_absorbed(&mut self, needed: Option<f64>) -> SolidError {
        let refusal = SolidError::boolean(
            OP_RESOLVE_PE,
            BooleanFailure::SelfLoopSplitReachesVertex { needed },
        );
        self.note_self_loop_refusal(refusal.clone());
        refusal
    }

    /// Not upstream (#18312): the refusal of a resolution that would grow a vertex
    /// definition or a proto vertex, to cover what it absorbed, to tolerance `needed` and
    /// into another vertex definition its tolerance kept clear of
    /// ([`Self::growth_reaches_another`]), kept for the Boolean to return as the self-loop
    /// refusals are.
    pub(crate) fn refuse_growth_overlap(&mut self, needed: f64) -> SolidError {
        let refusal = SolidError::boolean(
            OP_RESOLVE_PV,
            BooleanFailure::AbsorbedCoverReachesVertex { needed },
        );
        self.note_self_loop_refusal(refusal.clone());
        refusal
    }

    /// Not upstream (WR-306, #18312): whether a tolerance ball of radius `old_tol` at
    /// `before`, grown to radius `grown` at `centre`, reaches a vertex definition that the
    /// ball before kept clear of ([`grown_into`]): one of the proto vertex `own`, or of any
    /// other listed one, other than `skip` (the definitions the ball stands for) and deleted
    /// ones. An overlap the growth does not cause is left as upstream leaves it.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when `own` is deleted.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn growth_reaches_another(
        &self,
        k: &Kernel,
        before: Point3,
        old_tol: f64,
        centre: Point3,
        grown: f64,
        own: Option<ProtoVertexId>,
        skip: &[VertexDefId],
    ) -> Result<bool> {
        let mut pvs = Vec::new();
        if let Some(own) = own {
            self.proto_vertex(own)
                .ok_or(dangling("proto vertex is deleted"))?;
            pvs.push(own);
        }
        pvs.extend(self.proto_vertices().iter().filter(|&&pv| Some(pv) != own));
        for pv in pvs {
            // (a deleted proto vertex or definition holds no vertex)
            let Some(v) = self.proto_vertex(pv) else {
                continue;
            };
            for &x in v.vertex_definitions() {
                if skip.contains(&x) {
                    continue;
                }
                if let Some(d) = self.vertex_def(x) {
                    if grown_into(before, old_tol, centre, grown, d.position(), d.tolerance(k)) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Resolves the proto edge `pe` (`Resolve`). A proto edge from a proto vertex to
    /// itself loses its definitions when its first curve is shorter than twice its
    /// tolerance; when it is longer with ends farther apart than that, the start proto
    /// vertex gets a second vertex definition at the curve's end (unless the ends are
    /// already closest to different definitions). Not upstream: when the
    /// definition to split is on a vertex in either B-rep, the proto edge loses its
    /// definitions instead (tangency-labs/solidcore#480; see the module notes). Then, of every pair
    /// of definitions that can be combined, the redundant one is deleted. Returns the
    /// curves deleted.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted or null objects that are dereferenced;
    /// those of [`ProtoTopologyManager::create_vertex_definition`] and of the curve
    /// length and ends; [`SolidError::Unported`] from the comparisons.
    pub fn resolve_proto_edge(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        pe: ProtoEdgeId,
    ) -> Result<Vec<O>> {
        let pe = self.pe_live(Some(pe))?;
        let mut deleted = Vec::new();
        let e = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?;
        let eds = e.edge_definitions().to_vec();
        if eds.is_empty() {
            return Ok(deleted);
        }

        // Check for degenerate ProtoEdge: Start and End on same ProtoVertex, and very
        // short.
        let (start, end) = (e.start_proto_vertex(), e.end_proto_vertex());
        let pe_ivl = e.xsect_interval();
        let pe_tol = e.raw_tolerance();
        if start == end {
            let first = *eds.first().ok_or(dangling("no edge definition"))?;
            let int_curve = curve_of(brep, self.ed_ref(first)?.xsect_curve_3d())?;
            let (len, _) = int_curve.approximate_length(k, &pe_ivl, 5, false)?;

            // Not upstream (WR-306, #1058): a curve whose ends find one vertex definition
            // and that provably lies within that definition's tolerance is that vertex,
            // not a closed edge on it, whether or not it is also shorter than upstream's
            // test (twice our own tolerance), which stays as it is. Only a curve inside
            // the vertex's tolerance marks the vertex for ImprintIntersections' fit; a
            // short curve outside it is dropped as upstream drops it, unmarked.
            let short = len < 2.0 * pe_tol;
            let inside = self.curve_inside_one_vertex(k, &*brep, start, int_curve, &eds)?;

            if short || inside.is_some() {
                if let Some((vd0, hull)) = inside {
                    // The edges that end on our proto vertex end on our curve, up to its
                    // length away from the vertex: ImprintIntersections fits the vertex
                    // to them. The definition keeps the curve's hull covered if a later
                    // split moves it.
                    self.vd_mut(vd0)?.absorb_hull(hull);
                    let start = self.pv_live(start)?;
                    self.proto_vertex_mut(start)
                        .ok_or(dangling("proto vertex is deleted"))?
                        .set_absorbed_proto_edge();
                }
                // remove all of our EdgeDefinitions
                for &ed in &eds {
                    deleted.extend(self.destruct_and_delete(ed));
                }
                self.proto_edge_mut(pe)
                    .ok_or(dangling("proto edge is deleted"))?
                    .clear_edge_definitions();
            } else {
                // Our IntCurve is longer than tolerance. Check start & end points.
                let ends = int_curve.ends(k)?;
                let (pt0, pt1) = (ends.start, ends.end);

                // Not upstream (WR-579, #1517; the fork's tangency-labs/smlib@9eec159):
                // a curve joining two of the proto vertex's definitions is split at each
                // other definition of it that lies on the curve.
                let joins_two = {
                    let start = self.pv_live(start)?;
                    let vd0 = self.closest_vertex_definition(start, pt0)?;
                    let vd1 = self.closest_vertex_definition(start, pt1)?;
                    (vd0 != vd1).then_some((start, vd0, vd1))
                };
                let dist = pt0.distance(pt1);
                if let Some((start, vd0, vd1)) = joins_two {
                    self.split_at_captured_vertex_definitions(
                        k,
                        brep,
                        pe,
                        start,
                        (pt0, pt1),
                        [vd0, vd1],
                    )?;
                } else if dist > 2.0 * pe_tol {
                    // Check whether there are two VertexDefs, one for each end.
                    let start = self.pv_live(start)?;
                    let mut add_vtx_def = false;
                    let vd0 = self.closest_vertex_definition(start, pt0)?;
                    let num_vds = self
                        .proto_vertex(start)
                        .map_or(0, ProtoVertex::num_vertex_definitions);
                    if num_vds < 2 {
                        add_vtx_def = true;
                    } else {
                        let vd1 = self.closest_vertex_definition(start, pt1)?;
                        if vd0 == vd1 {
                            add_vtx_def = true;
                        }
                    }

                    // Not upstream (tangency-labs/solidcore#480): the new definition
                    // copies vd0's classifications, so when vd0 is on a vertex in
                    // either B-rep both ends land on that one vertex there while the
                    // other B-rep gets an edge between two vertices. Treat the proto
                    // edge as degenerate instead, as the short-curve branch above does,
                    // when the whole curve stays within its ends' reach of vd0 (its
                    // ends are already one place there); a curve that leaves that
                    // reach is a real intersection that neither the split nor the drop
                    // keeps, so the Boolean is refused.
                    let on_vertex = match vd0 {
                        Some(vd0) => {
                            let v0 = self.vd_ref(vd0)?;
                            v0.src_topo_type(1) == PointClassType::Vertex
                                || v0.src_topo_type(2) == PointClassType::Vertex
                        }
                        None => false,
                    };
                    if add_vtx_def && on_vertex {
                        let vd0 = vd0.ok_or(dangling("proto vertex has no vertex definition"))?;
                        let v0 = self.vd_ref(vd0)?;
                        let centre = v0.position();
                        let vd_tol = v0.tolerance(k);
                        // Every definition's curve, over its own interval, from its own
                        // ends: the proto edge may hold different curves.
                        let mut left = None;
                        for &ed in &eds {
                            let d = self.ed_ref(ed)?;
                            let ivl = d.xsect_interval();
                            let curve = curve_of(brep, d.xsect_curve_3d())?;
                            let ends = [
                                curve.evaluate_point(k, ivl.min())?,
                                curve.evaluate_point(k, ivl.max())?,
                            ];
                            let reach = max2(centre.distance(ends[0]), centre.distance(ends[1]))
                                + vd_tol
                                + pe_tol;
                            if !curve_piece_within(curve, &ivl, centre, reach) {
                                left = Some(reach);
                                break;
                            }
                        }
                        if let Some(reach) = left {
                            let refusal = SolidError::boolean(
                                OP_RESOLVE_PE,
                                BooleanFailure::SelfLoopLeavesReach { reach },
                            );
                            // The Boolean's resolution only warns on a proto edge's error:
                            // the manager keeps the refusal for the Boolean to return.
                            self.note_self_loop_refusal(refusal.clone());
                            return Err(refusal);
                        }
                        for &ed in &eds {
                            deleted.extend(self.destruct_and_delete(ed));
                        }
                        self.proto_edge_mut(pe)
                            .ok_or(dangling("proto edge is deleted"))?
                            .clear_edge_definitions();
                    } else if add_vtx_def {
                        // Create a new VtxDef, and set pVD0 to sPt0, and NewVD to sPt1.
                        let vd0 = vd0.ok_or(dangling("proto vertex has no vertex definition"))?;
                        // Not upstream (WR-306): vd0 may hold a curve it absorbed earlier,
                        // which the move must keep within its tolerance (`set_position`
                        // grows it). A move at no finite distance from that curve is
                        // refused before anything changes.
                        let Some(cover) = self.vd_ref(vd0)?.covering_tolerance(k, pt0) else {
                            return Err(self.refuse_moving_absorbed(None));
                        };
                        let v0 = self.vd_ref(vd0)?;
                        let mut pc1 = *v0.brep_point_classif(1);
                        let mut pc2 = *v0.brep_point_classif(2);
                        let created = self.create_vertex_definition(k, brep, &mut pc1, &mut pc2);
                        // the classifications are vd0's own (passed by reference)
                        let v0 = self.vd_mut(vd0)?;
                        *v0.brep_point_classif_mut(1) = pc1;
                        *v0.brep_point_classif_mut(2) = pc2;
                        let new_vd = created?;

                        // Not upstream (WR-306): vd0's growth may not make it reach another
                        // vertex definition, of this proto vertex or of any other listed
                        // one, the new one at pt1 included, that its tolerance before the
                        // move kept clear of, or the two would make overlapping vertices;
                        // refused then, with the new definition deleted. An overlap the
                        // growth does not cause (or a NaN distance) is left as upstream
                        // leaves it.
                        let old_tol = self.vd_ref(vd0)?.tolerance(k);
                        let new_tol = self.vd_ref(new_vd)?.tolerance(k);
                        if grown_into(pt0, old_tol, pt0, cover, pt1, new_tol)
                            || self.growth_reaches_another(
                                k,
                                pt0,
                                old_tol,
                                pt0,
                                cover,
                                Some(start),
                                &[vd0],
                            )?
                        {
                            self.delete_vertex_definition(new_vd);
                            return Err(self.refuse_moving_absorbed(Some(cover)));
                        }

                        let v0 = self.vd_mut(vd0)?;
                        v0.set_position(k, pt0)?;
                        v0.set_point_parameters(k, brep)?;
                        let nv = self.vd_mut(new_vd)?;
                        nv.set_position(k, pt1)?;
                        nv.set_point_parameters(k, brep)?;

                        // status ignored upstream (passed on as the fork does, bcc6f01)
                        self.add_vertex_definition(start, new_vd)?;
                    }
                }
            }
        }

        // Now check EdgeDefs.
        let num = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .num_edge_definitions();
        if num < 2 {
            return Ok(deleted);
        }

        // Pairwise (upstream steps the unsigned indices back after a removal; the
        // first index wraps around and back to 0):
        let mut ii: usize = 0;
        loop {
            let size = self.pe_eds_len(pe)?;
            if ii >= size.wrapping_sub(1) {
                break;
            }
            let Some(ed1) = self.pe_ed_at(pe, ii)? else {
                ii = ii.wrapping_add(1);
                continue;
            };
            let mut jj = ii + 1;
            while jj < self.pe_eds_len(pe)? {
                let Some(ed2) = self.pe_ed_at(pe, jj)? else {
                    jj += 1;
                    continue;
                };
                // statuses passed on as the fork does
                // (bcc6f01)
                let factor = self.edge_combine_factor(&*brep, ed1, ed2)?;
                let delete_which =
                    self.can_combine_edge_definitions_within(k, brep, ed1, ed2, factor)?;
                if delete_which == 1 {
                    self.proto_edge_mut(pe)
                        .ok_or(dangling("proto edge is deleted"))?
                        .remove_edge_definition(ed1)?;
                    deleted.extend(self.destruct_and_delete(ed1));
                    ii = ii.wrapping_sub(1);
                    break;
                } else if delete_which == 2 {
                    self.proto_edge_mut(pe)
                        .ok_or(dangling("proto edge is deleted"))?
                        .remove_edge_definition(ed2)?;
                    deleted.extend(self.destruct_and_delete(ed2));
                    jj -= 1;
                }
                jj += 1;
            }
            ii = ii.wrapping_add(1);
        }
        Ok(deleted)
    }

    /// Not upstream (WR-579, #1517; the fork's tangency-labs/smlib@9eec159): the proto
    /// edge `pe` from `start` back to itself, whose curve ends `ends` find the two
    /// definitions `joined`, is split at each other definition of `start` that lies on
    /// its curve, then `start` is resolved. The capture ball holds the curve's ends and a
    /// third definition as one place, the definitions stay three vertices: kept whole,
    /// the edge ran past the third one, a vertex of its own face (two tubes a hair apart
    /// end to end).
    ///
    /// A definition counts when it is farther than twice `on_tol` from both ends and
    /// `DropPoint` (no guess, `on_tol` = the larger of the proto edge's raw tolerance
    /// and the definition's) finds it on the curve within `on_tol`, inside the
    /// interval. Splits run from the far end back, so `pe` stays the piece from the
    /// start.
    ///
    /// # Errors
    ///
    /// Those of the drops (propagated, as the fork's `SER`), of
    /// [`ProtoTopologyManager::proto_edge_split`] and of
    /// [`ProtoTopologyManager::resolve_proto_vertex`].
    fn split_at_captured_vertex_definitions(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        pe: ProtoEdgeId,
        start: ProtoVertexId,
        ends: (Point3, Point3),
        joined: [Option<VertexDefId>; 2],
    ) -> Result<()> {
        let e = self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?;
        let ivl = e.xsect_interval();
        let pe_tol = e.raw_tolerance();
        let first = *e
            .edge_definitions()
            .first()
            .ok_or(dangling("no edge definition"))?;
        let curve = self.ed_ref(first)?.xsect_curve_3d();
        let vds = self
            .proto_vertex(start)
            .ok_or(dangling("proto vertex is deleted"))?
            .vertex_definitions()
            .to_vec();

        let mut splits = Vec::new();
        let int_curve = curve_of(brep, curve)?;
        for vd in vds {
            if joined.contains(&Some(vd)) {
                continue;
            }
            let v = self.vd_ref(vd)?;
            let pos = v.position();
            let on_tol = max2(pe_tol, v.tolerance(k));
            if pos.distance(ends.0) <= 2.0 * on_tol || pos.distance(ends.1) <= 2.0 * on_tol {
                continue;
            }
            let dropped = int_curve.drop_point(
                k,
                &ivl,
                pos,
                None,
                on_tol,
                None,
                SolverOperation::Minimize,
            )?;
            if dropped.success
                && dropped.distance <= on_tol
                && ivl.contains_value(dropped.parameter, -EFF_ZERO)
            {
                splits.push(dropped.parameter);
            }
        }

        // Split from the far end back, so that this proto edge stays the piece from the
        // start.
        splits.sort_by(f64::total_cmp);
        for &t in splits.iter().rev() {
            self.proto_edge_split(k, brep, pe, t)?;
        }
        // Each split left a definition of its own at the vertex's point: combined with
        // the vertex's there.
        if !splits.is_empty() {
            self.resolve_proto_vertex(k, &*brep, start)?;
        }
        Ok(())
    }

    fn pe_eds_len(&self, pe: ProtoEdgeId) -> Result<usize> {
        Ok(self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .num_edge_definitions())
    }

    fn pe_ed_at(&self, pe: ProtoEdgeId, i: usize) -> Result<Option<EdgeDefId>> {
        Ok(self
            .proto_edge(pe)
            .ok_or(dangling("proto edge is deleted"))?
            .edge_definitions()
            .get(i)
            .copied())
    }

    // ---- SmProtoTopologyManager ------------------------------------------------------

    /// Makes a vertex definition of the classifications `pc1`, `pc2`, neither listed
    /// nor owned (`CreateVertexDefinition`): at the position they agree on
    /// ([`find_position_from_pcs`], which may compute their parameters), with the
    /// largest of their tolerances and the manager's, and a gap of at least the
    /// classifications' and their deviation.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the position cannot be found (upstream then
    /// reads an uninitialised position or gap); those of [`VertexDefinition::new`] and
    /// [`PointClassification::gap_3d`].
    pub fn create_vertex_definition(
        &mut self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pc1: &mut PointClassification<O>,
        pc2: &mut PointClassification<O>,
    ) -> Result<VertexDefId> {
        // status ignored upstream; passed on as the fork does (bcc6f01)
        let found = find_position_from_pcs(k, brep, pc1, pc2)?;
        let pt = found.position;
        let mut gap = found.deviation;

        let mut tol = max2(pc1.tolerance(), pc2.tolerance());
        tol = max2(tol, self.raw_tolerance());

        gap = max2(gap, pc1.gap_3d(k, brep, None)?);
        gap = max2(gap, pc2.gap_3d(k, brep, None)?);

        let vd = VertexDefinition::new(k, brep, pc1, pc2, pt, tol, gap, None)?;
        self.insert_vertex_definition(vd)
    }

    /// Makes and lists a proto vertex of the classifications `pc1`, `pc2` when both
    /// can be imprinted (`CreateProtoVertex`; `None` otherwise): at the position they
    /// agree on, with a tolerance of at least their deviation, the manager's
    /// tolerance and both objects' zone tolerances.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] when the position cannot be found (upstream then
    /// reads an uninitialised position or deviation); those of the zone tolerances
    /// and of [`ProtoTopologyManager::new_proto_vertex_with_pcs`].
    pub fn create_proto_vertex(
        &mut self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        pc1: &mut PointClassification<O>,
        pc2: &mut PointClassification<O>,
    ) -> Result<Option<ProtoVertexId>> {
        // Check inputs
        if !can_imprint(pc1.point_class()) || !can_imprint(pc2.point_class()) {
            return Ok(None);
        }

        // status ignored upstream; passed on as the fork does (bcc6f01)
        let found = find_position_from_pcs(k, brep, pc1, pc2)?;

        let tol = self.proto_vertex_tolerance_of_pcs(k, brep, found.deviation, pc1, pc2)?;

        let pv = self.new_proto_vertex_with_pcs(k, brep, pc1, pc2, found.position, tol)?;
        self.add_proto_vertex(pv);
        Ok(Some(pv))
    }

    /// The raw tolerance [`ProtoTopologyManager::create_proto_vertex`] gives a proto vertex
    /// of the classifications `pc1`, `pc2` whose position deviates by `deviation`: the
    /// largest of that deviation, the manager's tolerance and both objects' zone
    /// tolerances.
    ///
    /// # Errors
    ///
    /// Those of the zone tolerances.
    pub(crate) fn proto_vertex_tolerance_of_pcs(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        deviation: f64,
        pc1: &PointClassification<O>,
        pc2: &PointClassification<O>,
    ) -> Result<f64> {
        let tol1 = zone_tol_of(k, brep, pc1.object())?;
        let tol2 = zone_tol_of(k, brep, pc2.object())?;
        Ok(max4(deviation, self.raw_tolerance(), tol1, tol2))
    }

    /// The listed proto vertex containing `position`, or a new listed one there with
    /// the manager's tolerance (`FindOrCreateProtoVertex`); and whether it was
    /// created. Not upstream: a proto vertex is found only within the combined tolerance
    /// of the two points ([`ProtoTopologyManager::find_proto_vertex_within`], the point's
    /// being the manager's tolerance it would be created with).
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::find_proto_vertex_within`].
    pub fn find_or_create_proto_vertex(
        &mut self,
        k: &Kernel,
        position: Point3,
    ) -> Result<(ProtoVertexId, bool)> {
        let found =
            self.find_proto_vertex_within(k, position, zone_tol3d(k, self.raw_tolerance()), None)?;
        if let Some(pv) = found {
            return Ok((pv, false));
        }
        let pv = self.new_proto_vertex(position, self.raw_tolerance())?;
        self.add_proto_vertex(pv);
        Ok((pv, true))
    }

    /// The first proto edge between `start` and `end` with a definition of the shape
    /// of the intersection interval `tgt_curve` over `tgt_domain` (`FindProtoEdge`),
    /// and whether that definition also has the midpoints' source objects. The
    /// candidates are `end`'s proto edges that `start` also lists, in `end`'s order.
    ///
    /// # Errors
    ///
    /// Those of [`ProtoTopologyManager::is_same_proto_edge`];
    /// [`SolidError::InvalidInput`] for deleted proto vertices.
    pub fn find_proto_edge(
        &self,
        k: &Kernel,
        brep: &impl ProtoBrep<O>,
        start: Option<ProtoVertexId>,
        end: Option<ProtoVertexId>,
        mid_pc1: &PointClassification<O>,
        mid_pc2: &PointClassification<O>,
        tgt_curve: Option<O>,
        tgt_domain: &Extent1d,
    ) -> Result<(Option<ProtoEdgeId>, bool)> {
        // no work - missing a start or end ProtoVertex
        let (Some(s), Some(e)) = (start, end) else {
            return Ok((None, false));
        };
        let pv1_edges = self
            .proto_vertex(s)
            .ok_or(dangling("proto vertex is deleted"))?
            .proto_edges();
        let pv2_edges = self
            .proto_vertex(e)
            .ok_or(dangling("proto vertex is deleted"))?
            .proto_edges();

        // find all existing PEs between the start and end PVs.
        let common: Vec<ProtoEdgeId> = pv2_edges
            .iter()
            .copied()
            .filter(|x| pv1_edges.contains(x))
            .collect();

        let mut identical = false;
        for pe in common {
            let (same, ident) = self.is_same_proto_edge(
                k, brep, pe, start, end, mid_pc1, mid_pc2, tgt_curve, tgt_domain,
            )?;
            identical = ident;
            if same {
                return Ok((Some(pe), identical));
            }
        }
        Ok((None, identical))
    }

    /// Makes and lists a proto edge from `start` to `end` with one definition of the
    /// interval pair `ci1`, `ci2` (`CreateProtoEdge`): copies of the intersection curve
    /// and of both classifications' first UV curves trimmed to `ci1`'s interval, on the
    /// intervals' midpoint objects, with the manager's tolerance. The proto edge is
    /// listed at both ends. `cc1`, `cc2` are the classifications the intervals belong
    /// to.
    ///
    /// # Errors
    ///
    /// Those of the curve copy and trim (`SM_ERR_NULL_POINTER` without a copy);
    /// `SM_ERR_NULL_POINTER` for a midpoint object that is not topology;
    /// [`SolidError::InvalidInput`] for a missing curve or end proto vertex.
    pub fn create_proto_edge(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        start: Option<ProtoVertexId>,
        end: Option<ProtoVertexId>,
        cc1: &CurveClassification<O>,
        ci1: &CurveInterval<O>,
        cc2: &CurveClassification<O>,
        ci2: &CurveInterval<O>,
    ) -> Result<ProtoEdgeId> {
        // Collect data for a new ProtoEdge.
        let p3d_curve = cc1.curve();
        let _ = cc2.curve() == p3d_curve;

        // Create a copy of the input curve and of its uv curves (copy statuses ignored), and
        // trim them to this CurveInterval.
        let mut copy = curve_of(brep, p3d_curve)?.copy(k)?;
        let mut uv1 = None;
        if let Some(c) = cc1.uv_curve1() {
            uv1 = ignore_soft(curve_of(brep, Some(c))?.copy(k))?;
        }
        let mut uv2 = None;
        if let Some(c) = cc2.uv_curve1() {
            uv2 = ignore_soft(curve_of(brep, Some(c))?.copy(k))?;
        }

        // The uv curves' trim statuses are ignored upstream, and passed on as the fork does
        // (bcc6f01): a uv curve whose trim refuses the interval refuses the edge. Not
        // upstream (WR-767): the 3D curve and the uv curves are trimmed to the one interval
        // none of their trims snaps ([`common_trim_interval`]). Upstream trims them in turn
        // with one interval, each trim snapping it onto a knot of its own curve, and the
        // edge takes the interval the last trim left: a uv curve's knot a few ulps from the
        // 3D curve's trim parameter leaves the edge's interval outside its 3D curve's
        // (`SmEdge` test 2).
        for u in [&uv1, &uv2].into_iter().flatten() {
            let mut probe = ci1.interval;
            u.copy(k)?.trim(k, &mut probe, TrimMode::IntervalOnly)?;
        }
        let joined: Vec<&Curve> = std::iter::once(&copy)
            .chain([&uv1, &uv2].into_iter().flatten())
            .collect();
        let domain = common_trim_interval(k, &joined, ci1.interval, OP_CREATE_PE)?;
        // Not upstream: a joined curve the common interval cannot hold refuses the edge. Its own
        // trim snaps an end further than 100 scaled zeros from `domain`, or it would have to be
        // reparametrised onto `domain` and is no B-spline (`trim_unsnapped`'s port invariant).
        // The curves then end at no one parameter, where upstream builds an edge that runs
        // past its 3D curve: the refusal replaces an invalid result, as in the offset's
        // `mate_edges`.
        trim_unsnapped(k, &mut copy, domain, OP_CREATE_PE)?;
        for u in [&mut uv1, &mut uv2].into_iter().flatten() {
            trim_unsnapped(k, u, domain, OP_CREATE_PE)?;
        }

        // Make an EdgeDefinition for the new ProtoEdge.
        let off = || SolidError::topology(OP_CREATE_PE, TopologyFailure::ClassificationOffTopology);
        let topo1 = as_topology(ci1.mid.object()).ok_or_else(off)?;
        let topo2 = as_topology(ci2.mid.object()).ok_or_else(off)?;
        let start_pv = self.pv_live(start)?;
        let end_pv = self.pv_live(end)?;

        let full = || dangling("curve store is full");
        let c3 = brep.insert_curve(copy).ok_or_else(full)?;
        let u1 = match uv1 {
            Some(c) => Some(brep.insert_curve(c).ok_or_else(full)?),
            None => None,
        };
        let u2 = match uv2 {
            Some(c) => Some(brep.insert_curve(c).ok_or_else(full)?),
            None => None,
        };

        // Create it.
        let new_pe = self.new_proto_edge(k, start, end, domain)?;
        let tol = self.raw_tolerance();
        let new_ed = self.insert_edge_definition(EdgeDefinition::new(
            Some(c3),
            domain,
            topo1,
            topo2,
            tol,
            u1,
            u2,
            Some(new_pe),
        ))?;

        // Hook up pointers (status ignored upstream, passed on as the fork does).
        self.add_edge_definition(k, brep, new_pe, new_ed)?;
        self.add_proto_edge(new_pe);
        for pv in [start_pv, end_pv] {
            self.proto_vertex_mut(pv)
                .ok_or(dangling("proto vertex is deleted"))?
                .add_proto_edge(new_pe);
        }
        Ok(new_pe)
    }

    /// Unlists the edge definition `ed` from its proto edge (not deleted), and with
    /// `remove_owner_if_empty` unlists and deletes that proto edge when it has no
    /// definitions left (`RemoveEdgeDefinition`).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for deleted objects or no proto edge; the removals'
    /// `SM_ERR` (passed on as the fork does, bcc6f01).
    pub fn remove_edge_definition(
        &mut self,
        ed: EdgeDefId,
        remove_owner_if_empty: bool,
    ) -> Result<()> {
        let owner = self.pe_live(self.ed_ref(ed)?.proto_edge())?;

        // status ignored upstream (passed on as the fork does)
        let o = self
            .proto_edge_mut(owner)
            .ok_or(dangling("proto edge is deleted"))?;
        o.remove_edge_definition(ed)?;

        if remove_owner_if_empty && o.num_edge_definitions() < 1 {
            self.remove_proto_edge(owner)?;
            // SolidCraft fix 93efd6d (UB-239): drop the references to it first
            self.forget_proto_edge(owner);
            let _ = self.delete_proto_edge_object(owner);
        }
        Ok(())
    }

    /// Updates the proto topology after the edge `old_edge` of B-rep `which` was split
    /// into `new_edges` (`UpdateEdges`; nothing without an old edge or unless there are
    /// two new edges, one of them `old_edge`). Vertex definitions on the old edge whose
    /// parameter is only on the new edge move to it; edge definitions starting or
    /// ending at the proto vertex of `calling_vd` (the definition being imprinted) that
    /// contain the split point are trimmed there; and edge definitions on the old edge
    /// move to the new edge when their midpoint drops onto the new edge only, or closer
    /// to it.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when neither new edge is `old_edge`;
    /// [`SolidError::InvalidInput`] for deleted or null objects that are dereferenced
    /// (`calling_vd` is, when the edge was split); [`SolidError::Unported`] from the
    /// point solves and drops.
    pub fn update_edges(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        which: i32,
        old_edge: Option<O>,
        new_edges: &[O],
        calling_vd: Option<VertexDefId>,
    ) -> Result<()> {
        // no work - no split edge
        let Some(old_edge) = old_edge else {
            return Ok(());
        };

        // no work - not 2 NewEdges
        let &[e0, e1] = new_edges else {
            return Ok(());
        };
        let new_edge = if e0 == old_edge {
            e1
        } else if e1 == old_edge {
            e0
        } else {
            return Err(SolidError::topology(
                OP_UPDATE_EDGES,
                TopologyFailure::OldEdgeNotAmongSplit,
            ));
        };
        let unknown = || dangling("edge handle is unknown");
        let old_ivl = brep.edge_interval(old_edge).ok_or_else(unknown)?;
        let new_ivl = brep.edge_interval(new_edge).ok_or_else(unknown)?;

        // move VertexDefinitions between split Child1 (pOldEdge) and Child2 (pNewEdge)
        for vd in self.all_vertex_definitions()? {
            // no work - VD is CallingVD
            if Some(vd) == calling_vd {
                continue;
            }
            let v = self.vd_ref(vd)?;

            // no Work pVD SrcTopo is not an Edge
            if v.src_topo_type(which) != PointClassType::Edge {
                continue;
            }
            let edge = as_kind(v.src_topo(which), ClassObjectKind::Edge);

            // no work - pVD->Edge is not Tgt SplitEdge
            if edge != Some(old_edge) {
                continue;
            }

            let t = v.brep_t_param(which);
            let on_old_edge = old_ivl.contains_value(t, old_ivl.scaled_zero());
            let on_new_edge = new_ivl.contains_value(t, new_ivl.scaled_zero());

            // when pVD is only on the new Edge - change its srcTopo to the new Edge
            if on_new_edge && !on_old_edge {
                // passed on as the fork does (bcc6f01)
                self.vd_mut(vd)?
                    .set_src_topo(which, PointClassType::Edge, Some(new_edge))?;
            }
        }

        // next: move EdgeDefinitions between split Child1 (pOldEdge) and Child2 (pNewEdge)
        let eds = self.all_edge_definitions()?;

        // Get the split point: It's one end of pOldEdge.
        let end_param = if old_ivl.mid() < new_ivl.mid() {
            old_ivl.max()
        } else {
            old_ivl.min()
        };
        let old_curve = edge_curve_of(brep, old_edge)?;
        let split_pt = curve_of(brep, Some(old_curve))?.evaluate_point(k, end_param)?;

        let calling_vd = calling_vd.ok_or(dangling("calling vertex definition is null"))?;
        let calling_pv = self.vd_ref(calling_vd)?.proto_vertex();
        let tol = self.raw_tolerance();

        // (with WR-533 the upper parts of definitions split below are classified in their
        // turn, appended to the list)
        let mut eds = eds;
        let mut next = 0;
        while let Some(&ed) = eds.get(next) {
            next += 1;
            let d = self.ed_ref(ed)?;
            let this_pe = self.pe_live(d.proto_edge())?;
            let pe = self
                .proto_edge(this_pe)
                .ok_or(dangling("proto edge is deleted"))?;
            let (pe_start, pe_end) = (pe.start_proto_vertex(), pe.end_proto_vertex());

            for at_start in [true, false] {
                // CallingPV is EdgeDef Start (End)
                let end_pv = if at_start { pe_start } else { pe_end };
                if end_pv != calling_pv {
                    continue;
                }
                let split_vtx = self.vd_ref(calling_vd)?.final_vertex(which);
                let tol_3d = xsect_tol_of(k, brep, Some(old_edge), split_vtx)?;

                // Split pt is inside the pED - trim pED
                let d = self.ed_ref(ed)?;
                let (mut do_trim, split_param) =
                    d.is_interior_point(k, brep, split_pt, tol_3d, at_start)?;
                if !do_trim {
                    continue;
                }
                // if we start and end on the same ProtoVertex, make sure we're
                // interior w.r.t. the other end as well.
                if pe_start == pe_end {
                    do_trim = d.is_interior_point(k, brep, split_pt, tol_3d, !at_start)?.0;
                }
                if !do_trim {
                    continue;
                }

                // trim instead of split
                let domain = d.xsect_interval();
                let (min, max) = if at_start {
                    (split_param, domain.max())
                } else {
                    (domain.min(), split_param)
                };
                self.ed_mut(ed)?
                    .set_xsect_interval_min_max(k, brep, min, max)?;

                // Also have to trim the curves: we want these to be in sync.
                let d = self.ed_ref(ed)?;
                let mut domain = d.xsect_interval();
                let (c3, u1, u2) = (d.xsect_curve_3d(), d.xsect_uv_curve(1), d.xsect_uv_curve(2));
                curve_of(brep, c3)?;
                trim_curve(k, brep, c3, &mut domain)?;
                trim_curve(k, brep, u1, &mut domain)?;
                trim_curve(k, brep, u2, &mut domain)?;
            }

            // Now check whether this EdgeDefintion is defined on the split SmEdge.
            let d = self.ed_ref(ed)?;
            if d.src_topo_type(which) != PointClassType::Edge {
                continue;
            }
            let edge = as_kind(d.src_topo(which), ClassObjectKind::Edge);
            if edge != Some(old_edge) {
                continue;
            }

            // Not upstream (WR-533, the fork's tangency-labs/smlib@f11d0ac): a coincident
            // EdgeDefinition that runs across the split point (an edge along the whole of
            // an arc that another edge's ends split twice in its middle) is split there
            // too, with its ProtoEdge (which finds the calling ProtoVertex at that point),
            // so each part lies on one of the two pieces of the edge. Upstream re-points
            // it whole to the piece under its midpoint, whose vertices are not its ends,
            // and Imprint finds no vertex for them (`SM_ERR_NULL_POINTER`). The upper
            // part, a new EdgeDefinition still on the old edge, is classified in its turn.
            let split_vtx = self.vd_ref(calling_vd)?.final_vertex(which);
            let tol_3d = xsect_tol_of(k, brep, Some(old_edge), split_vtx)?;
            let ed_ivl = d.xsect_interval();
            let ed_start = d.end_position(k, &*brep, true)?;
            let ed_end = d.end_position(k, &*brep, false)?;
            if split_pt.distance(ed_start) > tol_3d && split_pt.distance(ed_end) > tol_3d {
                let dropped = curve_of(brep, d.xsect_curve_3d())?.drop_point(
                    k,
                    &ed_ivl,
                    split_pt,
                    None,
                    tol_3d,
                    Some(ed_ivl.mid()),
                    SolverOperation::Minimize,
                );
                let at = match dropped {
                    Ok(drop)
                        if drop.success
                            && drop.distance <= tol_3d
                            && ed_ivl.contains_value(drop.parameter, -EFF_ZERO) =>
                    {
                        Some(drop.parameter)
                    }
                    Err(e) if e.is_unported_or_cancelled() => return Err(e),
                    _ => None,
                };
                if let Some(at) = at {
                    let (_, new_pe) = self.proto_edge_split(k, brep, this_pe, at)?;
                    if let Some(new_pe) = new_pe {
                        let upper = self
                            .proto_edge(new_pe)
                            .ok_or(dangling("proto edge is deleted"))?
                            .edge_definitions()
                            .to_vec();
                        for part in upper {
                            if !eds.contains(&part) {
                                eds.push(part);
                            }
                        }
                    }
                }
            }
            let d = self.ed_ref(ed)?;

            // Found a ProtoEdge on the edge that was split: drop the midpoint of the
            // definition to both edges' curves.
            // (status ignored upstream; passed on as the fork does, bcc6f01)
            let mid_pt = d
                .evaluate_mid(k, brep, true, false)?
                .0
                .unwrap_or(Vec3::UNDEF);

            let old_tol = brep.topology_tolerance(old_edge).ok_or_else(unknown)?;
            let this_tol = max2(10.0 * tol, 4.0 * old_tol);
            let sols_old = edge_point_solve(k, brep, old_curve, &old_ivl, mid_pt, this_tol)?;

            let new_tol = brep.topology_tolerance(new_edge).ok_or_else(unknown)?;
            let this_tol = max2(10.0 * tol, 4.0 * new_tol);
            let new_curve = edge_curve_of(brep, new_edge)?;
            let sols_new = edge_point_solve(k, brep, new_curve, &new_ivl, mid_pt, this_tol)?;

            let old_value = sols_old.as_ref().and_then(|s| s.first().copied());
            let new_value = sols_new.as_ref().and_then(|s| s.first().copied());
            if old_value.is_some() || new_value.is_some() {
                match (old_value, new_value) {
                    // Drop to Old failed (and to New succeeded).
                    (None, _) => {
                        self.ed_mut(ed)?
                            .set_src_topo(which, PointClassType::Edge, new_edge);
                    }
                    // Drop to New failed (and to Old succeeded).
                    (_, None) => {}
                    // both drops worked: pick the closer one.
                    (Some(o), Some(n)) => {
                        if n < o {
                            self.ed_mut(ed)?
                                .set_src_topo(which, PointClassType::Edge, new_edge);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Updates the proto topology after the face `old_face` of B-rep `which` was split
    /// by `splitting_edge` into itself and `new_face` (`UpdateFaces`): the faces' UV
    /// domains shrink to their trim curves; vertex and edge definitions on the old face
    /// move to the new face when their UV point is in the new face only. Not upstream:
    /// the proto faces on the old face are left as they are (upstream sorts them between
    /// the two children, work nothing reads; see the comment at the end). `splitting_edge`
    /// and `other_edge`, the splitting edge's counterpart in the other B-rep, were that
    /// sorting's inputs.
    ///
    /// # Errors
    ///
    /// Those of the owning-face domain and classification callbacks;
    /// [`SolidError::InvalidInput`] for deleted objects or unknown handles.
    pub fn update_faces(
        &mut self,
        k: &Kernel,
        brep: &mut impl ProtoBrep<O>,
        which: i32,
        old_face: O,
        new_face: O,
        splitting_edge: Option<O>,
        other_edge: Option<O>,
        calling_ed: Option<EdgeDefId>,
    ) -> Result<()> {
        let unknown = || dangling("face handle is unknown");

        // Face UVDomains = Min(current FaceUVDomain, UVTrimCurve domain) (statuses
        // ignored)
        let old_domain = brep.face_trim_uv_domain(k, old_face)?;
        let new_domain = brep.face_trim_uv_domain(k, new_face)?;

        if !brep
            .face_uv_domain(old_face)
            .ok_or_else(unknown)?
            .is_contained_by(&old_domain, 0.0)
        {
            brep.set_face_uv_domain(old_face, old_domain)
                .ok_or_else(unknown)?;
        }
        if !brep
            .face_uv_domain(new_face)
            .ok_or_else(unknown)?
            .is_contained_by(&new_domain, 0.0)
        {
            brep.set_face_uv_domain(new_face, new_domain)
                .ok_or_else(unknown)?;
        }

        // move VertexDefinitions between split Child1 (pOldFace) and Child2 (pNewFace)
        for vd in self.all_vertex_definitions()? {
            let v = self.vd_ref(vd)?;
            if v.src_topo_type(which) != PointClassType::Face {
                continue;
            }
            let face = as_kind(v.src_topo(which), ClassObjectKind::Face);
            if face != Some(old_face) {
                continue;
            }

            let uv = v.brep_uv_param(which);
            let zone_tol = zone_tol_for_xsect_result(k, brep, old_face, new_face)?;
            let which_face = classify_param_on_faces(
                k,
                brep,
                uv,
                zone_tol,
                old_face,
                &old_domain,
                new_face,
                &new_domain,
            )?;
            if which_face == 2 {
                // move the VertexDefinition (passed on as the
                // fork does, bcc6f01)
                self.vd_mut(vd)?
                    .set_src_topo(which, PointClassType::Face, Some(new_face))?;
            }
        }

        // move EdgeDefinitions between split Child1 (pOldFace) and Child2 (pNewFace)
        for ed in self.all_edge_definitions()? {
            if Some(ed) == calling_ed {
                continue;
            }
            let d = self.ed_ref(ed)?;
            let in_face = as_kind(d.src_topo(which), ClassObjectKind::Face);
            if in_face != Some(old_face) {
                continue;
            }

            let mut face_uv = None;
            let uv_curve = d.xsect_uv_curve(which);
            let mid_param = d.xsect_interval().mid();
            if uv_curve.is_some() {
                if let Some(p) =
                    ignore_soft(curve_of(brep, uv_curve)?.evaluate_point(k, mid_param))?
                {
                    face_uv = Some(Point2::new(p.x, p.y));
                }
            }
            if face_uv.is_none() {
                // drop a 3d MidPoint to the surface.
                let surface = brep.face_surface(old_face).ok_or_else(unknown)?;
                // (status ignored upstream; passed on as the fork does, bcc6f01)
                let mid_pt = d
                    .evaluate_mid(k, brep, true, false)?
                    .0
                    .unwrap_or(Vec3::UNDEF);
                face_uv = ignore_soft(surface_drop_point(k, brep, surface, mid_pt))?.flatten();
            }
            let Some(face_uv) = face_uv else { continue };

            let zone_tol = zone_tol_for_xsect_result(k, brep, old_face, new_face)?;
            let which_face = classify_param_on_faces(
                k,
                brep,
                face_uv,
                zone_tol,
                old_face,
                &old_domain,
                new_face,
                &new_domain,
            )?;
            if which_face == 2 && in_face == Some(old_face) {
                self.ed_mut(ed)?
                    .set_src_topo(which, PointClassType::Face, new_face);
            }
        }

        // Not upstream: the proto faces on the split face are not re-sorted. Upstream
        // assigns each to the child face coincident with its other face (by binormals
        // across the splitting edge, else by classifying its coincident proto edges'
        // midpoints) and moves the proto edges on the other child to a new proto face.
        // Nothing reads a proto face's faces but that sorting itself, and the one reader of
        // the coincident proto edges past this point, `glue_coincident_brep_vertices`, takes
        // each of them once whatever proto face lists it. The sorting classified every
        // coincident proto edge of every proto face on the face at every split of it: a
        // union of two halves of a woven panel of 32 strips made 2,191,773 classifications
        // there, 2,154,634 of them on no face's interior, and they grew about 65 times for
        // each doubling of the strips.
        let _ = (splitting_edge, other_edge);
        Ok(())
    }

    /// Whether two vertex definitions can be combined, and which to delete
    /// (`CanCombineVertexDefinitions`; 0 neither, 1 the first, 2 the second): for
    /// connected source pairs within five times their tolerances of each other, the
    /// one on higher-dimensional topology; otherwise, when both are on the same
    /// vertex in either B-rep or on the same objects in both, the one with the larger
    /// gap.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a definition without a topology source object; those
    /// of the `topology` walks of [`are_connected`]; [`SolidError::InvalidInput`] for
    /// deleted definitions.
    pub fn can_combine_vertex_definitions(
        &self,
        k: &Kernel,
        topology: &impl ProtoTopology<O>,
        vd1: VertexDefId,
        vd2: VertexDefId,
    ) -> Result<u64> {
        self.can_combine_vertex_definitions_within(k, topology, vd1, vd2, VD_TOL_FACTOR)
    }

    /// [`ProtoTopologyManager::can_combine_vertex_definitions`] with the distance allowed
    /// between the two definitions `factor` times their summed tolerances (upstream's 5,
    /// or [`EXACT_TOL_FACTOR`] for exactly placed definitions).
    fn can_combine_vertex_definitions_within(
        &self,
        k: &Kernel,
        topology: &impl ProtoTopology<O>,
        vd1: VertexDefId,
        vd2: VertexDefId,
        factor: f64,
    ) -> Result<u64> {
        let v1 = self.vd_ref(vd1)?;
        let v2 = self.vd_ref(vd2)?;
        let null = || {
            SolidError::topology(
                OP_COMBINE_VD,
                TopologyFailure::DefinitionWithoutSourceTopology,
            )
        };
        let t11 = v1.src_topo(1).ok_or_else(null)?;
        let t12 = v1.src_topo(2).ok_or_else(null)?;
        let t21 = v2.src_topo(1).ok_or_else(null)?;
        let t22 = v2.src_topo(2).ok_or_else(null)?;

        // If both VDs are on the same Vertex in either Brep, then definitely combine.
        let mut force_combine = false;
        let is_vertex = |t: O| t.class_kind() == ClassObjectKind::Vertex;
        if is_vertex(t11) && t11 == t21 {
            force_combine = true;
        }
        if is_vertex(t12) && t12 == t22 {
            force_combine = true;
        }

        // Now, if they're sitting on the same topology in both Breps, combine.
        if t11 == t21 && t12 == t22 {
            force_combine = true;
        }

        // If either pair is not connected in their Brep, can't combine.
        if !are_connected(topology, Some(t11), Some(t21))? {
            return Ok(0);
        }
        if !are_connected(topology, Some(t12), Some(t22))? {
            return Ok(0);
        }

        // Check distance: if they're too far apart, we can't combine them.
        let dist = v1.position().distance(v2.position());
        let mut tol = v1.tolerance(k) + v2.tolerance(k);
        tol *= factor;
        if dist > tol {
            return Ok(0);
        }

        // get rid of the ones on the higher-dimensional Brep objects.
        let dim11 = topo_dimension(t11);
        let dim12 = topo_dimension(t12);
        let dim21 = topo_dimension(t21);
        let dim22 = topo_dimension(t22);

        let mut can_delete = 0;
        if dim11 < dim21 && dim12 <= dim22 {
            can_delete = 2;
        }
        if dim21 < dim11 && dim22 <= dim12 {
            can_delete = 1;
        }
        if dim12 < dim22 && dim11 <= dim21 {
            can_delete = 2;
        }
        if dim22 < dim12 && dim21 <= dim11 {
            can_delete = 1;
        }

        // Keep the one with the tighter gap.
        if force_combine && can_delete == 0 {
            can_delete = if v1.gap() > v2.gap() { 1 } else { 2 };
        }
        Ok(can_delete)
    }
}

/// Not upstream (WR-214): whether `curve` over `interval` lies within `reach` of `centre`,
/// decided on points whose convex hull holds that piece of the curve
/// ([`Curve::piece_hull_points`], each with the error bound of its coordinates): every
/// point's distance grown by its own rounding, plus twice its coordinates' error bound
/// (the bound is per coordinate, so the Euclidean error is at most `sqrt(3)` of it), must be
/// at most `reach`. `false` when the curve has no such certain hull, or for non-finite
/// data, so an unbounded curve is never taken as near.
fn curve_piece_within(curve: &Curve, interval: &Extent1d, centre: Point3, reach: f64) -> bool {
    let Some(points) = curve.piece_hull_points(interval) else {
        return false;
    };
    !points.is_empty()
        && points.iter().all(|&(p, err)| {
            let d = p.distance(centre) * (1.0 + gamma(8)) + 2.0 * err;
            d.is_finite() && d <= reach
        })
}

/// Not upstream (WR-306): the vertex definition that absorbs a self-loop and the certain
/// hull points (each with the error bound of its coordinates) of its curves.
type AbsorbedCurve = (VertexDefId, Vec<(Point3, f64)>);

/// Not upstream (#16640): whether a definition's two curve ends, `gap` apart, both mapped to
/// one vertex of each B-rep (tolerances `vertex_tols`), are a stretch no vertex bounds rather than the
/// ends of a closed edge. The validity contract (`validity-and-tolerances.md`, Edge 16 and
/// 17) lets an edge end up to `edge_tol + vertex_tol` from its vertex, so a closed edge's
/// ends may lie twice that far apart; only a gap beyond it is a stretch.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or negative tolerance, or a NaN gap.
fn ends_are_a_stretch(gap: f64, edge_tol: f64, vertex_tols: [f64; 2]) -> Result<bool> {
    let usable = |t: f64| t.is_finite() && t >= 0.0;
    if !gap.is_finite() || !usable(edge_tol) || !vertex_tols.into_iter().all(usable) {
        return Err(SolidError::invalid_input(
            OP_IMPRINT_ED,
            "a definition's end gap and tolerances must be finite and not negative",
        ));
    }
    let vertex_tol = vertex_tols[0].max(vertex_tols[1]);
    Ok(gap > 2.0 * (edge_tol + vertex_tol))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::intersect::point_class::{ClassGeometry, ClassObjectKind};
    use crate::surface::surface::Surface;
    use crate::surface::volume::Volume;
    use crate::topology::Brep;

    /// #16640: the contract's `edge_tol + vertex_tol` at each end makes a closed edge's ends
    /// up to twice that apart; only a gap beyond it is a stretch.
    #[test]
    fn end_gap_is_a_stretch_only_beyond_the_validity_contract() {
        let (edge, vertex) = (1e-6, 4e-6);
        let bound = 2.0 * (edge + vertex);
        assert!(!ends_are_a_stretch(0.0, edge, [vertex, 1e-7]).unwrap());
        assert!(!ends_are_a_stretch(bound * (1.0 - 1e-9), edge, [vertex, 1e-7]).unwrap());
        assert!(ends_are_a_stretch(bound * (1.0 + 1e-9), edge, [1e-7, vertex]).unwrap());
        assert!(ends_are_a_stretch(1.6e-3, edge, [vertex, vertex]).unwrap());
    }

    /// A non-finite tolerance or gap is an error, never ignored.
    #[test]
    fn end_gap_with_non_finite_data_is_an_error() {
        for (gap, edge, vertex) in [
            (1.0, f64::NAN, [1e-6, 1e-6]),
            (1.0, 1e-6, [f64::NAN, 1e-6]),
            (1.0, 1e-6, [1e-6, f64::NAN]),
            (1.0, f64::INFINITY, [1e-6, 1e-6]),
            (1.0, 1e-6, [1e-6, -1.0]),
            (f64::NAN, 1e-6, [1e-6, 1e-6]),
            (f64::INFINITY, 1e-6, [1e-6, 1e-6]),
        ] {
            assert!(ends_are_a_stretch(gap, edge, vertex).is_err());
        }
    }

    /// Pool handles: an index and a kind.
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(usize, ClassObjectKind);

    impl ClassObject for H {
        fn class_kind(&self) -> ClassObjectKind {
            self.1
        }
    }

    #[derive(Default)]
    struct Geo {
        curves: Vec<Curve>,
        edge_ivl: Vec<(usize, Extent1d, f64, usize)>,
        points: Vec<Point3>,
        /// the zone tolerance of every vertex handle, when set (`None`: unresolved)
        vertex_tol: Option<f64>,
        /// whether the point classification fails (a status the split ignores)
        classify_fails: bool,
    }

    impl ClassGeometry<H> for Geo {
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
        fn point(&self, o: H) -> Option<Point3> {
            (o.1 == ClassObjectKind::Point)
                .then(|| self.points.get(o.0))
                .flatten()
                .copied()
        }
        fn classification_vertex_point(&self, vertex: H) -> Option<Point3> {
            (vertex.1 == ClassObjectKind::Vertex)
                .then(|| self.points.get(vertex.0))
                .flatten()
                .copied()
        }
        fn topology_zone_tol_3d(&self, object: H) -> Option<f64> {
            self.vertex_tol
                .filter(|_| object.1 == ClassObjectKind::Vertex)
        }
    }

    impl ProtoGeometry<H> for Geo {
        fn curve_mut(&mut self, o: H) -> Option<&mut Curve> {
            (o.1 == ClassObjectKind::Curve)
                .then(|| self.curves.get_mut(o.0))
                .flatten()
        }
    }

    /// The bare stand-ins: no uses, no B-rep; an edge's start vertex dereferences its
    /// missing primary edge use.
    impl ProtoTopology<H> for Geo {
        fn vertex_use_faces(&self, _vertex: H) -> Result<Vec<Option<H>>> {
            Ok(Vec::new())
        }
        fn edge_use_faces(&self, _edge: H) -> Result<Vec<Option<H>>> {
            Ok(Vec::new())
        }
        fn edge_start_vertex(&self, _edge: H) -> Result<Option<H>> {
            Err(SolidError::invalid_input("test", "no primary edge use"))
        }
        fn edge_other_vertex(&self, _edge: H, _vertex: Option<H>) -> Result<Option<H>> {
            Ok(None)
        }
        fn vertex_point(&self, _vertex: H) -> Result<Point3> {
            Ok(Point3::new(0.0, 0.0, 0.0))
        }
        fn edge_brep(&self, _edge: H) -> Result<Option<crate::geom::attribute::ObjectKey>> {
            Ok(None)
        }
        fn edge_faces(&self, _edge: H) -> Result<Vec<H>> {
            Ok(Vec::new())
        }
    }

    impl ProtoBrep<H> for Geo {
        fn brep_make_edge_in_face(
            &mut self,
            _k: &Kernel,
            _b: H,
            _f: H,
            _v1: H,
            _v2: H,
            _c: Curve,
            _uv: Option<Curve>,
            _ivl: Extent1d,
            _o: OrientType,
            _tol: f64,
        ) -> Result<(Option<H>, Option<H>, Option<H>)> {
            Err(SolidError::unported(
                "SmBrep::MakeEdgeInFace (no B-rep behind the test handles)",
                "harness:proto-test-brep-edit",
            ))
        }
        fn edgeuse_evaluate_binormal(&mut self, _k: &Kernel, _eu: H, _t: f64) -> Result<Vec3> {
            Err(SolidError::unported(
                "SmEdgeuse::EvaluateBinormal (no B-rep behind the test handles)",
                "harness:proto-test-brep-edit",
            ))
        }
        fn brep_squeeze_edge(&mut self, _k: &mut Kernel, _b: H, _e: H, _v: H) -> Result<()> {
            Err(SolidError::unported(
                "SmBrep::SqueezeEdge (no B-rep behind the test handles)",
                "harness:proto-test-brep-edit",
            ))
        }
        fn edge_interval(&self, edge: H) -> Option<Extent1d> {
            self.edge_ivl.iter().find(|e| e.0 == edge.0).map(|e| e.1)
        }
        fn edge_curve(&self, edge: H) -> Option<Option<H>> {
            self.edge_ivl
                .iter()
                .find(|e| e.0 == edge.0)
                .map(|e| Some(H(e.3, ClassObjectKind::Curve)))
        }
        fn topology_tolerance(&self, topo: H) -> Option<f64> {
            Some(
                self.edge_ivl
                    .iter()
                    .find(|e| e.0 == topo.0)
                    .map_or(0.0, |e| e.2),
            )
        }
        fn face_uv_domain(&self, _face: H) -> Option<Extent2d> {
            None
        }
        fn face_trim_uv_domain(&mut self, _k: &Kernel, _face: H) -> Result<Extent2d> {
            Err(dangling("test handle has no owning face"))
        }
        fn face_point_classify_uv(
            &mut self,
            _k: &Kernel,
            _face: H,
            _uv: Point2,
            _tolerance: f64,
        ) -> Result<PointClassType> {
            Err(dangling("test handle has no owning face"))
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
        fn vertex_common_edges(&self, _v1: H, _v2: H) -> Result<Vec<H>> {
            Ok(Vec::new())
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
        fn brep_point_3d_classify(
            &mut self,
            _k: &Kernel,
            _brep: H,
            point: Point3,
            tolerance: f64,
            _boundary_intersections: bool,
        ) -> Result<PointClassification<H>> {
            if self.classify_fails {
                return Err(SolidError::sm_err("classify"));
            }
            let idx = self.points.len();
            self.points.push(point);
            let v = H(idx, ClassObjectKind::Vertex);
            let mut pc = PointClassification::new(tolerance);
            pc.set_class_object(PointClassType::Vertex, Some(v));
            Ok(pc)
        }
    }

    fn line(p: [f64; 3], q: [f64; 3]) -> Curve {
        Curve::from(
            Line::create_line_segment(3, Vec3::from_array(p), Vec3::from_array(q), None).unwrap(),
        )
    }

    const R0: H = H(10, ClassObjectKind::Region);
    const R1: H = H(11, ClassObjectKind::Region);
    const L0: H = H(12, ClassObjectKind::Loop);

    fn manager() -> ProtoTopologyManager<H> {
        let b = H(100, ClassObjectKind::Other);
        let e = Brep::new();
        ProtoTopologyManager::new(&Kernel::default(), (b, &e), (b, &e), b, 1e-5, 1e-3, -1.0)
            .unwrap()
    }

    /// fork bcc6f01 (`SmProtoEdge::CombineProtoEdge` returns `SmStatus`,
    /// `SER(pOtherStartPV->RemoveProtoEdge(pOtherPE))`): the other proto edge's start
    /// vertex does not list it. Upstream drops the failed removal and goes on; the port
    /// returns its error, as the fork does.
    #[test]
    fn combine_proto_edge_passes_on_a_failed_removal() {
        let k = Kernel::default();
        let mut geo = Geo {
            curves: vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])],
            ..Geo::default()
        };
        let mut m = manager();
        let ivl = geo.curves[0].natural_interval();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v2 = m
            .new_proto_vertex(Point3::new(0.0, 1.0, 0.0), 1e-5)
            .unwrap();
        let v3 = m
            .new_proto_vertex(Point3::new(1.0, 1.0, 0.0), 1e-5)
            .unwrap();
        let pe = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        // `other` is not listed at v2 or v3: its removal there fails
        let other = m.new_proto_edge(&k, Some(v2), Some(v3), ivl).unwrap();
        let r = m.combine_proto_edge(&k, &mut geo, pe, other);
        assert!(r.is_err(), "{r:?}");
    }

    /// `SplitIntersectingProtoEdges` combines two overlapping proto edges and removes the
    /// second (#18546). Running the other way between the same two proto vertices, the
    /// second's ends were moved crosswise onto the first's: unlisted at both, pointed at
    /// proto vertices that no longer listed it, and the removal that follows failed at
    /// both, "the proto edge is not listed" (a status the port passes on, as the fork
    /// does). Now it keeps its ends, so the removal succeeds, the second is
    /// listed nowhere, and the first is listed exactly once at each of its ends, as when
    /// both run the same way. A second edge that ends at a third proto vertex still has
    /// that end moved as upstream, and its removal still fails there.
    #[test]
    fn a_proto_edge_combined_into_one_running_either_way_is_removed() {
        for way in ["same", "backwards", "elsewhere"] {
            let k = Kernel::default();
            let mut geo = Geo {
                curves: vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])],
                ..Geo::default()
            };
            let mut m = manager();
            let ivl = geo.curves[0].natural_interval();
            let v0 = m
                .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
                .unwrap();
            let v1 = m
                .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
                .unwrap();
            let v2 = m
                .new_proto_vertex(Point3::new(2.0, 0.0, 0.0), 1e-5)
                .unwrap();
            let (s, e) = match way {
                "same" => (v0, v1),
                "backwards" => (v1, v0),
                _ => (v0, v2),
            };
            let pe = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
            let other = m.new_proto_edge(&k, Some(s), Some(e), ivl).unwrap();
            // listed in the manager and once at each end, as `create_proto_edge` lists
            for (x, ends) in [(pe, [v0, v1]), (other, [s, e])] {
                m.add_proto_edge(x);
                for v in ends {
                    m.proto_vertex_mut(v).unwrap().add_proto_edge(x);
                }
            }
            let what = way;
            let count = |m: &ProtoTopologyManager<H>, v, x| {
                let listed = m.proto_vertex(v).unwrap().proto_edges();
                listed.iter().filter(|&&y| y == x).count()
            };
            let ends = |m: &ProtoTopologyManager<H>, x| {
                let x = m.proto_edge(x).unwrap();
                (x.start_proto_vertex(), x.end_proto_vertex())
            };

            m.combine_proto_edge(&k, &mut geo, pe, other).unwrap();
            let r = m.remove_proto_edge(other);
            assert!(!m.proto_edges().contains(&other), "{what}");
            assert_eq!(ends(&m, pe), (Some(v0), Some(v1)), "{what}");
            if way == "elsewhere" {
                assert_eq!(ends(&m, other), (Some(v0), Some(v1)), "{what}");
                assert!(r.is_err(), "{what}: {r:?}");
                continue;
            }
            assert_eq!(ends(&m, other), (Some(s), Some(e)), "{what}");
            assert!(r.is_ok(), "{what}: {r:?}");
            for v in [v0, v1] {
                assert_eq!(count(&m, v, other), 0, "{what}: {v:?}");
                assert_eq!(count(&m, v, pe), 1, "{what}: {v:?}");
            }
        }
    }

    /// fork bcc6f01 (`SER( pOwner->RemoveVertexDefinition(pVD) )`,
    /// `SER( pStartPV->RemoveProtoEdge(pPE) )`, `SER( pOwner->RemoveEdgeDefinition(pED) )`
    /// in the manager's `RemoveVertexDefinition`, `RemoveProtoEdge` and
    /// `RemoveEdgeDefinition`): the owner does not list the entry. Upstream drops the
    /// failed removal and reports success; the port returns its `SM_ERR`, as the fork
    /// does. Discriminating.
    #[test]
    fn manager_removals_pass_on_unlisted_entries() {
        let k = Kernel::default();
        let geo = Geo {
            curves: vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])],
            ..Geo::default()
        };
        let mut m = manager();
        let ivl = geo.curves[0].natural_interval();
        let p = Point3::new(0.0, 0.0, 0.0);
        let v0 = m.new_proto_vertex(p, 1e-5).unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
            .unwrap();

        // a vertex definition owned by v0 that v0 no longer lists
        let vd =
            VertexDefinition::new(&k, &geo, &region_pc(R0), &region_pc(R1), p, 1e-5, 0.0, None)
                .unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(v0, vd).unwrap();
        m.proto_vertex_mut(v0)
            .unwrap()
            .vertex_definitions_mut()
            .clear();
        let r = m.remove_vertex_definition(vd, false);
        assert!(r.is_err(), "{r:?}");

        // a listed proto edge that its end proto vertices do not list
        let pe = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        m.add_proto_edge(pe);
        let r = m.remove_proto_edge(pe);
        assert!(r.is_err(), "{r:?}");

        // an edge definition owned by a proto edge that does not list it
        let pe = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        let ed = m
            .insert_edge_definition(EdgeDefinition::new(
                Some(H(0, ClassObjectKind::Curve)),
                ivl,
                R0,
                R1,
                1e-5,
                None,
                None,
                Some(pe),
            ))
            .unwrap();
        let r = m.remove_edge_definition(ed, false);
        assert!(r.is_err(), "{r:?}");
    }

    /// fork bcc6f01 (`SER( pOwningPE->RemoveEdgeDefinition(pEdgeDef) )` in
    /// `SmProtoEdge::AddEdgeDefinition`): the definition's previous owner does not list
    /// it. Upstream drops the failed removal and adds it; the port returns the error, as
    /// the fork does. Discriminating.
    #[test]
    fn add_edge_definition_passes_on_a_failed_removal_from_its_owner() {
        let k = Kernel::default();
        let mut geo = Geo {
            curves: vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])],
            ..Geo::default()
        };
        let mut m = manager();
        let ivl = geo.curves[0].natural_interval();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let owner = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        let target = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        let ed = m
            .insert_edge_definition(EdgeDefinition::new(
                Some(H(0, ClassObjectKind::Curve)),
                ivl,
                R0,
                R1,
                1e-5,
                None,
                None,
                Some(owner),
            ))
            .unwrap();
        let r = m.add_edge_definition(&k, &mut geo, target, ed);
        assert!(r.is_err(), "{r:?}");
    }

    /// fork bcc6f01 (`SER( m_pXSectCurve3d->EvaluatePoint(dT, rPosition) )` in
    /// `SmEdgeDefinition::GetEndPosition`): the definition's curve cannot be evaluated.
    /// Upstream returns the unset point; the port the error, as the fork does.
    /// Discriminating.
    #[test]
    fn end_position_of_an_unevaluable_curve_is_passed_on() {
        let k = Kernel::default();
        let geo = Geo {
            curves: vec![Curve::from(crate::curve::offset_curve::OffsetCurve::empty())],
            ..Geo::default()
        };
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let d = EdgeDefinition::new(
            Some(H(0, ClassObjectKind::Curve)),
            ivl,
            R0,
            R1,
            1e-5,
            None,
            None,
            None,
        );
        let r = d.end_position(&k, &geo, true);
        assert!(r.is_err(), "{r:?}");
    }

    fn region_pc(o: H) -> PointClassification<H> {
        let mut pc = PointClassification::new(1e-5);
        pc.set_class_object(PointClassType::Region, Some(o));
        pc
    }

    #[test]
    fn combine_vertex_definitions_by_topology() {
        let k = Kernel::default();
        let geo = Geo::default();
        let mut m = manager();
        let p = Point3::new(1.0, 2.0, 3.0);
        let mk = |m: &mut ProtoTopologyManager<H>, a: H, b: H, gap: f64| {
            let vd =
                VertexDefinition::new(&k, &geo, &region_pc(a), &region_pc(b), p, 1e-5, gap, None)
                    .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let a = mk(&mut m, R0, R1, 2e-6);
        let b = mk(&mut m, R0, R1, 1e-6);
        let c = mk(&mut m, R0, L0, 1e-6);
        // same topology in both: the larger gap goes
        assert_eq!(m.can_combine_vertex_definitions(&k, &geo, a, b).unwrap(), 1);
        assert_eq!(m.can_combine_vertex_definitions(&k, &geo, b, a).unwrap(), 2);
        // a region and a loop are not connected
        assert_eq!(m.can_combine_vertex_definitions(&k, &geo, a, c).unwrap(), 0);

        // resolving a proto vertex with both keeps one
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        m.add_vertex_definition(pv, a).unwrap();
        m.add_vertex_definition(pv, b).unwrap();
        m.add_vertex_definition(pv, c).unwrap();
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[b, c]);
        assert!(m.vertex_def(a).is_none());
    }

    /// #16637 review: a proto vertex that combines a definition away moves to the average
    /// of those that remain, and its tolerance grows to the measured distance to the
    /// absorbed definition's point, by no estimate (no placement bound is known for it, and
    /// its tolerance is no such bound). Resolved again after another definition is
    /// absorbed and the position moves, the tolerance still covers the first one.
    #[test]
    fn a_resolved_proto_vertex_covers_every_point_it_absorbed() {
        let k = Kernel::default();
        let geo = Geo::default();
        let mut m = manager();
        let p = Point3::new(1.0, 2.0, 3.0);
        let mk = |m: &mut ProtoTopologyManager<H>, at: Point3, a: H, b: H, gap: f64| {
            let vd =
                VertexDefinition::new(&k, &geo, &region_pc(a), &region_pc(b), at, 1e-5, gap, None)
                    .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let far = p + Vec3::new(3e-5, 0.0, 0.0);
        let a = mk(&mut m, far, R0, R1, 2e-6);
        let b = mk(&mut m, p, R0, R1, 1e-6);
        let c = mk(&mut m, p, R0, L0, 1e-6);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [a, b, c] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b, c]);
        assert_eq!(v.position(), p);
        let reach = far.distance(p) * (1.0 + gamma(4));
        assert_eq!(
            v.raw_tolerance(),
            reach,
            "the measured distance, no estimate added"
        );

        // `d` takes `b`'s place (the smaller gap), and the survivors' average moves 1e-5.
        let d = mk(&mut m, p + Vec3::new(0.0, 2e-5, 0.0), R0, R1, 5e-7);
        m.add_vertex_definition(pv, d).unwrap();
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[c, d]);
        assert!(v.position().distance(p + Vec3::new(0.0, 1e-5, 0.0)) < 1e-15);
        for at in [far, p] {
            assert!(
                v.position().distance(at) <= v.raw_tolerance(),
                "{at:?} is {:e} from the vertex, its tolerance {:e}",
                v.position().distance(at),
                v.raw_tolerance()
            );
        }
    }

    /// The point of the classification vertices `V0`, `V1` (and `V2`, `V3` at `R_AT`) of
    /// the absorption tests below.
    const P: Point3 = Point3::new(1.0, 2.0, 3.0);
    const FAR: Point3 = Point3::new(1.0 + 3e-5, 2.0, 3.0);
    const Q: Point3 = Point3::new(1.0, 2.0, 3.0 + 4e-5);
    const R_AT: Point3 = Point3::new(1.0, 2.0 - 8e-5, 3.0);

    fn absorb_geo() -> Geo {
        Geo {
            points: vec![Q, Q, R_AT, R_AT],
            ..Geo::default()
        }
    }

    /// A vertex classification of `H(i, Vertex)` (its point `absorb_geo().points[i]`)
    /// whose stored gap is `gap`.
    fn vertex_pc_at(i: usize, gap: f64) -> PointClassification<H> {
        let mut pc = PointClassification::new(1e-5);
        pc.set_class_object(PointClassType::Vertex, Some(H(i, ClassObjectKind::Vertex)));
        pc.set_gap_3d(gap);
        pc
    }

    /// A proto vertex at `P` (tolerance 1e-5) listing vertex definitions on the vertices
    /// `V0` (B-rep 1) and `V1` (B-rep 2), both at `Q`, placed at `FAR` and `P` (in that
    /// order when `far_first`). The one at `FAR` has the larger definition gap (it is
    /// combined away), tolerance `far_tol` and classifications whose stored gap is
    /// `far_pc_gap`; the one at `P` has tolerance `near_tol`. Returns the
    /// manager, the proto vertex and the definitions at `FAR` and `P`, before resolution.
    fn vertex_defs_on_one_vertex(
        k: &Kernel,
        geo: &Geo,
        far_first: bool,
        near_tol: f64,
        far_tol: f64,
        far_vd_gap: f64,
        far_pc_gap: f64,
    ) -> (
        ProtoTopologyManager<H>,
        ProtoVertexId,
        VertexDefId,
        VertexDefId,
    ) {
        let mut m = manager();
        let mut mk = |at: Point3, tol: f64, vd_gap: f64, pc_gap: f64| {
            let vd = VertexDefinition::new(
                k,
                geo,
                &vertex_pc_at(0, pc_gap),
                &vertex_pc_at(1, pc_gap),
                at,
                tol,
                vd_gap,
                None,
            )
            .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let a = mk(FAR, far_tol, far_vd_gap, far_pc_gap);
        let b = mk(P, near_tol, 1e-6, 0.0);
        let pv = m.new_proto_vertex(P, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let order = if far_first { [a, b] } else { [b, a] };
        for vd in order {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        (m, pv, a, b)
    }

    /// Asserts that the proto vertex's tolerance reaches each of `points`.
    fn assert_covers(m: &ProtoTopologyManager<H>, pv: ProtoVertexId, points: &[Point3]) {
        let v = m.proto_vertex(pv).unwrap();
        for &at in points {
            assert!(
                v.position().distance(at) <= v.raw_tolerance(),
                "{at:?} is {:e} from the vertex at {:?}, its tolerance {:e}",
                v.position().distance(at),
                v.position(),
                v.raw_tolerance()
            );
        }
    }

    /// #16638 review: a definition on vertex classifications is combined away. The
    /// tolerance reaches its position and its object point (the vertex's point `Q`), each
    /// computed, and exactly that: the classifications' stored gap (2e-6, not a distance
    /// at their parameters) adds nothing.
    #[test]
    fn an_absorbed_vertex_classification_is_covered_at_its_object_point() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let (mut m, pv, a, b) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-4, 1e-4, 2e-6, 2e-6);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert_eq!(v.position(), P);
        // `a`'s position and object points; the survivor `b` is at `P` already, so there is
        // no move and its points are not recorded (#17839 review r2)
        assert_eq!(v.absorbed_points(), &[FAR, Q, Q]);
        assert!(!v.absorbed_unbounded());
        assert_eq!(
            v.raw_tolerance(),
            P.distance(Q) * (1.0 + gamma(4)),
            "the computed distance to the object point, no stored gap added"
        );
    }

    /// #16638 review: a classification whose stored gap is NaN still has its object point
    /// covered (the gap is not read), and the vertex is at the survivor.
    #[test]
    fn an_absorbed_classification_with_a_nan_gap_is_still_covered() {
        let k = Kernel::default();
        let geo = absorb_geo();
        // listed second: `b`'s gap is not greater than NaN, so the NaN one goes. Its
        // tolerance (6e-5) holds its object point `Q` (5e-5 from `FAR`), so the definition
        // is consistent (#17839 review r2: an object point outside it would not be covered).
        let (mut m, pv, a, b) =
            vertex_defs_on_one_vertex(&k, &geo, false, 1e-4, 6e-5, f64::NAN, f64::NAN);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert_eq!(v.position(), P);
        assert_covers(&m, pv, &[FAR, Q]);
        assert_eq!(v.raw_tolerance(), P.distance(Q) * (1.0 + gamma(4)));
    }

    /// #16638 review: an absorbed definition whose object point cannot be computed (its
    /// parameters were unset after it was made) has no bound, so the proto vertex is not
    /// moved: it stays where upstream leaves it, covering the points it has there, and the
    /// resolution still succeeds.
    #[test]
    fn an_unbounded_absorbed_definition_does_not_move_the_vertex() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let (mut m, pv, a, b) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-4, 1e-4, 2e-6, 0.0);
        m.vertex_def_mut(a)
            .unwrap()
            .brep_point_classif_mut(1)
            .unset_parameters();
        let start = P + Vec3::new(0.0, 1e-5, 0.0);
        m.proto_vertex_mut(pv).unwrap().set_position(start);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert!(v.absorbed_unbounded());
        assert_eq!(v.position(), start, "not moved without a bound");
        assert_covers(&m, pv, &[FAR, Q]);

        // and a later resolution that absorbs a bounded one (the larger gap) does not move
        // it either: the earlier unbounded one still has no bound
        let (pc1, pc2) = (vertex_pc_at(0, 0.0), vertex_pc_at(1, 0.0));
        let c = VertexDefinition::new(&k, &geo, &pc1, &pc2, P, 1e-4, 5e-6, None).unwrap();
        let c = m.insert_vertex_definition(c).unwrap();
        m.add_vertex_definition(pv, c).unwrap();
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert_eq!(v.position(), start);
    }

    /// #17839 review r2: a remaining definition whose object point lies far outside its own
    /// tolerance of its position (here `R_AT`, 8e-5 off with a tolerance of 1e-5: input no
    /// kernel-built definition can have) is not a point it stands for. It is not recorded,
    /// the result is unbounded and the vertex stays; the tolerance reaches only the absorbed
    /// position `FAR`. Before the fix the object point was recorded on a zero move and the
    /// tolerance grew to reach it (the `edge_vertex_defs_huge` golden row, 0.1 -> 9999).
    #[test]
    fn a_survivor_object_point_outside_its_tolerance_does_not_widen_the_vertex() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let mut m = manager();
        let mut mk = |pc2: PointClassification<H>, at: Point3, tol: f64, gap: f64| {
            let vd =
                VertexDefinition::new(&k, &geo, &region_pc(R0), &pc2, at, tol, gap, None).unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let a = mk(region_pc(R1), FAR, 1e-5, 2e-6);
        let b = mk(region_pc(R1), P, 1e-5, 1e-6);
        let c = mk(vertex_pc_at(2, 0.0), P, 1e-5, 1e-6);
        let pv = m.new_proto_vertex(P, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [a, b, c] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b, c]);
        assert_eq!(v.position(), P);
        assert!(!v.absorbed_points().contains(&R_AT));
        assert_eq!(
            v.raw_tolerance(),
            P.distance(FAR) * (1.0 + gamma(4)),
            "only the absorbed position is reached, not the inconsistent object point"
        );
    }

    /// #17839 review r2: a distance between finite points that overflows (x = -1e300 to
    /// x = 1e300) does not widen the tolerance to +inf; the point is left uncovered and
    /// marks the absorbed points unbounded, so the vertex does not move later.
    #[test]
    fn an_overflowing_absorbed_distance_marks_the_vertex_unbounded() {
        let mut m = manager();
        let pv = m
            .new_proto_vertex(Point3::new(-1e300, 0.0, 0.0), 1e-5)
            .unwrap();
        let v = m.proto_vertex_mut(pv).unwrap();
        v.absorb(&[Point3::new(1e300, 0.0, 0.0)], true);
        assert!(!v.absorbed_unbounded());
        v.cover_absorbed();
        assert_eq!(v.raw_tolerance(), 1e-5, "not widened");
        assert!(v.absorbed_unbounded());
    }

    /// #17839 review: an absorbed definition placed at an infinite position records only
    /// its finite points (its object points), and no infinite distance widens the
    /// tolerance: it stays finite (before the fix the infinite position was recorded and
    /// the tolerance became +inf).
    #[test]
    fn an_absorbed_definition_at_an_infinite_position_keeps_the_tolerance_finite() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let mut m = manager();
        let mut mk = |at: Point3, vd_gap: f64| {
            let vd = VertexDefinition::new(
                &k,
                &geo,
                &vertex_pc_at(0, 0.0),
                &vertex_pc_at(1, 0.0),
                at,
                1e-4,
                vd_gap,
                None,
            )
            .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        // both at an infinite x: their distance is NaN, which passes the distance check,
        // so `a` (the larger gap) is combined away
        let inf = Point3::new(f64::INFINITY, 2.0, 3.0);
        let a = mk(inf, 2e-6);
        let b = mk(inf, 1e-6);
        let pv = m.new_proto_vertex(P, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [a, b] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert!(v.absorbed_unbounded(), "the infinite position has no bound");
        assert!(
            v.absorbed_points().iter().all(|p| finite_point(*p)),
            "only finite points are recorded: {:?}",
            v.absorbed_points()
        );
        assert!(
            v.raw_tolerance().is_finite(),
            "tolerance {:e} at {:?}",
            v.raw_tolerance(),
            v.position()
        );
    }

    /// #16638 review: a proto vertex that absorbed definitions and is then combined with
    /// another (`CombineProtoVertex`, which re-averages the position) keeps covering them.
    #[test]
    fn a_combined_proto_vertex_covers_what_either_absorbed() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let (mut m, pv, _, _) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-4, 1e-4, 2e-6, 0.0);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert_covers(&m, pv, &[FAR, Q]); // before the combine
        let other_at = P + Vec3::new(0.0, -6e-5, 0.0);
        let e = VertexDefinition::new(
            &k,
            &geo,
            &region_pc(R0),
            &region_pc(L0),
            other_at,
            1e-5,
            1e-6,
            None,
        )
        .unwrap();
        let e = m.insert_vertex_definition(e).unwrap();
        let other = m.new_proto_vertex(other_at, 1e-5).unwrap();
        m.add_proto_vertex(other);
        m.add_vertex_definition(other, e).unwrap();
        m.combine_proto_vertex(pv, other).unwrap();
        let v = m.proto_vertex(pv).unwrap();
        let moved = P + Vec3::new(0.0, -3e-5, 0.0);
        assert!(v.position().distance(moved) < 1e-15);
        assert_covers(&m, pv, &[FAR, Q]);
        let reach = v.position().distance(Q) * (1.0 + gamma(4));
        assert_eq!(
            v.raw_tolerance(),
            reach,
            "the computed distance, no estimate"
        );
    }

    /// #16638 review: a definition added after a resolution (`AddVertexDefinition` from two
    /// classifications, which re-averages the position) leaves the absorbed points covered.
    #[test]
    fn a_definition_added_after_resolution_keeps_absorbed_points_covered() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let (mut m, pv, _, b) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-4, 1e-4, 2e-6, 0.0);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert_covers(&m, pv, &[FAR, Q]); // before the add
        let (vd, created) = m
            .add_vertex_definition_from_pcs(
                &k,
                &geo,
                pv,
                &mut vertex_pc_at(2, 0.0),
                &mut vertex_pc_at(3, 0.0),
            )
            .unwrap();
        assert!(created);
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b, vd]);
        let moved = P + Vec3::new(0.0, -4e-5, 0.0);
        assert!(v.position().distance(moved) < 1e-15);
        assert_covers(&m, pv, &[FAR, Q]);
    }

    /// #17839 re-review: a crossing that places a definition again after a resolution
    /// (`AddCrossing`, which moves the proto vertex with it) leaves the absorbed points
    /// covered.
    #[test]
    fn a_crossing_after_resolution_keeps_absorbed_points_covered() {
        let k = Kernel::default();
        let geo = Geo {
            vertex_tol: Some(1e-4),
            ..absorb_geo()
        };
        let (mut m, pv, _, _) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-6, 1e-4, 2e-6, 0.0);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert_covers(&m, pv, &[FAR, Q]); // before the crossing
                                          // the first crossing records its sine; the second, closer one places `b` again
        for sine in [0.5, 1.0] {
            let (pc1, pc2) = (vertex_pc_at(0, 0.0), vertex_pc_at(1, 0.0));
            m.add_crossing(&k, &geo, pv, &pc1, &pc2, sine, None)
                .unwrap();
        }
        let v = m.proto_vertex(pv).unwrap();
        assert!(v.position().distance(P) > 1e-6, "the crossing moved it");
        assert_covers(&m, pv, &[FAR, Q, P]);
    }

    /// The definitions of [`vertex_defs_on_one_vertex`] (`FAR` combined away into `P`),
    /// with a proto edge from the proto vertex along a line that starts at `end`, resolved.
    fn resolve_with_an_edge_from(end: Point3) -> (ProtoTopologyManager<H>, ProtoVertexId) {
        let k = Kernel::default();
        let mut geo = absorb_geo();
        let far_end = end + Vec3::new(1e-3, 0.0, 0.0);
        geo.curves.push(line(end.to_array(), far_end.to_array()));
        let (mut m, pv, _, _) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-4, 1e-4, 2e-6, 0.0);
        m.proto_vertex_mut(pv)
            .unwrap()
            .set_position(P + Vec3::new(0.0, 1e-5, 0.0));
        let other = m.new_proto_vertex(far_end, 1e-5).unwrap();
        m.add_proto_vertex(other);
        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        let ivl = geo.curves[0].natural_interval();
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);
        m.create_proto_edge(&k, &mut geo, Some(pv), Some(other), &cc, &ci1, &cc, &ci2)
            .unwrap();
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        (m, pv)
    }

    /// #17839 re-review: a curve end at the proto vertex that lies within the absorbed
    /// definition's tolerance of its object point `Q` traces to it, and is covered.
    #[test]
    fn an_attached_curve_end_traced_to_an_absorbed_definition_is_covered() {
        let end = Q + Vec3::new(0.0, 0.0, 2e-6);
        let (m, pv) = resolve_with_an_edge_from(end);
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.position(), P, "moved to the survivor");
        assert!(!v.absorbed_unbounded());
        assert_covers(&m, pv, &[FAR, Q, end]);
    }

    /// #17839 re-review: a curve end at the proto vertex that traces to no definition (it
    /// lies 5e-4 off, beyond every definition's 1e-4 tolerance) has no bound, so the
    /// proto vertex does not move.
    #[test]
    fn an_attached_curve_end_traced_to_no_definition_stops_the_move() {
        let end = P + Vec3::new(0.0, 0.0, 5e-4);
        let (m, pv) = resolve_with_an_edge_from(end);
        let v = m.proto_vertex(pv).unwrap();
        assert!(v.absorbed_unbounded());
        assert_eq!(v.position(), P + Vec3::new(0.0, 1e-5, 0.0), "not moved");
        assert_covers(&m, pv, &[FAR, Q]);
    }

    /// #17839 bot review: a curve end at the proto vertex that traces to a
    /// remaining definition stays covered when the vertex moves. The definitions are
    /// those of `the_move_to_the_survivors_covers_their_object_points` (the vertex moves
    /// from `P` to 4e-5 above it). The end lies 9e-5 below `R_AT`, within the tolerance
    /// 1e-4 of the remaining `s1`'s object point and 2.1e-4 from where the vertex moves:
    /// beyond the 1.2e-4 that reaching `R_AT` gives. Without recording it the moved
    /// vertex did not reach it.
    #[test]
    fn an_attached_curve_end_traced_to_a_remaining_definition_is_covered_after_a_move() {
        let k = Kernel::default();
        let mut geo = absorb_geo();
        let end = R_AT + Vec3::new(0.0, -9e-5, 0.0);
        let far_end = end + Vec3::new(1e-3, 0.0, 0.0);
        geo.curves.push(line(end.to_array(), far_end.to_array()));
        let mut m = manager();
        let s1 = VertexDefinition::new(
            &k,
            &geo,
            &vertex_pc_at(2, 0.0),
            &vertex_pc_at(3, 0.0),
            P,
            1e-4,
            1e-6,
            None,
        )
        .unwrap();
        let s1 = m.insert_vertex_definition(s1).unwrap();
        let mut region_vd = |at: Point3, gap: f64| {
            let vd = VertexDefinition::new(
                &k,
                &geo,
                &region_pc(R0),
                &region_pc(R1),
                at,
                1e-5,
                gap,
                None,
            )
            .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let s2_at = P + Vec3::new(0.0, 8e-5, 0.0);
        let s2 = region_vd(s2_at, 1e-6);
        let a_at = P + Vec3::new(0.0, 6e-5, 0.0);
        let a = region_vd(a_at, 2e-6);
        let pv = m.new_proto_vertex(P, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [s1, s2, a] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        let other = m.new_proto_vertex(far_end, 1e-5).unwrap();
        m.add_proto_vertex(other);
        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        let ivl = geo.curves[0].natural_interval();
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);
        m.create_proto_edge(&k, &mut geo, Some(pv), Some(other), &cc, &ci1, &cc, &ci2)
            .unwrap();
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[s1, s2]);
        assert!(!v.absorbed_unbounded());
        let moved = P + Vec3::new(0.0, 4e-5, 0.0);
        assert!(v.position().distance(moved) < 1e-15);
        assert_covers(&m, pv, &[a_at, P, s2_at, R_AT, end]);
    }

    /// #17839 re-review: the move to the remaining definitions also covers their own
    /// points. `s1` (on the vertices `V2`, `V3`, at `P`, object point `R_AT` 8e-5 below,
    /// within its tolerance 1e-4)
    /// stays alone; `a` combines into `s2` (both on the regions `R0`, `R1`). The vertex moves
    /// to the average of `s1` and `s2`, which is 1.2e-4 from `R_AT`: much more than the
    /// 2e-5 to what was absorbed.
    #[test]
    fn the_move_to_the_survivors_covers_their_object_points() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let mut m = manager();
        let s1 = VertexDefinition::new(
            &k,
            &geo,
            &vertex_pc_at(2, 0.0),
            &vertex_pc_at(3, 0.0),
            P,
            1e-4,
            1e-6,
            None,
        )
        .unwrap();
        let s1 = m.insert_vertex_definition(s1).unwrap();
        let mut region_vd = |at: Point3, gap: f64| {
            let vd = VertexDefinition::new(
                &k,
                &geo,
                &region_pc(R0),
                &region_pc(R1),
                at,
                1e-5,
                gap,
                None,
            )
            .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let s2_at = P + Vec3::new(0.0, 8e-5, 0.0);
        let s2 = region_vd(s2_at, 1e-6);
        let a_at = P + Vec3::new(0.0, 6e-5, 0.0);
        let a = region_vd(a_at, 2e-6);
        let pv = m.new_proto_vertex(P, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [s1, s2, a] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[s1, s2]);
        let moved = P + Vec3::new(0.0, 4e-5, 0.0);
        assert!(v.position().distance(moved) < 1e-15);
        assert_covers(&m, pv, &[a_at, P, s2_at, R_AT]);
        assert_eq!(
            v.raw_tolerance(),
            v.position().distance(R_AT) * (1.0 + gamma(4))
        );
    }

    /// #17839 review r3: an absorbed definition at `FAR` whose object points (both `Q`)
    /// lie 5e-5 off its position, outside its tolerance 1e-5, is inconsistent: no
    /// placement the kernel makes puts a position that far from the segment between its
    /// object points. Its object points are not recorded and the vertex does not move
    /// from where it starts (1e-5 off the survivor). Without the consistency rule the
    /// vertex moved to the survivor and its tolerance grew to reach `Q`.
    #[test]
    fn an_inconsistent_absorbed_definition_does_not_move_the_vertex() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let (mut m, pv, a, b) = vertex_defs_on_one_vertex(&k, &geo, true, 1e-4, 1e-5, 2e-6, 0.0);
        let start = P + Vec3::new(0.0, 1e-5, 0.0);
        m.proto_vertex_mut(pv).unwrap().set_position(start);
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert_eq!(v.position(), start, "not moved");
        assert!(v.absorbed_unbounded());
        assert_eq!(v.absorbed_points(), &[FAR], "only the position is recorded");
        assert_eq!(v.raw_tolerance(), start.distance(FAR) * (1.0 + gamma(4)));
    }

    /// #17839 review r3: as `the_move_to_the_survivors_covers_their_object_points`, but
    /// the survivor `s1` has tolerance 1e-5, so its object point `R_AT` (8e-5 off its
    /// position, both object points at `R_AT`) is inconsistent. The survivors' average is
    /// 4e-5 from the vertex, a real move, which is declined: `R_AT` is not recorded and the
    /// vertex stays at `P`. Without the consistency rule it moved and its tolerance grew to
    /// reach `R_AT` (1.2e-4).
    #[test]
    fn an_inconsistent_survivor_declines_a_real_move() {
        let k = Kernel::default();
        let geo = absorb_geo();
        let mut m = manager();
        let s1 = VertexDefinition::new(
            &k,
            &geo,
            &vertex_pc_at(2, 0.0),
            &vertex_pc_at(3, 0.0),
            P,
            1e-5,
            1e-6,
            None,
        )
        .unwrap();
        let s1 = m.insert_vertex_definition(s1).unwrap();
        let mut region_vd = |at: Point3, gap: f64| {
            let vd = VertexDefinition::new(
                &k,
                &geo,
                &region_pc(R0),
                &region_pc(R1),
                at,
                1e-5,
                gap,
                None,
            )
            .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let s2_at = P + Vec3::new(0.0, 8e-5, 0.0);
        let s2 = region_vd(s2_at, 1e-6);
        let a_at = P + Vec3::new(0.0, 6e-5, 0.0);
        let a = region_vd(a_at, 2e-6);
        let pv = m.new_proto_vertex(P, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [s1, s2, a] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[s1, s2]);
        assert_eq!(v.position(), P, "not moved");
        assert!(!v.absorbed_points().contains(&R_AT));
        assert_eq!(v.raw_tolerance(), P.distance(a_at) * (1.0 + gamma(4)));
    }

    /// #17839 review r3: a definition the kernel places midway between two object points
    /// 1e-4 apart (`find_position_from_pcs` with equal gaps) holds them 5e-5 off its
    /// position, beyond its tolerance 1e-5. It is consistent (its position is on the
    /// segment between them), so the vertex still moves from its start, 1e-5 off, to the
    /// survivor, and its tolerance covers both object points. A rule that held each object
    /// point within the tolerance of the position marked it unbounded and kept the vertex
    /// at its start.
    #[test]
    fn a_midpoint_definition_still_moves_the_vertex() {
        let k = Kernel::default();
        let half = Vec3::new(0.0, 0.0, 5e-5);
        let (top, bottom) = (P + half, P - half);
        let geo = Geo {
            points: vec![top, bottom],
            ..Geo::default()
        };
        let mut m = manager();
        let mut mk = |vd_gap: f64| {
            let vd = VertexDefinition::new(
                &k,
                &geo,
                &vertex_pc_at(0, 0.0),
                &vertex_pc_at(1, 0.0),
                P,
                1e-5,
                vd_gap,
                None,
            )
            .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let a = mk(2e-4);
        let b = mk(1e-4);
        let start = P + Vec3::new(0.0, 1e-5, 0.0);
        let pv = m.new_proto_vertex(start, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        for vd in [a, b] {
            m.add_vertex_definition(pv, vd).unwrap();
        }
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert!(m.vertex_def(a).is_none());
        let v = m.proto_vertex(pv).unwrap();
        assert_eq!(v.vertex_definitions(), &[b]);
        assert!(!v.absorbed_unbounded());
        assert_eq!(v.position(), P, "moved to the survivor");
        assert!(v.absorbed_points().contains(&top) && v.absorbed_points().contains(&bottom));
        assert_covers(&m, pv, &[top, bottom]);
        assert_eq!(
            v.raw_tolerance(),
            max2(P.distance(top), P.distance(bottom)) * (1.0 + gamma(4))
        );
    }

    /// A proto edge from a proto vertex back to itself along an open curve 5e-5 long
    /// (longer than twice the 1e-5 tolerance, ends as far apart), the proto vertex
    /// holding one vertex definition with `pc1` and `pc2`, resolved. Returns the
    /// manager, the proto vertex and edge, and what `resolve_proto_edge` returned.
    fn resolve_self_loop(
        pc1: &PointClassification<H>,
        pc2: &PointClassification<H>,
    ) -> (
        ProtoTopologyManager<H>,
        ProtoVertexId,
        ProtoEdgeId,
        Result<Vec<H>>,
    ) {
        resolve_self_loop_along(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]), pc1, pc2)
    }

    /// [`resolve_self_loop`] along `curve`, which starts at the proto vertex.
    fn resolve_self_loop_along(
        curve: Curve,
        pc1: &PointClassification<H>,
        pc2: &PointClassification<H>,
    ) -> (
        ProtoTopologyManager<H>,
        ProtoVertexId,
        ProtoEdgeId,
        Result<Vec<H>>,
    ) {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(curve);
        let c0 = H(0, ClassObjectKind::Curve);
        let mut m = manager();
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let vd = VertexDefinition::new(&k, &geo, pc1, pc2, p, 1e-5, 1e-9, None).unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(pv, vd).unwrap();

        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(c0), false);
        let ivl = geo.curves[0].natural_interval();
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);
        let pe = m
            .create_proto_edge(&k, &mut geo, Some(pv), Some(pv), &cc, &ci1, &cc, &ci2)
            .unwrap();
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);
        let deleted = m.resolve_proto_edge(&k, &mut geo, pe);
        (m, pv, pe, deleted)
    }

    fn vertex_pc() -> PointClassification<H> {
        let mut pc = PointClassification::new(1e-5);
        pc.set_class_object(PointClassType::Vertex, Some(H(20, ClassObjectKind::Vertex)));
        pc
    }

    /// upstream: the proto vertex gets a second definition for the curve's end.
    /// These pool stand-ins have no surfaces or vertex records, so
    /// `CreateVertexDefinition` stops (no position found, UB-115; or the unported
    /// `SmVertex::GetPoint`): reaching it shows the split branch ran, edge kept.
    fn assert_split_attempted(pc1: &PointClassification<H>, pc2: &PointClassification<H>) {
        let (m, pv, pe, result) = resolve_self_loop(pc1, pc2);
        let err = result.unwrap_err();
        let msg = err.to_string();
        // `sm_FindPositionFromPCs`'s status, passed on (bcc6f01), where main refused with
        // an internal "no position found"
        assert!(
            msg.contains("neither point classification has its parameters set")
                || msg.contains("SmVertex::GetPoint"),
            "{msg}"
        );
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 1);
    }

    #[test]
    fn resolve_self_loop_off_a_vertex_still_splits() {
        assert_split_attempted(&region_pc(R0), &region_pc(R1));
    }

    #[test]
    fn resolve_self_loop_on_a_vertex_is_degenerate() {
        // tangency-labs/solidcore#480: a definition on a vertex in either B-rep is not
        // split (both ends would be that vertex there); the proto edge is dropped
        for (pc1, pc2) in [(vertex_pc(), region_pc(R1)), (region_pc(R0), vertex_pc())] {
            let (m, pv, pe, deleted) = resolve_self_loop(&pc1, &pc2);
            assert_eq!(
                deleted.unwrap().len(),
                1,
                "the definition's curve copy is deleted"
            );
            assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 0);
            assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 1);
        }
    }

    /// WR-214's drop holds only for a curve that stays within its ends' reach of the
    /// vertex: a curve from the vertex that runs 1e-3 out and comes back to 5e-5 from it
    /// is a real intersection, and resolving it is refused, not dropped.
    #[test]
    fn resolve_self_loop_on_a_vertex_leaving_its_reach_is_refused() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let bulge = || {
            Curve::from(BSplineCurve::from_nurb(
                NlCurve::new(
                    vec![
                        CPoint::new(0.0, 0.0, 0.0, NOW),
                        CPoint::new(1e-3, 1e-3, 0.0, NOW),
                        CPoint::new(5e-5, 0.0, 0.0, NOW),
                    ],
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                )
                .unwrap(),
                3,
                false,
            ))
        };
        for (pc1, pc2) in [(vertex_pc(), region_pc(R1)), (region_pc(R0), vertex_pc())] {
            let (mut m, pv, pe, result) = resolve_self_loop_along(bulge(), &pc1, &pc2);
            let err = result.unwrap_err();
            assert_eq!(err.operation(), OP_RESOLVE_PE, "{err}");
            assert_eq!(err.sm_status(), Some(1001), "{err}");
            assert_eq!(err.kind(), crate::error::ErrorKind::Failed, "{err}");
            assert!(!err.is_defect(), "{err}");
            let reach = match err.failure().map(|f| &f.reason) {
                Some(crate::error::Reason::Boolean(BooleanFailure::SelfLoopLeavesReach {
                    reach,
                })) => *reach,
                other => panic!("{other:?}"),
            };
            // the reach covers the far end (5e-5 away), and the bulge, 1e-3 out, leaves it
            assert!(reach > 5e-5 && reach < 1e-3, "{reach:e}");
            // kept for the Boolean, whose resolution only warns on a proto edge's error
            assert_eq!(m.take_self_loop_refusal(), Some(err));
            assert_eq!(m.take_self_loop_refusal(), None);
            assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);
            assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 1);
        }
    }

    #[test]
    fn resolve_self_loop_inside_one_vertex_is_absorbed() {
        // tangency-labs/solidcore#1058 (WR-306): both ends of the line find the one vertex
        // definition (tolerance 1e-4), and the 0.01 capture distance makes the proto
        // vertex's containment ball far wider than either line. The 5e-5 line lies within
        // the definition's tolerance, so the proto edge is that vertex (dropped, the proto
        // vertex marked). The 5e-4 line reaches past it: a real curve, which the capture
        // distance alone never absorbs, so upstream's split runs. Below upstream's short
        // test (twice the proto edge's tolerance) the 1e-6 line is dropped either way, and
        // the vertex is marked only when the line lies within the definition's tolerance
        // (1e-4, not 1e-7).
        for (length, vd_tol, dropped, marked) in [
            (5e-5, 1e-4, true, true),
            (5e-4, 1e-4, false, false),
            (1e-6, 1e-4, true, true),
            (1e-6, 1e-7, true, false),
        ] {
            let k = Kernel::default();
            let mut geo = Geo::default();
            geo.curves.push(line([0.0, 0.0, 0.0], [length, 0.0, 0.0]));
            let c0 = H(0, ClassObjectKind::Curve);
            let mut m = manager();
            m.set_capture_distance(0.01);
            let p = Point3::new(0.0, 0.0, 0.0);
            let pv = m.new_proto_vertex(p, 1e-5).unwrap();
            m.add_proto_vertex(pv);
            let (pc1, pc2) = (region_pc(R0), region_pc(R1));
            let vd = VertexDefinition::new(&k, &geo, &pc1, &pc2, p, vd_tol, 1e-9, None).unwrap();
            let vd = m.insert_vertex_definition(vd).unwrap();
            m.add_vertex_definition(pv, vd).unwrap();

            let mut cc = CurveClassification::new(&k);
            cc.set_curve(Some(c0), false);
            let ivl = geo.curves[0].natural_interval();
            let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci1.mid = region_pc(R0);
            let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci2.mid = region_pc(R1);
            let pe = m
                .create_proto_edge(&k, &mut geo, Some(pv), Some(pv), &cc, &ci1, &cc, &ci2)
                .unwrap();
            let pe_tol = m.proto_edge(pe).unwrap().raw_tolerance();
            assert_eq!(
                length >= 2.0 * pe_tol,
                length > 1e-6,
                "{length:e} against upstream's short test ({pe_tol:e})"
            );
            let result = m.resolve_proto_edge(&k, &mut geo, pe);
            assert_eq!(m.proto_vertex(pv).unwrap().absorbed_proto_edge(), marked);
            if dropped {
                assert_eq!(
                    result.unwrap().len(),
                    1,
                    "the definition's curve copy is deleted"
                );
                assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 0);
            } else {
                // the split branch ran (see `assert_split_attempted`)
                assert!(result.is_err());
                assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);
            }
        }
    }

    /// The hull bound is met on the whole curve: a line from the vertex is inside a reach
    /// longer than it, not one shorter, and a NaN reach never holds it.
    #[test]
    fn curve_inside_one_vertex_needs_the_whole_hull_within_the_tolerance() {
        let near = line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]);
        let ivl = near.natural_interval();
        let o = Point3::new(0.0, 0.0, 0.0);
        assert!(curve_piece_within(&near, &ivl, o, 1e-4));
        assert!(!curve_piece_within(&near, &ivl, o, 4e-5));
        assert!(!curve_piece_within(&near, &ivl, o, f64::NAN));
    }

    /// WR-306 (#17888 review): a definition that absorbed a self-loop and is then moved by a
    /// later self-loop's split (`set_position(pt0)`) still holds the absorbed curve. The 5e-5
    /// line at the origin is absorbed into the definition (tolerance 1e-4); the second loop,
    /// 1e-3 long from (2e-4, 0, 0), is split and moves the definition there, 2e-4 from the
    /// line's start. The tolerance grows to cover the line's hull from there, so the
    /// certified hull test holds at the new position; a move to a non-finite position is
    /// refused and leaves the definition as it was.
    #[test]
    fn a_split_after_an_absorb_keeps_the_absorbed_curve_covered() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
        geo.curves.push(line([2e-4, 0.0, 0.0], [2e-4, 1e-3, 0.0]));
        geo.points.push(Point3::new(0.0, 0.0, 0.0));
        let mut m = manager();
        m.set_capture_distance(0.01);
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        // a point classification in B-rep 1 gives the split's new definition a position
        let mut pc1 = PointClassification::new(1e-5);
        pc1.set_class_object(PointClassType::Point, Some(H(0, ClassObjectKind::Point)));
        let vd =
            VertexDefinition::new(&k, &geo, &pc1, &region_pc(R1), p, 1e-4, 1e-9, None).unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(pv, vd).unwrap();

        let mut self_loop = |m: &mut ProtoTopologyManager<H>, c: usize| {
            let mut cc = CurveClassification::new(&k);
            cc.set_curve(Some(H(c, ClassObjectKind::Curve)), false);
            let ivl = geo.curves[c].natural_interval();
            let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci1.mid = region_pc(R0);
            let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci2.mid = region_pc(R1);
            let pe = m
                .create_proto_edge(&k, &mut geo, Some(pv), Some(pv), &cc, &ci1, &cc, &ci2)
                .unwrap();
            let deleted = m.resolve_proto_edge(&k, &mut geo, pe);
            (pe, deleted)
        };

        let (absorbed, deleted) = self_loop(&mut m, 0);
        assert_eq!(
            deleted.unwrap().len(),
            1,
            "the absorbed curve's copy is deleted"
        );
        assert_eq!(m.proto_edge(absorbed).unwrap().num_edge_definitions(), 0);
        assert!(m.proto_vertex(pv).unwrap().absorbed_proto_edge());
        assert!(!m.vertex_def(vd).unwrap().absorbed_hull().is_empty());

        let (split, result) = self_loop(&mut m, 1);
        result.unwrap();
        assert_eq!(m.proto_edge(split).unwrap().num_edge_definitions(), 1);
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 2);
        let def = m.vertex_def(vd).unwrap();
        assert_eq!(
            def.position(),
            Point3::new(2e-4, 0.0, 0.0),
            "the split moved it"
        );
        let absorbed_line = &geo.curves[0];
        let ivl = absorbed_line.natural_interval();
        assert!(
            curve_piece_within(absorbed_line, &ivl, def.position(), def.tolerance(&k)),
            "the absorbed line is within {:e} of the moved definition",
            def.tolerance(&k)
        );
        assert!(def.tolerance(&k) > 2e-4 && def.tolerance(&k) < 2.0001e-4);

        let mut moved = def.clone();
        let before = moved.clone();
        let e = moved
            .set_position(&k, Point3::new(f64::NAN, 0.0, 0.0))
            .unwrap_err();
        assert_eq!(
            e.topology_failure(),
            Some(&TopologyFailure::AbsorbedCurveAtNoFiniteDistance)
        );
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(moved, before, "a refused move changes nothing");
        // a B-rep other than 1 and 2 is the caller's argument, and changes nothing
        let e = moved
            .set_src_topo(3, PointClassType::Vertex, None)
            .unwrap_err();
        assert_eq!(
            e.topology_failure(),
            Some(&TopologyFailure::BrepSideNotOneOrTwo { which: 3 })
        );
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR));
        assert_eq!(moved, before);
    }

    /// WR-306 (#17888 review): a split that would move an absorbing definition so far that
    /// covering its absorbed curve reaches the split's new definition is refused, not grown
    /// into an overlap. The 5e-5 line at the origin is absorbed (tolerance 1e-4); the second
    /// loop, 1e-3 long from (5e-3, 0, 0), would need a tolerance of about 5e-3 at its start,
    /// which reaches its end's new definition 1e-3 away. The refusal is kept for the
    /// Boolean, the definition stays where it was, and the new one is deleted.
    #[test]
    fn a_split_whose_cover_would_reach_the_new_definition_is_refused() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
        geo.curves.push(line([5e-3, 0.0, 0.0], [5e-3, 1e-3, 0.0]));
        geo.points.push(Point3::new(0.0, 0.0, 0.0));
        let mut m = manager();
        m.set_capture_distance(0.01);
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let mut pc1 = PointClassification::new(1e-5);
        pc1.set_class_object(PointClassType::Point, Some(H(0, ClassObjectKind::Point)));
        let vd =
            VertexDefinition::new(&k, &geo, &pc1, &region_pc(R1), p, 1e-4, 1e-9, None).unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(pv, vd).unwrap();
        let mut self_loop = |m: &mut ProtoTopologyManager<H>, c: usize| {
            let mut cc = CurveClassification::new(&k);
            cc.set_curve(Some(H(c, ClassObjectKind::Curve)), false);
            let ivl = geo.curves[c].natural_interval();
            let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci1.mid = region_pc(R0);
            let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci2.mid = region_pc(R1);
            let pe = m
                .create_proto_edge(&k, &mut geo, Some(pv), Some(pv), &cc, &ci1, &cc, &ci2)
                .unwrap();
            m.resolve_proto_edge(&k, &mut geo, pe)
        };
        self_loop(&mut m, 0).unwrap();
        assert!(m.proto_vertex(pv).unwrap().absorbed_proto_edge());
        let next = VertexDefId::from_index(1);
        assert_eq!(vd, VertexDefId::from_index(0));
        assert!(m.vertex_def(next).is_none());

        let err = self_loop(&mut m, 1).unwrap_err();
        assert!(err.to_string().contains("absorbed a curve"), "{err}");
        assert_eq!(err.kind(), crate::error::ErrorKind::Failed, "{err}");
        assert!(!err.is_defect(), "{err}");
        let needed = match err.failure().map(|f| &f.reason) {
            Some(crate::error::Reason::Boolean(BooleanFailure::SelfLoopSplitReachesVertex {
                needed: Some(needed),
            })) => *needed,
            other => panic!("{other:?}"),
        };
        // the tolerance that covers the line at the origin from the loop's start
        assert!((5e-3..5.1e-3).contains(&needed), "{needed:e}");
        assert!(m.take_self_loop_refusal().is_some(), "kept for the Boolean");
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[vd]);
        let def = m.vertex_def(vd).unwrap();
        assert_eq!(def.position(), p, "not moved");
        assert_eq!(def.tolerance(&k), 1e-4, "not grown");
        assert!(m.vertex_def(next).is_none(), "the new one is deleted");
    }

    #[test]
    fn a_split_already_overlapping_before_the_growth_is_not_refused() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
        geo.curves
            .push(line([1.01e-4, 0.0, 0.0], [1.01e-4, 5e-5, 0.0]));
        geo.points.push(Point3::new(0.0, 0.0, 0.0));
        let mut m = manager();
        m.set_capture_distance(0.01);
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let mut pc1 = PointClassification::new(1e-5);
        pc1.set_class_object(PointClassType::Point, Some(H(0, ClassObjectKind::Point)));
        let vd =
            VertexDefinition::new(&k, &geo, &pc1, &region_pc(R1), p, 1e-4, 1e-9, None).unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(pv, vd).unwrap();
        let mut self_loop = |m: &mut ProtoTopologyManager<H>, c: usize| {
            let mut cc = CurveClassification::new(&k);
            cc.set_curve(Some(H(c, ClassObjectKind::Curve)), false);
            let ivl = geo.curves[c].natural_interval();
            let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci1.mid = region_pc(R0);
            let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci2.mid = region_pc(R1);
            let pe = m
                .create_proto_edge(&k, &mut geo, Some(pv), Some(pv), &cc, &ci1, &cc, &ci2)
                .unwrap();
            m.resolve_proto_edge(&k, &mut geo, pe)
        };
        self_loop(&mut m, 0).unwrap();
        assert!(m.proto_vertex(pv).unwrap().absorbed_proto_edge());
        let next = VertexDefId::from_index(1);
        assert_eq!(vd, VertexDefId::from_index(0));
        assert!(m.vertex_def(next).is_none());

        // The new definition at (1.01e-4, 5e-5) is within 2e-4 of the moved one even
        // at the old tolerance, so the growth causes no overlap: split as upstream.
        self_loop(&mut m, 1).unwrap();
        assert!(m.take_self_loop_refusal().is_none());
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 2);
        let def = m.vertex_def(vd).unwrap();
        assert_eq!(def.position(), Point3::new(1.01e-4, 0.0, 0.0));
        assert!(def.tolerance(&k) > 1e-4, "grown to cover the absorbed line");
    }

    /// Makes the self-loop proto edge on `pv` along `geo.curves[c]` and resolves it.
    fn resolve_loop_at(
        k: &Kernel,
        m: &mut ProtoTopologyManager<H>,
        geo: &mut Geo,
        pv: ProtoVertexId,
        c: usize,
    ) -> Result<Vec<H>> {
        let mut cc = CurveClassification::new(k);
        cc.set_curve(Some(H(c, ClassObjectKind::Curve)), false);
        let ivl = geo.curves[c].natural_interval();
        let mut ci1 = CurveInterval::with_interval(k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);
        let pe = m
            .create_proto_edge(k, geo, Some(pv), Some(pv), &cc, &ci1, &cc, &ci2)
            .unwrap();
        m.resolve_proto_edge(k, geo, pe)
    }

    /// #18154 (1): `resolve_vertex_definition` moves a definition between its two object
    /// points through `set_position`, so a self-loop it absorbed stays covered. The 5e-5
    /// line at the origin is absorbed (tolerance 1e-4); the definition's points, 3e-4
    /// apart with equal gaps, put it at (1.5e-4, 0, 0), 1.5e-4 from the line's start: the
    /// tolerance grows to cover the line's certified hull from there.
    #[test]
    fn a_resolved_definition_keeps_its_absorbed_curve_covered() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
        geo.points.push(Point3::new(0.0, 0.0, 0.0));
        geo.points.push(Point3::new(3e-4, 0.0, 0.0));
        let mut m = manager();
        m.set_capture_distance(0.01);
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let point_pc = |i: usize| {
            let mut pc = PointClassification::new(1e-5);
            pc.set_class_object(PointClassType::Point, Some(H(i, ClassObjectKind::Point)));
            pc.set_gap_3d(1e-5);
            pc
        };
        let vd = VertexDefinition::new(&k, &geo, &point_pc(0), &point_pc(1), p, 1e-4, 1e-9, None)
            .unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(pv, vd).unwrap();

        resolve_loop_at(&k, &mut m, &mut geo, pv, 0).unwrap();
        assert!(m.proto_vertex(pv).unwrap().absorbed_proto_edge());
        assert!(!m.vertex_def(vd).unwrap().absorbed_hull().is_empty());

        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        let def = m.vertex_def(vd).unwrap();
        assert_eq!(
            def.position(),
            Point3::new(1.5e-4, 0.0, 0.0),
            "resolved halfway"
        );
        let absorbed_line = &geo.curves[0];
        let ivl = absorbed_line.natural_interval();
        assert!(
            curve_piece_within(absorbed_line, &ivl, def.position(), def.tolerance(&k)),
            "the absorbed line is within {:e} of the resolved definition",
            def.tolerance(&k)
        );
        assert!(def.tolerance(&k) > 1.5e-4 && def.tolerance(&k) < 1.5001e-4);
    }

    /// #18154 (2): when `resolve_proto_vertex` combines away the definition that absorbed a
    /// self-loop, the one that stays takes over its hull and covers it. `a` absorbs the
    /// 5e-5 line at the origin (tolerance 1e-4); `b`, on the same objects 1.5e-4 away with
    /// the tighter gap, stays. Its tolerance grows to cover the line's certified hull from
    /// where it is, and it holds the hull for a later move.
    #[test]
    fn a_combined_away_absorbing_definition_hands_its_curve_to_the_survivor() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
        let mut m = manager();
        m.set_capture_distance(0.01);
        let p = Point3::new(0.0, 0.0, 0.0);
        let pv = m.new_proto_vertex(p, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let mk = |m: &mut ProtoTopologyManager<H>, geo: &Geo, at: Point3, gap: f64| {
            let vd =
                VertexDefinition::new(&k, geo, &region_pc(R0), &region_pc(R1), at, 1e-4, gap, None)
                    .unwrap();
            m.insert_vertex_definition(vd).unwrap()
        };
        let a = mk(&mut m, &geo, p, 2e-6);
        m.add_vertex_definition(pv, a).unwrap();
        resolve_loop_at(&k, &mut m, &mut geo, pv, 0).unwrap();
        let hull = m.vertex_def(a).unwrap().absorbed_hull().to_vec();
        assert!(!hull.is_empty());

        let at = Point3::new(1.5e-4, 0.0, 0.0);
        let b = mk(&mut m, &geo, at, 1e-6);
        m.add_vertex_definition(pv, b).unwrap();
        m.resolve_proto_vertex(&k, &geo, pv).unwrap();
        assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[b]);
        assert!(m.vertex_def(a).is_none(), "a is combined away");
        let def = m.vertex_def(b).unwrap();
        assert_eq!(def.position(), at);
        let absorbed_line = &geo.curves[0];
        let ivl = absorbed_line.natural_interval();
        assert!(
            curve_piece_within(absorbed_line, &ivl, def.position(), def.tolerance(&k)),
            "the absorbed line is within {:e} of the definition that stays",
            def.tolerance(&k)
        );
        assert!(def.tolerance(&k) > 1.5e-4 && def.tolerance(&k) < 1.5001e-4);
        assert_eq!(def.absorbed_hull(), &hull[..], "b holds a's hull");
    }

    /// #18154 (3): the split's overlap refusal looks at every listed proto vertex's
    /// definitions, not only its own. As in
    /// `a_split_after_an_absorb_keeps_the_absorbed_curve_covered`, the split moves the
    /// absorbing definition to (2e-4, 0, 0), growing it to about 2e-4. A definition of
    /// another proto vertex at (4.5e-4, 0, 0) with tolerance 1e-4, which the old 1e-4 ball
    /// kept clear of (2.5e-4 > 2e-4), would be reached (3e-4 >= 2.5e-4): the split is
    /// refused and nothing moves. One at (6e-4, 0, 0) stays clear (4e-4 > 3e-4), and one at
    /// (2.9e-4, 0, 0) already overlapped (9e-5 <= 2e-4): both split as before.
    #[test]
    fn a_split_whose_cover_would_reach_another_proto_vertex_is_refused() {
        for (x, refused) in [(4.5e-4, true), (6e-4, false), (2.9e-4, false)] {
            let k = Kernel::default();
            let mut geo = Geo::default();
            geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
            geo.curves.push(line([2e-4, 0.0, 0.0], [2e-4, 1e-3, 0.0]));
            geo.points.push(Point3::new(0.0, 0.0, 0.0));
            let mut m = manager();
            m.set_capture_distance(0.01);
            let p = Point3::new(0.0, 0.0, 0.0);
            let pv = m.new_proto_vertex(p, 1e-5).unwrap();
            m.add_proto_vertex(pv);
            let mut pc1 = PointClassification::new(1e-5);
            pc1.set_class_object(PointClassType::Point, Some(H(0, ClassObjectKind::Point)));
            let vd =
                VertexDefinition::new(&k, &geo, &pc1, &region_pc(R1), p, 1e-4, 1e-9, None).unwrap();
            let vd = m.insert_vertex_definition(vd).unwrap();
            m.add_vertex_definition(pv, vd).unwrap();

            let q = Point3::new(x, 0.0, 0.0);
            let pv2 = m.new_proto_vertex(q, 1e-5).unwrap();
            m.add_proto_vertex(pv2);
            let other = VertexDefinition::new(
                &k,
                &geo,
                &region_pc(R0),
                &region_pc(R1),
                q,
                1e-4,
                1e-9,
                None,
            )
            .unwrap();
            let other = m.insert_vertex_definition(other).unwrap();
            m.add_vertex_definition(pv2, other).unwrap();

            resolve_loop_at(&k, &mut m, &mut geo, pv, 0).unwrap();
            assert!(m.proto_vertex(pv).unwrap().absorbed_proto_edge());

            let result = resolve_loop_at(&k, &mut m, &mut geo, pv, 1);
            assert_eq!(m.proto_vertex(pv2).unwrap().vertex_definitions(), &[other]);
            let refusal = m.take_self_loop_refusal();
            let def = m.vertex_def(vd).unwrap();
            if refused {
                let err = result.unwrap_err();
                assert!(err.to_string().contains("absorbed a curve"), "{err}");
                assert!(refusal.is_some(), "kept for the Boolean");
                assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[vd]);
                assert_eq!(def.position(), p, "not moved");
                assert_eq!(def.tolerance(&k), 1e-4, "not grown");
            } else {
                result.unwrap();
                assert!(refusal.is_none(), "{x:e}");
                assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions().len(), 2);
                assert_eq!(def.position(), Point3::new(2e-4, 0.0, 0.0), "{x:e}");
                assert!(def.tolerance(&k) > 2e-4, "{x:e}: grown to cover the line");
            }
        }
    }

    /// A proto vertex at `at` (tolerance 1e-5) holding one definition there with tolerance
    /// `tol`, listed at `m`: a vertex the growth tests below may reach.
    fn another_vertex_at(
        k: &Kernel,
        m: &mut ProtoTopologyManager<H>,
        geo: &Geo,
        at: Point3,
        tol: f64,
    ) -> (ProtoVertexId, VertexDefId) {
        let pv = m.new_proto_vertex(at, 1e-5).unwrap();
        m.add_proto_vertex(pv);
        let vd = VertexDefinition::new(k, geo, &region_pc(R0), &region_pc(R1), at, tol, 1e-9, None)
            .unwrap();
        let vd = m.insert_vertex_definition(vd).unwrap();
        m.add_vertex_definition(pv, vd).unwrap();
        (pv, vd)
    }

    /// #18312 (1): `resolve_vertex_definition`'s move may not grow the definition into
    /// another vertex definition that its tolerance before the move kept clear of. As in
    /// `a_resolved_definition_keeps_its_absorbed_curve_covered`, the definition moves to
    /// (1.5e-4, 0, 0) and grows to about 1.5e-4 to cover the absorbed line. A definition of
    /// another proto vertex at (3.8e-4, 0, 0) with tolerance 1e-4 is 2.3e-4 from there:
    /// clear of the old tolerance (2e-4 < 2.3e-4) but reached by the grown one
    /// (2.5e-4 >= 2.3e-4), so the resolution is refused for the Boolean and the definition
    /// is left as it was (`resolve_proto_vertex` passes the definition's status on, as the
    /// fork does; the manager keeps the refusal too). One at (6e-4, 0, 0) stays clear (4.5e-4 > 2.5e-4): the move
    /// goes ahead.
    #[test]
    fn a_resolve_whose_cover_would_reach_another_definition_is_refused() {
        for (x, refused) in [(3.8e-4, true), (6e-4, false)] {
            let k = Kernel::default();
            let mut geo = Geo::default();
            geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
            geo.points.push(Point3::new(0.0, 0.0, 0.0));
            geo.points.push(Point3::new(3e-4, 0.0, 0.0));
            let mut m = manager();
            m.set_capture_distance(0.01);
            let p = Point3::new(0.0, 0.0, 0.0);
            let pv = m.new_proto_vertex(p, 1e-5).unwrap();
            m.add_proto_vertex(pv);
            let point_pc = |i: usize| {
                let mut pc = PointClassification::new(1e-5);
                pc.set_class_object(PointClassType::Point, Some(H(i, ClassObjectKind::Point)));
                pc.set_gap_3d(1e-5);
                pc
            };
            let vd =
                VertexDefinition::new(&k, &geo, &point_pc(0), &point_pc(1), p, 1e-4, 1e-9, None)
                    .unwrap();
            let vd = m.insert_vertex_definition(vd).unwrap();
            m.add_vertex_definition(pv, vd).unwrap();
            let (pv2, other) = another_vertex_at(&k, &mut m, &geo, Point3::new(x, 0.0, 0.0), 1e-4);

            resolve_loop_at(&k, &mut m, &mut geo, pv, 0).unwrap();
            assert!(!m.vertex_def(vd).unwrap().absorbed_hull().is_empty());
            assert_eq!(m.vertex_def(vd).unwrap().tolerance(&k), 1e-4);
            let moved = Point3::new(1.5e-4, 0.0, 0.0);
            let cover = m
                .vertex_def(vd)
                .unwrap()
                .covering_tolerance(&k, moved)
                .unwrap();
            assert!(cover > 1.5e-4 && cover < 1.5001e-4);

            let result = m.resolve_proto_vertex(&k, &geo, pv);
            let refusal = m.take_self_loop_refusal();
            assert_eq!(m.proto_vertex(pv2).unwrap().vertex_definitions(), &[other]);
            let def = m.vertex_def(vd).unwrap();
            if refused {
                let err = refusal.expect("kept for the Boolean");
                assert!(err.to_string().contains("another vertex"), "{err}");
                assert_eq!(err.kind(), crate::error::ErrorKind::Failed, "{err}");
                assert!(!err.is_defect(), "{err}");
                assert_eq!(
                    err.failure().map(|f| &f.reason),
                    Some(&crate::error::Reason::Boolean(
                        BooleanFailure::AbsorbedCoverReachesVertex { needed: cover }
                    )),
                    "{err}"
                );
                assert_eq!(result.unwrap_err(), err);
                assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[vd]);
                assert_eq!(def.position(), p, "not moved");
                assert_eq!(def.tolerance(&k), 1e-4, "not grown");
            } else {
                result.unwrap();
                assert!(refusal.is_none(), "{x:e}");
                assert_eq!(def.position(), moved, "{x:e}");
                assert_eq!(def.tolerance(&k), cover, "{x:e}: grown to cover the line");
            }
        }
    }

    /// #18312 (2): the definition that stays when `resolve_proto_vertex` combines away one
    /// that absorbed a self-loop may not grow into another vertex definition that its own
    /// tolerance kept clear of. As in
    /// `a_combined_away_absorbing_definition_hands_its_curve_to_the_survivor`, `b` at
    /// (1.5e-4, 0, 0) would grow to about 1.5e-4 to cover `a`'s line. A definition of another
    /// proto vertex at (3.8e-4, 0, 0) with tolerance 1e-4 is 2.3e-4 from `b`: clear of `b`'s
    /// tolerance (2e-4 < 2.3e-4), reached by the grown one (2.5e-4 >= 2.3e-4). The combine
    /// is refused for the Boolean, with both definitions kept and `b` not grown. One at
    /// (6e-4, 0, 0) stays clear and `a` is combined away.
    #[test]
    fn a_combine_whose_cover_would_reach_another_definition_is_refused() {
        for (x, refused) in [(3.8e-4, true), (6e-4, false)] {
            let k = Kernel::default();
            let mut geo = Geo::default();
            geo.curves.push(line([0.0, 0.0, 0.0], [5e-5, 0.0, 0.0]));
            let mut m = manager();
            m.set_capture_distance(0.01);
            let p = Point3::new(0.0, 0.0, 0.0);
            let pv = m.new_proto_vertex(p, 1e-5).unwrap();
            m.add_proto_vertex(pv);
            let mk = |m: &mut ProtoTopologyManager<H>, geo: &Geo, at: Point3, gap: f64| {
                let vd = VertexDefinition::new(
                    &k,
                    geo,
                    &region_pc(R0),
                    &region_pc(R1),
                    at,
                    1e-4,
                    gap,
                    None,
                )
                .unwrap();
                m.insert_vertex_definition(vd).unwrap()
            };
            let a = mk(&mut m, &geo, p, 2e-6);
            m.add_vertex_definition(pv, a).unwrap();
            resolve_loop_at(&k, &mut m, &mut geo, pv, 0).unwrap();
            let hull = m.vertex_def(a).unwrap().absorbed_hull().to_vec();
            assert!(!hull.is_empty());

            let at = Point3::new(1.5e-4, 0.0, 0.0);
            let b = mk(&mut m, &geo, at, 1e-6);
            m.add_vertex_definition(pv, b).unwrap();
            let (pv2, other) = another_vertex_at(&k, &mut m, &geo, Point3::new(x, 0.0, 0.0), 1e-4);
            let mut covering = m.vertex_def(b).unwrap().clone();
            covering.take_over_hull(&k, &hull).unwrap();
            let cover = covering.tolerance(&k);
            assert!(cover > 1.5e-4 && cover < 1.5001e-4);

            let result = m.resolve_proto_vertex(&k, &geo, pv);
            let refusal = m.take_self_loop_refusal();
            assert_eq!(m.proto_vertex(pv2).unwrap().vertex_definitions(), &[other]);
            let def = m.vertex_def(b).unwrap();
            assert_eq!(def.position(), at);
            if refused {
                let err = result.unwrap_err();
                assert!(err.to_string().contains("another vertex"), "{err}");
                assert!(refusal.is_some(), "kept for the Boolean");
                assert!(m.vertex_def(a).is_some(), "a is not combined away");
                assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[a, b]);
                assert_eq!(def.tolerance(&k), 1e-4, "not grown");
                assert!(def.absorbed_hull().is_empty(), "a's hull not handed over");
            } else {
                result.unwrap();
                assert!(refusal.is_none(), "{x:e}");
                assert!(m.vertex_def(a).is_none(), "{x:e}: a is combined away");
                assert_eq!(m.proto_vertex(pv).unwrap().vertex_definitions(), &[b]);
                assert_eq!(def.tolerance(&k), cover, "{x:e}: grown to cover the line");
                assert_eq!(def.absorbed_hull(), &hull[..], "{x:e}: b holds a's hull");
            }
        }
    }

    /// #18312 (3): a proto vertex that combines a definition away (#16637) may not grow its
    /// tolerance into another proto vertex's definition that its tolerance before kept
    /// clear of. As in `a_resolved_proto_vertex_covers_every_point_it_absorbed`, the proto
    /// vertex at `p` (tolerance 1e-5) absorbs `a`, 3e-5 away, and would grow to 3e-5. A
    /// definition of another proto vertex 3.5e-5 from `p` with tolerance 1e-5 is clear of
    /// the old tolerance (2e-5 < 3.5e-5) but reached by the grown one (4e-5 >= 3.5e-5): the
    /// resolution is refused for the Boolean and the proto vertex keeps its position and
    /// tolerance. One 5e-5 away stays clear (4e-5 < 5e-5): the tolerance grows.
    #[test]
    fn a_placement_whose_cover_would_reach_another_definition_is_refused() {
        for (dx, refused) in [(3.5e-5, true), (5e-5, false)] {
            let k = Kernel::default();
            let geo = Geo::default();
            let mut m = manager();
            let p = Point3::new(1.0, 2.0, 3.0);
            let mk = |m: &mut ProtoTopologyManager<H>, at: Point3, a: H, b: H, gap: f64| {
                let vd = VertexDefinition::new(
                    &k,
                    &geo,
                    &region_pc(a),
                    &region_pc(b),
                    at,
                    1e-5,
                    gap,
                    None,
                )
                .unwrap();
                m.insert_vertex_definition(vd).unwrap()
            };
            let far = p + Vec3::new(3e-5, 0.0, 0.0);
            let a = mk(&mut m, far, R0, R1, 2e-6);
            let b = mk(&mut m, p, R0, R1, 1e-6);
            let c = mk(&mut m, p, R0, L0, 1e-6);
            let pv = m.new_proto_vertex(p, 1e-5).unwrap();
            m.add_proto_vertex(pv);
            for vd in [a, b, c] {
                m.add_vertex_definition(pv, vd).unwrap();
            }
            let (pv2, other) =
                another_vertex_at(&k, &mut m, &geo, p - Vec3::new(dx, 0.0, 0.0), 1e-5);

            let result = m.resolve_proto_vertex(&k, &geo, pv);
            let refusal = m.take_self_loop_refusal();
            assert_eq!(m.proto_vertex(pv2).unwrap().vertex_definitions(), &[other]);
            let v = m.proto_vertex(pv).unwrap();
            assert_eq!(v.position(), p);
            if refused {
                let err = result.unwrap_err();
                assert!(err.to_string().contains("another vertex"), "{err}");
                assert!(refusal.is_some(), "kept for the Boolean");
                assert_eq!(v.raw_tolerance(), 1e-5, "not grown");
                assert!(v.absorbed_points().is_empty(), "nothing recorded");
            } else {
                result.unwrap();
                assert!(refusal.is_none(), "{dx:e}");
                assert_eq!(v.vertex_definitions(), &[b, c]);
                assert_eq!(v.raw_tolerance(), far.distance(p) * (1.0 + gamma(4)));
            }
        }
    }

    /// #18312: a distance that overflows (an exact distance above `√MAX`, about 1.34e154)
    /// still decides the growth test. A vertex 2e154 away along each of x and y (its
    /// squared distance overflows) with tolerance 1e154 is reached by a ball grown to 2e154,
    /// not by one grown to 1, and is clear of the ball before.
    #[test]
    fn a_growth_test_whose_distance_overflows_still_decides() {
        let o = Point3::new(0.0, 0.0, 0.0);
        let far = Point3::new(2e154, 2e154, 0.0);
        assert_eq!(o.distance(far), f64::INFINITY);
        assert!(grown_into(o, 1e-4, o, 2e154, far, 1e154));
        assert!(!grown_into(o, 1e-4, o, 1.0, far, 1e154));
        let nan = Point3::new(f64::NAN, 0.0, 0.0);
        assert!(!grown_into(o, 1e-4, o, 2e154, nan, 1.0));
    }

    #[test]
    fn proto_edges_create_add_combine_resolve() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let c0 = H(0, ClassObjectKind::Curve);
        let mut m = manager();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(2.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(v0);
        m.add_proto_vertex(v1);

        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(c0), false);
        let ivl = geo.curves[0].natural_interval();
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);

        let pe = m
            .create_proto_edge(&k, &mut geo, Some(v0), Some(v1), &cc, &ci1, &cc, &ci2)
            .unwrap();
        assert_eq!(m.proto_edges(), &[pe]);
        assert_eq!(m.proto_vertex(v0).unwrap().proto_edges(), &[pe]);
        assert_eq!(geo.curves.len(), 2);
        let eds = m.proto_edge(pe).unwrap().edge_definitions().to_vec();
        assert_eq!(eds.len(), 1);

        // the same interval pair matches; adding it again changes nothing
        assert_eq!(
            m.find_matching_edge_definition(&k, &geo, pe, &cc, &ci1, &ci2)
                .unwrap(),
            Some(eds[0])
        );
        m.add_edge_definition_from_intervals(&k, &mut geo, pe, &cc, &ci1, &cc, &ci2)
            .unwrap();
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);

        // another source pair adds a definition; resolving keeps both (not connected)
        let mut ci3 = ci2;
        ci3.mid = region_pc(L0);
        m.add_edge_definition_from_intervals(&k, &mut geo, pe, &cc, &ci1, &cc, &ci3)
            .unwrap();
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 2);
        assert!(m.resolve_proto_edge(&k, &mut geo, pe).unwrap().is_empty());

        // a second proto edge between the same vertices is found and combined
        let pe2 = m
            .create_proto_edge(&k, &mut geo, Some(v1), Some(v0), &cc, &ci1, &cc, &ci2)
            .unwrap();
        let (found, identical) = m
            .find_proto_edge(
                &k,
                &geo,
                Some(v0),
                Some(v1),
                &ci1.mid,
                &ci2.mid,
                Some(c0),
                &ivl,
            )
            .unwrap();
        assert_eq!(found, Some(pe));
        assert!(identical);
        // the other proto edge's coincident proto face moves to pe
        let pf = m.new_proto_face(None, None, &[]).unwrap();
        m.add_proto_face(pf);
        m.relate_coincident_pf(pe2, pf).unwrap();
        m.combine_proto_edge(&k, &mut geo, pe, pe2).unwrap();
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 3);
        assert_eq!(m.proto_face(pf).unwrap().coinc_proto_edges(), &[pe]);
        assert_eq!(m.proto_edge(pe).unwrap().coinc_proto_faces(), &[pf]);
        assert!(m.proto_edge(pe2).unwrap().coinc_proto_faces().is_empty());

        // removing the only definition of an edge deletes it
        let e2 = m.proto_edge(pe2).unwrap().edge_definitions().to_vec();
        assert!(e2.is_empty());
        let pe3 = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        m.add_proto_edge(pe3);
        // listed at its ends, as `CreateProtoEdge` lists a proto edge: removing it unlists
        // it there, and an end that does not list it is an error (passed on, bcc6f01)
        for v in [v0, v1] {
            m.proto_vertex_mut(v).unwrap().add_proto_edge(pe3);
        }
        let last = *m.proto_edge(pe).unwrap().edge_definitions().last().unwrap();
        m.add_edge_definition(&k, &mut geo, pe3, last).unwrap();
        assert_eq!(m.edge_def(last).unwrap().proto_edge(), Some(pe3));
        m.remove_edge_definition(last, true).unwrap();
        assert!(m.proto_edge(pe3).is_none());
    }

    #[test]
    fn find_or_create_and_create_proto_vertex() {
        let k = Kernel::default();
        let geo = Geo::default();
        let mut m = manager();
        let (pv, created) = m
            .find_or_create_proto_vertex(&k, Point3::new(1.0, 0.0, 0.0))
            .unwrap();
        assert!(created);
        assert_eq!(m.proto_vertices(), &[pv]);
        // the same position finds the existing proto vertex
        assert_eq!(
            m.find_or_create_proto_vertex(&k, Point3::new(1.0, 0.0, 0.0))
                .unwrap(),
            (pv, false)
        );
        assert_eq!(m.proto_vertices(), &[pv]);

        // curves cannot be imprinted
        let mut pc1 = PointClassification::new(1e-5);
        pc1.set_class_object(PointClassType::Curve, Some(H(0, ClassObjectKind::Curve)));
        let mut pc2 = region_pc(R0);
        assert_eq!(
            m.create_proto_vertex(&k, &geo, &mut pc1, &mut pc2).unwrap(),
            None
        );
        // regions have no object point: undefined upstream; `sm_FindPositionFromPCs`'s
        // status, passed on (bcc6f01), names it (main refused with an internal
        // "no position found")
        let mut pc1 = region_pc(R1);
        let e = m
            .create_proto_vertex(&k, &geo, &mut pc1, &mut pc2)
            .unwrap_err();
        assert_eq!(
            e.topology_failure(),
            Some(&TopologyFailure::ClassificationParametersUnset),
            "{e}"
        );
    }

    #[test]
    fn update_edges_moves_definitions_to_the_new_edge() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let ivl = geo.curves[0].natural_interval();
        let (a, b) = (ivl.min(), ivl.max());
        let mid = ivl.mid();
        let e_old = H(20, ClassObjectKind::Edge);
        let e_new = H(21, ClassObjectKind::Edge);
        geo.edge_ivl
            .push((20, Extent1d::from_min_max(a, mid), 1e-6, 0));
        geo.edge_ivl
            .push((21, Extent1d::from_min_max(mid, b), 1e-6, 0));

        let mut m = manager();
        let mut pc = PointClassification::new(1e-5);
        pc.set_class_object(PointClassType::Edge, Some(e_old));
        pc.set_t_param(ivl.evaluate(0.75), false);
        let calling = m
            .insert_vertex_definition(
                VertexDefinition::new(
                    &k,
                    &geo,
                    &region_pc(R0),
                    &region_pc(R1),
                    Point3::new(1.0, 0.0, 0.0),
                    1e-5,
                    1e-6,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let moved = m
            .insert_vertex_definition(
                VertexDefinition::new(
                    &k,
                    &geo,
                    &pc,
                    &region_pc(R1),
                    Point3::new(1.5, 0.0, 0.0),
                    1e-5,
                    1e-6,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let pv = m
            .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(pv);
        m.add_vertex_definition(pv, calling).unwrap();
        m.add_vertex_definition(pv, moved).unwrap();

        // nothing to do without a split
        m.update_edges(&k, &mut geo, 1, Some(e_old), &[e_old], Some(calling))
            .unwrap();
        let e = m
            .update_edges(&k, &mut geo, 1, Some(e_old), &[e_new, e_new], Some(calling))
            .unwrap_err();
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR_NULL_POINTER));
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput, "{e}");
        assert_eq!(
            e.topology_failure(),
            Some(&TopologyFailure::OldEdgeNotAmongSplit)
        );
        m.update_edges(&k, &mut geo, 1, Some(e_old), &[e_old, e_new], Some(calling))
            .unwrap();
        assert_eq!(m.vertex_def(moved).unwrap().src_topo(1), Some(e_new));
    }

    #[test]
    fn edge_definition_split_starts_unowned() {
        // tangency-labs/solidcore#518: SmEdgeDefinition::Split's new EdgeDefinition
        // starts unowned, so adding it to the upper proto edge does not fail attempting
        // to remove it from the original proto edge.
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let c0 = H(0, ClassObjectKind::Curve);
        let mut m = manager();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(2.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(v0);
        m.add_proto_vertex(v1);

        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(c0), false);
        let ivl = geo.curves[0].natural_interval();
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);

        let pe = m
            .create_proto_edge(&k, &mut geo, Some(v0), Some(v1), &cc, &ci1, &cc, &ci2)
            .unwrap();
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);

        // Split the proto edge at its midpoint parameter
        let (new_pv, new_pe) = m.proto_edge_split(&k, &mut geo, pe, ivl.mid()).unwrap();
        let new_pe = new_pe.expect("proto edge was split");
        assert!(new_pv.is_some());

        // The lower proto edge retains its edge definition, and the upper proto edge
        // receives the new split edge definition without error.
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 1);
        assert_eq!(m.proto_edge(new_pe).unwrap().num_edge_definitions(), 1);
    }

    /// A split point that classifies on neither B-rep gets no vertex definition: the
    /// classification's failure (upstream ignores it; the port passes it on, as the fork
    /// does) is the split's error, and the definition keeps its curve.
    #[test]
    fn edge_definition_split_without_a_definition_names_its_cause() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let mut m = manager();
        let mut pv = |x: f64| {
            let v = m.new_proto_vertex(Point3::new(x, 0.0, 0.0), 1e-5).unwrap();
            m.add_proto_vertex(v);
            v
        };
        let (v0, v_mid, v1) = (pv(0.0), pv(1.0), pv(2.0));
        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(H(0, ClassObjectKind::Curve)), false);
        let ivl = geo.curves[0].natural_interval();
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);
        let pe = m
            .create_proto_edge(&k, &mut geo, Some(v0), Some(v1), &cc, &ci1, &cc, &ci2)
            .unwrap();
        let ed = m.proto_edge(pe).unwrap().edge_definitions()[0];
        let before = m.edge_def(ed).unwrap().clone();

        geo.classify_fails = true;
        let e = m
            .edge_definition_split(&k, &mut geo, ed, ivl.mid(), Some(v_mid), None)
            .unwrap_err();
        // the classification's own error (main read the unclassified point and refused
        // with `SplitDefinitionNotAdded`, caused by `ClassificationOffTopology`)
        assert_eq!(e.operation(), "classify", "{e}");
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR), "{e}");
        assert!(!e.is_defect(), "{e}");
        assert_eq!(m.edge_def(ed).unwrap(), &before);
        assert!(m
            .proto_vertex(v_mid)
            .unwrap()
            .vertex_definitions()
            .is_empty());
    }

    /// `SmEdgeDefinition::Split` hangs the new upper definition on the upper proto edge
    /// (`AddEdgeDefinition`), which reparameterizes it when that edge's interval is not the
    /// definition's. Upstream's new definition still held the lower one's whole curve
    /// then, untrimmed: the whole line was mapped onto the upper proto edge, so the upper
    /// definition ran from x = 0, not from the split point (and where the lower
    /// definition's curve was the one moved, its trim missed it: `SM_ERR`). Each
    /// definition has its own curve, trimmed before it is added: x = 0 to 0.5 below the
    /// split, 0.5 to 2 above it.
    #[test]
    fn edge_definition_split_moves_only_its_own_curve() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        geo.curves.push(line([0.5, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let mut m = manager();
        let mut pv = |x: f64| {
            let v = m.new_proto_vertex(Point3::new(x, 0.0, 0.0), 1e-5).unwrap();
            m.add_proto_vertex(v);
            v
        };
        let (v0, v_mid, v1) = (pv(0.0), pv(0.5), pv(2.0));
        let mut edge = |c: usize, from, to| {
            let mut cc = CurveClassification::new(&k);
            cc.set_curve(Some(H(c, ClassObjectKind::Curve)), false);
            let ivl = geo.curves[c].natural_interval();
            let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci1.mid = region_pc(R0);
            let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
            ci2.mid = region_pc(R1);
            m.create_proto_edge(&k, &mut geo, Some(from), Some(to), &cc, &ci1, &cc, &ci2)
                .unwrap()
        };
        // the whole line over [0, 1], and the upper proto edge over [0, 1] on its own
        // line from the split point: the split's upper part, [0.25, 1], is not its interval
        let lower = edge(0, v0, v1);
        let upper = edge(1, v_mid, v1);
        let ed = m.proto_edge(lower).unwrap().edge_definitions()[0];

        let split = m.edge_definition_split(&k, &mut geo, ed, 0.25, Some(v_mid), Some(upper));
        let new_ed = split.unwrap().1.expect("the definition was split");
        let ends = |ed| {
            let d = m.edge_def(ed).unwrap();
            let c = geo.curve(d.xsect_curve_3d().unwrap()).unwrap();
            let (ivl, nat) = (d.xsect_interval(), c.natural_interval());
            assert!(nat.min() <= ivl.min() + 1e-12 && ivl.max() <= nat.max() + 1e-12);
            [ivl.min(), ivl.max()].map(|t| c.evaluate_point(&k, t).unwrap().x)
        };
        let near =
            |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12;
        assert!(near(ends(ed), [0.0, 0.5]), "{:?}", ends(ed));
        // upstream mapped the whole untrimmed line onto the upper proto edge: [0, 2]
        assert!(near(ends(new_ed), [0.5, 2.0]), "{:?}", ends(new_ed));
    }

    /// WR-101: the upper split child's interval. A split goes ahead only where an edge
    /// definition splits too, and that definition gives the upper child `[dT, old_max]`;
    /// the proto edge's own upper interval is `[dT, old_max]` too (upstream's pinned
    /// `[dT, dT]` was only left on a child no definition split onto, a split now
    /// refused). With two definitions, one splitting and one refusing (too near its
    /// end for its larger tolerance), the upper child gets
    /// the one that splits and still `[dT, old_max]`; the lower child is `[old_min, dT]`
    /// and keeps both. A split at or within the tolerance of an end is refused.
    #[test]
    fn proto_edge_split_upper_interval_keeps_the_old_max() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let c0 = H(0, ClassObjectKind::Curve);
        let mut m = manager();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(2.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(v0);
        m.add_proto_vertex(v1);

        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(c0), false);
        let ivl = geo.curves[0].natural_interval();
        let (old_min, old_max) = (ivl.min(), ivl.max());
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = region_pc(R0);
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);

        let pe = m
            .create_proto_edge(&k, &mut geo, Some(v0), Some(v1), &cc, &ci1, &cc, &ci2)
            .unwrap();
        let tol = m.proto_edge(pe).unwrap().raw_tolerance();

        // the two refusal guards: at the ends and within the tolerance of them
        for at in [old_min, old_min + tol / 4.0, old_max - tol / 4.0, old_max] {
            assert!(matches!(
                m.proto_edge_split(&k, &mut geo, pe, at),
                Ok((None, None))
            ));
        }

        // Near the end, within 10*tol, the edge definition refuses to split (upstream's
        // `cbiFudge`), so the proto edge does not split either: it would leave an upper
        // child with no definitions.
        let near_end = old_max - 2.0 * tol;
        assert!(matches!(
            m.proto_edge_split(&k, &mut geo, pe, near_end),
            Ok((None, None))
        ));
        assert_eq!(m.proto_edges(), [pe]);
        let whole = m.proto_edge(pe).unwrap().xsect_interval();
        assert_eq!((whole.min(), whole.max()), (old_min, old_max));

        // A second definition, of ten times the tolerance, refuses within 1e-3 of the end.
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let c1 = H(geo.curves.len() - 1, ClassObjectKind::Curve);
        let wide = m
            .insert_edge_definition(EdgeDefinition::new(
                Some(c1),
                ivl,
                R0,
                R1,
                10.0 * tol,
                None,
                None,
                None,
            ))
            .unwrap();
        m.add_edge_definition(&k, &mut geo, pe, wide).unwrap();

        // 2e-4 from the end the first definition splits and the wide one refuses.
        let dt = old_max - 20.0 * tol;
        let (new_pv, new_pe) = m.proto_edge_split(&k, &mut geo, pe, dt).unwrap();
        let new_pe = new_pe.expect("proto edge was split");
        assert!(new_pv.is_some());
        assert_eq!(m.proto_edge(new_pe).unwrap().num_edge_definitions(), 1);
        assert!(!m
            .proto_edge(new_pe)
            .unwrap()
            .edge_definitions()
            .contains(&wide));
        assert_eq!(m.proto_edge(pe).unwrap().num_edge_definitions(), 2);

        let lower = m.proto_edge(pe).unwrap().xsect_interval();
        assert_eq!((lower.min(), lower.max()), (old_min, dt));
        let upper = m.proto_edge(new_pe).unwrap().xsect_interval();
        assert_eq!((upper.min(), upper.max()), (dt, old_max));
    }

    /// WR-533 (the fork's tangency-labs/smlib@f11d0ac): an edge definition along the
    /// whole of an edge that is split in its middle is split there too, each part on its
    /// own piece (upstream keeps the whole definition on the old edge, its midpoint at
    /// the split point).
    #[test]
    fn update_edges_splits_a_definition_across_the_split_point() {
        let k = Kernel::default();
        let mut geo = Geo::default();
        geo.curves.push(line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]));
        let c0 = H(0, ClassObjectKind::Curve);
        let ivl = geo.curves[0].natural_interval();
        let (a, b, mid) = (ivl.min(), ivl.max(), ivl.mid());
        let e_old = H(20, ClassObjectKind::Edge);
        let e_new = H(21, ClassObjectKind::Edge);
        geo.edge_ivl
            .push((20, Extent1d::from_min_max(a, mid), 1e-6, 0));
        geo.edge_ivl
            .push((21, Extent1d::from_min_max(mid, b), 1e-6, 0));

        let mut m = manager();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(2.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(v0);
        m.add_proto_vertex(v1);

        // the definition along the whole curve, on the old edge in B-rep 1
        let mut cc = CurveClassification::new(&k);
        cc.set_curve(Some(c0), false);
        let mut on_edge = PointClassification::new(1e-5);
        on_edge.set_class_object(PointClassType::Edge, Some(e_old));
        on_edge.set_t_param(mid, false);
        let mut ci1 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci1.mid = on_edge;
        let mut ci2 = CurveInterval::with_interval(&k, &ivl, Some(&cc));
        ci2.mid = region_pc(R1);
        let pe = m
            .create_proto_edge(&k, &mut geo, Some(v0), Some(v1), &cc, &ci1, &cc, &ci2)
            .unwrap();
        let ed = m.proto_edge(pe).unwrap().edge_definitions()[0];
        assert_eq!(m.edge_def(ed).unwrap().src_topo(1), Some(e_old));

        // the vertex definition that split the edge at (1, 0, 0)
        let calling = m
            .insert_vertex_definition(
                VertexDefinition::new(
                    &k,
                    &geo,
                    &region_pc(R0),
                    &region_pc(R1),
                    Point3::new(1.0, 0.0, 0.0),
                    1e-5,
                    1e-6,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let pv = m
            .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
            .unwrap();
        m.add_proto_vertex(pv);
        m.add_vertex_definition(pv, calling).unwrap();

        m.update_edges(&k, &mut geo, 1, Some(e_old), &[e_old, e_new], Some(calling))
            .unwrap();
        let mut on: Vec<Option<H>> = m
            .all_edge_definitions()
            .unwrap()
            .into_iter()
            .map(|d| m.edge_def(d).unwrap().src_topo(1))
            .collect();
        on.sort_by_key(|h| h.map(|h| h.0));
        assert_eq!(on, [Some(e_old), Some(e_new)]);
    }

    /// WR-95 (fixed in v1.0.1 `3fd5b95`): `SmProtoEdge::CombineProtoEdge` walks a copy
    /// of the other proto edge's definitions list so all definitions (including odd-indexed
    /// ones) move into `pe1` and none remain on `pe2`.
    #[test]
    fn wr95_combine_proto_edge_moves_all_definitions() {
        let k = Kernel::default();
        let mut geo = Geo {
            curves: vec![
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ],
            ..Geo::default()
        };
        let mut m = manager();
        let ivl = geo.curves[0].natural_interval();
        let v0 = m
            .new_proto_vertex(Point3::new(0.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let v1 = m
            .new_proto_vertex(Point3::new(1.0, 0.0, 0.0), 1e-5)
            .unwrap();
        let pe1 = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();
        let pe2 = m.new_proto_edge(&k, Some(v0), Some(v1), ivl).unwrap();

        let ed0 = m
            .insert_edge_definition(EdgeDefinition::new(
                Some(H(0, ClassObjectKind::Curve)),
                ivl,
                R0,
                R1,
                1e-5,
                None,
                None,
                Some(pe2),
            ))
            .unwrap();
        let ed1 = m
            .insert_edge_definition(EdgeDefinition::new(
                Some(H(1, ClassObjectKind::Curve)),
                ivl,
                R0,
                R1,
                1e-5,
                None,
                None,
                Some(pe2),
            ))
            .unwrap();
        let ed2 = m
            .insert_edge_definition(EdgeDefinition::new(
                Some(H(2, ClassObjectKind::Curve)),
                ivl,
                R0,
                R1,
                1e-5,
                None,
                None,
                Some(pe2),
            ))
            .unwrap();

        m.proto_edge_mut(pe2)
            .unwrap()
            .edge_definitions_mut()
            .push(ed0);
        m.proto_edge_mut(pe2)
            .unwrap()
            .edge_definitions_mut()
            .push(ed1);
        m.proto_edge_mut(pe2)
            .unwrap()
            .edge_definitions_mut()
            .push(ed2);

        m.combine_proto_edge(&k, &mut geo, pe1, pe2).unwrap();

        assert_eq!(
            m.proto_edge(pe1).unwrap().edge_definitions(),
            &[ed0, ed1, ed2]
        );
        assert_eq!(m.proto_edge(pe2).unwrap().edge_definitions(), &[]);
    }

    /// A degree-1 B-spline over `[0, 1]` with 17 control points at uniform knots: the even
    /// ones (the points at `t = i / 8`) are `vertex`, the odd ones `vertex + bump`.
    fn zigzag(vertex: Point3, bump: Vec3) -> Curve {
        use crate::nurbs::b_spline_curve::{
            BSplineCurve, BSplineCurveForm, CanonicalData, KnotType,
        };
        let points: Vec<Point3> = (0..17)
            .map(|i| if i % 2 == 0 { vertex } else { vertex + bump })
            .collect();
        let knots: Vec<f64> = (0..17).map(|i| f64::from(i) / 16.0).collect();
        let mut mults = vec![1; 17];
        mults[0] = 2;
        mults[16] = 2;
        Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 1,
                control_points: &points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &mults,
                knots: &knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        )
    }

    /// WR-619 review: a closed curve on a vertex that is back on it at every one of the nine
    /// parameters the speck test once sampled but leaves the vertex's tolerance in between is
    /// no speck: the enclosure over its control points refuses it, so it is imprinted.
    #[test]
    fn a_curve_leaving_the_vertex_between_samples_is_no_speck() {
        let k = Kernel::default();
        let vertex = Point3::new(1.0, 2.0, 3.0);
        let tol = 1e-5;
        let curve = zigzag(vertex, Vec3::new(0.0, 100.0 * tol, 0.0));
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        for i in 0..9 {
            let pt = curve.evaluate_point(&k, f64::from(i) / 8.0).unwrap();
            assert!(pt.distance(vertex) <= tol, "sample {i} is on the vertex");
        }
        let between = curve.evaluate_point(&k, 1.0 / 16.0).unwrap();
        assert!(between.distance(vertex) > 50.0 * tol);
        assert_eq!(speck_on_vertex(&k, &curve, ivl, vertex, tol).unwrap(), None);
    }

    /// A speck whose control points all lie within the tolerance is proved one, and the gap
    /// it reports bounds the curve's distance from the vertex.
    #[test]
    fn a_speck_within_tolerance_is_proved() {
        let k = Kernel::default();
        let vertex = Point3::new(1.0, 2.0, 3.0);
        let tol = 1e-5;
        let curve = zigzag(vertex, Vec3::new(0.0, 0.5 * tol, 0.0));
        let ivl = Extent1d::from_min_max(0.0, 1.0);
        let (gap, p0, p1) = speck_on_vertex(&k, &curve, ivl, vertex, tol)
            .unwrap()
            .unwrap();
        // the bump is 0.5 tol, give or take the rounding of `vertex + bump`
        assert!(gap >= 0.499 * tol && gap <= 0.51 * tol, "gap {gap}");
        assert!(p0.distance(vertex) <= tol && p1.distance(vertex) <= tol);
        for i in 0..=64 {
            let pt = curve.evaluate_point(&k, f64::from(i) / 64.0).unwrap();
            assert!(pt.distance(vertex) <= gap);
        }
    }
}
