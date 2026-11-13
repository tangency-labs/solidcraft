Updates: WR-92

#### WR-92 (update, 2026-11-12). `is_topological_vertex_rejects_cusps_and_tangent_errors` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch B55, which retires the two flags
`is_topological_vertex_rejects_cusps_and_tangent_errors` and `topological_vertex_has_only_edge_uses`). `is_topological_vertex` (`topology/vertex.rs`) always orients both edge tangents through the vertex, so a 180 degree cusp is not topological, and propagates a failed start-tangent evaluation as `Err`.

- **Tests:** the cusp, reversed-edge, inside-out-line and failed-tangent tests in `topology/vertex.rs` run on `Kernel::default()` with the fixed expectations; `tangency_and_topological_vertices` now asserts that a cusp is not topological. The flag's lookup test is gone with the flag.
- **Results:** the goldens rows the fix moves, with their old and new values:
  `plan/findings/topology.vertex--b55-two-flags-retired-2026-11-12.md`.
