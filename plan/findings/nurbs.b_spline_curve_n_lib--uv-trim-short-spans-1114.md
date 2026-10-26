# nurbs.b_spline_curve_n_lib: preserve short UV trim spans (#1114, 2026-10-26)

Unit: `nurbs.b_spline_curve_n_lib`. PR: https://github.com/tangency-labs/solidcore/pull/1122.

`BSplineCurve::trim` faithfully called `N_CrvRemoveDegenSegs` after trimming near
an interior knot. That routine measures the resulting span in the curve's own
coordinate units against the 3D zone tolerance. On a UV curve, its permitted
movement is magnified when the curve is lifted onto the surface.

The minimal reproducer is a rational semicircle of radius 0.14, with two quadratic
Bezier spans and a double interior knot at 0.5. Its control points are
`(r,0,0,1)`, `(r*w,r*w,0,w)`, `(0,r,0,1)`, `(-r*w,r*w,0,w)`,
`(-r,0,0,1)`, where `r = 0.14`, `w = 1/sqrt(2)` and coordinates are homogeneous.
The knot vector is `[0,0,0,0.5,0.5,1,1,1]`.

Trimming the dimension-2 curve to `[0.4999965,0.9]` with the default zone tolerance
`1e-5` removes its first short span. On the plane `S(u,v) = (38u,38v,0)`, the
maximum gap over 65 equally spaced parameters is `4.2993900351216975e-5`, larger
than the 3D zone tolerance. The symmetric tail trim `[0.1,0.5000035]` exercises
the other end. Issue #1114 describes the same error in a stadium-ring Boolean.

The additive `Kernel::fixes.bspline_trim_preserves_uv_spans` option restricts
degenerate-span removal to dimension-3 model curves, matching the issue's
`GetDim() == 3` correction. With it enabled, the trim retains its interior knot,
and the lifted curve agrees with the original restriction within `1e-12`.
The default remains pinned SMLib behavior; dimension-3 cleanup, interval snapping,
and `TrimMode::IntervalOnly` keep their previous semantics.

The curve regressions cover both ends, `Notify` and `Silent`, default versus
enabled behavior, dimension-3 cleanup, interval-only requests, and the public
`Curve::trim` dispatcher. Both the direct UV preservation test and dispatcher test
failed before the dimensional gate was added and pass afterwards.

The temporary edge-zone kernels in `trim_other_uv_curves`, `trim_rail`, and
`edge_zone_kernel` also carry this option. They previously started with every fix
disabled, losing the caller's selection when trimming an owned UV curve. A fourth
regression exercises the actual sibling-edgeuse helper: before propagation its
lifted gap remains `2.3458426237309375e-5` even with the option enabled; afterwards
it is below `1e-12`. The helper's default still reproduces the original error.

`fixtures/nurbs.b_spline_curve_n_lib/issue1114_uv_trim_spans.json` adds twelve
native comparisons: dimensions 2/3, head/tail trims, and all three trim modes.
These compare the default with the unchanged pinned C++ reference, including the
trimmed interval, flags, control points and knots. The enabled behavior is tested
against the original exact curve restriction. No tolerance or mismatch waiver
changes are needed.
