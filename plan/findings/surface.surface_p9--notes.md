# surface.surface_p9

- Owns `FindTessellationSplits`, `CreateIsoBoundaries` and the CSI leaf `LocalSolve`
  body in `SmSurface.cpp:11644,13887,14877`. Default report: 778 cases (178 fixtures
  and 600 fixed-seed generated), 727 bit-identical, zero failed, 51 deferred,
  max relative error zero, no absolute floors.
- The 50 diagonal-tessellation deferrals reach curve classification in
  `intersect.curve_class_p4`. The CSI helpers now call `surface.surface_p1`;
  the disjoint fixture passes and the crossing fixture reaches `surface.surface_p6`.
  Later p4/p5/p6 callees and the identical-solution virtual remain explicit
  Unported back-edges. End-to-end intersections are not yet verified.
- **CO-PORTED(surface.surface_p1).** Reuse its existing `CsiGlobalSolver` and
  constructor, extending its state with optional borrowed curve/cache/kernel
  context for leaf solves. No duplicate solver type. `Step` surfaces explicitly
  defer their generic iso-curve construction to `surface.surface_p5`.
- Sixteen explicit real-plane cases with diagonals disabled and maximum side
  lengths including -0.5 and 0.5 pass against C++; diagonal classification still
  defers to its owning unit.
- CSI fixture caches use a nonzero chord tolerance. Cache preparation failures
  are harness failures, never matching operation errors. The reference solver
  must also return success. The surface fixture is one planar quadratic patch;
  Rust assembles its one-leaf arena because general surface tree construction is
  not available yet. Further CSI coverage must use fully equivalent constructed
  caches when those dependencies land.
- B-spline iso extraction is exact, so the approximation tolerance has no effect
  for that supported kind. Curve-bounded surface virtuals defer to surface_p5.
  The maximum-side branch retains the upstream last-sampled length for its
  equally-spaced-point tolerance, even if another sampled curve was longest.
- Input-driven tessellation recursion has an explicit depth limit of two (the
  recursive call sets maximum side length to zero). Split arrays are bounded to
  1,000,000 entries including endpoints. Invalid/nonfinite/negative derived counts
  return an error instead of converting to an unsigned count. The FFI excludes
  the all-degenerate maximum-side case where upstream's negative count conversion
  is unsafe. Two kernel tests cover analytic boundaries and excessive counts.
- Ignored non-Unported tessellation failures leave empty returned arrays in Rust;
  the C++ out-array API can retain partial arrays on failure. This deviation is
  documented in the module header. All Unported errors propagate explicitly.
  PR #6 review found an incorrect default degeneracy tolerance (1e-5 instead of
  1e-12) and dropped maximum-side arguments. A new 96-case corpus reproduced 32
  disagreements before those fixes and is bit-identical afterwards. Coverage now
  includes poles, nearly degenerate boundaries, tiny patches, subdomains, angle-only
  tessellation, negative maximum side lengths and real analytic SmPlane instances.
  The C++ guard tests the actual six sampled iso curves and rejects only the
  all-degenerate positive maximum-side case below 1 where the negative count cast
  is undefined, within the runner's documented finite input bounds.
- Wired FindTessellationSplits into SurfaceCache::SubdivideByBlock, with the caller's
  Kernel passed through SubdivideToTolerances. Cache p2 compares 808/905 cases
  (97 deferred); p3 compares 450/600 (150 deferred). No tolerances changed.
- CSI resolves the original owner handle against the supplied surface cache.
  IsClosed and tangent-plane drop now call the already ported surface_p7 operations.
  `surface_to_evaluate`, `is_node_valid` and `is_point_valid` call p1 directly.
  The single crossing fixture is deferred at its next genuine missing dependency.


- **Latest-main revolution kind.** The surface dispatcher now includes
  `SurfOfRevolution`. Borrow its underlying B-spline for implicit iso evaluation;
  use its real virtual iso constructor for extracted boundaries, retaining the
  returned circle's NURB. Eighteen canonical-cylinder comparisons pass (sixteen
  non-diagonal splitting cases and two boundary cases). This is a focused fixture
  surface, not a claim of exhaustive coverage of all revolution geometries.
  The reference owns its generator and checks `MakeNurb` setup independently.
  CSI inputs retain the previous B-spline cache construction; the FFI rejects
  revolution CSI because the one-leaf Rust fixture adapter does not model it.
