# offset.offset_geometry_creation: #1130 (a shelled dome's rim marched on B-spline extensions) does not reproduce in the port

**This is a fork report only.** tangency-labs/solidcore#1130 was reported by jyviko (SolidCraft rig shell/4847). It was confirmed on the SolidCraft fork's C++ (capi test `ops_kernel shelled_dome_rim`), not on upstream SMLib (neither 1.0.0, c9c5979, nor 1.0.1) and not on our C++ build. It is therefore not recorded in `plan/upstream-issues/`. Rechecked on the 1.0.1 port (3fd5b95): `issue_1130_shelled_dome_rim` passes unchanged.

## The fork's report

**Input:** a quarter disc r20 revolved about Z (an `SmSphere` and an `SmPlane`), shelled with distance -2 and the base face open.

**Fork result:**
- The inner rim comes out as a degree-3 B-spline from r 17.9999993485 to 18, where the exact rim is the circle r18.
- The volume is 4540.648962, where (2/3) pi (20^3 - 18^3) = 4540.648582.

**Reported cause:**
- `SmOffsetExecutive::GetCreateExtendedSurface` extends every offset surface with `CreateExtendedSurface(..., SM_CT_G1R)`, which makes it an `SmBSplineSurface`.
- `SmOffsetGeometryCreation::MateEdges` then intersects those B-splines by marching.

**Fork fix:** two commits, "an offset edge between two analytic faces is their exact line or conic" and "an offset's analytic copy grows only along its rulings, not round a turn". They intersect exact analytic copies of the two faces' surfaces, extended only where the G1R extension still is the surface. Each marched piece that lies on an exact line or conic between the same ends is replaced by that curve.

## In the port (2026-10-26)

**Fixture:** a sphere r20 with everything below z = 0 cut away (`boolean_difference` with a box), shelled -2 with the base open.

**Rim:** with the stock kernel, the rim is one ellipse-kind edge within 3.6e-15 of the circle r18. That was also the result with a port of the fork's fix switched on. The port of the fix was written as opt-in `offset::analytic_mate` and then dropped, because there is no failing case.

**Not established:** why the port does not march here. Possible reasons:
- the port's shell may not reach the marched `MateEdges` intersection for this rim;
- the fork's revolved-profile part may behave differently from the sphere-minus-box fixture.

A gdb run or a revolved-profile fixture would settle it.

**Regression test:** `issue_1130_shelled_dome_rim_is_the_exact_circle` in `crates/solidcore/tests/issue_1130_shelled_dome_rim.rs`.

## Open: the shell's volume

The shell integrates 4540.561585 (`brep_volume`, accuracy 1e-6), which is 0.087 (1.9e-5 relative) below the closed form 4540.648582. This happens with or without the fix.

`brep_volume` clamps its accuracy to [1e-8, 1e-1], so the clamp does not explain the shortfall.

The test prints PROBE lines for the next step:
- the input dome's volume at accuracy 1e-6 and 1e-8;
- the shell's volume at accuracy 1e-6 and 1e-8;
- per face, the surface type and the min/max distance from the origin.

These show whether the input is already short or only the shell is, and whether a face sits off r18 or r20. The test asserts only the rim until this is explained.
