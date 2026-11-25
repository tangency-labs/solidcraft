# geom.vector2d (green; no disagreements)

4060 default cases (60 fixture + 4000 generated) and a 200,060-case run (`--seeds 200000`)
agree bit for bit with upstream (`max_rel_err = 0`). Notes for later agents:

- **`Vec2` moved.** `math::Vec2`/`Point2` are now defined in `geom::vector2d` and
  re-exported from `math` (old paths still work).
- **Co-ported back-edges.** `sm_sqrt`, `sm_acos`, `sm_asin` (unit `geom.vector3d`) and
  `SM_IS_ZERO_SQUARED` (`SmTol::InTol` + `SmTol::GetScaledZeroSq`, unit `geom.tol`) are
  private helpers in `geom/vector2d.rs`, marked `// CO-PORT(<unit>): <symbol>`, rather
  than `Unported` stubs (they are pure one-liners; stubbing them would make `Length`
  fallible). The owning units may replace them with shared versions.
- **Upstream `operator==`/`operator!=` are tolerant and asymmetric.** The tolerance is
  `SM_EFF_ZERO * (1 + lhs.GetMaxDimension())`, so `a == b` and `b == a` can differ, and
  with a NaN distance both `==` and `!=` are false. Rust keeps exact `PartialEq` and
  exposes the upstream tests as `approx_eq`/`approx_ne`.
- **`operator/` silently skips the division** when `|d| < |x| * 1e-12` or
  `|d| < |y| * 1e-12` (returns the vector unchanged); `0/0` still gives NaN.
- **`CCWAngleBetween` snapping is one-sided.** Only cross products in
  `[-2*scaled_zero, 0)` snap to 0 or PI (upstream comment asks why it is not symmetric);
  ported as is.
- **`operator[]` out of range** returns 0.0 (const) or dereferences NULL (non-const) upstream;
  Rust returns `SolidError::Upstream { code: 1001 }`. The ffi wrapper only passes 0 or 1.
