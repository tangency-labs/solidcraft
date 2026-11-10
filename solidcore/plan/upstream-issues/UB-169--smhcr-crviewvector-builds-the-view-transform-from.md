#### UB-169. `SmHCR(.., crViewVector, ..)` builds the view transform from uninitialised vectors for a zero view vector
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmHCR.cpp:144` `SmHCR::SmHCR` (view-vector constructor).
- **What happens:** The status of `MakeUnitOrthoVectors` is ignored. When the view vector cannot be unitized (squared length below `SM_EFF_ZERO_SQ`), or when its squared length overflows to infinity (`Unitize` then yields `(0, 0, 0)`, both arbitrary-axis crosses are zero and `SER(SM_ERR)` returns, `SmVector3d.cpp:199-205`), `sPerp1` and `sPerp2` (`SmVector3d() = default`) stay uninitialised and `SetSTEPCanonical` reads them.
- **Trigger:** `SmHCR(ctx, pBrep, SmVector3d(0, 0, 0), ..)`; `SmVector3d(1e200, 0, 0)`.
- **How found:** read during porting (`blend.hcr`).
- **Confirmed:** no (the difftest guard rejects such view vectors; generated cases use `[1e200, 0, 0]`).
- **Severity:** crash/UB.
- **Rust port:** `view_transform_for_vector` returns `InvalidInput`.
