# Resolved: sweep5 bague e4 rail curve twice as long (2026-10-27)

Updates: plan/findings/blend.fillet_executive_p3--sweep5-bague-e4-rail-length.md

`sweep5-bague-e4-r1.0` now passes: like C++, `SmApiFilletEdges` returns 1001 and keeps the
body. Its known-mismatch entry is removed.

Where it diverged: not in the solver's `CalcFilletGeom` (both sides build the same first
rails, natural interval (0, 7.1466)), but in `SmFilletSolver::ProcessRollOver` ->
`SmFilletGeom::ReCalcFilletGeom` for the tangent-rollover geometry on the closed edge. Its
two rail-end points project to the same edge parameters on both sides. The first
`DoPointIntersection` try leaves the last point unprocessed, so the seam loop runs. Case 4
finds the last point's second-surface UV on the seam (u = 9.6e-20) and calls
`sm_MoveTSP(sTsectPnts, last, 0)`.

Cause: `sm_CheckSeamOn(pSurf, SmPoint2d & rUV, ..)` takes the UV by reference and moves a
seam point to the other side (u = 1) in the vertex use's own `SmTsectPnt`. The Rust seam
loop called `check_seam_on` on a copy of the UV, so the move was lost. The retry then
marched from u = 0 the long way round the closed edge (rail (0, 63.507)) instead of from
u = 1 (C++ rail (0, 31.940), which `TrimRailCurves` then refuses with 1007).

Fix (`blend/fillet_geom/p3.rs`, `re_calc_fillet_geom` seam loop): `check_seam_on` now
writes to the vertex use's `TsectPnt` UV, as upstream does. This is unconditional: it
makes Rust match C++.

Found with `fprintf` probes in a private copy of the reference library (`SmFilletSolver.cpp`
re-archived into a copy of `libSMLib.a`, and `SmFilletGeom.cpp` through the
`blend_fillet_geom_p1` shim's include).
