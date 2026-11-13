Updates: WR-99

#### WR-99 (update, 2026-11-13). `local_surface_intersect_guards_max_boundary_retry` is removed

`SmSurface::LocalSurfaceIntersect`'s out-of-bounds boundary retries guard both the minimum and the maximum boundary tests with `!bFoundSolution`. That is now the only path, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. With it, every goldens unit (283) gives the same output as at main `4f549e08c`, run on Windows; alone, its corrected path moves none of the units the batch was checked on. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** the `wr99_*` tests in `surface::surface::p11` now run the default kernel only. The flag's registration test and the default-path halves (the boundary hits upstream loses) are deleted.
