#### WR-155. Two volume boundary solution arrays share the same borrowed storage
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmCurveClass.cpp:10002`, `sSolutions4(20,aData4), sSolutions5(20,aData4)`.
- **What happens:** Both arrays initially refer to `aData4`. Writing the sixth boundary's solutions can overwrite the fifth boundary's saved solutions before the later classification scan. `aData5` is not declared; the arrays do not use independent buffers.
- **Trigger:** both affected boundary slots contain results that remain in their borrowed arrays.
- **How found:** read during porting `intersect.curve_class_p8` (PR #152).
- **Confirmed:** no isolated native reproduction.
- **Rust port:** independent owned vectors; this aliasing behavior is not certified or reproduced. All volume plane-intersection paths remain deferred to `surface.surface_p8`; revisit the output lifecycle when wiring that dependency.
