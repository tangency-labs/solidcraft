Updates: WR-253 (and WR-20)

#### Updates: WR-253 (2026-10-31). `FindFilletMinMaxRadii`'s 1-D solve takes the opt-in

- **Upstream:** `source/SMLib/src/SmSurface.cpp`, `SmSurface::FindFilletMinMaxRadii` builds
  `SmLocalSolve1d sLS( sEFO, sDomain, FALSE )` and refines each distance extremum it brackets with
  `SolveIt`, which can fall back on `SolveOscillating`, the function this record is about.
- **Fork fix:** tangency-labs/smlib@d2757e1 changes `SmLocalSolve1d` itself, so every 1-D
  solve in the fork has it, this one included.
- **Rust port:** the 1-D solver takes the fix per instance (`LocalSolve1d::set_failed_guess_no_sign_change`),
  and every caller passed `Kernel::fixes.oscillating_skips_failed_guesses` except
  `blend::identify_fillet::find_fillet_min_max_radii`, which built its solver without it. It now
  takes the flag through `radii_solver` (`crates/solidcore/src/blend/identify_fillet.rs`). Flag
  off: unchanged, as upstream.
- **Effect:** none known on a real fillet surface; found by reading the solver's callers (#2009).
- **Test:** `blend::identify_fillet::tests::issue_2009_radii_solve_honours_oscillating_skips_failed_guesses`
  (an evaluator that fails at `SolveOscillating`'s last guess: flag on, no root; flag off, the
  failed guess is reported as a root, as upstream). It fails without the change.
