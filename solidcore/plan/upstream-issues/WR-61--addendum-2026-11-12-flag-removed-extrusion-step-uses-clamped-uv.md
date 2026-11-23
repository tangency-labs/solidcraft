Updates: WR-61

#### WR-61 (update, 2026-11-12). `extrusion_step_uses_clamped_uv` is removed

`SurfOfExtrusion::evaluate_step` and `evaluate_step_point` evaluate the STEP parameters clamped to the analytic UV domain when out-of-bounds evaluation is off, the copy upstream clamps and then ignores. This is now the only path, and the flag is gone. SolidCraft and the kernel rig already ran with it on. Removed: the field, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** the goldens rows that move, with their old and new values and why the new ones are right, are in `plan/findings/surface.surface_approximate--b47-five-flags-retired-2026-11-12.md`.
- **Tests:** in `surface/surf_of_extrusion/p1.rs`, `step_clamped_uv_fix_is_off_by_default` becomes `step_evaluators_clamp_to_the_surface_domain` (the clamped values, with its matrix and error checks kept), `step_clamped_uv_fix_clamps_when_enabled_and_out_of_bounds_off` becomes `step_clamp_reads_the_analytic_domain_with_out_of_bounds_off` (its comparison of the flag off and on becomes the line's exact values in the domain), and `step_clamped_uv_fix_keeps_the_generator_flag_policy` becomes `step_clamp_overrides_the_generator_flag`.
