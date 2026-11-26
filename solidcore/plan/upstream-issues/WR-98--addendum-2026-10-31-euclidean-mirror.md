Updates: WR-98

#### WR-98 (update, 2026-10-31). The second division is now an opt-in fix

`SmTrimmingTools::MirrorCurve` (`source/SMLib/src/SmTrimmingTools.cpp:3496`-`3537`, the
division at `:3524`) reflects the Euclidean control points `GetCanonical`
returns and then divides each one by its weight again. The Rust port keeps that by
default (pinned C++ parity) and adds
`Kernel::fixes.mirror_curve_keeps_euclidean_control_points` (default `false`), which
skips only the second division. The reflection order (`high - (x - low)`), the
canonical/2D conversion, the curve-kind branches, the weights, the knots and the error
precedence are unchanged.

- **Regression, native fixture:** `mirror_rational_divides_twice` stores homogeneous
  control points `(0, 0, NOZ, 1)`, `(0.5, 0.5, NOZ, 0.7071067811865476)`,
  `(1, 0, NOZ, 1)`, so its canonical middle point is Euclidean
  `(0.5/w, 0.5/w) = (0.7071067811865476, 0.7071067811865476)`. For `side = U`,
  `low = 0`, `high = 2`, the default path gives the double-divided middle
  `(1.8284271247461903, 1.0)`; with the fix it is the reflection
  `(1.2928932188134525, 0.7071067811865475)`. The fixture itself is untouched and still
  gates the default path.
- **Regression, analytic:** a second curve is built from known Euclidean control points
  `(0, 0)`, `(0.5, 0.5)`, `(1, 0)` and weights `1`, `1/sqrt(2)`, `1`, stored
  homogeneously as `(w*x, w*y, NOZ, w)`. Its post-mirror evaluation is checked against an
  independent rational Bernstein oracle at `t = 0, 0.25, 0.5, 0.75, 1` for both `U` and
  `V` and shifted bounds `(-1.5, 0.75)` as well as `(0, 2)`; weights, knots and degree
  are preserved. A two-mirror involution control returns the original points, and a
  non-rational curve is unchanged between the two settings.
- **Limit:** there is no C++ parity claim for the opt-in path (no native case runs it),
  and no end-to-end `FixBackwardsOuterLoopOnSeam` caller case is measured; the fix is
  tested on the public `mirror_curve` operation only.
