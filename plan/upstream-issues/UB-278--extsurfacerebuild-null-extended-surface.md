#### UB-278. `SmFeatureExecutive::ExtSurfaceRebuild` dereferences a NULL extended surface
- **Severity:** Crash / undefined behaviour
- **Area:** Blends and defeaturing
- **Location:** `source/SMLib/src/SmFeatureExecutive.cpp:2061`, `SmFeatureExecutive::ExtSurfaceRebuild` (def. `:1922`)
  (the `CreateExtendedSurface` call and the `pNewSurf->GetNaturalUVDomain()` after it).
- **What happens:** for each face along the loop whose outer loop holds a loop edge, the
  surface is extended with `CreateExtendedSurface(m_crContext, dExt, SM_CT_G1, pNewSurf)` and
  its status is ignored. `SmSurface::CreateExtendedSurface` returns `SM_ERR` and leaves
  `pNewSurf` NULL for a surface without an override (an analytic cylinder, for example), and
  the next line calls `pNewSurf->GetNaturalUVDomain()`.
- **Trigger:** `DoDefeature` with `SM_RB_EXTEND_SURFACE` (or `SM_RB_FROM_OTHER_BREP`) where such
  a face borders the void, e.g. `Fillet/filleted_box.smb`, `AddFeature({face 0},
  SM_RB_EXTEND_SURFACE, SM_RB_FROM_OTHER_BREP)`, `DoDefeature`: segmentation fault in
  `ExtSurfaceRebuild` (`SmFeature::BuildPatch` -> `RebuildBrep`).
- **How found:** porting (`blend.feature_executive`), generated cases `single_face_*`. The
  shim tries each extension the rebuild would make first and refuses the script (`-995`).
- **Confirmed:** yes, native build (crash with that backtrace).
