#### UB-125. `sm_InsertSolutions` dereferences a null edge curve while ignoring short tangential ranges
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:4713`, `:4734`, within `sm_InsertSolutions` (`:4158`).
- **What happens:** `pEdgeCurve` is set only for an `SmEdge` target. For a bare curve or plane target, a forward range shorter than 5% of the classification interval reaches `SmTol::MapTo1d(..., *pEdgeCurve)` when tangencies are ignored. Ranges converted to single points by the earlier endpoint/arc-length test do not reach this dereference.
- **Trigger:** ignore tangencies; bare curve or plane target; a non-collapsing range with `start <= end` and relative interval length below `0.05`.
- **How found:** source review and FFI guard design during `intersect.curve_class_p5` (PR #141).
- **Confirmed:** no native crash executed; the shim reproduces the preceding native collapse predicates and rejects only ranges that would reach the null dereference.
- **Severity:** crash / undefined behaviour.
- **Rust port:** invalid-input error for the undefined branch; valid collapsed ranges and reversed ranges remain supported.
