#### UB-77. Old composite curves: a closed one without segments reads `sGaps[ULONG_MAX]`; nesting is unbounded [PSIRT]
- **Severity:** Crash / undefined behaviour
- **Area:** File reading
- **Location:** `source/SMLib/src/SmBrepData.cpp:5456` (`new SmCompositeCurve(lDimension, sCurves, bClosed, &sSenses, &sGaps)`) into `SmCompositeCurve.cpp:1090` (`(*cpGaps)[m_lNumSegments-1]`); recursion at `SmBrepData.cpp:5445`.
- **What happens:** A version 32 or older composite curve record with segment count 0 and the closed flag set indexes the empty gap array at `ULONG_MAX`. Each nested composite record recurses into `ReadCurveFromDB` with no depth limit.
- **Trigger:** `Curve Type 14032` / `0 3 1`; or thousands of nested composite records.
- **How found:** read during porting (`topology.brep_data`).
- **Confirmed:** no (guarded).
- **Severity:** crash/UB (out-of-bounds read, stack exhaustion).
- **Rust port:** `CompositeCurve::with_curves` returns `InvalidInput`; nesting is bounded by `DatabaseReader::enter_nested` (32).
