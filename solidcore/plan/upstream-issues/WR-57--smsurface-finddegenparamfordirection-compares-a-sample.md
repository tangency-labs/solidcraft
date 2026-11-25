#### WR-57. `SmSurface::FindDegenParamForDirection` compares a sample fraction with a parameter
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `SmSurface.cpp:6948`, `SmSurface::FindDegenParamForDirection` (def. `:6757`).
- **What happens:** The "nearly as good and closer to the input" tie-break compares `dThisParam` (a fraction in `[0, 1]` of the sample domain) with `dS` (the actual parameter), so it only means "closer" when the singular parameter runs over `[0, 1]`.
- **Trigger:** A degenerate boundary whose singular parameter interval is not `[0, 1]`, with two nearly equally good samples.
- **How found:** read during porting.
- **Confirmed:** no.
- **Severity:** wrong result (start guess of the Newton solve).
- **Rust port:** Reproduces it.
