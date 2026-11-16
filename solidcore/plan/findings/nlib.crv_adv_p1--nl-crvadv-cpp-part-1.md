# nlib.crv_adv_p1 (`NL_CrvAdv.cpp`, part 1)

No Rust-vs-C++ disagreement: 2670 of 2670 default difftest cases bit-identical (670
fixtures: the six upstream `.smc` curves, the `TestSmCreateRectangle` /
`TestSmCreateRegularPolygon` edges as degree-1 curves, and edge cases; 2000 generated),
and a 100,670-case run (`--seeds 100000`) also clean. The file-private `ST_curxcdr` /
`ST_curxcrr` are ported as `Curve::extend_by_dist_at_depth` /
`Curve::extend_by_param_dist_at_depth`; their one-line public wrappers
`N_CrvExtendByDist` / `N_CrvExtendByParamDist` belong to `nlib.crv_adv_p2` (call them
with `depth = 0`). The shim reaches the `ST_` routines through those wrappers. No
`UNPORTED(nlib.crv_adv_p1)` stubs existed to wire. Upstream behaviour worth knowing:

- **`ST_curxcrr` in place frees an uninitialised curve.** Its `EXIT` label runs
  `if( curP EQ curQ ) N_FreeCrv( &curA, SP )`, but `curA = *curP` is assigned only after
  the `NL_G2` two-step recursion (`par` more than `0.9213` parameter ranges outside) and
  the `N_BasisFindSpanAndMult` calls. An in-place `NL_G2` call that recurses (or fails
  before that point) frees stack garbage: the difftest crashed with SIGSEGV on such a
  case. The ffi rejects in-place `NL_G2` extensions that would recurse; out of place
  (and every nested recursive call: the second step is in place but never recurses) is
  fine. `ST_curxcdr` frees `curA` before its `EXIT` label and has no such problem.
- **Bare error flags.** The recursion limits (`ST_curxcdr` depth > 50, `ST_curxcrr`
  depth > 10) `return (1)` without an `NL_ERROR` number, as does `ST_curxcrr`'s
  `NL_QUIT` when the knot-inserted curve has an unexpected size (unreachable for clamped
  curves). The port returns `NotConverged` / `InvalidInput` (`BARE_FLAG_REASON`); the
  difftest maps both to status 1000. A relative `NL_G2` distance above ~24.8 curve
  lengths hits the `ST_curxcdr` limit, a `NL_G2` parameter more than ~6 ranges out hits
  the `ST_curxcrr` limit.
- **`ST_curxcdr` `NL_G2` parameter search never ends** when the step
  `du = range * ldst / d1` no longer changes the parameter (or is zero: `dist = 0`), and
  reads outside the knot range for a negative distance. The port fails with
  `NotConverged` when a step changes nothing; the ffi only passes positive distances of
  at least `1e-3` curve sizes for `NL_G2`.
- **Tangent fallbacks read past short curves.** When the end pair of control points
  cannot be normalised, `NL_G1`/`NL_G2` (of `ST_curxcdr`, `NL_G1` of `ST_curxcrr`) retry
  with the third point from the end, which does not exist for `n < 2`; the ffi rejects
  that. `ST_curxcrr`'s `NL_G2` has no fallback (`NL_GEO_ERR` at once).
- **`N_CptReflect`'s flag is ignored** by both extension routines; it cannot fail for
  their (normalised) plane normals. The port propagates it.
- **Extensions that do not extend.** `ST_curxcrr` with `par` on or inside the extended
  end, and `ST_curxcdr` with a non-positive distance, build knot vectors with too many
  equal or decreasing knots that later calls index with; the ffi requires a strict
  extension. `NL_CMAX` secant steps of `ST_curxcdr` can in principle leave the span
  length non-positive; never seen in the runs.
- `N_CrvEvalEvenSpacedPts` reads the global `NL_ITLIM` (default 50, lowered temporarily
  by some approximation routines); the port uses `crv_geom::ITER_LIMIT`.
