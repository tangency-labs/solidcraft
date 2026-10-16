//! FFI wrappers for unit `topology.proto_topology_p1`.
//!
//! Shim: `harness/cpp/shim/topology_proto_topology_p1.cpp` (`sc_topology_proto_topology_p1_run`).
//! Upstream: `source/SMLib/src/SmOwningTopology.cpp`, `SmAssembly.cpp`,
//! `SmAssemblyInstance.cpp`, `SmCopyBrepMap.cpp`, `SmProtoTopology.cpp` (and their headers),
//! `source/SM_API/src/SmApiGeneral.cpp`.
//!
//! A script is `kind body`:
//!
//! * **list** (`0 n kind*n n_ops (op a b c d)*n_ops`): a pool of `n` slots, each an owning
//!   topology object (`0`, a bare `SmOwningTopology`), an `SmAssembly` (`1`) or an empty
//!   instance slot (`2`, filled by [`list_op::INST_NEW`]), and one `SmCopyBrepMap`. Ids are
//!   slot indices (`-1` null, `-2` unknown, `-3` destroyed). After the ops the state is
//!   dumped: per slot `0` (dead / empty) or `1 owner next last list size`, then the map's
//!   original and copy B-reps.
//! * **proto** (`1 n_obj obj*n_obj approx_tol capture override n_ops op*n_ops`): a pool of
//!   objects (`0` a curve block as in unit `intersect.curve_class_p1`, `1 O[3] N[3]` an
//!   `SmPlane`, `2` a B-spline surface block, `3` a vertex, `4 c` an edge on pool curve `c`,
//!   `5 s` a face on pool surface `s`, `6` a region stand-in, `7` a loop stand-in, `8 c a b
//!   tol` an edge on pool curve `c` with interval `[a, b]` and zone tolerance `tol` (pool
//!   kind 4), `9 tol` a vertex with zone tolerance `tol` (pool kind 3), `10 P[3] tol` a vertex
//!   with a point and a zone tolerance (pool kind 3)), a
//!   manager `SmProtoTopologyManager(brep1, brep2, ti, capture, override)` whose topology
//!   intersector has approximation tolerance `approx_tol`, and the [`proto_op`] ops. Proto
//!   objects are numbered per kind in creation order. A point classification `pc` is
//!   `class obj tol flags t u v gap` (flags: 1 `SetTParam(t)`, 2 `SetUVParam(u, v)`, 4
//!   `SetGap3d(gap)`). The final state is documented on [`proto_op`].
//! * **transform** (`2 object op args`): one object (`0 owned curve-block`, `1 owned O N`
//!   plane, `2 owned surface-block`, `3` an empty `SmBrep`, `4` an empty `SmPolyBrep`, `5`
//!   another topology object, `6` null, `7 owned n curve-block*n` an `SmCompositeCurve` of 1
//!   to 4 curves, `8 owned surface-block a0 a1 b0 b1` an `SmCrvOnSurf` UV line on a
//!   B-spline surface) and one API call: `0 O X Y` `SmApiTransform`, `1 S`
//!   `SmApiScale`, `2 R S` `SmApiScaleByPt`, `3 V` `SmApiTranslate`, `4 R A deg`
//!   `SmApiRotate`. Output: the status, then for a curve its natural interval and 5 points
//!   (`status [P]`), for a surface its natural domain and 3x3 points (nothing more for kinds 3
//!   to 8).
//!
//! Each list or proto op writes `op status n outputs*n`. An op the shim refuses because it
//! is undefined upstream (a dangling or null pointer dereference, a double delete, a crash
//! of the bare topology stand-ins, a list that would not terminate, or any op leaving a
//! live object pointing at a deleted one) writes `op -2` ([`SKIPPED`]) and ends the output.
//!
//! [`run`] accepts only well-formed scripts with bounded finite data (counts, codes, curve
//! and surface blocks); the run-time state checks are the shim's.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_proto_topology_p1_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The status an op refused by the shim writes (its output then ends).
pub const SKIPPED: f64 = -2.0;

/// Script kinds.
pub mod kind {
    /// Owning topology, assembly and copy-map script.
    pub const LIST: i64 = 0;
    /// Proto topology script.
    pub const PROTO: i64 = 1;
    /// One API transformation.
    pub const TRANSFORM: i64 = 2;
}

/// List-script slot kinds.
pub mod slot {
    /// A bare `SmOwningTopology`.
    pub const OWNING: i64 = 0;
    /// An `SmAssembly`.
    pub const ASSEMBLY: i64 = 1;
    /// An empty instance slot.
    pub const INSTANCE: i64 = 2;
}

