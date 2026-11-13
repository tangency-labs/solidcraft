# Fuzz-corpus replay: Rust-side hangs, aborts and one difference (fix/fuzz-rust-side-guards, 2026-10-22)

Replay of the fuzz crash corpus against main (area 5f inventory: `difftest --fuzz-exec`,
4 GiB address-space limit, 20 s per side) left 1 input whose sides differ, 1 Rust abort,
25 Rust hangs and 23 inputs whose outcome depends on the machine's memory.

- **`surface.plane_p2` fuzz00842 (sides differ): the C++ is wrong.** An ASCII `read_db`
  stream with an empty plane header. `SmPlane::ReadFromDB` reads its 15 values from the
  B-spline header and control points, so `SmBSplineSurface::ReadFromDB` reads `m` from
  `.69...` and the stream fails; `n m p q rat` then stay uninitialised (UB-6). The C++
  returned a different surface on every run (`rec` lengths 40251, 62874, 65074) or
  segfaulted; the port refuses the unreadable header. The runner now refuses a plane
  header of other than 15 values on both sides (`db_bytes`; the generator always writes
  15). Regression: `tests/fuzz_corpus/surface.plane_p2.json`.
- **`curve.curve_p1` fuzz01255 (Rust abort, 6 GiB): unbounded approximation.** A break at
  `1e300` on a line: `ApproximateCurve`'s spans pass the accuracy test only when tiny
  (about `1e-4` at `t = 5e10`), so the control polygon grows without end (upstream too;
  HANG-46). `Curve::approximate_curve_base` now returns `InvalidInput` past
  `MAX_APPROX_SPANS = 2^20` spans and when a failing span can no longer be split (its
  midpoint rounds onto an end with that end's data, where upstream repeats one state
  forever). The input stops in 0.36 s.
- **Rust hangs (25 inputs): the Rust side ran inputs the FFI refuses.** In 24 the C++ side
  refused the input (`1e300` values, mixed rationality, `bad index/count`, HANG-29) and the
  faithful Rust code then spun where upstream does: `GlobalCoincidenceChecker` recursion
  (HANG-29 regime: composite_curve_p2 fuzz00430/01007, curve_p8 fuzz01456/01856,
  surface_p8 fuzz01603; heal_data_p1 fuzz00977/01666 hang in the C++ B-rep export the Rust
  side also calls), `CurveCache::tessellate` subdivision (composite_curve_p2
  fuzz00198/00435/01111/01988), `ApproximateCurve` (crv_on_surf fuzz00526/01785,
  curve_p1 fuzz01951), `GlobalSolver::solve_branch` (curve_p9 fuzz00144/01043/01222),
  `SurfaceCache::subdivide_to_tolerances_with` (surface_point_solve_p1 fuzz00979, p2
  fuzz01057, p3 fuzz00349), `remove_knots_constrained` (nlib.crv_approx fuzz05147) and
  `TessSrfCache::mark_face_containment` (query.tess fuzz00241/00353, a tree whose node is
  its own child). Each of those runners now applies the FFI's own validation to the Rust
  side first (`run_rust`), as `curve.curve_p8`'s generator and the offset runner already
  did, so both sides refuse with one message. Library change only for query.tess: the
  descent of `mark_face_containment` stops after as many visits as the tree has nodes
  (shared or cyclic child links), as its parent walks already did. curve_cache fuzz01735
  spent its time in the runner's quadratic `tree_out` (now a map). Regressions: the 24
  inputs in `crates/difftest/tests/fuzz_corpus/<unit>.json` (both sides refuse; 13
  earlier corpus cases whose Rust side ran on a refused input now record the refusal),
  and library-direct tests for the two library bounds (curve_p1, query.tess runners).
- **The 25th hang is not a hang:** `surface.surface_point_solve_p2` fuzz00474 runs on both
  sides and agrees, the C++ in 11 s, the Rust in 21 s (`SurfaceCache` subdivision of a
  tiny `1e-5` surface, as HANG-15: the port is about 2x slower there). Not changed.
- **Memory-dependent inputs (23).** All allocation sites are fallible (`try_reserve`) except
  that they zero-fill after a successful reservation, so a machine that overcommits 16-800
  GiB pages them in: `Grid::new` voxel counts (geom.grid, intersect.ray_tracer),
  `IterQueue1d::try_new`, `SolutionArray::reserve_to`, `fit_arrays::alloc` from a degree of
  2^31 (crv_fit p1/p2, srf_fit, srf_adv_p3, srf_offset, b_spline_curve_n_lib),
  `srf_basic::size_arrays`, `nurbs_crv::basis_derivs` (derivative order 2^31),
  `create_regular_polygon` (sides), `BSplineCurve::control_points_expert` (stride 2^31).
  The four nlib.crv_approx inputs are now refused on the Rust side by the FFI's
  validation; so are b_spline_curve_p2 fuzz00841 and b_spline_curve_n_lib fuzz00880. The
  others are sizes the caller asks for (upstream allocates them too) and stay as they are.
