Updates: WR-35

#### WR-35 (update, 2026-10-31). Eight-corner angle bounds, radian turning-angle cull and base `opt_vectors` in `SmCurvePropertyGS`

All three defects in the record are present in pinned v1.0.1 `source/SMLib/src/SmCurve.cpp`:

1. **`SmCurvePropertyGS::BranchMayContainAnswers` (`SmCurve.cpp:12223`):** in the triple loop
   (`i`, `j`, `k` in `0..1`) for `SM_CP_MINIMIZE_ANGLE_TO_PLANE`,
   `SM_CP_MINIMIZE_DIRECTED_ANGLE` and `SM_CP_MINIMIZE_CCW_ANGLE`, line `:12223` computes
   `dZV = 1.0 * j` instead of `1.0 * k`. The four corners with `y == z` indices are tested
   twice each and the four `y != z` corners are skipped.
2. **`SmCurvePropertyGS::BranchMayContainAnswers` (`SmCurve.cpp:12378-12379`):** for
   `SM_CP_PERPENDICULAR_TO_VECTOR` and `SM_CP_MAXIMA_TO_PLANE`, `dAngle` from
   `sLineVec.AngleBetween(sRef)` (`:12375`) is in radians while `dTurn` from
   `pBezSpan->m_dMaxTurningAngleDeg` (`:12346`, tested against `90.0` degrees at `:12347`) is
   in degrees; `:12378-12379` tests `dAngle + dTurn < SM_PI / 2.0 || dAngle - dTurn > SM_PI / 2.0`
   without converting `dTurn` to radians (`SM_DEG2RAD`).
3. **`SmCurvePropertyGS::m_cpOptVectors` (`SmCurve.cpp:11970`, `:12005`, `:13047`):**
   `SmCurvePropertyGS` declares its own `m_cpOptVectors`, shadowing
   `SmGlobalSolver::m_cpOptVectors` (`SmGlobalSolver.h:98`), and
   `SmCurve::GlobalPropertyAnalysis` (`:13047`) passes `NULL` (`&[]`) to `SolveIt`, leaving
   the base solver's `m_cpOptVectors` null (`base.opt_vectors` empty).

- **What happens (default, pinned):**
  - When an earlier-visited sibling branch lowers `m_dBestAnswerSoFarSq` and a valid
    `SM_CP_MINIMIZE_*ANGLE*` branch beats `m_dBestAnswerSoFarSq` only at its `y != z` box
    corners, `BranchMayContainAnswers` returns `FALSE` and prunes a branch containing the true
    global minimum.
  - Any curved `SM_CP_PERPENDICULAR_TO_VECTOR` or `SM_CP_MAXIMA_TO_PLANE` span whose turning
    angle in degrees exceeds `|SM_PI / 2 - dAngle|` (~`1.57` at most) fails to cull even when
    the span tangents stay far from perpendicular, descending into unnecessary subtree
    subdivisions and local Newton solves.
  - `GlobalSolver::opt_vector` on `CurvePropertyGs::base` has no vectors (the single-tree base
    traversal itself does not read `m_cpOptVectors`).
- **Fix (opt-in, #2144):** `Kernel::fixes.curve_property_gs_branch_bounds`, default `false`
  (pinned C++ parity). When enabled:
  - `CurvePropertyGs::may_contain_answers` (`crates/solidcore/src/curve/curve/p2.rs`) uses
    `zv = f64::from(k)` so all eight bounding-box corners are evaluated, and scales `turn` by
    `PI / 180.0` (`SM_DEG2RAD`) before testing `angle +/- turn_bound` against `PI / 2.0` (while
    keeping the `turn > 90.0` early-accept guard in degrees).
  - `CurvePropertyGs::new` (`p2.rs`) copies `k.fixes` and `opt_vectors` into `base`,
    `CurvePropertyGs::opt` reads `base.opt_vectors` when populated, and
    `Curve::global_property_analysis_into` (`crates/solidcore/src/curve/curve/p7.rs`) forwards
    `vecs` in `SolveRequest::opt_vectors` instead of `&[]`.
- **Tests:** `wr35_flag_lookup_and_base_opt_vectors`,
  `wr35_angle_minimize_eight_corners_keeps_valid_branch_default_prunes`,
  `wr35_perpendicular_and_maxima_to_plane_convert_turning_angle_to_radians` and
  `wr35_global_property_analysis_perpendicular_and_maxima_preserve_solutions_and_prune` in
  `crates/solidcore/src/curve/curve/p2.rs`.
- **Confirmed:** read at the pin (`3fd5b95`, `SmCurve.cpp:11970`, `:12005`, `:12223`,
  `:12378-12379`, `:13047`); default behaviour stays bit-for-bit identical to the pinned
  reference. `WR-35` is an archive record ID, so no numeric GitHub issue is closed here.
