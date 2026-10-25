# boolean.topology_intersector_p3 (`SmTopologyIntersector.cpp` part 3: `MergeCurveClasses`, `MergeCurveOnSurfaces`, `MergeCoincidentSurfaces`; PR #211, 2026-10-17; partial, no disagreements)

- **Dependencies.** Parts 1 and 2 (#146, #203) and `intersect.curve_class_p2` / `_p4`
  (#85, #137) are on `main`. The constructor `MergeCurveOnSurfaces` needs is part 4's
  `with_curve`, whose `ReSet` is part 2's. `main` still has part 4's stubs for `ReSet`,
  `ConnectIntervals` and `GetMinUV` / `GetMaxUV` (the two PRs merged separately); this
  branch wires them to part 2.
- **`query.face_p1` wired (2026-10-20).** `SmFace::Point3DClassify` and
  `CalculateBoundingBox` are `IntersectorBrepP3` hooks; the difftest adapter calls
  `query::face::p1`, mapping the classification's handles both ways with
  `PointClassification::map_objects`. Upstream passes the `double` `dCurveTolerance` where
  an `SmZoneTol3d` is expected (the classify call and the `SmPointClassification`
  constructor); the port applies `zone_tol3d` at both, as that implicit conversion does.
- **`curve_class_p7` cast fixed (2026-10-20).** With the edge queries wired, the fuzz run
  reached `TestForPartialCoincidentEdges` with `SmLine` edges (the shared import keeps
  lines). p7 accepted only a plain B-spline; upstream's `SM_CAST_PTR(SmBSplineCurve, ..)`
  succeeds for every kind derived from it, and `new SmBSplineCurve(*p)` copies its B-spline
  base. Three fuzz inputs gave `SM_ERR` in C++ and a null-dereference error in Rust; with
  the fix, 2000 fuzz cases give 0 differs.
- **Wired stubs of partial units.** `sm_MakeConsistentClassifications` (`curve_class_p1`)
  was a stub on `SmEdge::GetVertex` (`topology.edge`, already partial). It is now ported in
  full, reaching the edges and vertices through `ClassGeometry::topology_entity` and the new
  `ClassGeometry::topology_handle_in`. `curve_class_p7`'s `SmEdge::GetCurve` /
  `GetInterval` and the edge-pair tolerance are wired the same way where classification
  geometry is in scope. An adapter that cannot resolve a handle (no hooks, or a stand-in edge
  without a B-rep, as in the `curve_class_p7` runner) keeps the previous `Unported` deferral,
  so those runners' outcomes do not change.
- **Guard exposed by the wiring.** With `ConnectIntervals` real, a squeeze can empty a
  classification. `GetLoopCrossings` then reads `GetAt(0)` unchecked (undefined upstream).
  The `curve_class_p4` shim now refuses that (the first loop's face has a surface, there are
  loops and a curve, and the classification is empty), also for a degenerate curve, which
  would return first.
- **`CheckForSqueezeVertices` keeps partial appends.** Upstream can append in its first
  conflict block and then return `SM_ERR` from the second; `MergeCurveClasses` continues with
  those lists. Part 2 gained `check_for_squeeze_vertices_into`, which appends to the
  caller's lists.
- **Still deferred:**
  - `SmFace::GetEdges` / `GetUpwardLoopuses` (`topology.face_p4`), reached by
    `MergeCoincidentSurfaces` once the face boxes overlap;
  - the imprinting of `MergeInterval` (`curve_class_p6`);
  - `topology.brep_p5`'s own stubs (`m_bEditingEnabled`, `topology.brep_p1`; `SqueezeEdge`,
    `topology.brep_p3`), reached through `CombineCoincidentVertices` when squeezing;
  - `SmBrep::DeleteEdge` (`topology.brep_p4`) when repairing an edge relation;
  - the remaining `topology.edge` / `topology.vertex` stubs of `curve_class_p7`, which are
    in functions without classification geometry.
- **Review r1 (2026-10-21).**
  - *Guard:* an `SM_PC_UNKNOWN` start or end of a face-mid interval stays unknown, without
    object, when the point does not project onto that face (`sm_ComputeFacePointClass`);
    a region mid copies a null object (this harness names no region). `MergeInterval`
    then dereferences the null vertex (`SmCurveClass.cpp:17659` / `:17672`). The shim
    and the runner refuse that input; the generator only made edge mids, so the fuzz
    never reached it.
  - *UV curves:* `MergeCurveOnSurfaces` was compared only without UV curves. Handles of
    kind 13 name an edge use's UV trim curve (`GetUVTrimCurve`, own or mate's), and three
    inline cases pass the curve's own edge uses. They defer at `intersect.point_class`'s
    `SmVertex::GetTolerance` stub in `MergeIntoObject`, past the UV classification.
  - *Owner:* the 3D edge-curve copy in `MergeCoincidentSurfaces` gets the edge curve's
    owner as upstream (`SetOwner`, also after the trim on the other side); the
    `SM_NO_CHANGE_OWNER` notification of the temporary copy is not delivered (noted in the
    module's Deviations).
- **Back-edges.** `blend.fillet_geom_p4`'s `MergeCurveOnSurfaces` stub is retagged
  `blend.fillet_executive_p1`: it runs on the executive's intersector, which that unit
  provides (as its `RegisterExistingSSI` stub already is). `boolean.merge_p1`'s
  `MergeCoincidentSurfaces` stays `UNPORTED(boolean.topology_intersector_p3)`: wiring it
  needs an `IntersectorBrepP3` for `boolean.merge_p4`'s `ArenaPair` and the kernels and
  notification context in `fix_face_coincidence`; left for a follow-up PR.
