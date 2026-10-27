Updates: WR-461

#### WR-461 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Unchanged in v1.0.1: `NL_SrfAdv.cpp` is identical to c9c5979, so `N_FillNSidedHole` (`:18585`) and its "Adjust derivative magnitudes" step (`:19348`) are where the record says. The three tests of `crates/solidcore/tests/issue_1170_corner_patch_inside.rs` still pass on `main` with the v1.0.1 port (the upstream patch bulges past the box for a large setback; with `corner_blend_stays_inside_neighbours` it stays inside, G1 kept). The default path agrees with the C++ 1.0.1 in difftest (`nlib.srf_adv_p1`..`_p6`: all pass, max_rel_err 0). **Confirmed:** still no, as in the record (not our own C++ run of the setback corner).
