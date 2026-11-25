Updates: WR-408

#### WR-408 (update, 2026-11-15). `torus_signed_gen_curve_interval` is removed; the generator interval is always signed

`Torus::rebuild_step_from_nurb_parameters` and `Torus::is_nurb_surface_torus` (`surface/torus.rs`) always measure the
generating circle's interval with `torus_gen_curve_interval` (signed angles about the torus axis, zero
`SM_EFF_ZERO_DEG`, an inside-out torus's last point starting the interval). A failed angle is an error in the rebuild
and not a torus in the recogniser; upstream read it as 0. The flag is gone, so the opt-in in the original record and
the 2026-10-29 addenda no longer exists. Removed: the field, SolidCraft's preset line, its entries in the kernel
rig's `fixes.json` and in `issue_2260_gn_pipe_refined_range_end.rs`, the flag lines of seven `tests/` kernels
(`issue_1476`, `issue_1672`, `issue_1818`, `issue_1945`, `issue_1950`, `issue_2020` and `wr650`), and the two unsigned
`AngleBetween` branches.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes:
  a torus whose tube interval runs below its equator, past 180 degrees, or is inside out is rebuilt and recognised
  over its own interval, not the mirrored one.
- **Goldens:** the `surface.torus` `rebuild`, rebuilt-`offset` and `is_nurb` rows move (one table in the PR). The
  committed test `wr408_501_504_moved_rows_are_the_exact_torus` (goldens, `units/surface_torus.rs`) checks each
  result against the torus it was built from. It first certifies that the NURB lies on that torus with
  `surface::implicit::lies_on` (Bernstein coefficients of the implicit equation against their rounding), and pins
  the patch's extent from the control net with chosen allowances. Only column 0 is checked directly as arcs of the minor
  circle; the other columns follow from the rows turned alike and the tensor-product weights. It then checks radii,
  interval lengths, origin, axis, and the analytic surface through the NURB at its corners (which fix the patch's
  placement) and centre. The `edge_swap_inside_offset` row records an existing wrong offset side (#17091), not a
  proven value.
- **Tests:** the unit tests that set the flag run on the default kernel; their expected values are unchanged.
