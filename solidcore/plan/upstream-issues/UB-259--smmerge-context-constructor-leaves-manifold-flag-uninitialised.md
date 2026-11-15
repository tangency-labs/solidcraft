#### UB-259. `SmMerge(const SmContext&)` leaves `m_bManifoldBoolean` and `m_bIntersectionLoopClosed` uninitialised
- **Severity:** Crash / undefined behaviour
- **Area:** Booleans
- **Location:** `source/SMLib/src/SmMerge.cpp:167`, `SmMerge::SmMerge(const SmContext&)`; read by `SmMerge::ManifoldBoolean` (`:2953`, `:3009`) through `SmMerge::PiecewiseMerge` (`:341`).
- **What happens:** the one-argument constructor initialises every other member but not `m_bManifoldBoolean` (the full constructor sets `TRUE`) or `m_bIntersectionLoopClosed`. `SmFilletExecutive` derives from `SmMerge` through this constructor, and `DoPiecewiseMerge` calls the inherited `PiecewiseMerge`, whose first face's `ManifoldBoolean(SM_BO_PARTIAL_MERGE)` reads the indeterminate flag before the loop sets it `FALSE`.
- **Trigger:** `SmFilletExecutive::SetDoPiecewiseMerge(TRUE)` then `DoFillet()`. The read only decides anything when the partial merge has common vertices; merging the first fillet face into the new empty face B-rep has none, so the value has not been observed to change a result.
- **How found:** porting `SmFilletExecutive::DoPiecewiseMerge` (`blend.fillet_executive_p1`); the port starts the flag at `TRUE`, the full constructor's value.
- **Confirmed:** by reading only; the piecewise-merge difftest cases of `blend.fillet_executive_p3` agree with the C++ build.
