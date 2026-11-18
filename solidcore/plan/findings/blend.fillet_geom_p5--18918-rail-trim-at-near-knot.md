# The near-knot rail trim as the only path: the goldens rows it moves

`FilletBrep::trim_rail_curves` (`crates/solidcore/src/blend/fillet_geom/p5.rs`) snaps each end it trims at a vertex's drop onto the rail's next knot inside the trim when the rail between the two is no longer than the target B-rep's zone tolerance (WR-664). Before tangency-labs/solidcraft#18918 this ran only with `fixes.fillet_rail_trim_at_near_knot`, and it looked up the solver's first edge use first, refusing with 1006 when there was none. Schematic: `blend.fillet_geom_p5--18918-rail-trim-at-near-knot/near-knot-trim.png` (source `near-knot-trim.vectorcraft`).

Both units were run with the snap forced on against main's expectations, before and after the edge-use change; then at the PR head.

## `blend.fillet_geom_p5`: no row moves

With the lookup, 87 of the 300 rows went to 1006: 73 rows that build (0), 13 that `Extent1d::set_min_max` refuses (1007, WR-661) and one 1001. Their solvers are built without edge uses. Without the lookup every row is as on main. Test: `blend_fillet_geom_p5::tests::wr664_rail_trims_of_a_solver_without_edge_uses_build` (the 73).

## `blend.fillet_executive_p3`: five rows move

Status 0 and the vertex, edge and face counts are unchanged in all five. Measured per rail edge (an edge between a blend face the round made and a face it did not) with the probe kept with this record's PR: the chord of each end span (edge end to the curve's nearest inner knot), inner control points closer than 1e-6, and the level-2 `check_brep` lines "Bad Repeated CPts: BSplineCurve has Internal Coincident ControlPoints" (Test #15). The zone tolerance is 1e-5.

| row | rail starts, main | rail starts, snap | coincident control points | Test #15 lines |
|---|---|---|---|---|
| `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1` | 0.0039990420, 0.0039990463 | 0.004, 0.004 | 8 → 2 | 4 → 2 |
| `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5` | 0.0039990247, 0.0039990463 | 0.004, 0.004 | 11 → 4 | 4 → 4 |
| `stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1` | 0.0039990463, 0.0039990420 | 0.004, 0.004 | 8 → 2 | 4 → 2 |
| `stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5` | 0.0039990463, 0.0039990247 | 0.004, 0.004 | 11 → 2 | 4 → 2 |
| `stubs4-delete-edge-bottle-e8-r0.1` | no end span below 1e-5 | the same | 0 → 0 | 0 → 0 |

Why the new values are right:

- **sphere_diff rows.** On main each rail starts 9.5e-7 to 9.8e-7 before its knot at 0.004, so its first span is 9.5e-7 to 9.6e-7 long, below the zone tolerance, with coincident control points: the point singularity WR-664 describes. With the snap it starts on the knot, and its first span is 6.7e-3 to 6.9e-3 long (MEASURED; asserted for every rail edge by `blend_fillet_executive_p3::end_to_end::wr664_near_knot_trims`). In the r0.5 rows one rail end also lay 2.2e-8 past a knot; it moves onto it. What remains, on one or two rails per row, is the rail's own last span at its other end, knot to knot and 9.5e-7 long before and after, which no cut at a knot lengthens (tangency-labs/solidcraft#19075). Why e1-r0.5 keeps four Test #15 lines while its mirror e4-r0.5 drops to two is not explained.
- **Their golden values.** `mass_properties` entries [3], [10] and [11] (all below 3e-4 in magnitude) move by at most 2e-3 relative, e.g. e4-r0.1 [11] 8.153058577e-7 → 8.168156847e-7 and e1-r0.1 [10] 1.4063812e-5 → 1.4084182e-5; the other entries stay within the golden's 1e-9. The rail edges carry 3 to 6 control points fewer (e1-r0.1: 310 → 307 on one rail), so the result's geometry sums move with them (`result_geometry` block 0, e.g. e1-r0.1 third entry 29239.84 → 29063.77) and so do a few `result_topology` entries (e1-r0.1: 281779 → 281776).
- **bottle-e8-r0.1.** Its rails have no end span below the tolerance before or after; which of its rail ends the snap moves is not measured. A snap moves an end by no more than about the zone tolerance along its rail (ARGUED from `snap_rail_end_to_knot`'s length test, whose length is accurate to 1 % of the tolerance); its `mass_properties[0]` moves from 33.092189857598 to 33.092189749777 (3.3e-9 relative) and `result_geometry` blocks 0, 3, 4 and 5 by up to 4e-7 relative.
