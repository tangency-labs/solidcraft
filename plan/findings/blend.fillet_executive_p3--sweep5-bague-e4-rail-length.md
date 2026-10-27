# blend.fillet_executive_p3: sweep5 bague e4 rail curve twice as long, C++ TrimRails refuses (2026-10-27)

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

Input: `sweep5-bague-e4-r1.0`, file source `Fillet/bague.smb`, edge 4, circular G1 round,
radius 1.0, thumbweight 1.

Values: C++ returns `SmApiFilletEdges` status 1001 and keeps the body (4 vertices, 8 edges,
4 faces, mass ok). Rust succeeds (0; 16, 24, 8) and its mass computation fails (1007).

First divergence found so far: `SmFilletExecutive::CreateFilletBrep` ->
`SmFilletSolver::TrimRails` -> `SmFilletGeom::TrimRailCurves`. For the first rail, C++ has
natural interval (0, 31.939983029517229). Both vertex points are processed. The
`GlobalPointSolve` for V1 returns parameter 0, so `sTrimIvl` becomes (0, 0) and
`p3DRailCurve->Trim` fails with 1007. In Rust the same rail has natural interval
(0, 63.5071316082997), about twice as long. V1 drops to 63.507, the rail trims to its full
length and the fillet goes on. The second fillet geometry's rails then agree in shape
(0, 0.5332133108777974). The rail curves differ before `TrimRails`: they come from the
solver's `CalcFilletGeom` on a closed (full-circle) edge.

Suspected cause: the fillet solver's rail construction for the closed edge. Rust
seems to march or build the rail over the full closed edge twice, or C++ stops at half.
This is not localised further yet; the next step is to compare the solver's rail
knots and points after `CalcFilletGeom`. Found with `fprintf` probes in a scratch copy
of the reference (`SmFilletGeom.cpp` is compiled into the `blend_fillet_geom_p1` shim, so
the probe has to go there, not into `libSMLib.a`).
