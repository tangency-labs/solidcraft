Updates: WR-446

#### WR-446 (update, 2026-11-16). `validation_does_not_make_uv_trims` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch topology-2). `sm_CheckEdgeuseGeometry` (`topology/brep/validation/pointers.rs`) evaluates the clockwise neighbour with `ok_to_make_uv_trim_curve = FALSE`, so `ValidatePointers` no longer stores UV trims on the body it checks. SolidCraft's preset already had the flag on. Removed: the field `UpstreamFixes::validation_does_not_make_uv_trims`, SolidCraft's preset line, its entry in `fixes.json` and the `issue_2260` list, and every test setting of it.

- **Results:** 3 goldens rows of `topology.brep_constructor` move (fan_four_shared_stitched, fan_three_shared_stitched, gen0011): `EndShell(TRUE)`'s check made 9 to 12 trims there. The goldens test `topology_brep_constructor::wr446_tests::validation_rows_differ_only_by_the_trims_the_check_made` rebuilds each, makes the neighbour trims upstream's check made, and gets exactly the topology digests of the rows before.
- **Tests:** the `wr446_tests` in `pointers.rs` keep their no-new-trim halves; the upstream half that witnessed the creation is gone.
