#### WR-82. `SmCPolyFace::AddPolyFace` accepts a composite face, so composite queries recurse without bound
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmPoly.cpp` `SmCPolyFace::AddPolyFace`, and the composite overrides of `GetPolyEdges`, `GetNormal`, `CalculateBoundingBox` and `GetOKBackPtrs`.
- **What happens:** `AddPolyFace` takes any `SmPolyFace`, including another composite or the composite itself, and sets that face's `m_pCPolyFace` (overwriting a composite's own). The composite overrides then call the same virtual on every member, so a composite that contains itself recurses until the stack overflows.
- **Trigger:** `c->AddPolyFace(c)` followed by `c->GetPolyEdges(...)` (or `GetNormal`, `CalculateBoundingBox`, `GetOKBackPtrs`). Upstream callers only add shell faces (`SmLineSegClass.cpp:1879-1883`).
- **How found:** review of the `query.poly` port; the Rust mirror overflowed its stack on the same sequence.
- **Confirmed:** no (by reading; not run against the C++ build).
- **Severity:** crash (stack overflow) on misuse.
- **Rust port:** Deviates: `add_cpoly_face` / `cpoly_add_poly_face` return `InvalidInput` for a composite face.
