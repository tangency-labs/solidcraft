# blend.fillet_geom_p1: GetFilletSurface with the owning body after MakeFaceBrep (2026-10-27)

Updates: plan/findings/blend.fillet_solver_p1--the-geometry-for-the-surfaces--addendum-2026-10-27-lent-surfaces.md

**`SmFilletGeom::GetFilletSurface` after `MakeFaceBrep` is ported.** Once the surface is
transferred to a face of the intermediate fillet B-rep (`FilletSurfaceStorage::ManagedFace`),
`FilletGeom::fillet_surface` / `fillet_surface_mut` resolve the face's surface in the owning
body, which the caller passes as a `FilletSurfaceOwner` (the body and the identity
`MakeFaceBrep` recorded for it). The two `UNPORTED(blend.fillet_geom_p1)` stubs are gone.
A transferred surface with no owner, or with another body's identity, is refused with
`InvalidInput`; a face without a surface is `SM_ERR_NULL_POINTER`.

The session's resolver (`SessionObjects::fillet_surface`, used by `FilletSession::fillet_surface`,
face shrinking and the insertion checkpoints) now delegates to the accessor with the
intermediate body instead of duplicating the lookup.

Callers: every caller inside `fillet_geom` p1–p6, the corners and the solvers runs before
`CreateFilletBrep` makes the faces (upstream order: corner geometry, then every solver's
`MakeFaceBrep`, then `StitchFaces`), so they pass `None`. That includes the reads inside
`make_face_brep_resolved` before the transfer, which keep rejecting a second transfer.
The upstream callers after installation (`SmFilletGeom::InsertIntersectionTopology`,
`DoPiecewiseMerge`) are session code in Rust and already use the body.

Runtime probe (temporary backtrace at the managed branch; `cargo test -p solidcore` and all
22 `blend.*` units): before this change the stub was reached by no fixture. After it, the
managed branch is reached only through the session resolver with the owner
(`blend.fillet_executive_p3`: about 690 resolutions, mostly `shrink_geometry_face` in
`do_filleting`, plus the insertion-checkpoint and shrink-state snapshots), never with
`None`. Unit tests: `transferred_fillet_surface_resolves_in_its_owning_body` (both
accessors, the refusals, the write through to the face) and `tests/fillet_session.rs`
(the accessor returns the managed face's own surface). No case changed.