/// List-script ops (`op a b c d`; unused arguments are 0).
///
/// | code | op | upstream | outputs |
/// |------|----|----------|---------|
/// | 0, 1 | `owner obj` | `PreInsert`, `PostInsert` | |
/// | 2 | `owner ins cur` | `InsertAfter(ins, cur)` | |
/// | 3 | `owner obj` | `Remove` | |
/// | 4 | `owner obj` | `IsInList` | `flag index` |
/// | 5 | `owner attr target` | `GetAll` (`attr` `-1` none) | `n id*n` |
/// | 6 | `owner` | `UpdateListSize` | |
/// | 7 | `owner` | `ReverseList` | |
/// | 8 | `owner obj` | `SetList` | |
/// | 9 | `obj` | the slot's destructor | |
/// | 10 | `asm` | `SmAssembly::GetAssemblyInstances` | `n id*n` |
/// | 11 | `asm inst` | `SmAssembly::RemoveInstance` | |
/// | 12 | `slot asm component mirrored` | `SmAssemblyInstance(..)` | `component mirrored` |
/// | 13 | `inst` | `SmAssemblyInstance::GetParentAssembly` | `id` |
/// | 14 | `copy orig` | `SmCopyBrepMap::SetAt` (both live: the release `SM_ASSERT`s call `GetType()`) | |
/// | 15 | `copy` | `SmCopyBrepMap::GetAt` (null, or set to a live original) | `id` |
/// | 16 | `orig copy` | `SmCopyBrepMap::SetBreps` | |
/// | 17 | `obj id` | `AddAttribute(SmAttribute(id, SM_AB_STANDALONE_COPY))` | |
/// | 18 | `obj` | `GetType`, `IsKindOf` (the probe types), `GetTypeString`, `GetClassString` | `type flag*11 n char*n n char*n` |
/// | 19 | `owner` | `GetSize`, `GetList` | `size id` |
pub mod list_op {
    #![allow(missing_docs)]
    pub const PRE_INSERT: i64 = 0;
    pub const POST_INSERT: i64 = 1;
    pub const INSERT_AFTER: i64 = 2;
    pub const REMOVE: i64 = 3;
    pub const IS_IN_LIST: i64 = 4;
    pub const GET_ALL: i64 = 5;
    pub const UPDATE_SIZE: i64 = 6;
    pub const REVERSE: i64 = 7;
    pub const SET_LIST: i64 = 8;
    pub const DESTROY: i64 = 9;
    pub const ASM_INSTANCES: i64 = 10;
    pub const ASM_REMOVE_INSTANCE: i64 = 11;
    pub const INST_NEW: i64 = 12;
    pub const INST_PARENT: i64 = 13;
    pub const MAP_SET: i64 = 14;
    pub const MAP_GET: i64 = 15;
    pub const MAP_BREPS: i64 = 16;
    pub const ADD_ATTR: i64 = 17;
    pub const TYPE: i64 = 18;
    pub const GET_SIZE: i64 = 19;
    pub const LAST: i64 = 19;
}

/// The probe types of the type queries (`IsKindOf` is answered for each, in order).
pub const PROBE_TYPES: [i64; 11] = [
    11001, 12060, 16001, 16010, 16050, 16055, 16056, 16011, 16680, 16002, 0,
];

/// Proto-script pool object kinds (and the input codes `EDGE_IVL`, `VERTEX_TOL` that make
/// an edge or vertex with extra data).
pub mod pool {
    #![allow(missing_docs)]
    pub const CURVE: i64 = 0;
    pub const PLANE: i64 = 1;
    pub const BSURF: i64 = 2;
    pub const VERTEX: i64 = 3;
    pub const EDGE: i64 = 4;
    pub const FACE: i64 = 5;
    pub const REGION: i64 = 6;
    pub const LOOP: i64 = 7;
    pub const EDGE_IVL: i64 = 8;
    pub const VERTEX_TOL: i64 = 9;
    /// Input only: a vertex with a point and a zone tolerance (`10 P[3] tol`).
    pub const VERTEX_AT: i64 = 10;
}

