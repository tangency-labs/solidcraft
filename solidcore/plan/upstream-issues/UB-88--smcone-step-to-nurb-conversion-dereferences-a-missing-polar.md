#### UB-88. `SmCone` STEP-to-NURB conversion dereferences a missing polar curve
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmEllipse.cpp:489`, `SmPolarConversion::ConvertToNURBSParameter` (def. `:467`), reached from `SmCone.cpp:1808` `SmCone::ConvertUVFromSTEPToNURBS` and `:1682` `SmCone::CreateIsoCircleFromPoint`.
- **What happens:** `m_cpCurve->GetSTEPInterval()` is called without the `NER(m_cpCurve)` check that `ConvertToPolarParameter` has. A cone whose polar converter could not be set up has no curve; `SmSurfOfRevolution::MakeNurbWithSweepParams` ignores `RebuildPolarConverter`'s status, so `CreateCanonical` still succeeds.
- **Trigger:** `new SmCylinder(O, X, Y, -1.8, 0, 360, 4.3, FALSE, FALSE, FALSE)` (a negative radius) and `MakeNurb()`, which succeeds without polar curve, then `ConvertUVFromSTEPToNURBS`.
- **How found:** read during porting (`surface.cone_p1`).
- **Confirmed:** no. The shim refuses the op for a cone without polar curve.
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns `InvalidInput`.
