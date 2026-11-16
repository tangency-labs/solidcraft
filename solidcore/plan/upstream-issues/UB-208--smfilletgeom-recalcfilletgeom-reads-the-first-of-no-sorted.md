#### UB-208. `SmFilletGeom::ReCalcFilletGeom` reads the first of no sorted rail-end points
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:11648` (`SmFilletGeom::ReCalcFilletGeom`: `sDir = sTsectPnts[0]->CrvDeriv();`), after the sort loop of `:11431-11507`.
- **What happens:** Without a march direction (`pOptMarchDir == NULL`) and with no curve on rail 0, the direction is taken from the first sorted rail-end point without checking that there is one. The sort loop skips every vertex that is unprocessed, `SM_FV_MATE` or `SM_FV_RAIL_X_EXTENDED_EDGEUSE`, and every point whose uv is unset (`-SM_BIG_DOUBLE`, `:11494` and `:11505`). When it skips them all, `sTsectPnts` is empty, so `sTsectPnts[0]` reads past the array (its stack buffer) and dereferences that pointer.
- **Trigger:** `ReCalcFilletGeom(FALSE, NULL, NULL)` on a geometry whose rails' end vertices are all unprocessed (or all mates), both rails present and rail 0 without a curve: the `recalc_setup_fails` fixture with every vertex `status` 0 and a solver whose `SetupOffsetValues` succeeds (`regress_recalc_no_dir_all_unprocessed`), or with every vertex a processed mate (`regress_recalc_no_dir_all_mates`).
- **How found:** reviewer's adversarial fixtures on PR #242 (core dump in the difftest process).
- **Confirmed:** yes (both regression fixtures segfault the reference build, SIGSEGV in the difftest process, when the shim's guard is disabled).
- **Severity:** crash (out-of-range read, then a wild pointer dereference).
- **Rust port:** unreachable today (`FilletBrep::re_calc_fillet_geom` stops at the fillet intersector's point intersection first); the march direction there takes the first point through `.first()` and returns `SolidError::InvalidInput` when there is none. The `blend.fillet_geom_p3` shim refuses (`SC_ERR_ARG`) exactly that model: no march direction, rail 0 without a curve, and a sort loop that would finish without a point.
