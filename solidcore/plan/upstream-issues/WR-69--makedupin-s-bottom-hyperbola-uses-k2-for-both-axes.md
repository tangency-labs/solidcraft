#### WR-69. `MakeDupin`'s bottom hyperbola uses `k2` for both axes
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmAdvSurfaceIntersector.cpp:1120`-`1122` `SmAdvSurfaceIntersector::MakeDupin`.
- **What happens:** The four hyperbolas of a saddle point should be one indicatrix branch pair `(1/sqrt|k1|, 1/sqrt|k2|)` and its conjugate pair `(1/sqrt|k2|, 1/sqrt|k1|)`. The top hyperbola uses the latter, the bottom one `(1/sqrt|k2|, 1/sqrt|k2|)`, so the bottom curve is a different conic and the singular directions found from it are wrong unless `|k1| == |k2|`.
- **Trigger:** `MakeDupin` (or `ComputeSingularityPoint`) at a hyperbolic point with `|k1| != |k2|`.
- **How found:** read during porting.
- **Confirmed:** yes (`intersect.adv_surface_intersector` difftest: generated `dupin` ops and `edge_dupin_kinds` compare the conics).
- **Severity:** wrong result.
- **Rust port:** Faithful.
