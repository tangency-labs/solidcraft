Updates: WR-469

#### WR-469 (update, 2026-10-31). The fix is solidcore-only; the fork at 26cc451 does not have it

The record's fix was confirmed on a local patch over tangency-labs/smlib@5af7070; it was never committed to the fork. At tangency-labs/smlib@26cc451 `SmBrep::MakeEdgeInFaceWithVU` (`source/SMLib/src/SmBrep.cpp`, about lines 14070-14072) ends with `RefreshTolerance(FALSE)` on the start and end vertices only, with no growth to twice the end gap. `Kernel::fixes.new_edge_vertex_tolerance_covers_gap` is therefore a solidcore-only fix; its doc says so. No behaviour changes.
