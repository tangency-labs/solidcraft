# The near-knot rail trim as the only path: the goldens rows it moves

`FilletBrep::trim_rail_curves` (`crates/solidcore/src/blend/fillet_geom/p5.rs`) snaps each end it trims at a vertex's drop onto the rail's next knot inside the trim when the rail between the two is no longer than the target B-rep's zone tolerance (WR-664). Before tangency-labs/solidcraft#18918 this ran only with `fixes.fillet_rail_trim_at_near_knot`, and it looked up the solver's first edge use first, refusing with 1006 when there was none. Schematic: `blend.fillet_geom_p5--18918-rail-trim-at-near-knot/near-knot-trim.png` (source `near-knot-trim.vectorcraft`).

Both units were run with the snap forced on against main's expectations, before and after the edge-use change; then at the PR head. The snap also moved rail ends away from the end curves built before it, which opened a blend face's loop; the end curves now follow (last section).

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
| `stubs4-delete-edge-bottle-e8-r0.1` | rail 0: 3.6e-6 of rail before a knot | on the knot | 0 → 0 | 0 → 0 |

Why the new values are right:

- **sphere_diff rows.** On main each rail starts 9.5e-7 to 9.8e-7 before its knot at 0.004, so its first span is 9.5e-7 to 9.6e-7 long, below the zone tolerance, with coincident control points: the point singularity WR-664 describes. With the snap it starts on the knot, and its first span is 6.7e-3 to 6.9e-3 long (MEASURED; asserted for every rail edge by `blend_fillet_executive_p3::end_to_end::wr664_near_knot_trims`). In the r0.5 rows one rail end also lay 2.2e-8 past a knot; it moves onto it. What remains, on one or two rails per row, is the rail's own last span at its other end, knot to knot and 9.5e-7 long before and after, which no cut at a knot lengthens (tangency-labs/solidcraft#19075). Why e1-r0.5 keeps four Test #15 lines while its mirror e4-r0.5 drops to two is not explained.
- **Their golden values** (against main c9d9fc0bbe). Area (`mass_properties[0]`) moves by at most 4e-11 relative, volume, centroid and moments of inertia by at most 4e-11 relative. The products of inertia [10] and [11] and the centroid's x [3] are zero by the bodies' symmetry and sit at rounding level (1e-8 and 2e-7 against moments of inertia of 2e8, and 6e-14); they move by up to 22 % of their old values, e.g. e4-r0.5 [10] 9.501e-9 → 1.1626e-8. In e1-r0.1, e4-r0.1 and e4-r0.5 the rail edges carry 3 to 6 control points fewer (e1-r0.1: 310 → 307 on one rail), so the result's geometry sums move with them (`result_geometry` block 0, third entry, e.g. e1-r0.1 29239.84 → 29063.77, e4-r0.1 -10439.90 → -10792.27) and so do a few `result_topology` entries (e1-r0.1: 281779 → 281776). In e1-r0.5 `result_topology` is unchanged. The other `result_geometry` entries move by at most 4e-8 relative.
- **bottle-e8-r0.1.** MEASURED with a probe print in `snap_rail_end_to_knot` (not committed): the snap moves one end, the start of rail 0, from u = 0.068345145725960 onto the knot at 0.068348744829285, 3.6e-6 of rail, below the zone tolerance 1e-5: on main that rail started in a span of 3.6e-6, the WR-664 case. Why `wr664_near_knot_trims::bottle_e8_r0_1`'s first-span check passes on main as well is not explained. Its `mass_properties[0]` moves from 33.092189857598 to 33.092189749777 (3.3e-9 relative) and `result_geometry` blocks 0, 3, 4 and 5 by up to 5.3e-7 relative.

## The end curves follow a snapped rail end

The snap moves a rail end along the rail after the geometry's end curves and cross sections were built at the vertex (`create_end_curve`, the corners' cross sections). MEASURED with probe prints (not committed) on `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1` and `-e4-r0.1`:

- In the pseudo B-rep the end curve is the UV line u = 69.391665335930654 from v = 1 to v = 0, built before the trim. The snap moves both rails' ends: by 4.3e-9 and 9.56e-7 in u (one rail's last traced span, 2^-20 long, tangency-labs/solidcraft#19075).
- The face builder (`sm_FixUVTrimCurves`, `topology/face/p2.rs`) moves both curve ends at each loop corner onto the vertex's UV point, which is the end of the curve before it in the loop: at v = 0 the rail's snapped end, at v = 1 the end curve's old end. The face's loop is closed then.
- When the fillet merges, each rail's UV curve is dropped again from its trimmed 3D curve (`trim_original_surfaces` → `drop_iso_curve`). At v = 1 that undoes the face builder's move of the rail, and the corner opens again: by 9.6e-7 in u on e1-r0.1 (4.2e-9 on e4-r0.1, as on main where no end snaps).
- The volume enclosure (`mass/enclosure.rs`) closes that corner with a segment whose length it bounds by 400.9 (`side_speed_bound`: 2p over the 2^-20 span, times the diameter of the whole surface side's control points), and its cap of 1.78e7 makes the volume band [-1.77e7, 1.80e7]: `enclose_mass_properties` refuses the body ("a volume not proved positive"). That bound is loose by itself; it is filed as an issue of its own.

![Top: a blend face's corner in UV, the rail at v = 1 cut at a knot short of the vertex, the end curve still running from the vertex, the loop open between them. Bottom: the end curve's end at the rail moved to the knot, the loop closed, the vertex where it was.](blend.fillet_geom_p5--18918-rail-trim-at-near-knot/end-curve-follows.png)

*Look at the corner at the vertex: before, the rail and the end curve end at different points; after, at the same knot (source `end-curve-follows.vectorcraft`).*

`FilletBrep::trim_rail_curves` now, where the snap moved a rail end, moves the end of each UV trim curve of the geometry that ends at the rail's vertex (none of its rails) to the snapped end, as `sm_FixUVTrimCurves` moves an end. After the drop alone the two differ by the drop's accuracy (4.2e-9 in u on main's e1-r0.1), a gap main's rows carry and certify; a snap's gap is up to about the zone tolerance, some 200 times that here. Making the drop's difference follow as well moved 31 rows of this unit in which nothing snaps (corner3x2, curve-sih, tabbed plate rows; MEASURED, unexplained per row), a change of its own outside WR-664, so it is left as it was. The cliff-rollover and self-intersecting rail trims have no snap; whether their trims move a rail end away from an end curve is not measured. With it the blend faces' loops of the four sphere_diff rows close to rounding (`blend_fillet_executive_p3::end_to_end::wr664_near_knot_trims::*_loops_close`, red before) and e1-r0.1's volume is certified again, [169498.278, 169499.151] as on main (`round_definition_rows`). The bottle's other blend face, a patch without a definition, has three corners open by 1.4e-10 to 3.9e-6 on main as well; not touched.
