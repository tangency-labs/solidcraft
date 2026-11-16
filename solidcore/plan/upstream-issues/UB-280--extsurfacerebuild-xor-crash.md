#### UB-280. `SmFeatureExecutive::ExtSurfaceRebuild` crashes on planar faces of `box_with_large_fillet.smb`
- **Severity:** Crash / undefined behaviour
- **Area:** Blends and defeaturing
- **Location:** `source/SMLib/src/SmFeatureExecutive.cpp:1922`, `SmFeatureExecutive::ExtSurfaceRebuild`
  (the `SmMerge::ManifoldBoolean(SM_BO_EXCLUSIVE_OR, ..)` of the extended face and the face
  copy, or the `PiecewiseMerge` stitch after it; not isolated further).
- **What happens:** the process stops with a segmentation fault at a call through a NULL
  function pointer (`rip = 0`); the frames on the stack name `ExtSurfaceRebuild`,
  `SmMerge::PiecewiseMerge`, `SmMerge::ManifoldBoolean`, `SmFace::GetEdges` and
  `SmLoopuse::GetEdges`. Every surface extension the rebuild makes succeeds (the NULL
  extension of UB-278 is not involved).
- **Trigger:** `box_with_large_fillet.smb`, `AddFeature({face 2})` (also faces 3 and 4) with
  `SM_RB_EXTEND_SURFACE, SM_RB_FROM_OTHER_BREP`, then `DoDefeature`; and `AddFeature({face 6},
  SM_RB_FROM_OTHER_BREP, SM_RB_FROM_OTHER_BREP)`, `DoDefeature`.
- **How found:** porting (`blend.feature_executive`), generated cases
  `single_face_box_with_large_fillet_2`/`_3`/`_4` and `rb_33_box_with_large_fillet_6`. The
  runner runs each script in a fresh child process and reports these as rejected.
- **Confirmed:** yes, native build (all four crash in the child, every run).