/// Proto-script ops (arguments after the code; `pv`, `pe`, `ed`, `vd`, `pf` are proto ids,
/// `obj`, `c`, `f` pool indices, `-1` null; `P` three numbers; `pc` a point
/// classification; `mode` 1 position, 2 tangent, 3 both).
///
/// | code | args | upstream | outputs |
/// |------|------|----------|---------|
/// | 0 | `P tol` | `SmProtoVertex(P, tol, mgr)` | `pv` |
/// | 1 | `pc pc P tol` | `SmProtoVertex(pc1, pc2, P, tol, mgr)` | `pv vd` |
/// | 2 | `pc pc P tol gap pv` | `SmVertexDefinition(ctx, pc1, pc2, P, tol, gap, pv)` | `vd` |
/// | 3 | `pv pv a b` | `SmProtoEdge(start, end, [a, b], mgr)` | `pe` |
/// | 4 | `pe` | `SmProtoEdge(const SmProtoEdge&)` | `pe` |
/// | 5 | `c a b obj obj tol c c pe` | `SmEdgeDefinition(c3, [a, b], src1, src2, tol, uv1, uv2, pe)` | `ed` |
/// | 6 | `ed` | `SmEdgeDefinition(const SmEdgeDefinition&)` | `ed` |
/// | 7, 8 | `f f n pe*n` | `SmProtoFace(..)`, `CreateProtoFace` | `pf` |
/// | 9, 10, 11 | `pv` / `pe` / `pf` | `AddProtoVertex`, `AddProtoEdge`, `AddProtoFace` | |
/// | 12 | | `Reset` | |
/// | 13 | `pv pe` | `SmProtoVertex::AddProtoEdge` | |
/// | 14 | `pv vd` | `SmProtoVertex::AddVertexDefinition(vd)` | |
/// | 15 | `pe ed` | `GetEdgeDefinitions().Add(ed)` | |
/// | 16 | `pe pe` | `AddOverlappingPE` | |
/// | 17 | `pe n pe*n` | `AppendOverlappingPEs` | |
/// | 18, 19 | `pe` / `pv` | `ClearEdgeDefinitions`, `ClearVertexDefinitions` | |
/// | 20, 21 | `pe pv` | `SetStartProtoVertex`, `SetEndProtoVertex` | |
/// | 22 | `pe a b` | `SmProtoEdge::SetXSectInterval` | |
/// | 23 | `pe pv pv` | `ReplaceProtoVertex` | |
/// | 24, 25 | `pe ed` / `pe pv` | `SmProtoEdge::RemoveEdgeDefinition`, `RemoveProtoVertex` | |
/// | 26, 27 | `pv vd` / `pv pe` | `SmProtoVertex::RemoveVertexDefinition`, `RemoveProtoEdge` | |
/// | 28 | `pe pf` | `SmProtoEdge::RelateCoincidentPF` | |
/// | 29 | `pf which obj` | `SmProtoFace::SetSrcTopo` | |
/// | 30 | `vd which class obj` | `SmVertexDefinition::SetSrcTopo` | |
/// | 31 | `vd which obj` | `SetFinalVertex` | |
/// | 32, 33 | `vd P` / `vd pv` | `SetPosition`, `SetProtoVertex` | |
/// | 34, 35, 36 | `ed c` / `ed which c` / `ed pe` | `SetXSectCurve3d`, `SetXSectCurveUV`, `SetProtoEdge` | |
/// | 37 | `ed a b` | `SetXSectInterval(const SmExtent1d&)` | |
/// | 38 | `ed which obj` | `SmEdgeDefinition::SetSrcTopo` | |
/// | 39 | `ed which obj` | `m_pFinalEdge1` / `2` = obj | |
/// | 40 | `vd flag` | `SmProtoTopologyManager::RemoveVertexDefinition` | |
/// | 41, 42 | `pv` / `pe` | `RemoveProtoVertex`, `RemoveProtoEdge` | |
/// | 43, 44 | `v` | `SetTolerance`, `SetCaptureDistance` | |
/// | 45 | `pv pv` | `CombineProtoVertex` | |
/// | 46 | `ed` | `Destruct` | `n c*n` (curves deleted) |
/// | 47, 48, 49, 50 | `pe` / `pv` / `vd` / `ed` | `delete` (destructors) | `n c*n` for 47, 50 |
/// | 51 | | `delete mgr` | `n c*n` |
/// | 52 | `vd` | `SmVertexDefinition::Resolve` | `riStatus P` |
/// | 53 | `vd` | `SetPointParameters` | |
/// | 54 | `vd` | `GetNumProtoEdges` | `n` |
/// | 55, 56 | `ed t mode` / `ed mode` | `SmEdgeDefinition::Evaluate`, `EvaluateMid` | `status [P] [T]` |
/// | 57 | `ed at_start` | `GetEndPosition(SmBoolean, ..)` | `P` |
/// | 58 | `ed pv` | `GetEndPosition(SmProtoVertex*, ..)` | `[P]` |
/// | 59 | `ed` | `ReverseOrientation` | |
/// | 60 | `ed a b reverse` | `Reparameterize` (refused for `a == b`: HANG-12) | |
/// | 61 | `ed a b` | `SetXSectInterval(double, double)` | |
/// | 62 | `ed pv pv pc pc c a b` | `IsSameEdgeDefinition` | `same identical` |
/// | 63 | `ed P tol at_start` | `IsInteriorPoint` | `flag param` |
/// | 64 | `ed` | `GetStartProtoVertex`, `GetEndProtoVertex` | `pv pv` |
/// | 65, 66 | `pe at_start` / `pe pv` | `SmProtoEdge::GetEndPosition` | `[P]` |
/// | 67, 68 | `pe t mode` / `pe mode` | `SmProtoEdge::Evaluate`, `EvaluateMid` | `status [P] [T]` |
/// | 69 | `pe pv pv pc pc c a b` | `IsSameProtoEdge` | `same identical` |
/// | 70 | `pe` | `GetOverlappingPEGroup` | `n pe*n` |
/// | 71 | `pv obj obj` | `FindMatchingVertexDefinition` | `vd` |
/// | 72 | `pv P` | `GetClosestVertexDefinition` | `vd` |
/// | 73 | `pv which ed at_start` | `SmProtoVertex::GetFinalVertex` | `obj` |
/// | 74 | `P` | `FindProtoVertex` | `pv dist` |
/// | 75 | `obj which` | `FindEdgeDefinitionFromEdge` | `ed` |
/// | 76, 77 | | `GetAllVertexDefinitions`, `GetAllEdgeDefinitions` | `n id*n` |
/// | 78 | `c tol a b pc pc` | `CheckOverlappingProtoEdges` (a classification of curve `c`, source tolerance `tol`, its interval `[a, b]` with midpoint `pc1`; the other interval's midpoint `pc2`) | `n pe*n` |
/// | 79 | `ed ed` | `CanCombineEdgeDefinitions` | `choice` |
/// | 80 | `pe pv` | `RemoveCoincidentNoEdgeProtoVertex` | `flag` |
/// | 81 | `pe t pv` | `sm_WhichPVToCombine` | `pv` |
/// | 82 | `pc pc` | `sm_FindPositionFromPCs` | `P dev pc pc` |
/// | 83 | `c a b c a b tol` | `sm_AreSameCurve` | `flag` |
/// | 84 | `c a b t tol` | `sm_IsInterior` | `flag` |
/// | 85 | `obj obj` | `sm_AreConnected` | `flag` |
/// | 86 | `obj` | `sm_TopoDimension` | `dim` |
/// | 87 | `class` | `sm_CanImprint` | `flag` |
/// | 88 | `c tol a b pc ed` | `sm_AreOverlapping` (interval as in 78) | `flag` |
/// | 89 | | the manager's type queries (as list op 18) | |
/// | 90 | | `GetWhichBrep(1) == brep1`, `GetWhichBrep(2) == brep2`, `GetTolerance`, `GetCaptureDistance` | 4 values |
/// | 120 | `pc pc` | `FindOrCreateProtoVertex(pc1, pc2, ..)` | `pv created` |
/// | 121 | `c tol a b pc*6` | `FindOrCreateProtoEdge` (interval `[a, b]` of a classification of curve `c` with source tolerance `tol`; start, mid and end classifications of the first, then of the second interval) | `pe created` |
/// | 122 | `pe t` | `SmProtoEdge::Split` | `pv pe` |
/// | 123 | `ed t pv pe` | `SmEdgeDefinition::Split` | `vd ed` |
/// | 124 | `pe pe` | `SplitIntersectingProtoEdges` | `n c*n` (curves deleted, sorted) |
/// | 125 | | `GlueCoincidentBrepVertices` (refused when it would glue) | `n c*n` |
/// | 126 | | `ResolveOverlappingProtoEdges` (refused for edge sources) | `n c*n` |
///
/// Objects the p3 ops create are registered (numbered) after the op, as the p2 ops do.
///
/// Ops 91 to 108 belong to unit `topology.proto_topology_p2` ([`proto_op_p2`]).
///
/// A point classification state is `class obj t u v param_set deviation src_tol`. The final
/// state is `alive` (the manager), then when alive `tol capture n pv*n n pe*n n pf*n`; then
/// per vertex definition `0` or `1 pc pc P tol gap pv final1 final2 src1 src2 type1 type2`,
/// per edge definition `0` or `1 c3 uv1 uv2 min max src1 src2 type1 type2 pe tol final1
/// final2`, per proto vertex `0` or `1 P tol n vd*n n pe*n`, per proto edge `0` or `1 min max
/// start end tol n ed*n n pe*n n pf*n`, per proto face `0` or `1 face1 face2 type n pe*n`,
/// and per pool curve `0` (deleted) or `1 min max (status [P])*5`.
pub mod proto_op {
    #![allow(missing_docs)]
    pub const PV_NEW: i64 = 0;
    pub const PV_NEW_PCS: i64 = 1;
    pub const VD_NEW: i64 = 2;
    pub const PE_NEW: i64 = 3;
    pub const PE_COPY: i64 = 4;
    pub const ED_NEW: i64 = 5;
    pub const ED_COPY: i64 = 6;
    pub const PF_NEW: i64 = 7;
    pub const PF_CREATE: i64 = 8;
    pub const MGR_ADD_PV: i64 = 9;
    pub const MGR_ADD_PE: i64 = 10;
    pub const MGR_ADD_PF: i64 = 11;
    pub const MGR_RESET: i64 = 12;
    pub const PV_ADD_PE: i64 = 13;
    pub const PV_ADD_VD: i64 = 14;
    pub const PE_ADD_ED: i64 = 15;
    pub const PE_ADD_OVERLAP: i64 = 16;
    pub const PE_APPEND_OVERLAPS: i64 = 17;
    pub const PE_CLEAR_EDS: i64 = 18;
    pub const PV_CLEAR_VDS: i64 = 19;
    pub const PE_SET_START: i64 = 20;
    pub const PE_SET_END: i64 = 21;
    pub const PE_SET_IVL: i64 = 22;
    pub const PE_REPLACE_PV: i64 = 23;
    pub const PE_REMOVE_ED: i64 = 24;
    pub const PE_REMOVE_PV: i64 = 25;
    pub const PV_REMOVE_VD: i64 = 26;
    pub const PV_REMOVE_PE: i64 = 27;
    pub const PE_RELATE_PF: i64 = 28;
    pub const PF_SET_SRC: i64 = 29;
    pub const VD_SET_SRC: i64 = 30;
    pub const VD_SET_FINAL: i64 = 31;
    pub const VD_SET_POS: i64 = 32;
    pub const VD_SET_PV: i64 = 33;
    pub const ED_SET_C3: i64 = 34;
    pub const ED_SET_UV: i64 = 35;
    pub const ED_SET_PE: i64 = 36;
    pub const ED_SET_IVL: i64 = 37;
    pub const ED_SET_SRC: i64 = 38;
    pub const ED_SET_FINAL: i64 = 39;
    pub const MGR_REMOVE_VD: i64 = 40;
    pub const MGR_REMOVE_PV: i64 = 41;
    pub const MGR_REMOVE_PE: i64 = 42;
    pub const MGR_SET_TOL: i64 = 43;
    pub const MGR_SET_CAPTURE: i64 = 44;
    pub const PV_COMBINE: i64 = 45;
    pub const ED_DESTRUCT: i64 = 46;
    pub const DELETE_PE: i64 = 47;
    pub const DELETE_PV: i64 = 48;
    pub const DELETE_VD: i64 = 49;
    pub const DELETE_ED: i64 = 50;
    pub const MGR_DESTROY: i64 = 51;
    pub const VD_RESOLVE: i64 = 52;
    pub const VD_SET_POINT_PARAMS: i64 = 53;
    pub const VD_NUM_PES: i64 = 54;
    pub const ED_EVAL: i64 = 55;
    pub const ED_EVAL_MID: i64 = 56;
    pub const ED_END_POS: i64 = 57;
    pub const ED_END_POS_PV: i64 = 58;
    pub const ED_REVERSE: i64 = 59;
    pub const ED_REPARAM: i64 = 60;
    pub const ED_SET_IVL_MINMAX: i64 = 61;
    pub const ED_IS_SAME: i64 = 62;
    pub const ED_IS_INTERIOR: i64 = 63;
    pub const ED_START_END_PV: i64 = 64;
    pub const PE_END_POS: i64 = 65;
    pub const PE_END_POS_PV: i64 = 66;
    pub const PE_EVAL: i64 = 67;
    pub const PE_EVAL_MID: i64 = 68;
    pub const PE_IS_SAME: i64 = 69;
    pub const PE_GROUP: i64 = 70;
    pub const PV_FIND_MATCH: i64 = 71;
    pub const PV_CLOSEST: i64 = 72;
    pub const PV_FINAL_VTX: i64 = 73;
    pub const MGR_FIND_PV: i64 = 74;
    pub const MGR_FIND_ED_FROM_EDGE: i64 = 75;
    pub const MGR_ALL_VDS: i64 = 76;
    pub const MGR_ALL_EDS: i64 = 77;
    pub const MGR_CHECK_OVERLAP: i64 = 78;
    pub const MGR_CAN_COMBINE: i64 = 79;
    pub const MGR_REMOVE_NOEDGE_PV: i64 = 80;
    pub const WHICH_PV: i64 = 81;
    pub const FIND_POS: i64 = 82;
    pub const SAME_CURVE: i64 = 83;
    pub const IS_INTERIOR: i64 = 84;
    pub const ARE_CONNECTED: i64 = 85;
    pub const TOPO_DIM: i64 = 86;
    pub const CAN_IMPRINT: i64 = 87;
    pub const ARE_OVERLAPPING: i64 = 88;
    pub const MGR_TYPE: i64 = 89;
    pub const MGR_GET: i64 = 90;
    pub const FOC_PV: i64 = 120;
    pub const FOC_PE: i64 = 121;
    pub const PE_SPLIT: i64 = 122;
    pub const ED_SPLIT: i64 = 123;
    pub const SPLIT_INT: i64 = 124;
    pub const GLUE: i64 = 125;
    pub const RESOLVE: i64 = 126;
    pub const LAST: i64 = 126;
}

