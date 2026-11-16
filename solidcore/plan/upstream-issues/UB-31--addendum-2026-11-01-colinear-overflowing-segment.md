Updates: UB-31

`N_CPtsAreColinear(Pw, Qw, Rw, NL_THREED)` (`NL_Geom.cpp:6068`) also reads an unset distance for finite control points, not only for NaN input or a zero weight.

- **Trigger:** `N_DistPtLineSeg(Q, P, R, &d)` (`NL_Geom.cpp:2868`) returns `1` without writing `d` in either of two cases. In the first, `|PR|` is not below `NL_MTOL`, `Q` projects inside `PR` (both dot products are `>= 0`), and `N_FloatOpIsBad(|PQ x PR|, |PR|, NL_DIVISION)` holds. That happens for a segment whose squared length overflows: `|PR|` and the cross product's length are both infinite, and `inf / inf` is bad. In the second, neither end test holds, because a dot product is NaN. `N_CPtsAreColinear` ignores the status and compares the uninitialised `d` with `2 * NL_MTOL`.
- **How found:** the nightly fuzz run 41190675541 (2026-10-31), unit `nlib.geom`:
  - `fuzz00091` (base `gen1029`, `/rw/0 = -1e+300`): non-rational points `P = (-4.12, 4.71, 0)`, `Q = (-6.15, 6.96, 2.44)`, `R = (-1e300, -5.81, -11.43)`;
  - `fuzz00228` (base `gen0438`): `R`'s weight `-1e-300`, so its Euclidean image is about `(1.1e301, 1.7e304, 8.3e300)`.
- **What the two sides gave:** the C++ answered `NL_YES` both times. The Rust port's `cpts_are_colinear` gets the error from `dist_pt_line_seg_tol` and answers `false`, as the record already says.
- **Confirmed:** yes. Both inputs reach the unwritten branch: the decision steps were replayed in upstream's operation order (`solidcore_ffi::units::nlib_geom::colinear_3d_reads_unset_distance`). The C++ answer depends on the stack contents.
- **Harness:** the `nlib.geom` FFI now refuses `CptOps` control points for which `colinear_3d_reads_unset_distance` holds.
