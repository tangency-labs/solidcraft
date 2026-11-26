#### UB-72. Curve-in-domain check of a default `SmUnbendVolume` crashes
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `SmVolume.cpp:1538` `SmVolume::IsCurveInInSpaceDomain` -> `SmUnbendVolume.cpp:2467` `SmUnbendVolume::IsCurveInParamDomainSimple`.
- **What happens:** Segfaults in the unbend volume's curve check. That check builds its discontinuity half plane with the interval `SmExtent1d(m_dSingularityTol, -SM_BIG_DOUBLE)` (`:2479`), i.e. min above max. Whether this inverted plane domain is the cause was not verified.
- **Trigger:** A default-constructed `SmUnbendVolume` used as an orient map, then `IsCurveInInSpaceDomain` with a line.
- **How found:** crash in the C++ reference run.
- **Confirmed:** yes.
- **Severity:** crash/UB.
- **Rust port:** The unbend volume is not ported yet. The harness rejects these chains.
