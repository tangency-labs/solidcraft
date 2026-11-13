# nlib.srf_adv_p2 (`NL_SrfAdv.cpp`, part 2)

No Rust-vs-C++ disagreement: 1695 of 1695 default difftest cases bit-identical (195
fixtures: extensions of the three `pt_TestFiles` `.sms` surfaces and the
`TestSmCreateRectangle` patch in every direction / end / continuity, small and large
distances, in place and not, plus normal derivatives; 1500 generated), and a 30,195-case
run (`--seeds 30000`) also clean. The file-private `ST_surxsdr` is ported as
`srf_adv::p2::extend_by_dist_at_depth`; its one-line public wrapper `N_SrfExtendByDist`
belongs to `nlib.srf_adv_p1` (call it with `depth = 0`). The shim calls `ST_surxsdr`
directly (it has external linkage). No `UNPORTED(nlib.srf_adv_p2)` stubs existed to wire.
Upstream behaviour worth knowing:

- **Stale error number past the recursion limit.** `ST_surxsdr` returns `1` without
  `N_ErrSet` at `depth > 10`. `NL_ERROR` then holds whatever was recorded last: errors of
  the sampled iso-curve extensions (which it ignores), `N_VectorNormalize`'s `NL_GEO_ERR`
  for a degenerate reflection plane, or `0` after a callee's `N_ErrClear` (e.g. in
  `NL_CrvNurb.cpp`). `DropCurveSurface.sms` extended 900 units in v hits the limit with
  `eno = 17` at one end and `0` at the other. The port returns `NotConverged`; the runner
  compares an upstream failure as the bare flag (status 1000) only when the port reports
  the recursion limit (a C++ success there still fails the case).
- **Zero distance hangs for G1/G2.** `NL_G1`/`NL_G2` extend the sampled iso-curves with
  `ST_curxcdr` `NL_G2`, whose parameter search never ends for `dist = 0` (see
  `nlib.crv_adv_p1`). The difftest uses `dist = 0` only with `NL_CMAX` / `NL_G1R`.
- **Degree-1 reflection fallback reads past `Rw`.** When the first reflection-plane
  normal is degenerate, `ST_surxsdr` retries with `Rw[ii][kby + 2]` (start) /
  `Rw[ii][kby - 2]` (end); for a span of a degree-1 direction that is outside the
  `2 * kby + 1` row. The port returns `InvalidInput` there.
- `NL_G1` maps to the same reflection algorithm as `NL_G2` (only the number of boundary
  knots removed afterwards differs: 1 vs 2); `NL_G1R` extends the iso-curves with `NL_G1`
  but builds a ruled extension. A direction flag other than `NL_UDIR` extends in v; an
  `end` other than `NL_START` extends the end. The `static int cbi = 0` branch
  (`N_SrfExtendByParamDist`) is dead.
- `N_SrfEvalPtNormalDeriv` fails with `NL_NUM_ERR` where `|S_u x S_v|` is zero (a pole).
