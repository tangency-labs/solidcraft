#### UB-108. Public topology queries dereference edges whose curve was removed
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SM_API/src/SmApiQueries.cpp` (`SmApiGetClosestPoint`, `SmApiBrepClosestPoint`, `SmApiBrepClosestPointAll`, `SmApiBrepRelationship`, `SmApiBrepDistance`, `SmApiFindEdge`, `SmApiFindFaces`, `SmApiEdgeClosestPoint`, `SmApiFaceInternalPoint`, `SmApiFaceClassifyUV`, `SmApiFaceClosestPoint`, `SmApiBrepBoundingBox`, `SmApiFaceBoundingBox`) and their topology-query callees.
- **Trigger:** Construct a line/B-spline wire or a face-bearing B-rep, then clear an edge's geometry with `SetCurve(nullptr, TRUE, FALSE, FALSE)`. Solver/picker paths dereference the missing curve. Face closest point crashes on a box; interior-point/classification paths crash on vertex-loop/holed faces when they reach that boundary. Tight B-rep/face boxes also crash on a curved quadratic patch with a missing boundary curve.
- **How found:** PR #125 review, followed by 220 isolated subprocess probes of line/B-spline wires, boxes, vertex-loop and holed faces with both box modes, extended to 308 inputs to include planar and curved patches.
- **Confirmed:** yes; affected native processes terminated with SIGSEGV. After the FFI guards were expanded, 396 isolated probes completed without a crash or timeout (192 guarded, 204 native calls completed). Regression tests preserve defined missing-curve edge queries and null top-level inputs.
- **Severity:** crash.
