#### WR-65. `SmVolume` "simple" box samplers and inverse solvers mix simple and compound maps
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmVolume.cpp:5865` `EvaluateBoundingBoxSimple` and `:6057` `EvaluatePseudoBoxSimple` (sample `EvaluatePoint` and `Evaluate1stDerivatives` with compounding); `:919` `IsPointInParamDomain` (passes the last out-space image to the next map's in-space check); `:6857` `LocalPointSolveSimple` (solution value is the distance to the compound image); `:6665` `InvEvaluatePointSimple` (a failed local solve forces `rbSuccess = FALSE` after a successful global solve); `:3411` `InvEvaluatePoint`, `:3907` `InvMapPoint` (always report success); `:4512` `GlobalPointSolve` (found flag of the last point only).
- **What happens:** The projection-space boxes of a compound volume are boxes of its last out-space; domain checks and solution values use the wrong space; success flags are lost or always set.
- **Trigger:** `EvaluatePseudoBoxSimple` of a volume with a next map; `IsPointInParamDomain` of a chain with a plain `SmVolume` second map.
- **How found:** read during porting.
- **Confirmed:** yes (surface.volume_p2 difftest: the Rust port reproduces these and matches the C++ on generated cases and the `twist_then_base_*` fixtures).
