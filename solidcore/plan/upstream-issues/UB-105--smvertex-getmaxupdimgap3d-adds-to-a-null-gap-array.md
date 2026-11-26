#### UB-105. `SmVertex::GetMaxUpDimGap3d` adds to a NULL gap array
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmVertex.cpp:2978` `SmVertex::GetMaxUpDimGap3d` (`pOptGapArray->Add(...)` for both gap kinds); reached by `GetMaxGap3d()` with its default arguments.
- **What happens:** The gap array is optional (default `NULL`) and is reset only when given, but every gap longer than `pOptTol3d` (all gaps without `pOptTol3d`) is added through `pOptGapArray->Add` without a check: a call without an array dereferences `NULL` as soon as one vertex use has a vertex/edge or vertex/face gap.
- **Trigger:** `pVertex->GetMaxGap3d()` (or `GetMaxUpDimGap3d()`) on any vertex of a wire edge with a curve or of a face with a surface.
- **How found:** read during porting (unit `topology.vertex`).
- **Confirmed:** no (the difftest shim always passes an array and reports the null dereference when none was asked for and a gap was added).
- **Severity:** crash.
- **Rust port:** `topology::vertex::max_up_dim_gap_3d` returns `SolidError::InvalidInput` when a gap is to be listed without a list.
