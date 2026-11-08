#### UB-261. `SmFilletGeom::TrimOriginalSurfaces` reads the curve of an invalid fillet-B-rep edge
- **Severity:** Crash / undefined behaviour
- **Area:** Fillets
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:12316` (`SmFilletGeom::TrimOriginalSurfaces`, def. `:12226`): `pFBEdge->GetCurve()` on `pRail->m_pFilletBrepEdge1` after `rTI.GetBrepMate( pFBEdge )` returned NULL.
- **What happens:** SIGSEGV inside the inlined `SmEdge::GetCurve()`: `m_pFilletBrepEdge1` of a rail is not NULL but does not point to a live edge (suspected: an edge of the fillet B-rep deleted or merged after `SetFilletBrepEdgeMap` without the rail's map entry being cleared). Root cause not confirmed.
- **Trigger:** `SmApiFilletEdges` on edge 30 of `TestFiles/pt_TestFiles/Fillet/sphere_diff_3cylinders.smb` at radius 0.1 (circular, G1, thumbweight 1).
- **How found:** fillet executive p1 round-4 stub sweep (difftest `blend.fillet_executive_p3`); reproduced in a standalone program linked against the pinned libraries, backtrace `TrimOriginalSurfaces` <- `SmFilletExecutive::DoFilleting` <- `SmApiFilletEdges`, line from `SmFilletGeom.cpp` rebuilt `-g`.
- **Confirmed:** yes (native crash); root cause unconfirmed.
- **Rust port:** not compared: the p3 runner's fork crash trap rejects the case (`sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1`).
