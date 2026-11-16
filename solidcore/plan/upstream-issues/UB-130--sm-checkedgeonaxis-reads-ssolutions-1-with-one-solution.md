#### UB-130. `sm_CheckEdgeOnAxis` reads `sSolutions[1]` with one solution
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmRotationalSweepGeometry.cpp:224`-`226` (`sm_CheckEdgeOnAxis`, file static), reached by `SmRotationalSweepGeometry::EdgeSweepHigher` and `EdgeSweepSame`.
- **What happens:** For a curved edge with both ends on the rotation axis the curve/axis intersection is tested with `nSolutions == 0 || nSolutions > 1 || sSolutions[1].m_eSolutionType == SM_ST_SINGLE_VALUE`. With exactly one solution `sSolutions[1]` is past the array's size (the release `operator[]` does not check). It lies inside the allocation (`SM_SOLUTION_ARRAY_MIN_SIZE` is 4, `smos_Calloc`), so it reads a zero-filled slot (`SM_ST_NODES_ONLY`) or a solution left behind by an earlier `RemoveAt` of the intersector. The single solution is then treated as a coincident range (index `0` was clearly meant).
- **Trigger:** a curved (not line-like) edge with both end vertices on the axis whose intersection with the axis is one solution (a curve partly running along the axis, a closed curve touching it at its seam).
- **How found:** read during porting (unit `sweep.rotational_sweep_geometry`).
- **Confirmed:** in part: the C++ build and the port agree on every such fixture (`nurb_zigzag_on_axis*`, `circle_touching_axis*`), consistent with a zero-filled slot there; a stale slot cannot be excluded in general.
- **Severity:** UB (out-of-range read; wrong result if the slot is stale).
- **Rust port:** reads a zero-filled slot (`SM_ST_NODES_ONLY`) deterministically (`sweep::rotational_sweep_geometry::check_edge_on_axis`).
