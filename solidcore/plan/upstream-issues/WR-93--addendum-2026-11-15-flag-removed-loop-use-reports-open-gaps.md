Updates: WR-93

#### WR-93 (update, 2026-11-15). `loop_use_reports_open_gaps` is removed; `IsClosed3d` reports an over-tolerance gap open

`is_closed_3d` (`topology/loopuse.rs`) always computes the over-tolerance value as `gap - xsect_tol_3d`, so a gap over its limit makes the loop open, with that gap's edge use, the gap and the limit. The 2026-10-31 addendum's opt-in no longer exists. Removed: the field, SolidCraft's preset line, its entries in the kernel rig's `fixes.json` and `issue_2260_gn_pipe_refined_range_end.rs`, and the flag-off half of `is_closed_3d_reports_open_gaps`.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes: a loop with a gap over its limit is open.
- **Goldens:** `heal.face_props` stage-3 rows (45 stored rows; 57 cases in full arrays, 12 of them hidden by block sums). Every change is a loop's `closed_3d` and `good_closed` going from 1 to 0 together (122 pairs) or a problem counter or flag going up by one (14). Upstream's `XSectTol3d - Gap` is negative whenever `Gap > XSectTol3d`, so it never exceeds the zero initial maximum and the old value could not be "open" for any input.
- **Tests:** `loopuse.rs` `closure` now expects the 0.5 gap over the 2e-4 limit to be open; the other `is_closed_3d` tests run on the default kernel.
- **Related:** #16296 (vshortt73) changes the limit inside this path; it needs a small merge with this removal.
