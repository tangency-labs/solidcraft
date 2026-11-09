Updates: WR-459

#### WR-459 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Unchanged in v1.0.1 apart from line numbers: `SmFilletExecutive::DoFilleting` moved from `SmFilletExecutive.cpp:2917` to `:2957` (a new edge check before it adds 40 lines); `RemoveTopoEdges()` is called at `:3685` (was `:3645`) and `BooleanPostProcess( bCheckAllEdges )` at `:3702` (was `:3662`). `SmMerge::BooleanPostProcess` is still `SmMerge.cpp:827`-`:1317` with its loop over every edge at `:857`; the one change in that file is later, in the closing of intersection loops (`:2956`).

Rust: `issue_423_default_removes_the_targets_own_edge` and `issue_423_kept_faces_same_volume` still pass on `main` with the v1.0.1 port. The default path agrees with the C++ 1.0.1 in difftest (`blend.fillet_executive_p1`, `_p2`, `boolean.merge_p1`..`_p5`: all pass, max_rel_err 0). **Confirmed:** still no, as in the record: the C++ 1.0.1 was not run on the six-patch sphere.
