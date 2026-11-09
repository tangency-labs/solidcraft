#### UB-210. `SmFilletEdge::CalcFilletIntFillet` reads the context of a missing fillet corner
- **Severity:** Crash / undefined behaviour
- **Area:** Topology
- **Location:** `source/SMLib/src/SmFilletGeom.cpp:6051` (`SmFilletEdge::CalcFilletIntFillet`, from `:5959`: `sm_SrfSrfIntersection(m_cpCorner->GetContext(), ...)`); the failure branch after it calls `pCorner->RecordFilletError` (`:6072`).
- **What happens:** When the fillet/fillet edge has no B-spline 3D curve yet, the surface/surface intersection is called with the context of the edge's own fillet corner, which is never checked. An edge without corner (`m_cpCorner == NULL`) dereferences `NULL`. Upstream's corners make these edges with a corner, so only a caller that builds or calls one without it reaches this.
- **Trigger:** `CalcFilletIntFillet(crContext, pCorner)` on an `SM_FE_FILLET_X_FILLET` edge without curve or corner, both end vertices processed and both geometries with a solver and a fillet surface: fixture `regress_fif_no_curve_no_corner` (generated seed 748 of the 2000-seed sweep).
- **How found:** generated difftest case (the 2000-seed sweep while addressing the PR #242 review); gdb puts the SIGSEGV in `SmFilletEdge::CalcFilletIntFillet`.
- **Confirmed:** yes (SIGSEGV in the reference build before the shim refused it).
- **Severity:** crash (NULL dereference).
- **Rust port:** `FilletBrep::calc_fillet_int_fillet` returns `SM_ERR_NULL_POINTER` for an edge without corner on that path, before the unported intersection. The `blend.fillet_geom_p3` shim refuses (`SC_ERR_ARG`) a fillet/fillet edge without corner when it has no B-spline curve.
