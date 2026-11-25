# topology.edge — inexact geometry import reporting (issue #294, PR #356)

The edge adapter imports analytic curves other than lines, and analytic surfaces
other than planes, as their B-spline parts. Its existing `inexact` predicate skips
operations that would evaluate those stand-ins. The C++ runner still refuses those
inputs with `inexact_geometry`; the Rust runner now reports a harness deferral on
`topology.edge`, with symbol `difftest: exact analytic geometry import for edge
evaluation`. Neither runner evaluates the skipped operation. This corrects coverage
accounting without adding analytic import or kernel coverage.

A fresh default-seed baseline on `7b57d8de` reproduced the committed report exactly:
1787 cases, 1781 passed, 6 deferred, zero failed. Capturing both runner outputs proved
that all 186 empty passes returned `inexact_geometry` on both sides: 62 `seam` cases
(both overloads), and 31 each for `topological`, `tangent`, `blend`, and `radial`.
The other 1595 passes compared real values; the six existing deferrals belong to
`intersect.surface_drop_lib_p3`.

After the correction, the same 1787 cases produce 1595 passes, 192 deferrals
(`topology.edge` 186, `intersect.surface_drop_lib_p3` 6), zero failures, and zero
empty passes. Comparing by case name proved that all 1601 other case records and
both runner outputs are identical. The C++ refusal output is also unchanged for
each of the 186 reclassified cases. The unit remains `partial`, with relative
tolerance `1e-9`, no absolute floors, and maximum relative error zero.

Exact controls still compare 4 values (`line_join_fw_tangent_start`), 18
(`line_join_fw_trimmed`), 18 (`net_trimmed_e0`, a B-spline), 1
(`plane_square_e0_tangent`), and 50 (`Mouse_e0_query`). The Mouse query uses the
same imported B-rep as the deferred operations: the guard still applies per
operation, not to the whole B-rep.
