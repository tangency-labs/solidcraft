#### UB-206. `SmFilletExecutive::SetFilletParameters` dereferences a null `pBrep` and a null `pOptVarRadLaws`
- **Severity:** Crash / undefined behaviour
- **Area:**
- **Location:** `source/SMLib/src/SmFilletExecutive.cpp:917` (`pBrep->GetContext()`) and `:998` (`pOptVarRadLaws->GetSize()`), the edge-pointer overload at `:865`.
- **What happens:** `pBrep` may be null (`:884` only skips the target assignment), but once the edge list is non-empty and the list sizes pass, `:917` reads its context. `pOptVarRadLaws` is optional (`NULL` default), yet the first variable-radius edge reads its size.
- **Trigger:** a null B-rep with a non-empty edge list whose type and value lists have one entry or one per edge; or a variable-radius type with a null law list (with a valid cross-section type, so the edge loop is reached).
- **How found:** read during porting (`blend.fillet_executive_p2`, PR #267).
- **Confirmed:** no. The shim refuses exactly these inputs, following upstream's order of early returns.
- **Severity:** crash/UB (null dereference).
- **Rust port:** `SM_ERR_NULL_POINTER` at the same points; the difftest compares both as guard errors.
