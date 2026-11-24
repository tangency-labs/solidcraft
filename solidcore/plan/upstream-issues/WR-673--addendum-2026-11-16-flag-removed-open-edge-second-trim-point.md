Updates: WR-673

#### WR-673 (update, 2026-11-16). `offset_open_edge_second_trim_point` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch offset). For an open edge's second trim point, `trim_extended_edge_ends` (`offset/offset_geometry_creation.rs`) passes over a solution only when it is the first trim point again, reached round a closed 3D curve (`is_first_point_again`):
- the lifted 3D curve is closed over its natural interval (`SmCurve::IsClosed`, within `10 * this_approx_tol_3d`); and
- the span from the first trim point's parameter to the candidate's is itself closed: its ends lie within that tolerance of each other and its middle does not.

That is the twin across the seam that gave half a sphere cap's rim the whole circle (#2174). The fork's tangency-labs/smlib@7ee0876, and the flag, passed over every solution within the tolerance of the first point whenever the edge was open, whether or not the curve was closed. That also dropped the real second point of an open edge that is short. A closed edge's second point is unchanged (#14062).

The same search serves both callers of `trim_extended_edge_ends`: `ExtendTrimEdges` (`trim_extended_edge`) and `MateEdges` (`mate_edges`), so it runs on every offset, shell and local operation. WR-666's choice between two cuts at a vertex of more than three faces (`first_cut_from_far_end`) uses the same test. Before, it passed over every cut within the tolerance of the first point, by distance alone. No goldens row moves for that (full bless).

The test runs for every candidate that improves on the nearest so far, on an open edge's second end: whether the curve is closed over its natural interval (`SmCurve::IsClosed`, three evaluations), and only for a closed curve, whether the span is (three more).

Removed:
- the field `UpstreamFixes::offset_open_edge_second_trim_point`;
- SolidCraft's preset line;
- its entries in `fixes.json` and in the flag lists of `issue_1945`, `issue_2020`, `issue_2260` and `wr650`.

- **Tests:**
  - `tests/issue_2174_cap_rim_two_edges.rs` thickens the three sheets both ways under the issue's preset to manifold solids within 1e-6 of 331 pi/3 and 271 pi/3. Removed are the tests that set the flag alone (`intersected_cap_thickens_with_this_fix_alone`) and that ran the preset without it (`flag_off_closes_the_gap_at_the_vertex_images`). Both now run the same kernel as the kept test.
  - `a_short_open_edge_keeps_its_second_trim_point` (unit test): a point 4e-6 along an open line, and 5e-6 along a closed circle of radius 10, stays a candidate at a tolerance of 1e-5, while the circle's seam twin of its start is passed over. A test of the distance alone fails it.
  - `wr_673_extend_keeps_a_second_point_near_the_first` (goldens crate): the first `ExtendTrimEdges` of `gen0051`, `gen0186` and `gen0681` succeeds. With the call site in `trim_extended_edge_ends` put back to the distance-only guard, it fails (`gen0051` returns 1001; measured).
  - `wr_673_moved_rows_trimmed_an_open_edge_round_to_its_start` (goldens crate), below.
- **Results:** two goldens rows of `offset.offset_geometry_creation` move: `gen0448` and `gen0723` (random scripts on `vase_rev.smb` and `cone.smb`, whose lifted curves are closed circles).
  - A row can move only if main's chosen second point is one the new test passes over, since the test only drops candidates of a strict-minimum search (argued from the code, not asserted).
  - On main, `TrimExtendedEdge` returned [0, 91.53] (`gen0448`, its second trim) and [0, 15.31] (`gen0723`, its first). Now it finds no second trim point and returns none: the edge is a bad edge, as upstream reports one.
  - That main's result was the defect is proven on those inputs by `wr_673_moved_rows_trimmed_an_open_edge_round_to_its_start`, at the op's state. The 3D curve's natural interval is exactly main's interval, the curve is closed, and `is_first_point_again` holds from either end. So main's trim ran the whole curve, from the first trim point back to it. Yet the base edge is open and its two vertices lie more than 10 x the approximation tolerance apart. The test also asserts the op now returns no interval.
  - With the distance-only test of the fork, `gen0051`, `gen0186` and `gen0681` also moved, from 0 to `SM_ERR` at `ExtendTrimEdges`. They keep main's result now: their dropped point was not a seam twin.
  - The `issue2174` rows of `offset.offset_executive_p2` (the default kernel) do not move.
