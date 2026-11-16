Updates: WR-69

#### WR-69 (update, 2026-11-15). `dupin_bottom_hyperbola_axis` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch intersect). `make_dupin` (`intersect/adv_surface_intersector.rs`) always gives the bottom hyperbola `1/sqrt(abs(k1))` as its second axis, the same two axis lengths as its conjugate top branch. Upstream uses `k2` for both. Removed: the field `UpstreamFixes::dupin_bottom_hyperbola_axis`, the `fix_dupin_bottom_axis` parameters of `make_dupin` and `adv_compute_singularity_point`, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260`'s list.

- **Tests:** `tests/wr69_dupin_bottom_hyperbola.rs` (`bottom_hyperbola_uses_other_principal_curvature`: at three saddle points, both normals, every conjugate branch has axes `(1/sqrt|k1|, 1/sqrt|k2|)` exactly, closed form) runs on the default kernel; its upstream half (the bottom axis `1/sqrt|k2|`) is removed.
- **Results:** no goldens row moves (leave-one-out run of the batch: every unit passes against main's goldens with this flag off). One `surface.surface_p2` row, `ssi_adv_ssi_clasic_ruled_f0_f0`, moves by rounding (about 6e-12 relative in its checksums) only together with `ssi_split_curves_at_singular_points`, which this batch leaves out.
