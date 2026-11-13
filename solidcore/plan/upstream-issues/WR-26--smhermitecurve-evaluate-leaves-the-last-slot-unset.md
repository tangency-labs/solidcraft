#### WR-26. `SmHermiteCurve::Evaluate` leaves the last slot unset
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmHermiteCurve.cpp:299` (`for (ULONG i=4; i<lNumDerivatives; i++)`), `SmHermiteCurve::Evaluate` (def. `:283`).
- **What happens:** The zeroing loop stops one short, so `aPointAndDerivatives[n]` keeps the caller's value (`SM_UNDEF_DOUBLE` for a fresh array).
- **Trigger:** `Evaluate` with `lNumDerivatives = 5`.
- **How found:** read during porting.
- **Confirmed:** no. The shim pre-fills zeros.
- **Severity:** wrong result.
- **Rust port:** Returns zero there.
