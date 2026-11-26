# blend.fillet_geom_p5: examine solvers, wired (2026-10-26)

Updates: plan/findings/blend.fillet_geom_p5--examine-solvers.md

`SmFilletEdge::CalcCrossSection` now calls `sm_ExamineNFixFilletGeoms` through
`blend::fillet_geom::p1::examine_n_fix_fillet_geoms_with_solvers`, which takes the
geometries' `FilletSolver` bases (resolved by the caller's solver lookup; the same
solver may be passed twice for a closed loop). The adjusting path drops the rebuilt rail
onto a copy of the solver's offset surface with its offset distance set to 0 (upstream
sets and restores it on the solver's own surface; the drop already ran on a copy). The
original `examine_n_fix_fillet_geoms` (co-ported `ConstantRadiusAssistedFs` solvers)
delegates to it. The status stays ignored, as upstream. Reached live by the closed Nx1
fixtures of `blend.fillet_executive_p3` (cylinder rims, cyl_on_box edge 4,
box_diff_cyl_big_rad edges 12/14), which now pass.
