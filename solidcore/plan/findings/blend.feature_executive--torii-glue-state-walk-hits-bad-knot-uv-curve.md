# blend.feature_executive: torii's glue-state validity walk fails to evaluate a UV curve that C++ reports as a bad knot count (2026-11-01)

Unit `blend.feature_executive`, case `fillet_each_torii_fillet_b` (`Fillet/torii_fillet.smb`, one feature per fillet face, faces 11 and 12). The case's final result matches C++ (since #2293). This finding concerns intermediate states only, and only probes reach it. No gate compares these states.

## What the probe sees

A temporary probe (not committed) runs a validity walk (`AssertValid` after `ValidatePointers`) on both B-reps before and after every `GlueCoincidentBrepVertices` of the case. It runs on the Rust side with `FEXEC_GLUE_CENSUS`, and on the C++ side through `SmBrep::AssertValid` from gdb. Batches 54g9f3b08 (state comparison), 54g9f3b17 (backtraces) and 54k6 (recheck on main after #2293) give the same picture.

- **Rust:** 16 walk errors of one kind: `nurbs::b_spline_curve::p1::BSplineCurve::evaluate_point: upstream status 1001` on a 2-D B-spline (a UV trim curve). Each error is the NL evaluator failing at an interior parameter. There are 103 failing evaluations in all, along these paths:
  - 34 at t = 0.4: `assert_valid::edge::edgeuse` → `uv_trim_checks` → `approximate_length` of the `CrvOnSurf`;
  - 17 + 15 + 15 + 2 + 2 at t = 0.0094: `pointers::validate_pointers` / `validate_geometry` → `edge_geometry` → `Curve::curve_max_distance_between` (through the `CrvOnSurf`);
  - 14 + 2 at t = 0.456: `assert_valid::edge::edgeuse` → `edgeuse::p3::evaluate_binormal` → `evaluate_binormal_uv`;
  - 2 at t = 0.2: `assert_valid::loops::loop_` → `loopuse::is_loopuse_side_in_surface` → `edgeuse::p2::is_edgeuse_side_in_surface`.
- **When:** at the states of the other B-rep around glue calls 7, 8, 10, 12, 14, 16 and 18. That body has 8 faces / 56 edges / 54 vertices, and `ValidatePointers` already reports it invalid there.
- **C++:** the same snapshots of the other B-rep are invalid too. Among the C++ failure kinds at glue calls 7, 8, 10, 12, 14, 16, 18, 19 and 20 is `SmBSplineCurve::Test #1` (14033#1, "Bad Knot Count"), next to "Bad UVTrimCurve" (16005#11) and loop-structure failures. The C++ probe's own evaluations (each edge curve at both ends) all succeed, and `AssertValid` reports no evaluation failure.
- **Reading:** both sides hold the same malformed 2-D B-spline (a knot count that doesn't fit its control points) in an intermediate state of the merge. C++'s `AssertValid` reports it through the curve's own knot test. The port's walk reaches it first through evaluations (`approximate_length`, `curve_max_distance_between`, `evaluate_binormal`) that the NL evaluator refuses. So the walk reports an evaluation error where C++ reports a knot-count failure. The curve itself, and the body being invalid at that point, are the same on both sides as far as the probes show. This was not checked curve by curve: the probes compare failure kinds and counts per snapshot, not the curve data.

## Status

Low priority, documentation only. No change to the port. The validity walk at these states is a probe's view, and the operation's result matches C++. If the port's walk should report this the way C++ does (the curve test first, no evaluation of a curve with an invalid knot vector), that is a change to `topology::brep::validation` for a later PR. It would need its own C++ comparison of walk results on malformed curves.
