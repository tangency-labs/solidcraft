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
- **`InsertAt(0, ..)` into a non-empty classification** is UB-158 (unsigned shift loop);
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
- **Back-edges wired after merging main (2026-10-16).** `UNPORTED(intersect.curve_class_p2)`
  sites in `intersect::curve_class::{p3,p5,p6}`, `query::face::p2` (`CurveOnClassify`'s
  `ReSet`) and `heal::face_props` (`SetProps_Stage2`'s `ReSet`) now call the part-2
  members. Two sites did not become complete ports, because the code after the part-2
  call belongs to other units and was never ported:
  - `sm_InsertSolutions`' edge branch (part 5): its first upstream call on an edge target
    is `pEdge->GetCurve()`. Opaque edge handles cannot be resolved to a B-rep (the same gap
    part 3 documents), so it is now `UNPORTED(topology.edge)`. The branch body (vertex
    snapping, `InsertAt`, `sm_RefineCCIToSurfaces`) still has to be ported with it.
  - `MergeInterval` (part 6): the two `GetPointObjZoneTol3d` calls are wired. Everything
    after them is part 6's unported remainder (about 1,000 lines of vertex/edge imprinting
    through `MergePointClass` and the B-rep). It returns `Unported` naming
    `intersect.curve_class_p6` itself, with no `UNPORTED(` marker because it is not a call
    into another unit.
  - `sm_RefineCCIToSurfaces` (this unit) keeps `UNPORTED(topology.edge)` at
    `SmEdge::GetCurve`. `topology.edge` is now partial, but the classification's edge is
    an opaque handle with no B-rep, so the stub waits on handle-to-B-rep resolution, not on
    that unit. The same applies to `UNPORTED(topology.face_p2)` `SmFace::GetSurface` in
    `GetMinUV` / `GetMaxUV`.
- **Second review (2026-10-16).** The `InsertAt(0)` upstream issue was renumbered
  UB-98 -> UB-158 (main already had a UB-98, `SmCopyBrepMap`). `UpdatePointClass` now
  takes the mate's index from `point_class_mate_interval` instead of searching the
  intervals by address (same result). Added `fixtures/intersect.curve_class_p2/review_extra.json`
  (128 reviewer cases: `EditIntervalMin`/`Max` at scales 1e-6..1e5, offsets 0/1e5, shrink
  steps 0..2e-5 including the stale-slot path; `ConnectIntervals` over all end codes x
  allow x j; `UpdatePointClass` on a closed size-1 circle at index 0 and size; `ReSet`
  with tolerance 0/negative/1e-12/1e3 and an inverted interval, on an empty
  classification; assign from empty; mid insert then edits); all pass, perturb detects
  all. Open items for other owners, flagged by the review:
  - `intersect.curve_class_p6` is `partial` while 52 of its deferred cases are its own
    unported `MergeInterval` imprinting (no `UNPORTED(` marker, by design). Its porter
    should finish that remainder; those cases are not a back-edge.
  - The ~1,250-line body of `sm_RefineCCIToSurfaces` after `SmEdge::GetCurve` has never
    been compared against the C++ (the shim stops every refine call past the entry
    checks). Whoever wires `UNPORTED(topology.edge)` here ports and compares it.
  - `UNPORTED(topology.edge)` / `UNPORTED(topology.face_p2)` here name units that are now
    `partial`, against the letter of AGENTS.md 4a: the real blocker is cross-unit
    opaque-handle-to-`Brep` resolution (same as parts 1/3/7 and `intersect.point_class`),
    which needs its own tracked work item.
- **Merge of main (2026-10-18), new back-edges.** Main had moved on by about 460 commits
  (parts 8 and 9, `heal.heal_data_p3`, the merge-status rework of part 5). Changes:
  - `MergeIntervals` keeps main's partial-output contract (`merge_intervals_with_partial`)
    and now takes `k` and the caller's `ClassGeometry` (for part 6's
    `GetPointObjZoneTol3d`). `SmFace::SplitAtSeam` (`topology.face_p1`) gains a
    `geometry` argument; the healer and `SmTrimmingTools::SplitAtSeams` pass geometry that
    resolves nothing (their classification objects are loops and vertices, whose
    tolerances come from the topology). `split_at_seam`'s merge helper now propagates
    `InvalidInput` (an unresolvable handle) instead of reading it as an ignored status.
  - `heal.heal_data_p3`'s `HasDeadTopologyPointObject` calls the part-2 member, which now
    takes `Option<&Brep>` (it stops at `UNPORTED(topology.brep_p1)`
    `SmBrep::IsLiveTopologyMember` for any topology object, as before).
  - `SmSurface::CurveOnClassify` (`surface.surface_p2`) now runs part 8's
    `IntersectNaturalBounds` (part 8 is partial) and the end classifications; it takes the
    caller's `ClassGeometry`. It has no caller or difftest yet; a unit test covers a curve
    inside a plane. A curve crossing the plane's boundary returns `InvalidInput` from
    part 3's `find_interval` (a temporary boundary handle the test geometry does not
    resolve); not investigated further, it belongs to part 8's adapter.
  - `surface.surf_of_extrusion_p2`'s `IntersectWithPlane` stub is retagged
    `UNPORTED(intersect.curve_class_p8)` (see that unit's entry).
  - `query.face_p2`'s `CurveOnClassify` cases now complete in Rust (ReSet from this unit,
    part 9 from main), so its runner compares them: the classified line is
    `SmBSplineCurve::CreateLineSegment` on both sides (it was an analytic line over
    `[0, |PQ|]` while every case deferred), and the status, modified flag and each
    interval's range and three point classifications are compared. All pass.
