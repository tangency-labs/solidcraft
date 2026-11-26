# curve.curve_p6 (SmCurve part 6: boxes, planarity, drops, gaps, coincidence, deform)

No Rust/C++ disagreements (3136 cases, 3048 bit-identical, 88 deferred on `curve.curve_p5`;
30k-seed run clean; `--perturb` SENSITIVE). Notes for later porters:

- **`CurveMaxDistanceBetween` with `lNumSamples == 1`** divides `0.0 / 0.0` (`ii /
  (lNumSamples - 1)`) and evaluates both curves at NaN parameters. The port computes the
  same NaN; the ffi rejects one sample.
- **`CurveMaxDistanceBetween` early returns** (the gap limit `pdOptMaxDistanceNeeded` hit,
  the same curve over the same interval) leave `pbOptIncorrectDir` unwritten; the port
  returns `incorrect_dir: None` there.
- **`DeformToEndPointTargets`** ignores the status of `ApproximateCurve` and then calls the
  B-spline deform through `*ppOptNewCurve` (a null dereference when the approximation
  fails); the port returns the error. The `SmCrvOnSurf` branch is taken only with the
  `pOptSurfCrvOnSurf` argument the port drops (see `curve.crv_on_surf` below).
- **`GlobalCoincidenceChecker`** recurses on closed and half closed curves without a
  depth bound; the port stops at `MAX_COINCIDENCE_DEPTH` (200) nested splits with
  `InvalidInput` (AGENTS.md recursion rule). Its recursive calls use the default
  `bSnapToEnds = TRUE`, whatever the caller passed; the port keeps that.
- **`IsPlanar`** ignores the status of `CalculateBoundingBox`; a failed box leaves the
  default-constructed pseudo box, which the port reproduces.
- **Stubs left.** `UNPORTED(curve.curve_p6)` in `geom/tools.rs` (`DropPoint`,
  `CurveMaxDistanceBetween` on the opaque curve handle `C` and on edge curves) stay, as
  the `curve.curve_p2` stubs there did: the handle cannot become a `Curve` until edges
  carry real curves (`topology.edge`). They are behind `topology.*` stubs, so no
  `geom.tools` case is deferred on `curve.curve_p6`. The ellipse's
  `GlobalCoincidenceChecker` stub (`curve.ellipse_p2`) is retagged
  `UNPORTED(curve.ellipse_p1)`: the checker is ported but needs `SmEllipse` as a curve
  kind (its `IsDegenerate` / `GetDegree` overrides), as `curve.curve_p1` did for the
  ellipse's `GlobalPointSolve` (`curve.ellipse_p2`: 1 case moved from `curve.curve_p6` to
  `curve.ellipse_p1`).
- **Wired at merge.** `curve.curve_p1` landed while this unit was in flight; its
  `GlobalPointSolve`, `Tessellate` and `ApproximateCurve` are called directly (288
  deferred cases became passes).
