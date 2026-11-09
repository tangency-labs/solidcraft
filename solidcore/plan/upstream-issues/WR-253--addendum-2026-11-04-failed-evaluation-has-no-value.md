Updates: WR-253

#### Updates: WR-253 (2026-11-04). A failed evaluation has no value: the default, and the minimum-radius search

- **Upstream:** three places together report a radius of 0 on a curve whose radius of
  curvature is unbounded (tangency-labs/solidcore#2574):
  - `source/SMLib/src/SmCurve.cpp:15402` (`SmCurvePropertyEFO::Evaluate`, case
    `SM_CP_MINIMIZE_RADIUS_OF_CURVATURE`) returns `SM_ERR` where `|C' x C''|` vanishes,
    after zeroing `rdFOfT` and `rdFPrimeOfT` at `:14919`. The closed form there is a
    radius without bound, so that parameter is not a minimum of it.
  - `source/SMLib/src/SmLocalSolve1d.cpp:799` and `:1083` (`SmLocalSolve1d::SolveIt`) and
    `:431`, `:473` (`SolveOscillating`) evaluate straight into `m_vCurr` / `sTempIter`, so
    a failed evaluation leaves its zeroed `F` as the iteration value. `SolveIt` without an
    interval then iterates from it, and `SolveOscillating`'s sign test at `:495` reads the
    last guess's `F` whether it evaluated or not (this record, and #481).
  - `source/SMLib/src/SmCurve.cpp:15993-16012` (`SmCurve::LocalPropertyAnalysis`) exempts
    the four minimisation and maximisation properties from the no-solution return and
    reports the parameter the solver stopped at, with `sLS.m_vCurr.m_dFOfT` as its
    property value — a value the search may never have evaluated.
- **Effect:** every parameter of a straight B-spline trim has `|C' x C''| = 0`, so the
  `SM_CP_MINIMIZE_RADIUS_OF_CURVATURE` search evaluates nowhere, and the chain above
  answers "minimum radius 0" at the first zeroed guess. The radius test of
  `SmBSplineCurve::AssertValid` (13 in 2D, 14 in 3D) compares that 0 against its radius
  tolerance and reports a cusp on a straight line.
- **Rust port (the default since #2574):**
  - `CurvePropertyEfo::evaluate` (`crates/solidcore/src/curve/curve/p4.rs`) answers the
    closed form where `|C' x C''|` vanishes: `F = BIG_DOUBLE` with `F' = 0` and no found
    answer, instead of an error with `F = 0`.
  - `LocalSolve1d::eval_into` (`crates/solidcore/src/geom/local_solve1d.rs`) leaves the
    iteration value as it was when the evaluation fails. With that, `SolveOscillating`
    takes a sign change only from a guess that evaluated and `SolveIt` without an interval
    returns the evaluator's status, so WR-253's correction is the behaviour of the solver
    itself: the per-instance switch `LocalSolve1d::set_failed_guess_no_sign_change` and
    the `Kernel::fixes.oscillating_skips_failed_guesses` forwarding to the 1-D solver are
    gone. The flag stays for `LocalSolveNd` (WR-20, #1945), which is untouched here.
  - `local_property_analysis` (`crates/solidcore/src/curve/curve/p5.rs`) reports a
    minimisation's or maximisation's stop point only when `F` there is a value the search
    really has. Their `found` is decided by `|F| < acceptable_accuracy`, a test those
    objectives are not built for, so the stop point stays the solution; what is dropped is
    the case where `F` is not finite or is the `BIG_DOUBLE` the objective answers where it
    has no finite value, and the case where a failed evaluation left no value at all.
    Upstream reports the stop point whatever `m_vCurr.m_dFOfT` holds. For the other
    properties a search that found nothing reports no solution, as before.
- **cpp_defect:** the pinned C++ (usd-brep v1.0.1) differs by design from here on. On a
  straight trim `SmCurvePropertyEFO::Evaluate` fails with `F = 0` where solidcore answers
  an unbounded radius, `SmLocalSolve1d` keeps a failed guess's `F` where solidcore keeps
  the last evaluated one, and `SmCurve::LocalPropertyAnalysis` reports the solver's stop
  point as a minimum whatever value stands there, where solidcore reports it only when the
  search has a finite value for it. The difftest is a diagnostic, not the gate
  (AGENTS.md §1), so no tolerance or known-mismatch entry is changed.
- **Tests:** `crates/solidcore/tests/issue_2574_straight_trim_min_radius.rs` (the real
  trim out of the check: straight in closed form, no finite minimum, both radius tests
  clean; and a cubic with a tight but regular bend next to either end of its domain, whose
  real minimum radius of 5.8156e-6 the radius test still reports against its tolerance of
  1.4142e-4, so the change does not become a blanket "no solution"), `geom::local_solve1d::tests::failed_first_evaluation_without_interval_has_no_value`,
  `geom::local_solve1d::tests::issue_481_failed_guess_is_no_sign_change`,
  `curve::curve::p5::tests::issue_481_the_failed_last_guess_is_no_root` and
  `blend::identify_fillet::tests::issue_2009_radii_solve_takes_no_value_from_a_failed_guess`.
  They fail without the change.
