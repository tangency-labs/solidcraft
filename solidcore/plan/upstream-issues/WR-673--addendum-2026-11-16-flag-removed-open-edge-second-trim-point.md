Updates: WR-673

#### WR-673 (update, 2026-11-16). `offset_open_edge_second_trim_point` is removed

The fix is the default and its flag is gone (tangency-labs/solidcraft#14246, batch offset). For an open edge's second trim point, `trim_extended_edge` (`offset/offset_geometry_creation.rs`) always refuses any solution point within `10 * this_approx_tol_3d` of the first trim point, as the fork's tangency-labs/smlib@7ee0876 does. A closed edge's second point is unchanged (#14062).

Removed:
- the field `UpstreamFixes::offset_open_edge_second_trim_point`;
- SolidCraft's preset line;
- its entries in `fixes.json` and in the flag lists of `issue_1945`, `issue_2020`, `issue_2260` and `wr650`.

- **Tests:** `tests/issue_2174_cap_rim_two_edges.rs` thickens the three sheets both ways under the issue's preset to manifold solids within 1e-6 of 331 pi/3 and 271 pi/3. Removed are the tests that set the flag alone (`intersected_cap_thickens_with_this_fix_alone`) and that ran the preset without it (`flag_off_closes_the_gap_at_the_vertex_images`). Both now run the same kernel as the kept test.
- **Results:** five goldens rows of `offset.offset_geometry_creation` move: `gen0051`, `gen0186`, `gen0448`, `gen0681` and `gen0723`.
  - The change only adds a conjunct to the candidate test of a strict-minimum search, so it can only drop candidates. A row moves only if main's chosen second trim point was dropped, that is, it lay within `10 * this_approx_tol_3d` of the first trim point. The open edge's trimmed interval then ran from a point back to the same point, round the whole lifted closed curve.
  - Raw records of main and the branch show where each row diverges:
    - In `gen0448` and `gen0723`, a `TrimExtendedEdge` op returned [0, 91.53] and [0, 15.31] on main. On the branch it finds no second trim point.
    - In `gen0051`, `gen0186` and `gen0681`, an `ExtendTrimEdges` op returned 0 on main and returns `SM_ERR` (1001) on the branch. Its trim finds no second point, and the script's later ops then run on a different state.
  - The `issue2174` rows of `offset.offset_executive_p2` (the default kernel) do not move.
