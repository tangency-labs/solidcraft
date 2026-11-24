Updates: WR-496 (`WR-496--isnurbsurfacecylinder-inside-out-axis-reversed.md`)

#### WR-496 (update, 2026-11-15). `nurb_cylinder_inside_out_frame` is removed

`SmCylinder::IsNurbSurfaceCylinder` (`Cone::is_nurb_surface_cylinder`) always gives an inside-out NURB the frame `IsNurbSurfaceCone` gives it: origin the top-knot circle, Z against the NURB's linear direction. The flag is gone. Removed: the field, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and every test setting of it; `issue_1463_revolve_inside_out_cylinder.rs` drops its upstream half (the cylinder mirrored in height, STEP and NURB forms 11.68 apart).

- **Results:** no goldens row and no kernel rig case changes.
- **Tests:** `issue_1463_revolve_inside_out_cylinder.rs` (`issue_1463_inside_out_nurb_cylinder_keeps_its_axis`, `issue_1463_revolved_cylinders_are_never_inside_out`).
