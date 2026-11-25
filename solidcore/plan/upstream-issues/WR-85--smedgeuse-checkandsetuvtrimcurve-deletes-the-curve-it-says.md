#### WR-85. `SmEdgeuse::CheckAndSetUVTrimCurve` deletes the curve it says the caller keeps
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmEdgeuse.cpp:4732` `SmEdgeuse::CheckAndSetUVTrimCurve`; contract in `SmEdgeuse.h:160`.
- **What happens:** The header says a rejected `pUVTrimCurve` stays "managed by caller", but the curve was attached first and the restoring `SetUVTrimCurve(pOldUVTrimCurve, 0.0, TRUE, ...)` deletes it (it is the attached curve), so a caller that deletes it frees it twice. Passing the curve already attached is worse: the first `SetUVTrimCurve` does nothing and the accepted branch then deletes `pOldUVTrimCurve`, the attached curve, leaving a dangling pointer. An `SER` return inside the accepted branch (the precise distance measurement, `CreateUVTrimCurve`) leaks the detached old curve.
- **Trigger:** Any UV curve that is not good enough; any call with `GetUVTrimCurve()` as the argument.
- **How found:** read during porting (unit `topology.edgeuse_p2`); the difftest shim relies on the deletion (it never frees the curve it passes).
- **Confirmed:** no.
- **Severity:** crash (double free / use after free).
- **Rust port:** Takes the curve by value: it is attached or dropped; the attached curve cannot be passed.