/// Proto-script ops of unit `topology.proto_topology_p2` (shapes as in [`proto_op`]; `ci`
/// is a curve interval `c uv tol a b pc`: a curve classification of pool curve `c` with first
/// UV curve `uv` and source zone tolerance `tol`, its interval `[a, b]` with midpoint
/// classification `pc`). Objects a call makes are numbered after the op in the order the
/// port makes them.
///
/// | code | args | upstream | outputs |
/// |------|------|----------|---------|
/// | 91 | `vd` | `SmVertexDefinition::Imprint` | `final1 final2` |
/// | 92 | `ed` | `SmEdgeDefinition::Imprint` | `final1 final2` |
/// | 93 | `pv` | `SmProtoVertex::Resolve` | |
/// | 94 | `pv pc pc` | `SmProtoVertex::AddVertexDefinition(rPC1, rPC2, ..)` | `vd created pc pc` |
/// | 95 | `pe ci ci` | `SmProtoEdge::FindMatchingEdgeDefinition` | `ed rev` |
/// | 96 | `pe ed` | `SmProtoEdge::AddEdgeDefinition(SmEdgeDefinition*)` | |
/// | 97 | `pe ci ci` | `SmProtoEdge::AddEdgeDefinition(crCI1, crCI2)` | |
/// | 98 | `pe pe` | `SmProtoEdge::CombineProtoEdge` | |
/// | 99 | `pe` | `SmProtoEdge::Resolve` | `n c*n` (curves deleted, sorted) |
/// | 100 | `pc pc` | `SmProtoTopologyManager::CreateVertexDefinition` | `vd pc pc` |
/// | 101 | `pc pc` | `SmProtoTopologyManager::CreateProtoVertex` | `pv pc pc` |
/// | 102 | `P` | `FindOrCreateProtoVertex` | `pv created` |
/// | 103 | `pv pv pc pc c a b` | `FindProtoEdge` | `pe identical` |
/// | 104 | `pv pv ci ci` | `CreateProtoEdge` | `pe` (`-1` on an error return) |
/// | 105 | `ed flag` | `SmProtoTopologyManager::RemoveEdgeDefinition` | |
/// | 106 | `which edge n edge*n vd` | `UpdateEdges` (`which` 1 or 2, `n` 0 to 4) | |
/// | 107 | `which f f e e ed` | `UpdateFaces` (always refused: bare faces) | |
/// | 108 | `vd vd` | `CanCombineVertexDefinitions` | `choice` |
pub mod proto_op_p2 {
    #![allow(missing_docs)]
    pub const VD_IMPRINT: i64 = 91;
    pub const ED_IMPRINT: i64 = 92;
    pub const PV_RESOLVE: i64 = 93;
    pub const PV_ADD_VD_PCS: i64 = 94;
    pub const PE_FIND_MATCH: i64 = 95;
    pub const PE_ADD_ED: i64 = 96;
    pub const PE_ADD_ED_CIS: i64 = 97;
    pub const PE_COMBINE: i64 = 98;
    pub const PE_RESOLVE: i64 = 99;
    pub const MGR_CREATE_VD: i64 = 100;
    pub const MGR_CREATE_PV: i64 = 101;
    pub const MGR_FIND_OR_CREATE_PV: i64 = 102;
    pub const MGR_FIND_PE: i64 = 103;
    pub const MGR_CREATE_PE: i64 = 104;
    pub const MGR_REMOVE_ED: i64 = 105;
    pub const MGR_UPDATE_EDGES: i64 = 106;
    pub const MGR_UPDATE_FACES: i64 = 107;
    pub const MGR_CAN_COMBINE_VDS: i64 = 108;
    pub const FIRST: i64 = 91;
    pub const LAST: i64 = 108;
}

