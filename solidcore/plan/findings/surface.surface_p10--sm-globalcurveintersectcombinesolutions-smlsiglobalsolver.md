# surface.surface_p10 (`sm_GlobalCurveIntersectCombineSolutions`, `SmLSIGlobalSolver::LocalSolve`, PR #93 review, 2026-10-15; partial, no disagreements)

- **Adversarial fixtures.** 31 review cases were added to `fixtures/surface.surface_p10/cases.json`
  (`adv_*`): inverted, degenerate and ray-degenerate line intervals, a 1e-13 line vector,
  zero tolerances with a grazing line, rays starting on, above and in the plane of the patch,
  an interval ending within 2*tol3d of the patch, domains inside by EFF_ZERO / a v sliver /
  a touching corner, the point/range swap in the closest-solution search (point index left
  on a range), the deviation and length exceptions for touching ranges, reverse-u overlap,
  overlap not between, small ranges with start dev > end dev, culling with invalid nodes,
  nvars=2 and a huge tolerance. Rust and C++ agree on all of them or both defer. Run: 867
  cases, 515 pass, 0 fail, 352 deferred; `--perturb` 515/515. Seven review candidates were
  left out because the FFI guard rejects them (inverted interval/domain, `ivl=[0.5,0.5]`
  combine, negative tolerance, and the three below).
- **FFI guard stricter than needed for the line inputs.** `MAX_COORD = 1e9` (the repo
  convention) rejects line points/vectors such as 1e12 and infinite-parameter intervals
  (+-1e30), which the C++ line solve would handle. Those inputs are therefore not compared.
  A later pass could relax the guard for the line of the line solve only.
- **`LsiGlobalSolver::new` (surface.surface_p1) leaves `surface_cache` and `kernel` as
  `None`.** A library caller that builds the solver with `new()` and runs it gets
  `SM_ERR_NULL_POINTER` from `local_solve` (formerly `Unported`). No library caller exists
  yet; whoever adds one must set both fields (as the surface_p10 runner does) or extend the
  constructor.
- **Coverage limit.** Every line-solve case that passes the box culls reaches
  `SmSurface::LocalCurveIntersect` (surface.surface_p6) and is deferred, so the guess drops,
  filtering, polar-box exit, line-end `LocalPointSolve` and the patch-boundary iso and
  coincidence code are checked only by reading (against SmSurface.cpp:18075-18823: guess
  order [first, .5, .1, .9], dUVEps only on the first containment test, the
  degenerate-interval exit only on the V boundary, the unexpanded pseudo box passed to
  `SimpleCoincidenceChecker`, `IsDegenerate` defaulting to `SM_EFF_ZERO`). Re-run this
  unit's difftest when surface.surface_p6 lands.
