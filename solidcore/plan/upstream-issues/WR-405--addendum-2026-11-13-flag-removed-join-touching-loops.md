Updates: WR-405

#### WR-405 (update, 2026-11-13). `join_touching_loops` is removed

`create_plane_with_3d_curves` joins a loop touching another loop of its face at a vertex into that loop. That is now the only path, and the flag is gone. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** no golden changes. With it, every goldens unit (283) gives the same output as at main `4f549e08c`, run on Windows; alone, its corrected path moves none of the units the batch was checked on. SolidCraft's preset and the kernel rig's `fixes.json` already had the flag on, so their results don't change.
- **Tests:** `issue_892_join_touching_loops_cuts_the_hole` (renamed from `issue_892_only_join_touching_loops_changes_the_sweep`; the flag-off half is deleted) and the plane-loop test before it in `surface::surface::p2`; `tests/wr591_touching_loops_sweep_any_order.rs`.
