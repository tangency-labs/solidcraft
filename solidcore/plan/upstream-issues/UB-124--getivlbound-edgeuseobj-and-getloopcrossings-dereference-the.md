#### UB-124. `GetIvlBound_EdgeuseObj` and `GetLoopCrossings` dereference the face, surface and curve before their NULL checks
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:20480-20488`, `SmCurveInterval::GetIvlBound_EdgeuseObj` (def. `:20458`); `:11342-11360`, `SmCurveClassification::GetLoopCrossings` (def. `:11303`).
- **What happens:** `GetIvlBound_EdgeuseObj` calls `pClassifyFace->GetUpwardFaceuse()` and `GetSurface()` in its locals, then tests `pClassifyFace == NULL` ("no work"); a classification without face reads `m_pFU` of `NULL`. It also needs the interval's back pointer (`GetCurveClassification()->GetCurve()`). `GetLoopCrossings` calls `pSurface->GetNaturalUVDomain()` (and, for a face without surface, `NULL->` through it) before `if(pFace == NULL || pSurface == NULL) return SM_SUCCESS`, and `pClassifyCrv->GetNaturalInterval()` before `if(pClassifyCrv == NULL ...)`; both early returns are dead.
- **Trigger:** `GetIvlBound_EdgeuseObj` on a classification without `SetFace`; `GetLoopCrossings` over loops of a face without surface, or of a classification without curve.
- **How found:** read during porting (unit `intersect.curve_class_p4`).
- **Confirmed:** no (the difftest shim refuses those calls).
- **Severity:** crash.
- **Rust port:** `SolidError::InvalidInput` (the dead "no work" returns are not reached).
