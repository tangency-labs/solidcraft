#### HANG-52. `SmFeatureExecutive::DoDefeature` does not finish on two fillet-face features
- **Severity:** Hang
- **Area:** Blends and defeaturing
- **Location:** `source/SMLib/src/SmFeatureExecutive.cpp:1588`, `SmFeatureExecutive::DoDefeature`
  (inside `SmFeature::BuildPatches`; the looping call is not isolated).
- **What happens:** the call is still running after 90 s (the other scripts of the unit,
  on the same files, take at most 4 s).
- **Trigger:** `box_with_large_fillet.smb`, `AddFeature({face 6}, SM_RB_EXTEND_SURFACE,
  SM_RB_FROM_OTHER_BREP)`, `SetBaseExtDist(0)`, `SetFeatureExtDist(0)`, `DoDefeature`; and
  `Fillet/torii_fillet.smb`, `AddFeature({faces 11, 12}, SM_RB_EXTEND_SURFACE,
  SM_RB_FROM_OTHER_BREP)`, `DoDefeature`.
- **How found:** porting (`blend.feature_executive`), generated cases
  `ext_dist_0.0_box_with_large_fillet` and `fillet_group_torii_fillet_b_rb23`. The runner's
  child process ends them after 60 s and reports them as rejected.
- **Confirmed:** yes, native build (still running after 90 s, every run).