/// Transform-script object kinds.
pub mod tobj {
    #![allow(missing_docs)]
    pub const CURVE: i64 = 0;
    pub const PLANE: i64 = 1;
    pub const BSURF: i64 = 2;
    pub const BREP: i64 = 3;
    pub const POLY: i64 = 4;
    pub const OTHER: i64 = 5;
    pub const NULL: i64 = 6;
    pub const COMPOSITE: i64 = 7;
    pub const CRV_ON_SURF: i64 = 8;
}

const MAX_SLOTS: i64 = 32;
const MAX_OPS: i64 = 256;
const MAX_OBJECTS: i64 = 64;
const MAX_ID: f64 = 1.0e6;
const MAX_DEGREE: i64 = 9;
const MAX_POINTS: i64 = 64;
const MAX_LIST: i64 = 16;
/// NLib's non-rational weight marker `NL_NOW`.
const NOW: f64 = -f64::MAX;

struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(what.into())
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = *self.input.get(self.at).ok_or_else(|| bad("script ends early"))?;
        self.at += 1;
        Ok(v)
    }
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if v.is_finite() {
            Ok(v)
        } else {
            Err(bad("non-finite number"))
        }
    }
    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }
    /// An integer in `lo..=hi`.
    fn int(&mut self, lo: i64, hi: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad("integer out of range"));
        }
        Ok(v as i64)
    }
    /// An id (`-1` null) the shim range-checks at run time.
    fn id(&mut self) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || !(-1.0..=MAX_ID).contains(&v) {
            return Err(bad("id out of range"));
        }
        Ok(v as i64)
    }
    fn flag(&mut self) -> Result<(), FfiError> {
        self.int(0, 1).map(|_| ())
    }
    /// `n p m cpts(x y z w) knots` with `n + p + 1 == m`, positive weights, nondecreasing
    /// knots.
    fn nurb(&mut self) -> Result<(), FfiError> {
        let n = self.int(1, MAX_POINTS)?;
        let p = self.int(1, MAX_DEGREE)?;
        let m = self.int(0, MAX_POINTS + MAX_DEGREE + 1)?;
        if n + p + 1 != m || p > n {
            return Err(bad("nurb counts"));
        }
        for _ in 0..=n {
            self.vec3()?;
            let w = self.finite()?;
            if w <= 0.0 && w != NOW {
                return Err(bad("weight"));
            }
        }
        let mut prev = f64::NEG_INFINITY;
        for _ in 0..=m {
            let u = self.finite()?;
            if u < prev {
                return Err(bad("knots"));
            }
            prev = u;
        }
        Ok(())
    }
    /// A curve block `kind dim ...`.
    fn curve(&mut self) -> Result<(), FfiError> {
        let k = self.int(0, 4)?;
        let dim = self.int(2, 3)?;
        match k {
            0 => {
                self.flag()?;
                self.nurb()
            }
            1 => {
                for _ in 0..4 {
                    self.vec3()?;
                }
                Ok(())
            }
            2 => {
                if dim != 3 {
                    return Err(bad("line dim"));
                }
                self.vec3()?;
                self.nonzero3()?;
                self.finite()?;
                self.finite()?;
                let scale = self.finite()?;
                if scale <= 0.0 {
                    return Err(bad("line scale"));
                }
                self.flag()
            }
            _ => {
                if dim != 3 {
                    return Err(bad("conic dim"));
                }
                self.vec3()?;
                self.nonzero3()?;
                self.nonzero3()?;
                self.finite()?;
                self.finite()?;
                let rx = self.finite()?;
                if rx <= 0.0 {
                    return Err(bad("radius"));
                }
                if k == 3 && self.finite()? <= 0.0 {
                    return Err(bad("radius"));
                }
                self.flag()
            }
        }
    }
    fn nonzero3(&mut self) -> Result<(), FfiError> {
        let x = self.finite()?;
        let y = self.finite()?;
        let z = self.finite()?;
        if x == 0.0 && y == 0.0 && z == 0.0 {
            return Err(bad("zero vector"));
        }
        Ok(())
    }
    /// `oob n m p q r s cpts(x y z w) u v`.
    fn surface(&mut self) -> Result<(), FfiError> {
        self.flag()?;
        let n = self.int(1, 16)?;
        let m = self.int(1, 16)?;
        let p = self.int(1, MAX_DEGREE)?;
        let q = self.int(1, MAX_DEGREE)?;
        let r = self.int(0, 32)?;
        let s = self.int(0, 32)?;
        if n + p + 1 != r || m + q + 1 != s || p > n || q > m {
            return Err(bad("surface counts"));
        }
        for _ in 0..(n + 1) * (m + 1) {
            self.vec3()?;
            let w = self.finite()?;
            if w <= 0.0 && w != NOW {
                return Err(bad("weight"));
            }
        }
        for count in [r, s] {
            let mut prev = f64::NEG_INFINITY;
            for _ in 0..=count {
                let u = self.finite()?;
                if u < prev {
                    return Err(bad("knots"));
                }
                prev = u;
            }
        }
        Ok(())
    }
    fn pc(&mut self) -> Result<(), FfiError> {
        self.int(100, 110)?;
        self.id()?;
        self.finite()?;
        self.int(0, 7)?;
        for _ in 0..4 {
            self.finite()?;
        }
        Ok(())
    }
    /// A curve interval `c uv tol a b pc`.
    fn ci(&mut self) -> Result<(), FfiError> {
        self.id()?;
        self.id()?;
        self.finite()?;
        self.finite()?;
        self.finite()?;
        self.pc()
    }
    fn ids(&mut self) -> Result<(), FfiError> {
        let n = self.int(0, MAX_LIST)?;
        for _ in 0..n {
            self.id()?;
        }
        Ok(())
    }
}

