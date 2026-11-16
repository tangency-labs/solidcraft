#### WR-177. `SmFace::FindVertexSector` evaluates an empty domain for poles after its first vertex use
- **Severity:** Wrong result
- **Area:** Topology, attributes, solvers
- **Location:** `source/SMLib/src/SmFace.cpp:9762`, `SmFace::FindVertexSector` (def. `:9408`; the pole points `:9788`-`:9803`).
- **What happens:** In the 3D branch (an edge use without UV trim curve), `SmExtent2d sSurfDomain` is declared inside the loop over vertex uses, but the natural domain is stored in it only on the pass that first fetches the poles (`bCheckedPoles`). On later passes a vertex at a pole gets `sEU1UV` from a default (inside-out) extent, which is then passed to `SectorClassifyCurveOn` as the singular UV point.
- **Trigger:** A vertex at a surface pole with two or more vertex uses on the face's upward face use, their edge uses without UV trim curves, and sector validation requested.
- **How found:** read during porting (`topology.face_p4`).
- **Confirmed:** no.
- **Severity:** wrong result.
- **Rust port:** Reproduces it (the domain is only set on the pass that fetches the poles).
