#### UB-170. `SmHCR::IntersectCurveWithSilhouettes` reads silhouette edges past the queried array [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmHCR.cpp:2121` `SmHCR::IntersectCurveWithSilhouettes`.
- **What happens:** The loop that copies the silhouette edges found in the curve's box is bounded by the size of the vertex query (`sObjects`) instead of the edge query (`sObjectsInBox`). With more silhouette vertices than edges in the box it reads past the edge array (unchecked `operator[]` in release) and uses the values as edges; with fewer it skips edges.
- **Trigger:** A curve whose box holds an open chain of silhouette edges (one more vertex than edges), for example any `ComputeGlobalVisibility` whose silhouettes are open curves.
- **How found:** read during porting (`blend.hcr`).
- **Confirmed:** no (the difftest shim skips the call when the vertex count exceeds the edge count; the skipped-edge behaviour is compared).
- **Severity:** crash/UB.
- **Rust port:** Reads the edges up to the vertex count as upstream and returns `InvalidInput` at the first index past the edge list.
