#### WR-56. `sm_GoodDropResult` swaps the guess and result distances
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface.cpp:10349`, `sm_GoodDropResult` (file static, def. `:10256`).
- **What happens:** `dDsqGuess` holds the distance to the result point and `dDsqResult` the distance to the guess point, so the final "result farther than the guess" test rejects drops that improved on the guess. The caller then runs the expensive global solve.
- **Trigger:** A local drop whose result is closer to the point than the guess, with a small residual that passes the earlier tests.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (performance: an unneeded global solve).
- **Rust port:** Reproduces it (`good_drop_result`).
