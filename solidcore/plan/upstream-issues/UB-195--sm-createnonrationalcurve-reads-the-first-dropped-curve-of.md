#### UB-195. `sm_CreateNonRationalCurve` reads the first dropped curve of an empty array [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** Healer target management
- **Location:** `source/SMLib/src/SmBSplineSurfaceNLib.cpp:2938`-`2949` (`sm_CreateNonRationalCurve`, rational curve branch), reached from `SmBSplineSurface::CreateCornerBlend` (`:3100`-`3110`) and so from `SmBrep::CreateCornerBlend`.
- **What happens:** For a rational B-spline boundary curve with a neighbour surface, the curve is approximated, reparametrised and dropped onto the surface with `DropCurve`. `DropCurve` can return `SM_SUCCESS` with no curve; `SM_ASSERT(sUVCurves.GetSize() == 1)` is a release no-op and `pNewUVTrimCurve = sUVCurves[0]` reads the array's NULL data (SIGSEGV).
- **Trigger:** a corner blend whose ordered boundary curves include a rational B-spline edge curve of a tangent edge use whose drop onto its face surface yields no curve, with no earlier status stopping the call. The difftest met it on a face of a perturbed closed surface.
- **How found:** difftest fuzzing of `topology.brep_p1` (the follow-up to #145); located with gdb on a `-g` build of the C++ reference.
- **Confirmed:** yes (C++ reference run; the fault is at `:2949` with `m_lSize == 0`, `m_pData == NULL`).
- **Severity:** crash/UB. **[PSIRT]:** reachable from a B-rep read from a `.smb` file through `SmBrep::CreateCornerBlend`.
- **Rust port:** `nurbs::b_spline_surface_n_lib::p1::create_non_rational_curve` returns `InvalidInput` ("no dropped curve"). The `topology.brep_p1` shim refuses exactly those inputs by replaying `SmBrep::CreateCornerBlend` and the curve conversion on a copy of the B-rep up to the drop; the runner reports both sides as the null-dereference guard.
