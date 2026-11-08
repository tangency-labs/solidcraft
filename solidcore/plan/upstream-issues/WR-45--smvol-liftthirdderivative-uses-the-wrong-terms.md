#### WR-45. `smvol_LiftThirdDerivative` uses the wrong terms
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmVolume.cpp:9851` (surface form), `:10126` (volume form), `smvol_LiftThirdDerivative`.
- **What happens:** `crDxxz` is used where `Dxzz` belongs in `rDuuv`, `rDuvv`, `rDuuw`, `rDuww`, `rDvvw`, `rDvww`. `rDuww` is built from `Fuv`/`Fv` instead of `Fuw`/`Fw`, and `rDvww` from `Fvv`/`Fv` instead of `Fvw`/`Fw`.
- **Trigger:** Third mixed derivatives of any compound volume (and of `SmSrfInVolume`).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Keeps the formulas so results match.
