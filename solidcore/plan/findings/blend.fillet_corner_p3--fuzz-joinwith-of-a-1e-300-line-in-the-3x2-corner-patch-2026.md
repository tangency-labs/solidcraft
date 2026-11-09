# blend.fillet_corner_p3 fuzz: JoinWith of a 1e-300 line in the 3x2 corner patch (2026-10-24, PR #722)

Exposed when PR #722 let `SmFillet3x2MixedConvexityCorner::CreateCornerPatch` run past its
`JoinWith` (before, the shim stopped there as "not run": `MakeFaceBrep` was a Rust stub).
`difftest --unit blend.fillet_corner_p3 --fuzz 2000` finds one differing input
(fuzz00457): the on-extended-edge curve is a cubic from (-1, 0, 0) to (0, 5e-4, 0), the
on-edge curve a line from (0, 0, 0) to (1e-300, 0, 0). `curve1->JoinWith(1, curve2, 0)`
succeeds in C++ (the corner then fails later with `SM_ERR_NULL_POINTER`); in Rust
`BSplineCurve::join_with` fails with `SM_ERR`: the line test is false for the degenerate
line on both sides (its base vector is below the tolerance), and `create_by_joining`'s
`degree_elevate` of the degenerate degree-1 segment to degree 3 returns `SM_ERR`
(`nurbs.b_spline_curve_p2`). Suspected cause: the degree elevation (or a knot / weight
check before it) of a curve whose control points are 1e-300 apart. Only a fuzzed model
reaches it; no fixture or generated case does. Not fixed here (unit
`nurbs.b_spline_curve_p2` / `nurbs.crv_tool_p1`); the same fuzz run also found a C++
use-after-free (a repeated on-edge corner edge, freed by the 3x2 patch while still in
`m_vEdges`), now refused by the shim's pre-check.
