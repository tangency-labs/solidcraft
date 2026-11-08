#### UB-100. `SmDerivSurfDefinition::GetSampleValues` dereferences a missing tangent reference
- **Severity:** Crash / undefined behaviour
- **Area:** Surfaces and volumes
- **Location:** `source/SMLib/src/SmSurface.cpp:3127`, `SmDerivSurfDefinition::GetSampleValues` (def. `:2941`); the check it relies on is `SmDerivSurfDefinition::AssertValid` test 2 (`:2286`).
- **What happens:** For `SM_TT_DIR_TO_REFERENCE_CURVE`, `m_pTangentReference->EvaluatePoint` is called unchecked. `AssertValid` accepts a `NULL` reference (its last clause, `m_pTangentReference == NULL`, holds for every tangency type), so the definition passes the state check and the call dereferences `NULL`.
- **Trigger:** `GetSampleValues` with `m_eTangencyType = SM_TT_DIR_TO_REFERENCE_CURVE`, `m_pTangentReference = NULL` and a boundary curve that drops onto the surface.
- **How found:** crash in the C++ reference run (generated difftest case of `surface.surface_p6`).
- **Confirmed:** yes (segfault).
- **Severity:** crash/UB (null dereference).
- **Rust port:** Returns `SM_ERR_NULL_POINTER`; the shim refuses the input.
