# intersect.curve_class_p1 (`SmCurveClassification`, part 1, PR #47 review, 2026-10-14)

- **CI red on clippy with the C++ feature.** `cargo clippy --workspace --all-targets
  --features difftest/cpp -- -D warnings` reported 11 errors in the runner
  (`crates/difftest/src/units/intersect_curve_class_p1.rs`: identical match arms in
  `gen_tol`, needless range loop and unchecked indexing in `gen_script`, `range_plus_one`,
  `default_trait_access`). A plain `cargo clippy` does not build the `cpp`-gated
  runner, so run clippy with `--features difftest/cpp` too. Fixed without changing the RNG call order: the
  generated cases are identical in name, value count and outcome.
- **UV trim curves only reached by fixtures.** The generator always passes `uvc: []` to
  `xsect_curve`, so `SetIntersectionCurveThroughPoint` with UV trim curves is covered
  only by `fixtures/intersect.curve_class_p1/adversarial.json` (75 cases: UV-curve,
  huge/tiny-scale and out-of-range-UV xsect/apply cases, analytic Line/Circle/Ellipse/
  Hermite source curves, a container case past `MIN_ARRAY_SIZE`). At unit and huge scale
  the UV-curve cases defer on `SmSurface::LocalPointSolve` (`surface.surface_p5`, 20
  cases); only the tiny-scale ones are compared until that unit lands.
- **SmTol::MapTo2d for a UV curve (second review).** `sm_GetXSectTol2dOr3d_ForClassifyPair`
  case 1 (2D curve with `m_cpSurface1`) was stubbed `UNPORTED(surface.surface_p1)` after
  that unit became partial. Its UV-curve branch is now ported as the public
  `surface::surface::p1::map_to_2d_on_curve` (co-port of geom.tol's overload;
  `SmCurve::Evaluate` then `map_to_2d_dir`,
  through `SmTol2d` and `SmZoneTol3d` as upstream); only a curve of another dimension
  still needs `SmSurface::DropPoint` (`surface.surface_p5`, unreachable from the classify
  pair). Covered by `fixtures/intersect.curve_class_p1/review_tol_pair.json` (37 cases:
  flat, curved, pole-degenerate, huge and tiny B-spline surfaces and a plane; zero-tangent,
  collapsed-edge, out-of-bounds and Hermite UV curves; USE_DEFAULT, negative, 0 and 1e3
  tolerances; in- and out-of-domain params), and half the generated TolPair ops now use a
  fresh UV curve on a surface. All pass with max_rel_err 0.
- **Concrete-Surface mapping wired.** `cull_or_refine_cci_in_3d`
  (`sm_CullOrRefineCCIIn3d`) now builds its `CrvOnSurf` over the actual classification
  surface. Exact UV/surface snapshots preserve shared raw knots and owners instead of
  replacing a plane with its B-spline part. The old `curve.crv_on_surf` construction
  deferral is removed; genuine classification/topology dependencies remain explicit.
- **Not compared.** The shim never builds an `SmSurfOfRevolution` or `SmSurfOfExtrusion`,
  so the `SurfOfRevolution` arm of `out_of_bounds()` (the base-flag-only
  `SmTemporaryChangeValue`) is ported but not difftested; planes and B-spline surfaces are.
