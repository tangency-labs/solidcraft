# sphere_diff_3cylinders all-edges r=0.1: cause narrowed to one lazily dropped UV trim curve (2026-10-26)

Updates: plan/findings/blend.fillet_executive_p3--sweep4-sphere-diff-3cylinders-and-brepforfillet2-all-edges.md

`sweep4-sphere_diff_3cylinders-all-r0.1` still disagrees and keeps its known-mismatch
entry. The fillet result does not differ in any datum that matters here. The
disagreement comes from one UV trim curve that both sides create *after* the fillet,
when the mass properties are computed.

Evidence (temporary dumps in the p3 runner and shim, not committed):

- The whole area difference is one face. With per-face `SmApiFaceComputeArea(f, 1e-4)`
  taken straight after the fillet, only result face 7 (a cone, type 15012) differs:
  C++ 3159.32103643069786, Rust 3158.72012897968443. That gap (0.6009) is the gap in
  the total area (38235.606154 vs 38235.004875). The earlier record of faces 3, 6 and 7
  differing came from a later evaluation order.
- Face 7's oriented trim boundary (`CreateOrientedTrimBoundary(TRUE, TRUE)`) has 9
  curves. Eight agree (knots, natural intervals, 9 samples each). Curve 2 does not. It
  belongs to edge use 2 of the loop, natural interval [0.49999995785315, 0.749999986635809].
  C++ has one cubic Bezier span, with control points (0.3773763754, 0), (0.3773763722,
  0.0416667), (0.3893482375, 0.0833333), (0.4132919774, 0.125), so its start tangent is
  purely in v. Rust has a 15-span cubic (16 unique knots). The ends agree; the interior
  differs by up to about 3.7e-4 in u.
- Neither side has that edge use's UV trim curve after the fillet: both show it absent
  before any area call. It is made lazily by `GetOrCreateUVTrimCurve` ->
  `SmEdgeuse::CreateUVTrimCurve` -> `DropCurve` inside `CreateOrientedTrimBoundary`
  (Rust backtrace: `face_properties` -> `create_oriented_trim_boundary` ->
  `create_uv_trim_curves` -> `get_or_create_uv_trim_curve`).
- The `DropCurve` inputs agree. The edge curve is a rational quadratic B-spline
  (14033). Its unique knots [0.49999995785315, 0.5, 0.749999986635809] (mults 3, 2, 3),
  all 5 control points and weights, the edge interval and the edge tolerance (1e-5) are
  identical to 17 significant digits. The cone agrees at a 5x5 grid of points and first
  derivatives, and the face UV domain, the face boxes and the B-rep box agree. The curve
  begins with a 4.2e-8-long knot span, and its start lies at v = 0, which looks like the
  cone's singular row.
- Caches are ruled out. After the fillet, C++ keeps an `SmTrimSrfCache` on every cone
  face surface and an `SmCurveCache` on the edge curves; Rust keeps none. Deleting all
  of them in C++ (`SmCacheMgr::DeleteObjectsCache` on every face surface and edge curve)
  before the area calls leaves the C++ face 7 area unchanged.
- A body datum the record does not capture, with no effect here: C++ reports
  `GetRectangularTrim() == FALSE` on every result face, while Rust leaves `UNSURE`.

Same divergence as upstream issue WR-449
(`plan/upstream-issues/WR-449--sphere-diff-3cylinders-one-fillet-face-integrates-0-6-too-large.md`),
found independently on the unfilleted input file through `corpus.parity`: the edge-30
trim of face 7, one cubic span in C++ against the port's drop. There, C++'s face 7 stands
apart from its five symmetric siblings, which the port matches, so the C++ value is the
suspect one. The fillet case inherits that defect.

Conclusion: on identical inputs, `SmEdgeuse::CreateUVTrimCurve` / `DropCurve`
(topology.edgeuse_p3 and its surface drop-curve callees) accepts a single Hermite span
in C++, while the Rust port subdivides. Next step: a direct `DropCurve` /
`CreateUVTrimCurve` difftest on this edge curve and cone, then trace the singular-start
handling and the span acceptance test. The fillet executive is not involved.
