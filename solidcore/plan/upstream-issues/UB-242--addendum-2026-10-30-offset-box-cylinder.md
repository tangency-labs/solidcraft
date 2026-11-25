Updates: UB-242

#### UB-242 (update, 2026-10-30). Reached by an extended-offset shell of a box with a cylinder, where C++ then fails with SM_ERR
- **Location:** as UB-242; reached from `SmOffsetExecutive::PiecewiseMerge` (`source/SMLib/src/SmOffsetExecutive.cpp:2595`) through `SmMerge::PiecewiseMerge` (`source/SMLib/src/SmMerge.cpp:664`), `ManifoldBoolean` and `IntersectInsertRelate`.
- **What happens:** in the 14th `ManifoldBoolean` of the piecewise merge, the fourth edge definition's imprint makes its edge in the first B-rep; its second `MakeEdgeInFace`, on a closed edge, then fails because `SmFace::FindVertexSector` finds no sector for the edge's vertex. Both sides return `SM_ERR_NULL_POINTER` (1006) from the imprint after that first edit. Upstream goes on, and the merge later fails in `SmMerge::MakeFaceuse` with `SM_ERR` (1001), reported by `PiecewiseMerge` at `SmMerge.cpp:672`.
- **Trigger:** `SmApiShellBrepFull` of the #1528 box-and-cylinder operand (`fixtures/offset.offset_executive_p2/issue1528_query.json`, cases `issue1528_box_cylinder_api_shell_2_volume` and `_first`).
- **How found:** gdb on both sides.
- **Confirmed:** yes, on the C++ build.
- **Rust port:** stops at the failed imprint, as for every UB-242 case that may have edited a B-rep, and the shell fails with 1006. The two cases are parked with `cpp_defect = "UB-242"` (`crates/difftest/known_mismatches/offset.offset_executive_p2.toml`).
