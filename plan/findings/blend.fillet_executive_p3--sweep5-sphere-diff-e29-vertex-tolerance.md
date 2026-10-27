# blend.fillet_executive_p3: sweep5 sphere_diff_3cylinders e29 vertex tolerance (2026-10-27)

Unit: `blend.fillet_executive_p3`. Fixture: `fixtures/blend.fillet_executive_p3/sweep_round5.json`.

Input: `sweep5-sphere_diff_3cylinders-e29-r1.0` (`Fillet/sphere_diff_3cylinders.smb`, edge 29,
circular G1, radius 1.0, thumbweight 1).

Values: everything agrees except `result_geometry[111]`, the tolerance of result vertex 27:
C++ 9.100516385135238e-5, Rust 9.304666970990486e-5 (rel 2.2e-2). The rail trims agree
(rail 0 (82.647291068524822, 102.85355699527092), rail 1 (82.647290569503795,
102.85355973819911), both sides). So do the per-stage UV-trim counts and lengths from
CreateFilletBrep to the end of merge.

Localised further: the vertex (15.2135, -15.9047, 15.9047) has the same sequence of
`SmVertex::SetTolerance` calls on both sides up to the eleventh one, which raises it from
7.556389994188304e-5 to C++ 9.1005163851352381e-5 / Rust 9.30466697099048589e-5. That
call comes from the trim of the original surfaces (`blend/fillet_geom` p2 -> p6 ->
p1 -> `topology/face` ShrinkGeometry -> `CreateUVTrimCurves` -> `GetOrCreateUVTrimCurve` ->
`SetUVTrimCurve`). There the edge tolerance becomes `2.0 * dMaxDistanceToSurface` and
cascades to the vertex. So the `DropCurve` max distance of that edge differs: C++
4.5503e-5, Rust 4.6523e-5.

Suspected cause: the edge curve's `DropCurve` onto the original face's surface measures a
different maximum gap, in the drop lib / drop curve. `plan/findings/blend.fillet_executive_p3--bottle-e29-r0.1-vertex-tolerance.md`
was a similar case (a `Surface::clone` standing in for the shared surface). Switching the
remaining `clone_keeping_owner` callers in `blend::fillet_standard_solver::p1/p2` and
`intersect::curve_class::p5` to exact copies does not change this case. The next step is to
compare that drop's points and `dMaxDistToSurf`.
