#### WR-87. `sm_CheckSeamCrossing` returns `SM_SUCCESS` as its `SmBoolean`
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmFace.cpp:145`, `sm_CheckSeamCrossing` (def. `:132`).
- **What happens:** For a surface closed in neither direction it returns `SM_SUCCESS` (`1000`), which reads as `TRUE` ("a seam was crossed"), from a function documented to return whether a shift happened.
- **Trigger:** `bClosedU == bClosedV == FALSE`.
- **How found:** read during porting.
- **Confirmed:** yes, by the C++ build (fixture `seam_neither` of `topology.face_p2`). Its only caller (`SmFace::ComputeLoopOrientation`) ignores the return value, so nothing observable depends on it today.
- **Severity:** wrong result (latent).
- **Rust port:** Reproduces it (`topology::face::p2::check_seam_crossing` returns `true`).
