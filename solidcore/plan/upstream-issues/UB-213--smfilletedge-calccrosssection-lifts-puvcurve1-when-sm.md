#### UB-213. `SmFilletEdge::CalcCrossSection` lifts `*pUVCurve1` when `sm_CreateUVCurve` succeeds without any curve
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:5759-5768` (`SmFilletEdge::CalcCrossSection`): `SER(sm_CreateUVCurve(.., p3DCurve, pUVCurve1, pUVCurve2))`, then `if (p3DCurve == NULL) { SER(pSurface1->LiftCurve(.., *pUVCurve1, pUVCurve1->GetNaturalInterval(), ..)); NER(p3DCurve); }`.
- **What happens:** `NER(p3DCurve)` comes after the lift and nothing checks `pUVCurve1` before it. When `sm_CreateUVCurve` returns `SM_SUCCESS` with neither a 3D curve nor a UV curve (both out-pointers stay `NULL`), `*pUVCurve1` and `pUVCurve1->GetNaturalInterval()` dereference a null pointer.
- **Trigger:** a fillet edge for which `sm_CreateUVCurve` (`:4212`) returns `SM_SUCCESS` with `rp3DCurve` and `rpUVCurve1` both `NULL`, i.e. a success exit that builds no curve.
- **How found:** line-by-line comparison while porting `blend.fillet_geom_p5` (PR #264); the shim guards the call.
- **Confirmed:** no (source inspection; the difftest shim refuses the input rather than running it, and no generated input has been seen to reach it).
- **Severity:** UB (null dereference).
- **Rust port:** `FilletBrep::calc_cross_section` returns `SM_ERR_NULL_POINTER` at the lift (`blend/fillet_geom/p5.rs`, the `UB-213` comment). The `blend.fillet_geom_p5` shim runs the same `sm_CreateUVCurve` call first (`p5_cross_runs`) and refuses (`SC_ERR_ARG`) an input where it succeeds with no curve, so no run reaches the dereference.
