#### UB-158. `SmCurveClassification::InsertAt(0, ..)` into a non-empty classification never ends its shift loop
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `SmCurveClass.cpp:5349` (`InsertAt`: `for(i1=m_lSize-1,ii=i1-nCount;ii>=nIndex;ii--,i1--)`).
- **What happens:** `ii` and `nIndex` are `ULONG`. With `nIndex == 0` the test `ii >= 0` is always true: after `ii = 0` the loop decrements `ii` to `ULONG_MAX` and copies `m_pData[ULONG_MAX]` into `m_pData[0]`, then keeps walking down memory.
- **Trigger:** `InsertAt(0, interval)` on a classification with at least one interval.
- **How found:** read during porting.
- **Confirmed:** no (the harness keeps this call away from the C++).
- **Severity:** crash/UB.
- **Rust port:** `CurveClassification::insert_at` returns `InvalidInput`.
