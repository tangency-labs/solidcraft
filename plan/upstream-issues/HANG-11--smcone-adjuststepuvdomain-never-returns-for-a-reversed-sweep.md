#### HANG-11. `SmCone::AdjustSTEPUVDomain` never returns for a reversed sweep interval
- **Severity:** Hang
- **Area:**
- **Location:** `SmCone.cpp:458` `SmCone::AdjustSTEPUVDomain` -> `SmSurfOfRevolution.cpp:300` `SmSurfOfRevolution::AdjustSTEPUVDomain` (`Reparameterize`, then `MakeNurb`).
- **What happens:** The C++ reference run did not finish within 7 minutes. The domain is stored and the NURB rebuilt without checking `start <= end`. Which loop spins was not located.
- **Trigger:** `SmCone::CreateCanonical(O, 1, 2, 3, cone)`, then `AdjustSTEPUVDomain(SmExtent2d(90, 0, 10, 1))`.
- **How found:** adversarial / edge-case fixture (`surface.cone_p1`).
- **Confirmed:** yes.
- **Severity:** hang.
- **Rust port:** `SmSurfOfRevolution::AdjustSTEPUVDomain` is not ported yet (`Unported`); the FFI rejects a reversed sweep.
