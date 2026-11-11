#### WR-8. `SmTransform::EvaluateBoundingBoxSimple` aligned branch is not the image of the box
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmTransform.cpp:1224`, `SmTransform::EvaluateBoundingBoxSimple`.
- **What happens:** It scales parameter intervals by `1/|axis|` and shifts them by `origin·axis`, which is not the transformed box (e.g. under a pure translation).
- **Trigger:** A translation-only transform with an aligned parameter box.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