fn validate_list(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.int(0, MAX_SLOTS)?;
    for _ in 0..n {
        rd.int(0, 2)?;
    }
    let n_ops = rd.int(0, MAX_OPS)?;
    for _ in 0..n_ops {
        rd.int(0, list_op::LAST)?;
        for _ in 0..4 {
            rd.id()?;
        }
    }
    Ok(())
}

/// The argument shape of an op of unit `topology.proto_topology_p2` (as in
/// `validate_proto`; `C` a curve interval, `L` up to 4 ids after their count).
fn p2_shape(code: i64) -> Option<&'static str> {
    use proto_op_p2 as q;
    Some(match code {
        q::VD_IMPRINT | q::ED_IMPRINT | q::PV_RESOLVE | q::PE_RESOLVE => "i",
        q::PV_ADD_VD_PCS => "icc",
        q::PE_FIND_MATCH | q::PE_ADD_ED_CIS => "iCC",
        q::PE_ADD_ED | q::PE_COMBINE | q::MGR_REMOVE_ED | q::MGR_CAN_COMBINE_VDS => "ii",
        q::MGR_CREATE_VD | q::MGR_CREATE_PV => "cc",
        q::MGR_FIND_OR_CREATE_PV => "3",
        q::MGR_FIND_PE => "iicciff",
        q::MGR_CREATE_PE => "iiCC",
        q::MGR_UPDATE_EDGES => "iiLi",
        q::MGR_UPDATE_FACES => "iiiiii",
        _ => return None,
    })
}

