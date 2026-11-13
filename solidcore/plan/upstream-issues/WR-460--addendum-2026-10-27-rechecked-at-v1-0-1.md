Updates: WR-460

#### WR-460 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Unchanged in v1.0.1: `SmFilletExecutive::DoFilleting` has no comparison of the rails of solvers that share a face; it moved from `SmFilletExecutive.cpp:2917` to `:2957` and ends at `:3759` (was `:3719`). On `main` with the v1.0.1 port the five `issue_1073_*` tests still pass (the open body by default, the refusal with `fillet_refuse_crossing_rails`). The record's C++ agreement was run on c9c5979; difftest against the C++ 1.0.1 still passes `blend.fillet_executive_p1`, `_p2` (155 and 278 cases, max_rel_err 0), but the `real_solid` cases of the record were not re-run on the 1.0.1 build.
