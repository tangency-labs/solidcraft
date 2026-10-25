#### UB-110. `SmCurveClassification::IsCoincident` walks past the intervals when its end index wraps or a search failed
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmCurveClass.cpp:7016` (`lIndex2--` of an unsigned `ULONG`) and `:7020`-`7022` (`m_pData[ii]` for `ii <= lIndex2`).
- **What happens:** when the end parameter classifies to the start of interval 0 (`ePosition2 == SM_IP_START`, `lIndex2 == 0`) the decrement wraps to the largest `ULONG` and the loop reads `m_pData[ii]` for every `ii` until it finds a mid classification that is not an edge: when all the intervals classify to edges it reads past the array. The same loop runs with `lIndex == 999999` (the value `FindInterval` leaves when it fails; both statuses are ignored) after a failed search.
- **Trigger:** `IsCoincident(rInterval, tol)` with `rInterval.GetMax()` equal (within tolerance) to the start of the curve classification, or with a parameter off either end of the classification that does not snap, and every interval mid classified to an edge.
- **How found:** read during porting (unit `intersect.curve_class_p3`).
- **Confirmed:** no (the difftest shim replays the walk and rejects scripts that leave the intervals).
- **Severity:** crash/UB (out-of-bounds read).
- **Rust port:** `SolidError::InvalidInput` when the walk leaves the intervals; otherwise identical (the wrap is kept as an unbounded end).
