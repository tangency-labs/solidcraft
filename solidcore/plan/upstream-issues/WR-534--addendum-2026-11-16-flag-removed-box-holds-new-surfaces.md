Updates: WR-534

#### WR-534 (update, 2026-11-16). `local_operation_box_holds_new_surfaces` is removed, its fix superseded

The flag is gone, and with it its code (tangency-labs/solidcraft#14246, batch offset). With it, `local_operation` added each given surface's box over its natural domain to the body's box before taking the bounding sphere.

The default no longer needs it for its purpose (argued, and measured on the cases below). The neighbours' extension is derived from the moved surface (`reach::local_extension_reach`), not from the box, so a face moved further than the body's size is reached without it. A 20 mm cube's top moved up by 60, 200 and 1000 closes to the 20 x 20 x (20 + d) box at the defaults.

The radius still feeds other consumers: the executive's offset distance (radius / 2) is read by `is_cavity_region` (`offset_executive/p2.rs`), the collapsed-end displacement (`p2.rs`), `VertexOffset`'s radius (`p1.rs`) and `check_resolvable` (`p2.rs`). The block inflated that radius from a plane's natural domain, ±1234567 in u and v. Measured on `gen0020` and `gen0029` of `topology.brep_local_operation` (a disc of body radius 5.59 given three planes), the box with the planes had sphere radius 1.91e6 and 1.81e6, so the consumers saw an offset distance about 3.4e5 times the body's. Those two rows are the only ones the flag moved.

Removed:
- the field `UpstreamFixes::local_operation_box_holds_new_surfaces`;
- the `CalculateBoundingBox` block in `topology/brep/local_operation.rs`;
- SolidCraft's preset line;
- its entry in `fixes.json`.

- **Tests:** `tests/wr_534_local_operation_moves_a_face_far.rs` (`wr_534_far_moved_top_closes`) and the unit tests `moves_the_top_of_a_box_further_than_its_size` and `a_far_top_on_a_unit_square_plane_is_refused_as_not_sound` run on the default kernel. Removed is the test that set the flag; its cases and checks are the default test's.
- **Results:** no goldens row moves. The flag alone moved `gen0020` and `gen0029` of `topology.brep_local_operation`; with the block removed they keep main's result.
