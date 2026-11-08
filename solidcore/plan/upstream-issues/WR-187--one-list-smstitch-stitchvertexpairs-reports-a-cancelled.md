#### WR-187. One-list `SmStitch::StitchVertexPairs` reports a cancelled squeeze as done
- **Severity:** Wrong result
- **Area:** Numerical integration
- **Location:** `source/SMLib/src/SmStitch.cpp:3127`-`3194` (one-list `StitchVertexPairs`).
- **What happens:** A second short common edge cancels the squeeze (`bDoSqueeze = bDoGlue = FALSE`), but `pSqueezeEdge` keeps the first short lamina edge. Nothing is glued or squeezed, yet the pair's vertex is added to `rDeletedVerts`, `pSqueezeEdge` to `rDeletedEdges`, the list entry is cleared and, keeping the second vertex, `pV` moves to it. The same happens when the short common edge is manifold (no edge reported).
- **Trigger:** Two coincident vertices joined by two short edges, or by one short manifold edge.
- **How found:** read during porting (`heal.stitch_p2`).
- **Confirmed:** no (source inspection).
- **Severity:** wrong result.
- **Rust port:** Reproduces it.