fn validate_proto(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    use proto_op as p;
    let n_obj = rd.int(0, MAX_OBJECTS)?;
    for _ in 0..n_obj {
        match rd.int(0, pool::VERTEX_AT)? {
            pool::CURVE => rd.curve()?,
            pool::PLANE => {
                rd.vec3()?;
                rd.nonzero3()?;
            }
            pool::BSURF => rd.surface()?,
            pool::EDGE | pool::FACE => {
                rd.id()?;
            }
            pool::VERTEX_AT => {
                rd.vec3()?;
                rd.finite()?;
            }
            pool::EDGE_IVL => {
                rd.id()?;
                rd.finite()?;
                rd.finite()?;
                rd.finite()?;
            }
            pool::VERTEX_TOL => {
                rd.finite()?;
            }
            _ => {}
        }
    }
    rd.finite()?;
    rd.finite()?;
    rd.finite()?;
    let n_ops = rd.int(0, MAX_OPS)?;
    for _ in 0..n_ops {
        let code = rd.int(0, p::LAST)?;
        // argument shapes: i id, f finite, 3 point, c pc, l id list
        let shape: &str = match code {
            p::PV_NEW => "3f",
            p::PV_NEW_PCS => "cc3f",
            p::VD_NEW => "cc3ffi",
            p::PE_NEW => "iiff",
            p::PE_COPY
            | p::ED_COPY
            | p::MGR_ADD_PV
            | p::MGR_ADD_PE
            | p::MGR_ADD_PF
            | p::PE_CLEAR_EDS
            | p::PV_CLEAR_VDS
            | p::MGR_REMOVE_PV
            | p::MGR_REMOVE_PE
            | p::ED_DESTRUCT
            | p::DELETE_PE
            | p::DELETE_PV
            | p::DELETE_VD
            | p::DELETE_ED
            | p::VD_RESOLVE
            | p::VD_SET_POINT_PARAMS
            | p::VD_NUM_PES
            | p::ED_REVERSE
            | p::ED_START_END_PV
            | p::PE_GROUP
            | p::TOPO_DIM
            | p::CAN_IMPRINT => "i",
            p::ED_NEW => "iffiifiii",
            p::PF_NEW | p::PF_CREATE => "iil",
            p::MGR_RESET
            | p::MGR_DESTROY
            | p::MGR_ALL_VDS
            | p::MGR_ALL_EDS
            | p::MGR_TYPE
            | p::MGR_GET
            | p::GLUE
            | p::RESOLVE => "",
            p::PV_ADD_PE
            | p::PV_ADD_VD
            | p::PE_ADD_ED
            | p::PE_ADD_OVERLAP
            | p::PE_SET_START
            | p::PE_SET_END
            | p::PE_REMOVE_ED
            | p::PE_REMOVE_PV
            | p::PV_REMOVE_VD
            | p::PV_REMOVE_PE
            | p::PE_RELATE_PF
            | p::VD_SET_PV
            | p::ED_SET_C3
            | p::ED_SET_PE
            | p::MGR_REMOVE_VD
            | p::PV_COMBINE
            | p::ED_END_POS
            | p::ED_END_POS_PV
            | p::PE_END_POS
            | p::PE_END_POS_PV
            | p::MGR_CAN_COMBINE
            | p::MGR_REMOVE_NOEDGE_PV
            | p::ARE_CONNECTED
            | p::MGR_FIND_ED_FROM_EDGE
            | p::ED_EVAL_MID
            | p::PE_EVAL_MID
            | p::SPLIT_INT => "ii",
            p::PE_APPEND_OVERLAPS => "il",
            p::PE_SET_IVL | p::ED_SET_IVL | p::ED_SET_IVL_MINMAX => "iff",
            p::PE_REPLACE_PV
            | p::PF_SET_SRC
            | p::VD_SET_FINAL
            | p::ED_SET_UV
            | p::ED_SET_SRC
            | p::ED_SET_FINAL
            | p::PV_FIND_MATCH => "iii",
            p::VD_SET_SRC | p::PV_FINAL_VTX => "iiii",
            p::VD_SET_POS | p::PV_CLOSEST => "i3",
            p::MGR_SET_TOL | p::MGR_SET_CAPTURE => "f",
            p::ED_EVAL | p::PE_EVAL | p::WHICH_PV => "ifi",
            p::ED_REPARAM => "iffi",
            p::ED_IS_SAME | p::PE_IS_SAME => "iiicciff",
            p::ED_IS_INTERIOR => "i3fi",
            p::MGR_FIND_PV => "3",
            p::MGR_CHECK_OVERLAP => "ifffcc",
            p::FIND_POS | p::FOC_PV => "cc",
            p::SAME_CURVE => "iffifff",
            p::IS_INTERIOR => "iffff",
            p::ARE_OVERLAPPING => "ifffci",
            p::FOC_PE => "ifffcccccc",
            p::PE_SPLIT => "if",
            p::ED_SPLIT => "ifii",
            _ => p2_shape(code).ok_or_else(|| bad("op"))?,
        };
        for ch in shape.chars() {
            match ch {
                'i' => {
                    rd.id()?;
                }
                'f' => {
                    rd.finite()?;
                }
                '3' => {
                    rd.vec3()?;
                }
                'c' => rd.pc()?,
                'C' => rd.ci()?,
                'l' => rd.ids()?,
                'L' => {
                    let n = rd.int(0, 4)?;
                    for _ in 0..n {
                        rd.id()?;
                    }
                }
                _ => return Err(bad("shape")),
            }
        }
    }
    Ok(())
}

