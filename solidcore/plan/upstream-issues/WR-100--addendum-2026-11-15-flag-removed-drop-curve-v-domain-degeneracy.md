Updates: WR-100

#### WR-100 (update, 2026-11-15). `drop_curve_v_domain_degeneracy` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch intersect). `Surface::drop_curve` (`intersect/surface_drop_lib/p3.rs`) always tests the drop domain's own V length for `bDegenSurfV`, so a surface whose natural domain is degenerate only in V reports `SM_ERR_DEGENERATE_SURFACE` (`SM_CTC_DEGENSURFACE_FAIL`) and its failure record sets `bDegenSurfV` from V. Upstream tests the U length twice. Removed: the field `UpstreamFixes::drop_curve_v_domain_degeneracy`, SolidCraft's preset line, its entries in `fixes.json` and `issue_2260`'s list.

- **Tests:** the unit tests in `surface_drop_lib/p3.rs` run on the default kernel: the V-degenerate witness (natural domain `[0,0,1,0]`, `1034 / 17`, flags `(0, 1)`), the U-degenerate counterpart (flags `(1, 0)`) and the positive control (`1020 / 19`, flags `(0, 0)`). Removed is the test that pinned the upstream path, `wr100_v_domain_degeneracy_is_off_by_default`, and the control's off-path comparison.
- **Results:** two goldens rows of `intersect.surface_drop_lib_p3` move, `wr100_u_natural_degenerate_point_far` (its V flag 1 -> 0) and `wr100_v_natural_degenerate_point_far` (now the degenerate-surface failure), as the unit tests above pin them.
