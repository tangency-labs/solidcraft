Updates: plan/findings/offset.offset_geometry_creation--drops-onto-a-seeded-extended-surface-through-a-freshly-built.md

# offset.offset_geometry_creation: two of the four "fresh face cache" mismatches were a sliced UV curve copy (2026-10-25)

`gen0248`, `gen0346`, `gen0350`, `gen0508` were parked on the guess that upstream
reuses a face cache built earlier in the script. Tracing both sides showed that guess
was wrong. No face surface had a cache before the `EdgeuseOffset` op on either side,
and the drop went the same way on both: the lifted curve was perpendicular to the
other extended surface (a plane touched only at a corner, in `gen0248`), so
`DropAndTrimCurve` returned no UV curve.

The first difference is in the fallback that follows. `TrimExtendedEdge` (cpp:1655)
and the tangent-vertex branch of `ExtendTrimEdges` (cpp:1181) call
`pUVCurve->Copy(crContext, pUVCurve)` on an `SmBSplineCurve*`. That binds the
non-virtual `SmBSplineCurve::Copy(const SmContext&, SmBSplineCurve*&)` overload, which
is `new SmBSplineCurve(*this)`: an `SmLine` UV trim curve comes out as a plain
B-spline. The original source cast the out-pointer to `SmCurve*&` (the virtual copy)
and had that commented out to quiet gcc's strict-aliasing warning, which changed the
behaviour. The port used the virtual `Curve::copy`, so the offset edge uses kept an
`SmLine`. The recorded state then differed in the curve type (C++ 14033
`SmBSplineCurve` vs Rust 14010 `SmLine`) and in the line's extra fields, which is where
the length and value differences came from.

Fix: the two sites now copy through `b_spline_overload_copy` (the curve's B-spline
part, cloned). `gen0248` and `gen0508` are no longer known mismatches. The `SmCurve*` copies at
cpp:2405/2446 and cpp:1712 stay virtual, as upstream has them.

## The other two cases are still parked, for different causes

With the fix, `gen0248` and `gen0508` pass (898 cases: 880 pass, 16 rejected). The
other two still disagree, and neither involves face-cache history:

- **`gen0346`.** The script's last op is `MateEdges`, where both extended surfaces are
  the same `SmSphere` surface (a self-intersection). The Rust
  `GlobalSurfaceIntersect` returns no curve, so `MateEdges` returns `SM_ERR` before
  `ReplaceEdgeCurves`. C++ also returns `SM_ERR` (1001), but only after
  `ReplaceEdgeCurves` on the offset edge use has called `SmEdge::SetCurve`. That call
  dropped the UV trim curves of the edge's other two edge uses, which C++ then records
  as null and Rust records as still set (state length C++ 547 vs Rust 615, first
  difference at index 384). Suspected cause: sphere self-intersection in the surface
  intersector.
- **`gen0350`.** `ExtendTrimEdges` on a sphere face. The final UV trim curve of the
  offset edge use has the same control points on both sides but different interior
  knots: C++ `0.029631480111470525, 0.15880441968927933, 0.8554225435781` vs Rust
  `0.041052170113668345, 0.1687048170713494, 0.857124138670334` (the Rust values are the
  input curve's). The Rust drop of the lifted meridian onto the other extended surface
  returns two seam pieces. Where the C++ reparametrisation happens is not localised yet.
