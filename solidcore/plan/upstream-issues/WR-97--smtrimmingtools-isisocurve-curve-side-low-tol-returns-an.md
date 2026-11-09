#### WR-97. `SmTrimmingTools::IsIsoCurve(curve, side, low, tol)` returns an error status as `TRUE`
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:3632` (`SER (cpUVCurve->GetCanonical (...))` in a function returning `SmBoolean`).
- **What happens:** When `GetCanonical` fails its nonzero status is returned as the `SmBoolean`, so a curve whose control points cannot be read counts as an iso curve (and `FixBackwardsOuterLoopOnSeam` then mirrors it).
- **Trigger:** a UV trim curve without NURB.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** no (the difftest wrapper only passes curves with a NURB).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code (`is_iso_curve_at` returns `true`).
