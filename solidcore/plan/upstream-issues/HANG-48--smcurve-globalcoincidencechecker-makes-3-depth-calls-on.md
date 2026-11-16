#### HANG-48. `SmCurve::GlobalCoincidenceChecker` makes `3^depth` calls on overflowing curves
- **Severity:** Hang
- **Area:**
- **Location:** `SmCurve.cpp:19950` (`SmCurve::GlobalCoincidenceChecker`): a closed or half closed interval is split in three and each piece checked recursively, with four end-point drops (global solves) per call.
- **What happens:** with `1e300` control points the split test keeps answering yes 15 to 30 levels deep, so the call count grows as `3^depth` (hundreds of thousands after 10 s in the port).
- **Trigger:** `fuzz-out/n2000/curve.curve_p8/fuzz01456.json`, `fuzz01856.json`, `surface.surface_p8/fuzz01603.json`, `curve.composite_curve_p2/fuzz00430.json`, `fuzz01007.json`.
- **How found:** fuzz-corpus replay of the Rust side alone (issue #504).
- **Confirmed:** no (the FFIs refuse the `1e300` values).
- **Severity:** hang (exponential time).
- **Rust port:** `InvalidInput` past `MAX_COINCIDENCE_CALLS = 2^18` calls per check (`curve/curve/p6.rs`); the global solves under it stop past `MAX_SOLVE_BRANCHES = 2^20` visits (`geom/global_solver.rs`).
