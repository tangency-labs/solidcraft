#### WR-71. `SmSurfPtSolveStepper::DropToIsoCurve` passes the same variable as guess and output of `DropPoint`
- **Severity:** Wrong result
- **Area:** Surfaces and volumes
- **Location:** `SmSurfacePointSolve.cpp:4733` (`pIsoCurve->DropPoint(.., &dGuessParam, bGotThisDrop, dGuessParam, dDist)`), `SmCurve::DropPoint` at `SmCurve.cpp:3290`.
- **What happens:** `DropPoint` zeroes its output parameter before it reads the guess, so the guess (the crease parameter clamped to the section) is always `0`.
- **Trigger:** Any call (the method is not used upstream).
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (a poorer guess for the local solve).
- **Rust port:** Passes `Some(0.0)`, as upstream's code ends up doing.
