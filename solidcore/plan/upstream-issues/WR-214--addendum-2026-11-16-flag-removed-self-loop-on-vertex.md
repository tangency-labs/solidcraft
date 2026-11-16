Updates: WR-214

#### WR-214 (update, 2026-11-16). `proto_edge_self_loop_on_vertex_degenerate` is removed; a self-loop proto edge on a vertex is always degenerate

`ProtoTopologyManager::resolve_proto_edge` (`topology/proto_topology/p2.rs`) always treats a proto edge from a proto
vertex back to itself as degenerate when the vertex definition it would split is on a vertex in either B-rep. The
opt-in of the 2026-10-26 addendum no longer exists. Removed: the field and its doc, the manager's
`split_self_loops_on_vertices` toggle, the topology intersector's opt-in, SolidCraft's preset line, its entries in
`kernel_cases/fixes.json` and the `issue_2260_gn_pipe_refined_range_end.rs` flag list, the test settings in
`issue_1476`, `issue_1818`, `issue_1945`, `issue_1950`, `issue_2020` and `wr650`, and the flag-off tests
(`issue_480_ring_lip_union_with_the_seam_at_the_crossing_fails_by_default` and the manager's
`resolve_self_loop_on_a_vertex_splits_by_default`).

- **Why it is correct.** The split adds a second vertex definition that copies the first one's two classifications. In
  the B-rep where that definition is on a vertex, both ends of the curve are then that one vertex, while the other
  B-rep's edge is split twice. The two B-reps disagree about the curve's ends. The merge then either fails in
  `SmMerge::MakeFaceuse` (`SM_ERR`, this record's trigger), or returns a body whose loops gap by the curve's length.
- **A second outcome of the split: a body that fails the check.** In `boolean.api_brep` tess0033 a box's corner lies
  1.58e-5 inside a sphere (r 5.08e-4), less than the box's and the sphere's zone tolerances together (2e-5). The
  sphere crosses the corner's three edges 9.9e-5, 2.3e-5 and 2.2e-5 from it; `CanCombineVertexDefinitions` combines all
  three crossings with the corner (within `5 * (tol1 + tol2)` = 1e-4), so every arc of the cut runs from the corner's
  proto vertex back to itself. One proto edge holds them. With the split, the union fails `check_brep` Level 2 (test
  #0 edge gaps of 1.0e-4 and 9.4e-5 against tolerances of 2e-5, then `ValidatePointers`). With the proto edge dropped,
  the union is the box and the sphere meeting at the corner: Level 2 clean, 7 faces, 13 edges, and the volume is the
  box's plus the sphere's. That sum exceeds the exact union by the corner's overlap with the ball, which lies inside the
  box spanned by the three crossings (1.11e-14 of 7.43e-10; at most 4.8e-14). Keeping the cut would need the
  crossings not to be combined with the corner, which `CanCombineVertexDefinitions` decides for every Boolean.
- **Goldens:** four rows move. tess0033 is the case above. `wr614_strip_measured_union_d1e-05` unites two z-prisms on
  [0, 4], so the exact union is symmetric under z -> 4 - z. With the split, one top vertex of the strip sits 2.3e-6
  off the bottom one (1.99979974, 19.99999525 against 1.9998, 19.9999975), so the body is not a prism. Without it the
  top and bottom sections are equal and the volume is four times the section's area (128.0000317498, to 1e-14). The
  remaining 3.17e-5 above the closed form 128 - 20.2 d^2 comes from where the WR-614 strip rule puts the section's
  two strip vertices, which the split's bottom section already had. tess0095 (a sphere united with a box, then
  tessellated with three edges' curves removed) stays `SM_ERR` with different partial output. With the split its
  union is the sphere alone (volume 7.5705329e-10, the sphere's), though the box reaches outside the sphere. Without it
  the box is kept (volume 1.19167e-9, against 1.19146e-9 for the union by quadrature). That body still fails Level 2
  (test #0 gaps of 9.4e-5, test #61), as the split's did. `blend.fillet_executive_p3`
  `stubs4-delete-edge-bottle-e8-r0.1` keeps its topology counts and its eight Level 2 gap reports. Its centroid x
  moves by 3.1e-10 on a body about 4 wide. That is less than the row's own integration error at its accuracy of 1e-4
  (4.7e-10 from the value converged at 1e-8). At 1e-8 the two bodies' areas agree to 4.6e-9 and volumes to 2.4e-10
  relative.
- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Tests:** `issue_480_ring_lip_union_with_the_seam_at_the_crossing` runs on the default kernel with its assertions
  unchanged. `box_corner_inside_a_sphere_by_less_than_tolerance_unites_validly` (same file) pins tess0033's union.
