# Platform-skipped controls — post-#793 outcomes re-derived (2026-10-24)

Fresh full runs, not the pre-#793 sweep's failure counts:

- `surface.srf_in_volume`: 2112 cases, 2037 passed, 0 failed, 0 deferred,
  75 `skipped(platform)`, 0 rejected, 0 known mismatches, `max_rel_err=0`,
  exit 0. The existing non-libstdc++ hook skips `ReadDb` operations only.
- `topology.database_io`: 2066 cases, 0 passed, 0 failed, 0 deferred,
  2066 `skipped(platform)`, 0 rejected, 0 known mismatches,
  `max_rel_err=0`. All cases are skipped for libc++ extraction differences.
  Although #793's aggregate verdict prints `PASS`, the command exits 1:
  `GATE-FAIL (value-share) 0 of 0 passing cases compare a value (minimum 75% and at least one)`.
  This is an independent coverage gate, not 2066 passes and not a crash.
- `curve.curve_p6`: 3138 cases, 3135 passed, 0 failed, 0 deferred,
  3 `skipped(platform)`, 0 rejected, 0 known mismatches, `max_rel_err=0`,
  exit 0. Exact skip names: `gen0161`, `gen0953`, `gen2879`.
- `nurbs.b_spline_curve_n_lib`: 2326 cases, 2319 passed, 0 failed,
  0 deferred, 7 `skipped(platform)`, 0 rejected, 0 known mismatches,
  `max_rel_err=0`, exit 0. Exact skip names: `gen0695`, `gen0720`,
  `gen0880`, `gen0932`, `gen1279`, `gen1374`, `gen1482`.

The latter ten JSON case records all have `pass=false`,
`outcome="skipped(platform)"`, `values_compared=0` and `exact_compared=0`.
They are the landed #793 controls for #449 / UB-251's allocator-dependent
`DeformToEndPointTargets` use-after-free, not ten newly passing deform checks.
The skipped reader controls cover the already documented libc++/libstdc++
environment differences. What would settle broader support: validate a safe
same-platform oracle and an explicit coverage policy; no loosening of the
zero-value gate or blanket conversion of skips to passes is proposed here.
No new upstream entry: these mechanisms already have upstream issue records.
