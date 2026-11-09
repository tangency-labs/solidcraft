Updates: UB-241

#### UB-241 (update, 2026-11-01). Reproduced in the solidcore difftest: the #2174 sphere cap thickened outward
- **Location:** as UB-241. `SmProtoTopologyManager::GlueCoincidentBrepVertices` (`source/SMLib/src/SmProtoTopology.cpp:5902`); the faulting call is its Brep 1 common-edge loop gluing a long common edge (`GetWhichBrep(1)->GlueVertices( pBrep1Vtx1, pBrep1Vtx2)`, `:6112`) after an earlier pass of the same loop already deleted `pBrep1Vtx2`.
- **Trigger:** `SmApiShellBrepFull` thickening the r 10 sphere's face above z = 5, taken alone as a sheet, by +1 (outward). The flags are `create_offset_solid`, extended convex edges and merged results. Its rim is two half circles meeting at (8.66, 0, 5) and (-8.66, 0, 5) (tangency-labs/solidcore#2174; the sheet is `fixtures/offset.offset_executive_p2/data/issue2174_cap_two_rim_edges.smb`). The crash comes in the result merge: `SmOffsetExecutive::DoExtendedOffset` -> `PiecewiseMerge` -> `SmMerge::ManifoldBoolean` -> `SmTopologyIntersector::ImprintIntersections` -> `GlueCoincidentBrepVertices`. The same call inward (-1) runs to completion.
- **How found:** WR-673 (#2181) recorded the outward crash as a SIGSEGV in `SmApiShellBrepFull` with an untraced site. A one-off ASan build of the pinned reference (`-O1 -g -fsanitize=address -fno-omit-frame-pointer`, otherwise the harness's flags), running difftest case `issue2174_cap_two_rim_edges_api_shell_out`, gives:

  ```
  ERROR: AddressSanitizer: SEGV on unknown address 0x00000000074b (READ; zero page)
    #0 sm_RmFromSpatialTree(SmTopology*, SmBrep*)                  SmBrep.cpp:24028
    #1 SmBrep::GlueVertices(SmVertex*, SmVertex*, SmTArray<SmEdge*>*)
         (inlined SmBrep::Notify(SM_NO_MERGE_IN_BREP, ...)            SmBrep.cpp:24188)
                                                                   SmBrep.cpp:9623
    #2 SmProtoTopologyManager::GlueCoincidentBrepVertices()        SmProtoTopology.cpp:6112
    #3 SmTopologyIntersector::ImprintIntersections()               SmTopologyIntersector.cpp:2387
    #4 SmTopologyIntersector::IntersectInsertRelate()
    #5 SmMerge::ManifoldBoolean(SmBooleanOperationType, SmBrep*&, SmNewMarkAndLock*)
    #6 SmMerge::PiecewiseMerge(SmRelation<SmSurface, SmSurface>*, int, int, SmBrep*&)
    #7 SmOffsetExecutive::PiecewiseMerge(SmBrep*&)
    #8 SmOffsetExecutive::DoExtendedOffset(SmBrep*&)
    #9 SmOffsetExecutive::ShellBrep(SmContext const&, SmBrep*, double, int, int, int, ...)
    #10 SmApiShellBrepFull(SmBrep const*, double, int, int, int, SmTArray<SmFace const*> const&, SmBrep*&)
  ```

  ASan reports a wild read, not a heap-use-after-free. The deleted vertex's memory is owned by SMLib's own `SmObject::operator new` pool, which ASan does not track. `GetType()` on it reads a garbage vtable. The port confirms the path: on this input its UB-241 stop in `ProtoTopologyManager::glue_coincident_brep_vertices` (`crates/solidcore/src/topology/proto_topology/p3.rs`, before the second `GlueVertices`) fires once, for Brep 1 at vertex definition 1. That is exactly the frame-#2 call.
- **Confirmed:** yes, now also in the solidcore C++ reference run (pinned v1.0.1, x86-64 Linux, release harness: SIGSEGV; ASan build: the stack above).
- **Rust port:** not affected (it carries the fork's da8d05e stop unconditionally). At the defaults the outward thicken returns a B-rep that is not a manifold solid (4 faces, 8 edges, 5 vertices). This is the same upstream behaviour WR-673 records; `Kernel::fixes.offset_open_edge_second_trim_point` gives the sound sector. Regression test: `crates/solidcore/tests/issue_2174_cap_outward_ub_241.rs`.
- **Guard:** `offset.offset_executive_p2` runs API shells of file sheets with merged results through a crash trap (`solidcore_ffi::units::offset_offset_executive_p2::eval_in_child`, a fresh forked child). A child that crashes is `rejected` with this record named. Only calls that actually crash are rejected; the rest compare as before. The outward case `issue2174_cap_two_rim_edges_api_shell_out` is in `fixtures/offset.offset_executive_p2/issue2174.json`.
