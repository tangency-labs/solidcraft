#### WR-604. `SmFilletGeom::Trim` drops trim and intersection errors (#483)
- **Severity:** Wrong result
- **Area:** Fillets and blends
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:10333`, `SmFilletGeom::Trim` at pinned v1.0.1 (`3fd5b95`).
- **What happens:** primary and other rail curve trims, edge-interval intersections, the center-line trim, the surface U-domain intersection and the surface trim all drop their statuses. An out-of-domain rail trim can leave the curve unchanged while its edge interval changes; a disjoint interval installs the unset extent `[1e20,-1e20]`. The method returns success and continues editing later geometry.
- **Trigger:** rail curve domain `[0,0.5]`, requested trim `[0.25,0.75]`; or a valid curve with edge interval `[2,3]` and the same trim.
- **How found:** adversarial fixtures in PR #674 (`blend.fillet_geom_p1`: `trim_failed_rail0`, `trim_disjoint_rail_interval`, and seven companion fixtures), with the native reproduction recorded in `plan/findings/blend.fillet_geom_p1--fillet-trim-status-483.md`.
- **Confirmed:** yes, the nine regression fixtures compare default dropped-status behavior against pinned v1.0.1 in the diagnostic native run described in the follow-up finding. The opt-in behavior is tested on the Rust side.
- **Rust policy:** default retains the defined pinned-C++ result. `Kernel::fixes.fillet_trim_status` is the default-off opt-in that returns the first trim/intersection error, preserving earlier successful edits and knot snapping. The other boolean, sweep and offset paths in #483 are separate work.
