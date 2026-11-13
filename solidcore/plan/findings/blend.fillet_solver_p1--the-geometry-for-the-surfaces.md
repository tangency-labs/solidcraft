# blend.fillet_solver_p1: the geometry for the surfaces

`SmFilletSolver::GetSurface` / `SetSurface` defer to `m_pFGForSurfaces->Get/SetOffsetSurface` while a geometry is set for the
surfaces (`SetFilletGeomForSurfaces`, called only by `SmFilletGeom::ReCalcFilletGeom` and `SmFilletCorner`, all unported).
`FilletSolver::surface` / `surface_mut` / `set_surface` do not take the pseudo B-rep that geometry lives in, and return the
solver's `OffsetSurfaceSlot` (with the identity key `SetOffsetsData` compares) where the geometry stores a plain
`OffsetSurface`. Threading the pseudo B-rep through `GetSurface` touches every solver (474 upstream call sites), so these three
sites stay `UNPORTED(blend.fillet_geom_p1)` marked `justified:` with this section; nothing in Rust sets the geometry yet. Note for
their porter: upstream `GetSurface` recurses without bound when the set geometry has no own surface and its solver is this
solver (`GetOffsetSurface` falls back to `m_pFilletSolver->GetSurface`, which asks the geometry again); `FinishConstruction`
gives every geometry its own copies, so the normal flow does not reach it (`plan/upstream-issues.md` UB-204).
