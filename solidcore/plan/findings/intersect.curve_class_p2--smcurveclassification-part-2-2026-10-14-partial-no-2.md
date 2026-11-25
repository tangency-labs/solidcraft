# intersect.curve_class_p2 (`SmCurveClassification`, part 2, 2026-10-14; partial, no disagreements)

- **`sm_RefineCCIToSurfaces` is only partly ported.** Its entry checks (recursion
  counter, point-solution test, both classification surfaces) are ported and compared;
  everything after them starts with `pEdge->GetCurve()` and walks the edge's faces, edge
  uses and Boolean mate, so it is the `UNPORTED(topology.edge)` site in
  `intersect::curve_class::p2::refine_cci_to_surfaces`. Whoever wires that site must port
  the remaining ~1,250 lines (`SmCurveClass.cpp` 1690-2940); the note is also in
  `topology.edge`'s `notes`. The shim runs only the entry checks (past them the bare
  stand-in edge's topology would be walked and its curve edited).
- **`EditIntervalMin` reads the stale slot past `m_lSize`.** When collapsing the
  neighbour removes the interval before the last one, upstream's next `(*this)[lIndex]`
  is past the end: the slot `RemoveAt` left behind, a copy of the last interval. The port
  reads that copy the same way; the following `CollapseInterval` fails (status ignored).
- **`InsertAt(0, ..)` into a non-empty classification** is UB-98 (unsigned shift loop);
  Rust returns `InvalidInput` and the shim rejects it.
- **Shim guards.** Edits are rejected when any interval end classifies to an
  `SmPoint3d` (UB-79: its zone tolerance is taken through `SmObject` virtuals); this is
  conservative (not every end is reached). Owned curves (`ReSet(.., bIsCurveOwned=TRUE)`,
  copied by `operator=`) are disowned before the ops that delete them, since they may be
  pool objects; a curve deep-copied by `operator=` reports the id of its source.
- **`operator=` keeps the old curve when `SmCurve::Copy` fails** (review fix, PR #85).
  For an owned source curve upstream frees this classification's owned curve, then calls
  `crOther.m_cpCurve->Copy(..)` and ignores its status. `SmHermiteCurve`, `SmConic` and
  `SmIsoCurve` have no `Copy` override, so the base `SmCurve::Copy` returns `SM_ERR` and
  leaves `m_cpCurve` unchanged: the target keeps its old (non-owned) curve, or `NULL` if
  it owned the old one. The first port always copied the handle; `--seeds 30000` found 4
  generated failures (gen8797, gen10415, gen11711, gen21161: a Hermite curve `ReSet`
  owned, then assigned), now `fixtures/intersect.curve_class_p2/adv.json`. Because the
  shim disowns the target before `operator=` (its curve may be a pool object), the
  "owned old curve freed, copy fails -> `NULL`" branch is not compared against the C++;
  it is covered by the Rust unit test `assign_owned_curve_without_copy` only.
- **`ReSet` with a new curve and no interval** dereferences `NULL` upstream; Rust returns
  `InvalidInput` before changing anything (the first port had already set
  `m_bIsCurveCurrent`).
