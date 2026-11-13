#### WR-383. `sm_RefineCCSolutions` judges a range solution's end by its distance from the start, so a forward range's end is never refined
- **Severity:** Lost precision (a curve/curve overlap's end keeps its coarse parameters and deviation)
- **Area:** Curve intersection
- **Location:** `source/SMLib/src/SmCurve.cpp` (around `:107`), `sm_RefineCCSolutions`: for both the start and the end, `dFrac1/2 = SmExtent1d::Inversion(...)` (flipped for a reversed range) and `if (fabs(dFrac1) < dShiftLimit && fabs(dFrac2) < dShiftLimit)`, `dShiftLimit = 0.002`.
- **What happens:** the fraction is measured from the range's start. At the start a small move gives a fraction near 0 and passes; at the end the refined parameter's fraction is near 1, so `fabs(dFrac) < 0.002` fails and the end is never refined.
- **Trigger:** any curve/curve range solution whose end deviates by more than `1e-6`, e.g. a range on a line (0,0,0)-(1,0,0) recorded to end at its parameter 1 while the other curve crosses at x = 0.999: the local intersection finds the crossing (a move of 0.001 of the range), which the test refuses.
- **How found:** tangency-labs/solidcore#527 (SolidCraft regression rig on SMLib).
- **Confirmed:** source; the port reproduces upstream with the default.
- **Fix:** judge the end by `fabs(1 - dFrac)`, the start by `fabs(dFrac)`.
- **Rust port:** opt-in `Kernel::fixes.refine_cc_range_ends` (default `false`, upstream's test) in `refine_cc_solutions`, `crates/solidcore/src/curve/curve/p1.rs`. Test `issue_527_refine_cc_range_end`. With the default, `curve.curve_p1` / `curve.curve_p8` (global curve intersection) compare as before. Fixes tangency-labs/solidcore#527.
