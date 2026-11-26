Updates: WR-469

#### WR-469 (update, 2026-11-23). `new_edge_vertex_tolerance_covers_gap` is removed; the step is always on

`make_edge_in_face_with_vu` (`crates/solidcore/src/topology/brep/p9.rs`) always grows each end vertex's tolerance to
twice its gap to the new edge's end, after `refresh_tolerance(false)` has kept it (tangency-labs/solidcraft#17985).
The step only grows a tolerance. Removed: the field and its doc file, the flag-off routine test
`new_edge_keeps_its_vertices_tolerance_by_default`, the field's sets in the `wr650`, `issue_2020` and `issue_1945`
tests and its name in the `issue_18166` list, and the flag-off run of the #1480 union test
(`wr469_union_flag_alone_grows_eight_vertex_tolerances`; since #17888 both runs were the same body).

- **Whole body:** `crates/solidcore/tests/issue_17985_projected_arc_vertex_tolerance.rs`, the goldens case
  `primitive.api_trim_ops` `gen0072`: a box with every edge rounded to r 0.567, trimmed by an arc projected along
  about -Z. The new trim edge ends 5.3682556e-6 from its vertex on the front face, whose tolerance is the box's
  1e-5. Without the step the vertex keeps 1e-5 and level 2 reports "Edge/Vertex gap across Edgeuse start larger
  than tol (offset=5.3682556e-06, tolerance= 5e-06)"; with it the tolerance is 1.0736511129039172e-5 (twice the
  gap) and level 2 is clean. The other 17 vertices have the same tolerances either way (MEASURED). The test pins
  the 18 vertices and tolerances, the `2 * gap` relation and the clean check, and fails without the step.
- **Booleans:** forcing the step on in every test of the `booleans` and `topology` integration suites changed no
  final vertex tolerance of any Boolean result (MEASURED): on the coped-tube rigs (wr490) and the #480 ring lip it
  grows tolerances in intermediate bodies only, which later steps refit; on the #1480 union WR-306's fit (#17888)
  grows the eight affected vertices past the step's value.
- **Defaults:** SolidCraft's preset (`UpstreamFixes::all()`, `crates/smlib/src/core.rs`) had the step on, so the
  app's results do not change. `Kernel::default()` changes.
- **Goldens:** 1 row of 1 unit moves, by one value: `primitive.api_trim_ops` `gen0072` (the whole body above),
  one vertex tolerance, 1e-5 -> 1.0736511129039172e-5. `topology.face_p1` does not move. Before #21381 its 37
  `MoveSeam` rows on `vase_rev.smb` face 0 moved under the step (the UV-trim tally 15 -> 2, a split-edge vertex
  2.0952 -> 4.1903). Those rows split the surface along a seam the surface does not have; #21381 (UB-138, #21368)
  drops that seam move, so the split and its edge are gone. With #21381 under this change, answering the step with
  the tolerance the vertex already had changes no `topology.face_p1` row (MEASURED, all 2569 rows).
- **Below the goldens' resolution:** `intersect.curve_class_p6` `imprint_face_split_f0` and `_f2` also move, but
  their stored block sums (6e20, the box's huge tolerances) cannot show it: the new vertex 0.059021 past the placed
  line's end had 0.059022494374947436 (1e-6 over its gap) and now has twice its gap, 0.11804298874989479 (MEASURED
  with the step forced off and on). `imprint_face_split_new_vertex_tolerance_is_twice_its_gap` in that unit's tests
  replays both rows and asserts the value from the line's end and the vertex. `boolean.api_brep`, whose 8 rows moved
  when the flag was first measured, moves in no value now (MEASURED: its full debug digest is the same either way).
