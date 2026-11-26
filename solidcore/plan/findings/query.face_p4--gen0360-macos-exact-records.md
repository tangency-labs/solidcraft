# query.face_p4: the importer's analytic approximation and placeholder gaps flipped loop classifications; resolved by exact records and refilled gaps (2026-10-25)

Follows "query.face_p4: two scale-1e-3 cylinder cases move with the importer's curve
approximation" in the archive `plan/findings.md`. On macOS (Apple Silicon, Apple clang
21) a third cylinder case failed: `gen0360` (seed `4634847627957580463`, a seam-crossing
band and a rectangle on an `SmCylinder`, scale 1, swapped): `classify_loop_ios[8]` cpp 1
rust 2, `[10]` cpp 1 rust 0, `[11]` cpp 1 rust 5, `looptree.len` cpp 17 rust 9. Linux CI
passed it.

Two harness defects, no kernel change:

1. **Approximated geometry.** The C++ side evaluates the real `SmCylinder` and its circle
   edges, the Rust side imported their B-spline parts only. Fix: the `query.face_p4` shim
   (and `query.face_p3`, which includes it) puts the exact database record before an
   analytic face surface (`sc_analytic_export.h`) and an ellipse or circle edge curve
   (`sc_exact_curve_export.h`); the shared `topology.face_p2` importer rebuilds them with
   the ported readers. `gen0157` and `gen0300` (the archived known mismatches, entries
   removed) pass with this alone.
2. **Placeholder gaps.** With exact geometry `gen0360` passed on macOS but failed on
   Linux x86-64: `classifiable` cpp 0 rust 1 (`NoClassifyZeroCrosscnt`). Traced in an
   Ubuntu 24.04 amd64 container with the prebuilt reference: the rectangle's bisector
   line (the same UV line, the same target loops and the same B-rep data as in a
   `query.face_p3` `bisector` case that agrees with C++) got 1 interval inside
   `ClassifyLoops` instead of 4, because `CurveOnClassify` (`bHasBigGaps`) read the edge
   uses' max edge/face gaps as `-SM_UNDEF_DOUBLE` (`-1.23456e-61`). The export marks a
   cached gap as set but carries no value; the p4 shim fills the caches while building
   `SetProps_Stage3`'s props, before its export, so the import kept the placeholders.
   Fix: both runners call `topology_vertexuse::refill_gaps` after the import, as the
   refilling importers do (0.00072 / 4.7e-5 instead of the placeholder).

Ruled out on the way: Apple's `__sincos_stret` returns a different `sin` than `sin()` for
about 0.5% of arguments in `[-7, 7]` (`cos` agreed for all 10M probed arguments), but
`gen0360` failed the same with a C++ reference built with `-fno-builtin-sin
-fno-builtin-cos` and with a harness `__sincos_stret` that calls `sin` and `cos`
separately. On Linux the bisector, `Cone::evaluate_step` and `Ellipse::eval_step` /
`step_inversion` fuse `sin`/`cos` on both sides.

Result on macOS arm64 and Linux x86-64: `query.face_p4` 503/503 (the three cases are
named regressions `cyl_*` in `fixtures/query.face_p4/cases.json`), `query.face_p3`
1199/1199 with 197 more cases compared with values (`no_values` 235 -> 38: faces and
edges that were `inexact_geometry` before). PR #949.
