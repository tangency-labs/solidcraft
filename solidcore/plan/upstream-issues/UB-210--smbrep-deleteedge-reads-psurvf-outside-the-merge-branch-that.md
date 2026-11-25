#### UB-210. `SmBrep::DeleteEdge` reads `pSurvF` outside the merge branch that sets it (unreachable on well-formed topology)
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmBrep.cpp:6043` (`pOptSurfaceToKeep != pSurvF->GetSurface()`) and `:6046` (`pDelF->GetSurface()`) in `SmBrep::DeleteEdge` (def. `:5752`); `pSurvF`/`pDelF` are set only inside `if(bMergeFace)` (`:5911`), while the surface-swap block's guard (`:6037-6039`) is only `!bSplitLoop && !(both loops inner)`.
- **What happens:** The two guards differ by the strut test (`bMergeFace = !bSplitLoop && !bVIsStrut && !bVRadIsStrut && !bothInner`). If an end of the edge were a strut end without a split loop, a non-NULL `pOptSurfaceToKeep` would dereference the NULL `pSurvF`.
- **Reachability:** none on well-formed topology. `SmEdgeuse::IsStrut` (`SmEdgeuse.cpp:294-301`) is `GetRadial() == m_pCW`, and `m_pCW` is in the edge use's own loop use. `pEURad = pEU->GetRadial()`, so `pEU->IsStrut()` puts `pEURad` in `pLU`, i.e. `pLURad == pLU` (`bSplitLoop`). For a manifold edge the edge-use list alternates orientations (`GetRadial`, `SmEdgeuse.cpp:3561-3578`, is the previous use for a use oriented like the primary, the next one otherwise), so `pEURad->GetRadial() == pEU` and `pEURad->IsStrut()` likewise implies `bSplitLoop`. Hence `!bSplitLoop` forces both strut flags false, and with `!bothInner` that is `bMergeFace`: `pSurvF` is set (and `NER`-checked). Only an edge-use list whose orientations do not alternate could reach it.
- **How found:** read during porting (unit `topology.brep_p4`).
- **Confirmed:** no; unreachable by the argument above (no fixture B-rep has a non-alternating manifold edge-use list).
- **Severity:** crash (theoretical; malformed topology only).
- **Rust port:** `topology::brep::p4::delete_manifold_edge` keeps a defensive `SolidError::InvalidInput` there (code comment cites UB-210).
