#### WR-41. `SmPlane::Reparameterize` does not move the origin in release builds
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmPlane.cpp:3065` `SmPlane::Reparameterize`. The new origin `sOriginMax` (`:3086`) is used only by the `SM_DEBUG_CODE` check (`:3090`-`3099`). `m_vPosition` is never updated; only the domain and scales change (`:3115`-`3116`).
- **What happens:** The analytic plane no longer matches its NURB unless the new domain starts where the old one did.
- **Trigger:** Reparameterize a plane from `[0,1]^2` to `[2,3]^2`.
- **How found:** adversarial / edge-case fixture `edge_reparameterize_keeps_origin`.
- **Confirmed:** yes.
- **Severity:** wrong result (release-only).
- **Rust port:** Ported as is.
