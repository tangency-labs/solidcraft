Updates: ND-3

#### ND-3 (update, 2026-10-27). Per-point step opt-in; trigger from #1173
- **Trigger:** box 60 x 40 x 20, r5 on the top edge at y = 40, r3 on the top edge at x = 60, then r2.5 or r2.9 on the vertical edge at x = 60, y = 40, after earlier blends in the process (#1173). The same inputs gave a sound body in a fresh process and an unsound one (internal pole, repeated control points, cusp, missing seam) after earlier blends: which step the static latched decided it.
- **Fork's fix:** the step is chosen per point (tangency-labs/smlib@0a05860).
- **Confirmed:** no, not with our C++ build. Read at c9c5979 `SmFilletSolver.cpp:3604`; reported and fixed in the SolidCraft fork. The C++ static's history dependence is shown by the `blend.fillet_solver_p4` latch probe (`sc_blend_fillet_solver_p4_static_step`).
- **Rust port:** `Kernel::fixes.fillet_point_step_per_point` (opt-in, off by default) chooses the step per point (1e-3 at a tangent point, 1e-5 otherwise) and neither reads nor sets the `Kernel::fillet_point_step` latch. With it off the latch reproduces upstream. Toggling it on a kernel that already latched a value keeps that value for later flag-off runs.
- **Test:** `per_point_step_ignores_the_latch_with_the_fix` (the first finite-difference guess is the same on a fresh kernel and one latched at 1e-5 with the flag on, and differs with it off). No end-to-end test of #1173's sequence: the corner path stops at the unported `SmFace::PointClassify`.
- **With WR-456** (`fillet_point_bigger_step_retry`): with per-point steps a tangent point always starts at 1e-3, so the `dStep < 1e-4` bigger-step retry is never taken for it; a crossing point keeps 1e-5 and can still reach it.
