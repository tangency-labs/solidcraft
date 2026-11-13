#### UB-101. `SmPolyBrep::GetPolyEdges` with a common edge use reads loops as edges
- **Severity:** Crash / undefined behaviour
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmPoly.cpp:6505` (`(SmPolyEdge*) pPolyFace->GetList()`), `SmPolyBrep::GetPolyEdges`.
- **What happens:** The filtered branch walks each face's `m_pList`, which holds the face's loops, casts them to `SmPolyEdge*`, reads `GetOriginalEdgeuse()` at the edge member offset of an `SmPolyLoop`, and would add loop pointers to the edge array.
- **Trigger:** `GetPolyEdges(edges, pEdgeuse)` with a non-null edge use.
- **How found:** read during porting (`query.poly`).
- **Confirmed:** no (the harness never passes an edge use).
- **Severity:** crash/UB (type confusion, out-of-object read).
- **Rust port:** `PolyBrep::poly_edges(Some(..))` returns `InvalidInput`.
