#### WR-98. `SmTrimmingTools::MirrorCurve` divides rational control points by their weights twice
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmTrimmingTools.cpp:3516`-`3517`.
- **What happens:** `GetCanonical` already returns Euclidean control points; the loop divides each one by its weight again before `SetCanonical`, so a rational UV curve is moved and distorted, not just mirrored.
- **Trigger:** `MirrorCurve` (reached from `FixBackwardsOuterLoopOnSeam`) on a rational UV curve with weights other than 1.
- **How found:** read during porting (unit `topology.trimming_tools`).
- **Confirmed:** yes (fixture `mirror_rational_divides_twice`).
- **Severity:** wrong result.
- **Rust port:** Faithful to the code (`mirror_curve`).
