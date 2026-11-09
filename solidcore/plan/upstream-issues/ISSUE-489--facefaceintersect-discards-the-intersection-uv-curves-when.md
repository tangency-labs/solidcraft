#### Issue #489. `FaceFaceIntersect` discards the intersection UV curves when inserting edges
- **Severity:** Wrong result
- **Area:**

- **Location:** `source/SMLib/src/SmTopologyIntersector.cpp:8246,8294`, the two `MakeEdgeInFace` calls in `SmTopologyIntersector::FaceFaceIntersect`.
- **What happens:** each face's classification retains the intersection UV curve, but edge insertion receives `NULL` instead. `SmBrep::MakeEdgeInFaceWithVU` projects the 3D curve when no UV trim is supplied, adding another approximation.
- **Trigger:** a new edge inserted by this face/face path with a current intersection UV curve available. Issue #489 reports 2–4e-6 gaps on threaded-shaft flank/shaft edges (the #474 reproduction).
- **How found:** issue #489 from SolidCraft's `kernel-smlib` regression rig; confirmed the explicit-null calls by reading the pinned source.
- **Confirmed:** source confirmed; the gap magnitude and complete shaft operation have not been reproduced with this harness's C++ reference.
- **Severity:** accuracy loss from an additional projection fit.
- **Rust port:** copies and forwards the classification's current UV curve in `TopologyIntersector::make_edge_for_interval`; absent/stale UVs retain projection. The path still defers at insertion adapters, so end-to-end gap elimination is not certified (see `plan/findings.md`, issue #489).
