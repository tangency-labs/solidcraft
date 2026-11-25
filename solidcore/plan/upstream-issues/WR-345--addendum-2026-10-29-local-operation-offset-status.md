Updates: WR-345

#### WR-345 (update, 2026-10-29). `SmBrep::LocalOperation` returns the offset's status (opt-in)

The rest of the fork's tangency-labs/smlib a759622 ("SMLib: an offset whose walls would meet is refused as such"), the part the first addendum (`WR-345--addendum-2026-10-29-walls-touch-refused.md`, #1457) left to the owner of `LocalOperation`, is ported as an opt-in, `Kernel::fixes.local_operation_offset_status` (default `false`), for tangency-labs/solidcore#1442.

- **What it changes:** upstream `SmBrep::LocalOperation` maps every failure of its extended offset to `SE(SM_ERR)` (generic, 1001). With the opt-in `local_operation` returns the offset's own status, as `SolidError::Upstream { code, .. }` with the callee's reason: `SM_ERR_OFFSET_WALLS_TOUCH` (1042, with `offset_resolve_crossing_loops` and `offset_walls_touch_refused`), `SM_ERR_BAD_INTERSECTIONS` (1027) and the like. Unported statuses were already passed on.
- **Independent of the other flags:** it only changes what `LocalOperation` does with the error; the offset fails the same way with or without it.
- **Tests:** `crates/solidcore/tests/issue_1442_local_operation_offset_status.rs`. A 40 x 40 x 10 plate with an r2 hole at x = 5 whose side face x = 0 is moved to x = 3 (the hole wall tangent to the moved side) fails with 1042 with the opt-in and `SM_ERR` without it.
- **Difftest:** no change on the default path. `topology.brep_local_operation` uses the default kernel, so no case compares the opt-in path.