fn validate_transform(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match rd.int(0, tobj::CRV_ON_SURF)? {
        tobj::COMPOSITE => {
            rd.flag()?;
            let n = rd.int(1, 4)?;
            for _ in 0..n {
                rd.curve()?;
            }
        }
        tobj::CRV_ON_SURF => {
            rd.flag()?;
            rd.surface()?;
            for _ in 0..4 {
                rd.finite()?;
            }
        }
        tobj::CURVE => {
            rd.flag()?;
            rd.curve()?;
        }
        tobj::PLANE => {
            rd.flag()?;
            rd.vec3()?;
            rd.nonzero3()?;
        }
        tobj::BSURF => {
            rd.flag()?;
            rd.surface()?;
        }
        _ => {}
    }
    match rd.int(0, 4)? {
        0 => {
            rd.vec3()?;
            let x = rd.vec3()?;
            let y = rd.vec3()?;
            // SetCanonical leaves the placement undefined for parallel or zero axes
            let c = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            let n = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let lim = 1e-6 * n(x) * n(y);
            if !matches!(n(c).partial_cmp(&lim), Some(core::cmp::Ordering::Greater)) {
                return Err(bad("placement axes"));
            }
        }
        1 | 3 => {
            rd.vec3()?;
        }
        2 => {
            rd.vec3()?;
            rd.vec3()?;
        }
        _ => {
            rd.vec3()?;
            rd.vec3()?;
            rd.finite()?;
        }
    }
    Ok(())
}

/// Checks the static shape of a script (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    match rd.int(0, kind::TRANSFORM)? {
        kind::LIST => validate_list(&mut rd)?,
        kind::PROTO => validate_proto(&mut rd)?,
        _ => validate_transform(&mut rd)?,
    }
    if rd.at != input.len() {
        return Err(bad("trailing data"));
    }
    Ok(())
}

/// Runs a script (see the module docs) and returns its output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script; [`FfiError::Status`] when the
/// shim rejects it.
pub fn run(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and is a validated script (every
        // count, code and curve or surface block the shim reads); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_topology_proto_topology_p1_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
