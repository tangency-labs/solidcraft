Updates: WR-17

#### WR-17 (update, 2026-11-12). `stop_tracking_checks_own_callback` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch A15b). `TrackTopologyChanges::stop_tracking` and `stop_tracking_with_brep` (`topology/topology.rs`) clear the context's system notify callback, and record the post-edit dependencies, only when the installed callback is the tracker's own (`is_tracking`), and the healer's `capture_changes` (`heal/heal_data/p3.rs`) does the same. So a tracker started twice now stops too, where upstream leaves its callback installed (the second `StartTracking` finds it in the slot). `TrackTopologyChanges::new_with_kernel`, `stop_checks_own_callback` and `set_stop_checks_own_callback` are gone with the flag.

- **Tests:** the WR-17 tests in `topology/topology.rs` keep their corrected halves on `TrackTopologyChanges::new`; `captured_lists_survive_callback_replacement_and_active_reset` now asserts that the replacing callback stays installed, and `capture_refuses_a_replaced_tracker_and_leaves_it_installed` (`heal/heal_data/p3.rs`) covers the healer. The flag's lookup test is gone with the flag.
- **Results:** the goldens runners build their trackers with `TrackTopologyChanges::new`, which never read the flag, so the batch survey saw no change. Retiring it moves 15 rows of `topology.topology` and 3 of `topology.brep_p5`, blessed with their old and new values and reasons in `plan/findings/topology.topology--a15b-stop-tracking-own-callback-retired-2026-11-12.md`.
