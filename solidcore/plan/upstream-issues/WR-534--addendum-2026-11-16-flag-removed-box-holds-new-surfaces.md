Updates: WR-534

#### WR-534 (update, 2026-11-16). `local_operation_box_holds_new_surfaces` is removed, its fix superseded

The flag is gone, and with it its code (tangency-labs/solidcraft#14246, batch offset). With it, `local_operation` added each given surface's box over its natural domain to the body's box before taking the bounding sphere.

The default no longer needs it. The neighbours' extension is derived from the moved surface (`reach::local_extension_reach`), so a face moved further than the body's size is reached without it. A 20 mm cube's top moved up by 60, 200 and 1000 closes to the 20 x 20 x (20 + d) box at the defaults.

Removed:
- the field `UpstreamFixes::local_operation_box_holds_new_surfaces`;
- the `CalculateBoundingBox` block in `topology/brep/local_operation.rs`;
- SolidCraft's preset line;
- its entry in `fixes.json`.

- **Tests:** `tests/wr_534_local_operation_moves_a_face_far.rs` (`wr_534_far_moved_top_closes`) and the unit tests `moves_the_top_of_a_box_further_than_its_size` and `a_far_top_on_a_unit_square_plane_is_refused_as_not_sound` run on the default kernel. Removed is the test that set the flag; its cases and checks are the default test's.
- **Results:** no goldens row moves. The flag alone moved `gen0020` and `gen0029` of `topology.brep_local_operation`; with the block removed they keep main's result.
