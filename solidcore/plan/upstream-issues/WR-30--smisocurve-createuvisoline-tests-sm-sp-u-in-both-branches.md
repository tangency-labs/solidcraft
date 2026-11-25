#### WR-30. `SmIsoCurve::CreateUVIsoLine` tests `SM_SP_U` in both branches
- **Severity:** Wrong result
- **Area:** Curves
- **Location:** `SmIsoCurve.cpp:34`, `:38`, `SmIsoCurve::CreateUVIsoLine` (def. `:24`).
- **What happens:** A constant-v iso curve gets `SM_ERR_INVALID_INPUT`. The constant-u branch builds the UV line over the domain's u interval although the line runs along v.
- **Trigger:** `CreateUVIsoLine` for `SM_SP_V`.
- **How found:** generated difftest case (`*_uvline`).
- **Confirmed:** yes.
- **Severity:** wrong result.
- **Rust port:** Ported as is.
