#### UB-97. `ComputeSingularityPoint` can write past `aSingDirections[8]`
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmAdvSurfaceIntersector.cpp:164` (`SmVector3d aSingDirections[8]`), filled by `sm_AddSingDir` (`:54`) from the Dupin indicatrix intersections at `:616`-`:685`; `aSingUVSurf1/2[8]` and `lInsideIndex[8]` at `:775`-`:776`.
- **What happens:** Every intersection of the above (and below) indicatrix curves of the two surfaces adds a direction unless one within `acos(0.9)` (about 26 degrees) is stored already. Up to 13 directions pass that test, but the array holds 8 and `sm_AddSingDir` does not check the count (upstream's own comment: "we have experienced examples with 8 vectors"). Saddle points generate four hyperbolas per surface, two of which are different conics (see WR-69), so the count is not bounded by the 4 + 4 of two central conics.
- **Trigger:** Two non-planar surface points with parallel normals, no shared principal curvature, whose indicatrices meet in more than 8 directions more than 26 degrees apart.
- **How found:** read during porting.
- **Confirmed:** no (the difftest shim predicts the count and does not run such inputs; no generated case reached it).
- **Severity:** crash/UB.
- **Rust port:** the array keeps 8 slots; a ninth direction is `InvalidInput`.
