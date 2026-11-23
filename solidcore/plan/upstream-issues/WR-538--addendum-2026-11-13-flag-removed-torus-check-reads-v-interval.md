Updates: WR-538

#### WR-538 (update, 2026-11-13). `torus_check_reads_v_interval` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A13a, which retires three surface flags). `torus_assert_valid` (`surface/surface/assert_valid.rs`) tests the V interval in its V overlap test and reports the U and V overlap tests as 3 and 4; the surface of revolution's U domain test reports as 13. These are the ids their labels give them.

- **Tests:** `wr_538_torus_check_reads_its_v_interval` runs on `Kernel::default()`; its upstream half is gone. `issue_1672_torus_minus_z.rs` no longer sets the flag.
- **Results:** no goldens row moves (all 283 units, measured at the retiring change on the default kernel). The batch survey's group A holds for this flag.
