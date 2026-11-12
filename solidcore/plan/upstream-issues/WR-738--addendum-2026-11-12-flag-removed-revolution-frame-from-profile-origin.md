Updates: WR-738

#### WR-738 (update, 2026-11-12). `revolution_frame_from_profile_origin` is removed

`SurfOfRevolution::create_canonical` takes the frame's X axis from the profile's point at parameter 0 when its STEP interval holds it (then its start, then its 45% point, each when the one before is on the axis, by its distance from the axis), and `step_inversion` turns a point of a planar generator onto both half planes of the start plane. This is now the only path, and the flag is gone. SolidCraft already ran with it on; the kernel rig's `fixes.json` did not list it. Removed: the field, SolidCraft's preset line and every test setting of it.

- **Results:** the goldens rows that move, with their old and new values and why the new ones are right, are in `plan/findings/surface.surface_approximate--b47-five-flags-retired-2026-11-12.md`.
- **Tests:** in `surface/surf_of_revolution/p1.rs` the tests of #2628 items 4 and 7 to 9 run on the default kernel; their halves that pinned upstream's frame or refusal are removed, and `bounded_profile_frame_is_upstreams_with_the_fix` becomes `bounded_profile_frame_is_upstreams` (the frame's axes, exact).
