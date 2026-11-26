# topology.face_p3 `cyl_bspline_uv_trim_*`, boolean.merge_p2 `make_faceuse_sphere`: nested UV trim curve creation while the surface owner is cleared (fixed, PR #586, 2026-10-23)

Follow-up to "PR #463: 27 Rust-vs-C++ mismatches newly reachable once
`SmSurface::DropCurve` is wired" (topology.face_p3 and boolean.merge_p2 groups; one
cause). `SmFace::CreateUVTrimCurves` and `SmEdgeuse::CreateUVTrimCurve` clear the
surface's `m_pOwner` for the rest of the call, so a nested `smsurf_GetSurfaceCache`
(`EvaluateBinormal(bUse3DOnly)` from `CreateUVTrimCurve`'s seam choice) builds a
standalone `SmTrimSrfCache` and makes no UV trim curves. The port's
`query::face::p2::face_surface_cache` handed the face to the cache build regardless
(`TrimSrfCache` uses "the caller's face, else the surface's owner"; imported face
surfaces carry no owner), so on the B-spline cylinder the nested build created the
other edges' UV trim curves and the outer `CreateUVTrimCurves` skipped them: its maximum
curve/surface gap (`out[2]`) was `0` instead of `9.10934752180535e-17` (traced with a
debug print of each `GetOrCreateUVTrimCurve` distance: Rust dropped edge use 0 in the
outer loop and 0..3 in a nested one). The B-rep now records the clearing scope
(`Brep::begin_surface_owner_cleared`), `face_surface_cache` builds the standalone cache
inside it, and `create_uv_trim_curve` clears the B-rep face surface's owner too. A first
attempt that keyed this on the surface owner alone (`TrimSrfCache::build_tree_body`
ignoring the caller's face when the surface has no owner) broke 274 `topology.edgeuse_p3`
binormal cases, because imported face surfaces have no owner; the explicit scope avoids
that. Full `--green` sweep: 0 failed; face_p3 +2, merge_p2 +1, brep_p4 +2 (deferred ->
pass), nothing else changed.
