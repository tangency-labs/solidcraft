#### WR-84. `SmEdgeuse::IsConvexRadialSector` reports a failed binormal evaluation as convex
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:3592` `SmEdgeuse::IsConvexRadialSector` (the two `SER(EvaluateBinormalStepOff(...))`).
- **What happens:** `SER` returns the non-zero `SmStatus` from a function returning `SmBoolean`, so a failing step-off evaluation (stepping off the face's UV domain, a degenerate binormal) makes the sector count as convex (`TRUE`), as in WR-52.
- **Trigger:** A sample whose step-off point leaves the face's UV domain (large edge tolerance with `bOptStepOff`, sliver faces), or an edge use whose binormal cannot be evaluated.
- **How found:** read during porting (unit `topology.edgeuse_p2`).
- **Confirmed:** no (the step-off path is deferred on `topology.edgeuse_p3`).
- **Severity:** wrong result.
- **Rust port:** Faithful (`Ok(true)` on such an error).
