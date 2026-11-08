# blend.fillet_solver_p1: the geometry for the surfaces, lent to the solver (2026-10-27)

Updates: plan/findings/blend.fillet_solver_p1--the-geometry-for-the-surfaces.md

Also updates: plan/findings/blend.fillet_geom_p5--solver-and-corner-callers.md (the
`CreateEndCurve` stub in `FilletSolver::split_fillet_geom`).

**`GetSurface` / `SetSurface` with `m_pFGForSurfaces` set are ported.** Every scope that
makes a geometry the solver's geometry for the surfaces (`SmFilletGeom::RailEdgeuseIntersect`,
`SmFilletNx1ClosedCorner::AdjustTangentCorner`'s and the tangent split's `ReCalcFilletGeom`)
now lends that geometry's offset surfaces (`m_pOffSurfs`) to the solver for the scope
(`FilletSolver::lend_geometry_surfaces` / `return_geometry_surfaces`,
`blend/fillet_solver/surfaces.rs`) and keeps `GetFilletGeomForSurfaces()` returning the
geometry, as upstream does. `FilletSolver::surface` / `surface_mut` then return the
geometry's surface (`SmFilletGeom::GetOffsetSurface`) and `set_surface` / `set_offsets_data`
replace it (`SmFilletGeom::SetOffsetSurface`, deleting the old one). A nested scope for the
same geometry reuses the lent surfaces; another geometry's scope stacks and restores them.

This replaces three mechanisms that moved the geometry's surfaces into the solver's own
slots while clearing `m_pFGForSurfaces` (`slot_rail_edgeuse_intersect`,
`base_rail_edgeuse_intersect`, `with_geometry_surfaces`). With them, solvers that use the
default `rail_edgeuse_intersect_on_geometry` (the custom solvers, `SmSurfaceSurfaceFS`, the
corner's plain constant-radius solver) and `FilletBrep::rail_edgeuse_intersect` called
`GetSurface` with the geometry set and returned `Unported`. A nested scope for the same
geometry also swapped the already-moved (empty) surfaces back in.

A rail the geometry has no offset surface for is refused (`InvalidInput`, UB-204):
`GetOffsetSurface` falls back to `m_pFilletSolver->GetSurface`, which asks the same geometry
again without bound. `SetFilletGeomForSurfaces` without a lending scope (only the
`blend.fillet_executive_p2` loader harness does this, and it reads the geometry first) gets
an `InvalidInput` from `GetSurface`, because the solver cannot reach the pseudo B-rep.
No fixture reaches either refusal (probe sweep over every `blend.*` difftest unit and
`cargo test -p solidcore` on main `dbaedda4a`). The previous slot-swap returned no surface
in both cases.

Not covered here: `ConstantRadiusOps::tangent_rollover_guess` is still `None` for
`SmSurfaceSurfaceFS` (`SmConstantRadiusFS::LoadInitialValues` line 493 reads
`GetFilletGeomForSurfaces()`). Only the assisted constant-radius solver's view
(`GeometrySolver`) supplies it. That gap is separate and older than this change.

**`CreateEndCurve` in `SplitFilletGeom`** now always runs. The body-less legacy entries
(`FilletSolver::split_fillet_geom`, `split_and_trim_fillet_surface`) take the base B-reps
of rails 0 and 1, as `split_fillet_geom_with_target` already did. The
`split_fillet_geom_scoped` variant is removed. Upstream reaches those B-reps through the
rail vertices' pointers.

**`SmFilletGeom::GetFilletSurface` after `MakeFilletFace`** (`FilletGeom::fillet_surface`
/ `fillet_surface_mut` on `FilletSurfaceStorage::ManagedFace`) stays
`UNPORTED(blend.fillet_geom_p1)`. The surface then belongs to a face of the intermediate
fillet body. These accessors take no body, and threading one through every caller (about 50
in `fillet_geom` p3–p6, the corners and the solvers) is a design change. All of those
callers run before `make_face_brep_resolved` installs the surface. The session resolves
installed surfaces with the body (`SessionObjects::fillet_surface`, insertion
`surface_binding`). The probe sweep reaches the branch only in the unit test that asserts it
(`tests/fillet_session.rs`). Presence-only checks (`GetFilletSurface() == NULL`) no longer
go through the borrow: `FilletGeom::has_fillet_surface` serves them in
`calc_rail_int_extended_edgeuse`, `c5_undo_reversal`, `RecordFilletError` and the session's
`MakeFilletBrep` loop. Those checks would otherwise have failed with `Unported` once a
surface was installed.

**`RailOwnership::owner_changed`** now receives the notifying curve, so
`RetainedRailOwnership` delivers `SM_NO_CHANGE_OWNER` with the curve's actual identity and
its `set_curve_owner` routes through it. No disagreement.
