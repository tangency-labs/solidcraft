#### UB-165. `SmBrep::MakeFaceWithCurves` with `pbOptCrvOnSurf` and no UV curves dereferences a null array
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp:16591-16606` `SmBrep::MakeFaceWithCurves` (the `pbOptCrvOnSurf && *pbOptCrvOnSurf == TRUE` block after `MakeFace`).
- **What happens:** The UV curve array `cpOptUVCurves` is optional (only one of the 3D and UV arrays must be given), but the curve-on-surface block copies `cpOptUVCurves->GetAt(ii)` for `ii < cpOptUVCurves->GetSize()` without a null check, so a call with 3D curves only and `*pbOptCrvOnSurf == TRUE` dereferences null as soon as `MakeFace` succeeds (even for one empty loop).
- **Trigger:** `MakeFaceWithCurves(region, {0}, &curves3d, NULL, orients, {}, plane, domain, SM_OT_SAME, .., &TRUE)` (difftest cases `face_curves_cos_no_uv_*` of `topology.brep_p7`).
- **How found:** crash in the C++ reference run (pre-review fuzz of PR #155).
- **Confirmed:** yes (SIGSEGV).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput`; the difftest shim keeps such calls away from upstream.
