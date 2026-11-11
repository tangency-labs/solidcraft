#### WR-182. `SmSurfaceDropCurve::DoTrace` leaves its start points linked on the curve list
- **Severity:** Wrong result
- **Area:** Intersection
- **Location:** `source/SMLib/src/SmSurfaceDropCurve.cpp:1766` `SmSurfaceDropCurve::TraceCurve2`: `m_vCurvePoints.Append(&rStartPoint)`, called by `DoTrace` (`:1689`-`1703`) for every node of `m_vStartPoints`; `SmTList` is intrusive (`SmTracePnt : SmListNode`, `SmTList.h:224`-`244`).
- **What happens:** The start node is appended to `m_vCurvePoints` without being removed from `m_vStartPoints` (the base tracer removes coincident start points first, `SmSurfaceTracer.cpp:621`-`637`). `Append` overwrites the node's links, so `m_vStartPoints` (a head pointer) then walks the curve ring: the other start points drop out of it and every point later added to the curve shows up in it, and a later `Remove` from either list unlinks the node from the other. `DoTrace` itself iterates a snapshot of the start list, but a `TraceCurve` fallback walks `m_vStartPoints` in its end test and `AddStartPoint`.
- **Trigger:** Any `DoTrace` with a start point (every drop that reaches the trace), followed by a read of the start points or by more curve points.
- **How found:** difftest `intersect.surface_drop_curve` gen0354 (a trace once the offset surface and `SmSurface::DropCurve` were ported): after `DoTrace` and further `ComputeNextPoint`/`TraceCurve2` ops, the C++ start list held the three new curve points.
- **Confirmed:** yes (C++ build, gen0354).
- **Severity:** wrong result (list state; the traversals still end, the nodes live in the block manager).
- **Rust port:** Does not reproduce it: the start and curve lists are id `Vec`s (`intersect::surface_tracer`). The difftest reports a start list that shares a node with the curve list as `-3` on both sides instead of reading it.
