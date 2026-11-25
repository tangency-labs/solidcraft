#### UB-179. `SmCone::ConvertUVFromSTEPToNURBS` dereferences a missing polar curve
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmCone.cpp:1808`, called by `IntersectWithCone` at `:2966-2967`; `source/SMLib/src/SmEllipse.cpp:491` (`SmPolarConversion::ConvertToNURBSParameter`). Construction ignores `RebuildPolarConverter`'s status at `SmSurfOfRevolution.cpp:1033`.
- **What happens:** A degenerate sweep can leave the polar converter without a circle even though `MakeNurb` returns success. STEP-to-NURBS conversion unconditionally reads `m_cpCurve->GetSTEPInterval()` and segfaults. `HavePolarConversion()` alone does not identify this: a converter with a circle but no exact table can still perform a defined numerical conversion.
- **Trigger:** Issue #207: equal-radius cones, first origin `(0,0,0)`, X `(1,0,0)`, Y `(0,-1e-300,0)`, radius `1.5628`, height `1.10780`, inside-out; second origin `(0,0.5539,0.5539)`, X `(1,0,0)`, Y `(0,0,1)`, same radius/height; full sweeps, tolerance `1e-7`, domains `[0,0,1,1, 0,0.125,1,0.875]`. Unitizing the tiny Y axis uses the `(0,0,1)` fallback, and the sweep iso curves used to build the converter are degenerate. Either operand can have the missing converter; direct cylinder intersection also reaches this call.
- **How found:** crash in the C++ reference fuzz run reported in issue #207; traced through cone construction and the virtual parameter-conversion call.
- **Confirmed:** yes (issue #207 reports native exit 139; guarded native probes confirm successful construction with a null polar curve and stop at the conversion without executing the null dereference).
- **Severity:** crash/UB.
- **Rust port:** Missing polar state returns an error. The differential adapter rejects only an attempted STEP-to-NURBS conversion with a null polar curve; self-intersection and disjoint early returns stay callable.
