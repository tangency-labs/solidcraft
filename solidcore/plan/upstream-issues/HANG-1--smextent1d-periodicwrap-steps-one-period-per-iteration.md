#### HANG-1. `SmExtent1d::PeriodicWrap` steps one period per iteration
- **Severity:** Hang
- **Area:**
- **Location:** `SmExtent1d.cpp:907` `SmExtent1d::PeriodicWrap`. Reached from `SmExtent2d.cpp:1038` `ContainsPeriodicPoint2d` and from the `SmLocalSolveNd` periodic clamping (`SmLocalSolveNd.cpp:90`).
- **What happens:** The run time is proportional to distance / period.
- **Trigger:** Period `1e-9`, value `1e15` (~1e24 iterations); a diverging Newton step of `1e8` with period `0.01` (~1e10).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** hang.
- **Rust port:** Same loop (faithful). The generator avoids such ratios.
