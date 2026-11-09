# blend.fillet_solver_p1: FilletSolverOps::load_initial_values takes the surface by value of reference

`FilletSolverOps::load_initial_values` (`fillet_solver/p1.rs`) takes `crSurface1` as
`&mut OffsetSurface`. Upstream's callers pass `*GetSurface(lRailIndex)`, the solver's own
rail surface, so a derived `LoadInitialValues` that changes it (`SetOffsets`, as
`SmConstantDistanceFS::LoadInitialValues` does) changes the solver's. A caller that owns
the solver (`blend.fillet_standard_solver_p2`) can only pass a clone and write it back,
which overwrites such a change. `#168`'s `ConstantDistanceFs::load_initial_values` already
models this with `InitialSurface::{Rail(i), Other(&mut)}`. The fix is to make the trait
method take `InitialSurface` and give `ConstantDistanceFs` a `FilletSolverOps` impl; it
touches the trait, `EiffelTowerFs`, `SecantConstantRadiusFs` (they read `surface1` only
through `&self.base`, so `Rail(i)` resolves there), the three runners and the
shims' callers, so it is deferred to a follow-up rather than done in this round. Until
then a caller must write the clone back only when the callee did not call `SetOffsets`.
