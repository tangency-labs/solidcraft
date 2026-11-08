#### WR-29. `SmProjectedCurve::Transform` only works for `m_bOwnsCurve == UNSURE`
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmProjectedCurve.cpp:434` (`if (m_bOwnsCurve < 2)`), `SmProjectedCurve::Transform` (def. `:417`).
- **What happens:** Curves that own (`TRUE`, e.g. everything from `ReadFromDB`) or borrow (`FALSE`) their base fail with `SM_ERR`, after a pre-edit notify.
- **Trigger:** `Transform` on a projected curve read from a file.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
