#### WR-640. `SmFeatureExecutive::ValidatePatch` never rejects a patch that crosses the B-rep
- **Severity:** Wrong result
- **Area:** Blends and defeaturing
- **Location:** `source/SMLib/src/SmFeatureExecutive.cpp:1851`, `SmFeatureExecutive::ValidatePatch` (def. `:1735`).
- **What happens:** quality test 2 intersects every patch face with every overlapping target face and
  checks the mid point of each intersection classification with
  `AERN_MSG(sPtClass.GetClassType() != SM_PC_FACE, ..)`. `GetClassType()` is the static `SM_COMMON`
  class id (`SmPointClassification_TYPE`), not the point's class (`GetPointClass()`), so the test is
  always true and a patch whose intersection with the B-rep runs through a face interior is accepted.
- **Trigger:** any patch validation where an intersection curve lies inside a target face, e.g.
  `prog_test my_test_extended_surface` case 7 (`Defeature/LShape_I.smb`, face 3, loop edges
  1 2 8 13 21 30 31 32 37 59): the shared base patch is validated against the feature B-rep, and
  patch face 3 meets feature face 3 along a curve whose mid point classifies `SM_PC_FACE`; the patch
  is accepted and applied to the feature.
- **How found:** porting (`blend.feature_executive`): a Rust port using the point class rejected that
  patch; a native probe of the same loop printed `SM_PC_FACE` while `GetFeatureRebuilt()` was TRUE.
- **Confirmed:** yes, native build (probe), cases `extended_surface_7_LShape_I` and
  `extended_surface_9_Platte` of `blend.feature_executive`. The port keeps upstream's behaviour.
