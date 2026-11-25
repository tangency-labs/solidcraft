Updates: WR-586

#### WR-586 (update, 2026-11-10). `boolean_temporary_transform_statuses` is removed

The Boolean's temporary origin shift (`SmTemporaryBrepTransform`) returns a failed bounding box of either operand before choosing a shift, stops on a failed forward translation (undoing the primary's when the other's fails, a failed undo taking precedence), and on cleanup attempts every surviving operand and returns the first inverse-transform error. This is now the only path, and the flag is gone. Upstream ignores all of these statuses. SolidCraft already ran with the flag on. Removed: the field, the `propagate_statuses` member of `TemporaryBrepShift` and `TemporaryBrepTransform`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change. Every goldens unit (283) passes unchanged, and the kernel rig passes, with this flag and `remove_slit_loops_where_solids_touch` removed together; the survey of #14246 found no golden or rig case that reaches a different result with the flag on.
- **Tests:** `boolean::merge::p1::tests` keeps the checked cases (`audit_483_boxes_fail_before_temporary_translation`, `checked_temporary_translation_stops_on_failure_and_undoes_the_first_operand`, `checked_temporary_translation_reports_a_failed_undo`, `checked_temporary_cleanup_attempts_every_survivor_and_returns_the_first_error`, `checked_temporary_translation_round_trips_real_boxes`); `temporary_translation_default_still_ignores_forward_statuses` and the assertion that the default ignored a failed box are removed.
