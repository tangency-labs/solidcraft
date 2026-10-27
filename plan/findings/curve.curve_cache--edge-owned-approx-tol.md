# curve.curve_cache: the approximation tolerance of an edge curve's cache (2026-10-27)

Unit: `curve.curve_cache` (caller `curve.curve_p5`, `make_curve_cache`).

Upstream builds an edge curve's cache on the curve object itself
(`SmCurve::CacheMakeOrValidate`, `SmCurve.cpp:9899`), and `SmCurveCache::Tessellate`
(`SmCurveCache.cpp:1488`) takes `dApproxTol = SmTol::GetApproxTol3d(&m_crCurve)`, which
for a curve with an edge is half the edge's tolerance (`SmTol.cpp:783-786`, `927-937`).
The port caches an owner-free copy (`clone_detached`), so every cache used half the
system tolerance. The tolerance only gates `dCurveDistance > dApproxTol` (the distance
between a Bezier span and the curve, `SmCurveCache.cpp:1641/1748`), so the trees differ
when that distance lies between the two halves: the rounding of the Bezier
decomposition at coordinates of about 1e10 and more (an edge tolerance of 1e-2 or more
then gives 21 spans in C++ against about 4000 in Rust), or the cubic Hermite spans of
kinds without a NURB.

Inputs: `fixtures/curve.curve_p5/edge_cache.json` (op `edge_cache`: a wire edge's
curve, its tolerance set). Before the fix 9 of its 25 cases failed (span counts and
intervals, e.g. `edge_cubic_1e10_tol1`), all at 1e10 scale with an edge tolerance other
than the system value; after it all pass.

Fix: `make_curve_cache_with_zone_tol` takes the edge's tolerance from a caller that
holds the B-rep (the memo key includes it); `CurveCache::set_owner_zone_tol_3d` carries
it into `Tessellate`. Wired: `LoadFilletSolver`'s edge caches (session and the
`blend.fillet_executive_p2` runner).

Still on the system value (no `&Brep` at the build; follow-ups):
- the drop of an edge curve on a face surface (`create_uv_trim_curve` ->
  `Surface::drop_curve` -> `SurfaceDropCurve` / `surface_drop_lib_p3`), which the
  fixtures reach with edge tolerances up to 9.57 (system 1e-5), at ordinary scale;
- `Curve::global_point_solve_cached` on edge curves (fillet `remove_topo_edges`);
- the edge-use curve caches of `TrimSrfCache` (an owner-free `clone_exact`);
- `Curve::length_base` builds over the curve itself, so an edge-owned curve returns
  `Unported` there (no silent value).
At ordinary scale B-spline and analytic spans are exact, so those sites agree with C++
unless the coordinates are large. A probe over 108 brep-level units (rebuilding every
owned-curve cache with an unbounded tolerance and comparing span counts) found ~35,000
owned builds; the tree depends on the tolerance in 26, all on curves of length 7e17 to
1e50 (`primitive.api_primitives`: the edge-curve drops of `create_uv_trim_curve` and
`TopologySolver::solve_point_edge`; `blend.fillet_geom_p1`: `create_uv_trim_curve` and
`query::loop::drop_on_edge`). Every unit passed on its compared values; whether their
edge tolerances differ from the system value was not checked.
