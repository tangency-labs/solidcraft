Updates: plan/findings/blend.fillet_executive_p3--bottle-e29-r0.1-vertex-tolerance.md

# Resolved: bottle e29 r=0.1 vertex tolerance (2026-10-26)

`sweep4-bottle-e29-r0.1` now passes: topology and all 1301 compared values agree. The
known-mismatch entry is removed.

**Where C++ sets the tolerance.** During `SmFilletExecutive::DoFilleting` ->
`SmFace::ShrinkGeometry` -> `SmFace::CreateUVTrimCurves`, the fillet face edge
(0.950, 0.450, 3.953)-(0.976, 0.484, 3.944) fails `DropCurve` at the edge tolerance
(iterations 0 and 1) and succeeds at iteration 2 with the gap-based tolerance 5.275e-3.
Because `ii > 1`, `SmEdgeuse::CreateUVTrimCurve` sets the edge tolerance to
`1.01 * sqrt(dist^2 + dev^2)` = 5.43e-3, and `SmEdgeuse::SetUVTrimCurve` then raises it
to 1.07e-2. The edge tolerance cascades to the vertex, which keeps it through the merge.
In Rust the iteration 2 `DropCurve` returned no curve, so neither raise happened.

**Why Rust's drop failed.** The fillet surface has very short first knot spans in u
(about 7e-10). Two Rust stand-ins for "this same surface object" used `Surface::clone`,
which runs the port of the upstream copy constructor's knot fix-up and moves those near
coincident knots (the second knot moves from 6.78e-10 to 7.66e-10). That changes the
derivatives at u = 0 (Su -67.24 becomes -59.52):

1. `DropOffset::offset_of` (`SmDropPt::SetProperties` -> `rOffset.DropVectors`): upstream's
   `SmOffsetSurface(1.0, *this, FALSE, ...)` references the drop surface
   (`m_pSurface(&rSurface)`). Rust built the offset over `clone_keeping_owner()`, which
   is a `Clone`. The start UV tangent came out (0.0884, -0.8052) instead of
   (0.0783, -0.8052).
2. `DropPt::get_next_point`, the global-solve retry: upstream nulls `m_pOwner` on the
   surface itself (`SmTemporaryChangeValue`). Rust ran `GlobalPointSolve` on
   `surface.clone()`, which found (1.6223e-8, 0) instead of (1.4793e-8, 0).

**Fix.** Both sites now use `Surface::clone_for_evaluation` (an exact copy that keeps the
owner), and the retry clears the owner on that copy. `topology::face::p2::offset_evaluate_simple`
builds the same non-owning `SmOffsetSurface` over a shared surface. It had the same
`clone_keeping_owner` stand-in and is changed in the same way.

Found by tracing both sides on the case. The C++ side used a scratch copy of the
reference library with `fprintf` in `SmVertex::SetTolerance`,
`SmEdgeuse::CreateUVTrimCurve`, `SmDropPt::GetNextPoint`/`SetProperties`,
`SmSurface::DropVectors` and `SmOffsetSurface::EvaluatePlainOrSimple`. That copy was
not committed.

Other `clone_keeping_owner` callers (`surface_cache::p1`, `intersect::curve_class::p5`,
`blend::fillet_standard_solver::p1`/`p2`) may have the same copy-versus-shared-object
difference on surfaces whose knots the fix-up moves. They were not changed here.
