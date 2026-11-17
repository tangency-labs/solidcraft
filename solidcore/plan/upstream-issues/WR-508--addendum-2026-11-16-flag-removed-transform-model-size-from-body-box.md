Updates: WR-508

#### WR-508 (update, 2026-11-16). `transform_model_size_from_body_box` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch topology-2). `SmBrep::Transform` (`topology/brep/p5.rs`) takes a scaled body's model size from the diagonal of its bounding box (vertices, edges and faces, not grown by tolerances), so a body without spanning vertices (a lone torus) no longer gets a zero size and tolerance. SolidCraft's preset already had the flag on. Removed: the field `UpstreamFixes::transform_model_size_from_body_box`, SolidCraft's preset line, its entry in `fixes.json` and the `issue_2260` list, and every test setting of it.

- **Results:** see WR-507's addendum of the same date: the same certificate tests check the model size of every scaled body against its box diagonal.
- **Tests:** `brep/p5.rs::tests::transform_sizes_a_body_without_vertices_wr508` and `tests/wr507_508_transform_matrix_tolerances.rs` run on the default kernel; their upstream halves are gone.
