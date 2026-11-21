Updates: WR-343

#### WR-343 (update, 2026-11-12). `revolution_offset_axis_both_sides` is removed

`SurfOfRevolution::create_offset_surface` intersects the offset generator with an axis segment that spans the offset's whole extent along the axis, on both sides of the frame's origin, trims only at a crossing inside the interval, and keeps the part on the generatrix's side. This is now the only path, and the flag is gone. SolidCraft and the kernel rig already ran with it on. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** the goldens rows that move, with their old and new values and why the new ones are right, are in `plan/findings/surface.surface_approximate--b47-five-flags-retired-2026-11-12.md`.
- **Tests:** `issue_973_offset_cut_at_axis_below_origin` (`surface/surf_of_revolution/p2.rs`) runs on the default kernel; its half that pinned upstream's stretch past the axis is removed.
