#### WR-6. `SmPeriodicExtent1d::AddInterval` on an empty extent gives the full period
- **Severity:** Wrong result
- **Area:** Geometry and tolerances
- **Location:** `SmPeriodicExtent1d.cpp:262`, `SmPeriodicExtent1d::AddInterval`.
- **What happens:** The empty branch tests `(m_dPeriod - |m_dMax - m_dMin|) < ScaledZero` on the empty sentinels `[1e20, -1e20]`. This is true for any period below `2e20`, so the result is `[0, period]` full instead of `[from, to]`.
- **Trigger:** Empty extent, period 1, `AddInterval(0.2, 0.3)`.
- **How found:** adversarial / edge-case fixture `add_empty_quirk`.
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
