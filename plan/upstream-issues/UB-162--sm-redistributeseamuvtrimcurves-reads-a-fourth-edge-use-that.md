#### UB-162. `sm_RedistributeSeamUVTrimCurves` reads a fourth edge use that need not exist
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp:12097`-`12105` (`sm_RedistributeSeamUVTrimCurves`): `SM_PTR_ARRAY(sNewEdgeuses, SmEdgeuse, 16)`, `pNewE->GetEdgeuses(sNewEdgeuses); SM_ASSERT(sNewEdgeuses.GetSize() == 4)`, then `sNewEdgeuses[0]` and `sNewEdgeuses[3]`.
- **What happens:** The assertion is a no-op in release builds. For a seam edge with fewer than four edge uses the function reads slot 3 of the 16-slot stack array, an uninitialised pointer, and calls `GetCWEdgeuse()` on it.
- **Trigger:** A UV trim curve given, a seam direction other than `SM_SP_NEITHER`, and an edge with one, two or three edge uses (a wire or lamina seam edge, or an edge whose other face is not attached yet).
- **How found:** source review while porting `topology.brep_p1` (PR #145); the difftest shim refuses the call for edges with fewer than four edge uses.
- **Confirmed:** no (source inspection only; the shim never runs it).
- **Severity:** crash/UB.
- **Rust port:** `topology::brep::p1::redistribute_seam_uv_trim_curves` returns `InvalidInput` for an edge with fewer than four edge uses.
