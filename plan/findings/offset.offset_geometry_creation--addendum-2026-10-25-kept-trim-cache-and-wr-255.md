# offset.offset_geometry_creation: gen0346 was the trimmed cache not being kept; gen0350 is WR-255 (2026-10-25)
Updates: plan/findings/offset.offset_geometry_creation--addendum-2026-10-25-slicing-uv-curve-copy.md

Both remaining causes in the earlier addendum were wrong. Instrumented upstream copies,
linked into a private reference build, traced both sides op by op.

## gen0346: fixed

`MateEdges` does not differ. On both sides the self-intersection of the one `SmSphere`
extended surface finds no curve, and `MateEdges` returns `SM_ERR` before
`ReplaceEdgeCurves`. Upstream does not run `ReplaceEdgeCurves` first, as the earlier
addendum said. The edge states differ before the intersection, when its surface cache is
fetched:

- The script's first op (`ExtendTrimEdges`) drops a curve onto the sphere. That fetches
  the surface's cache, which builds an `SmTrimSrfCache` with its face
  (`SmTrimSrfCache::BuildTree` -> `SmFace::CreateUVTrimCurves`). Upstream keeps the
  cache on the surface (`SmCacheMgrTSrf::CacheMakeOrValidate`).
- `set_eu_uv` (null) then clears an edge use's UV trim curve and its mate's
  (`SmEdgeuse::SetUVTrimCurve`). That does not mark the face modified, so the cache
  stays valid.
- In `MateEdges`, upstream's intersection reuses that cache and builds nothing, so the
  two UV trim curves stay null. The port built a new trimmed cache on every solve
  (`OffsetBrep::prepare_surface_solve_cache`). The new build ran `CreateUVTrimCurves`
  again, which recreated the two trims as B-spline lines. That is the 547 vs 615 state
  length.

Fix: the solve cache of a face-owned surface now goes through the surface's cache slot,
the same one `face_surface_cache` uses. It reuses the kept cache, or builds one with the
face and keeps it, unless the face was modified while its B-rep is not being edited.
The solve gets a copy in its own handle space (`query::face::p2::solve_surface_cache_of_face`).
This applies to both the unit runner and `offset_executive`'s `EdgeBrep`. `gen0346` passes.

## gen0350: cause confirmed (WR-255), still parked

The drop in `ExtendTrimEdges` matches on both sides (two seam lines). The knots differ
inside that drop: the drop builds the sphere face's trimmed cache, and its
`CreateUVTrimCurves` -> `sm_FixUVTrimCurves` closes a 0.61 UV corner gap at the start of
the edge use's 5-point UV trim (the curve the script set with `replace_edge_curves`).
Upstream's `DropPoint` lands at t = 0.01177. Upstream then trims the curve there and
stretches it back over [0, 1] (`Trim` + `EditParameterization`), so the knots go from
0.0411/0.1687/0.8571 to 0.0296/0.1588/0.8554, which is `(k - 0.01177) / 0.98823`.
`EditEndPoint` then moves the end point on both sides. This is WR-255
(`plan/upstream-issues/WR-255--sm-fixuvtrimcurves-trims-a-uv-trim-curve-at-the-vertex-s.md`).
The port follows the fork fix by default and only moves the end point.

To confirm, a temporary run with `Context::set_trim_uv_curves_at_corner_drops(true)` in the
runner made `gen0350` pass. That change was not committed. Which side difftest should
compare is still open in #959, and the other three WR-255 cases are parked too. So
`gen0350` stays a known mismatch with this cause and joins them: `topology.brep_p10`
gen0115 and `topology.brep_p3` `nurbs_off_Mouse_0.1` / `nurbs_off_Mouse_-0.05`.
