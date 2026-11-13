#### WR-168. `MakeWireEdge` / `MakeWireEdgeVertex` give the new edge a sliced copy of an edge-owned curve
- **Severity:** Wrong result
- **Area:** NLib
- **Location:** `source/SMLib/src/SmBrep.cpp:21676`-`21678` (`SmBrep::MakeWireEdge`) and `:21801`-`21803` (`SmBrep::MakeWireEdgeVertex`).
- **What happens:** When the curve passed in already belongs to an edge (`pCurve->GetEdge() != NULL`), the new wire edge gets `new (pCurve->GetContext()) SmCurve(*pCurve)`: the `SmCurve` base-class copy constructor, which copies none of the derived class's geometry (an `SmBSplineCurve`'s NURB, a line's point and vector). The new edge's curve is a plain `SmCurve` whose evaluation falls back to the base class (`SM_ERR`). The vertex tolerance checks right after still evaluate the original curve, so the calls succeed.
- **Trigger:** `MakeWireEdge(NULL, v1, v2, pOtherEdge->GetCurve(), ...)` (or `MakeWireEdgeVertex`) with the curve of an existing edge.
- **How found:** read during porting (`topology.brep_p3`, PR #204).
- **Confirmed:** no (not run: the port and its difftest stop at `topology.shell` inside `MakeEdgeTopology` first). Update (PR #204, after topology.shell was wired): the wire makers now run and are compared (49 passing cases), but every wire fixture passes a curve no edge owns, so this branch is still not run.
- **Severity:** wrong result (an edge without usable geometry).
- **Rust port:** Deviation: the port clones the curve with its geometry (`Curve`'s `Clone`, the copy constructor of each curve kind); a geometry-less base curve cannot be represented.
