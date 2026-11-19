Updates: WR-530

#### WR-530 (update, 2026-11-16). `local_operation_refuses_loose_tangent_edges` is removed

The refusal is the default and its flag is gone (tangency-labs/solidcraft#14246, batch offset). In a local operation, `ExtendTrimEdges` (`offset/offset_geometry_creation.rs`) refuses a tangent edge whose lift drops onto the other face's new surface farther than `10 * max(edge tolerance, approximation tolerance)`. The refusal is now typed: `OffsetFailure::LooseTangentEdge { edge, max_drop_to_surf, meet_tol }` (`SM_ERR`, kind `Unsupported`), raised by `ExtendTrimEdges` under `offset::offset_geometry_creation::extend_trim_edges`. It names the tangent edge of the operation's input, and `entity()` returns it. It carries the drop it measured and the limit it held it to, and its message gives both.

Only a drop within the limit passes (`drop_is_loose`: the drop is loose unless `max_drop_to_surf <= meet_tol`). A drop or limit that is not a number is refused, where the fork's `max_drop_to_surf > meet_tol` passed it. No goldens row moves for that change (full bless).

`EdgeuseOffset` passes on only that refusal. Any other failure of `ExtendTrimEdges` is reported and ignored, as upstream's. The fork and the former flag passed every failure on. That refused a lone disc's lamina edge in a local operation, an edge no tangent gap is involved in: 8 goldens rows of `topology.brep_local_operation` returned 1001 with the flag. They now keep main's result.

Offsets and shells are unchanged: the check runs only under `OffsetExecutive::is_local_operation`.

Removed:
- the field `UpstreamFixes::local_operation_refuses_loose_tangent_edges`;
- SolidCraft's preset line;
- its entries in `fixes.json` and in the flag lists of `issue_1945`, `issue_2020`, `issue_2260` and `wr650`.

- **Tests:**
  - `tests/wr_530_local_operation_loose_tangent_edge.rs` runs on the default kernel. Its first test is WR-530's boss wall moved out 1 off its base round. That is refused up front, before the edge is reached, as the wall's seam would open (`OffsetFailure::SeamOpened`), with or without the flag.
  - A new test, `wr_530_turned_and_raised_piece_beside_its_coplanar_twin_is_refused_as_loose`, gives a split block's top piece its own plane turned 0.02 rad about the split line and raised 1. The two pieces' planes are not parallel, so no step wall applies. The edge's drop lands far off the twin's plane, and the operation is refused with `LooseTangentEdge` (`SM_ERR`), its input unchanged. The refusal names the input edge between the piece and its twin, on the split line, and carries a drop past its limit, both given in the message (asserted).
  - `a_drop_that_is_not_a_number_is_loose` (unit test) pins the test at the limit, past it, and for a NaN drop or limit.
  - Removed are the tests that pinned the flag-off path.
  - The variant's class is pinned as row 21 of the table in `error/offset.rs`'s tests (kind `Unsupported`, no phrase, no defect).
- **Results:** no goldens row moves for this removal: the 8 rows the flag moved are back at main's.
