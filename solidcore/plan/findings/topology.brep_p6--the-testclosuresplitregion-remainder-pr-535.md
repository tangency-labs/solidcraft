# topology.brep_p6 — the TestClosureSplitRegion remainder (PR #535)

`SmBrep::TestClosureSplitRegion(SmFaceuse*)` is ported past `CalculateBoundingBox`
(SmBrep.cpp:27966-28583): the infinite-region test (`FindAndSetInfiniteRegion` with the
new shell, `SM_VS_INFINITEREGION_SWAP`, the cached box), the inner-shell choice, the
ray-fire distribution of face uses joined only by a vertex (the 100-try step-off loop),
wires and the old region's other shells (`OrderShells` after a move), and the deletion
of emptied shells and regions. No disagreement with the C++; upstream details kept:

- The face-use loop's inner re-mark loop reuses the outer index `ii`, so after each
  move the scan resumes at `sReplacedFaceusesAll.GetSize() + 1`.
- `IsInnerShell`'s status is ignored and a failure leaves `UNSURE`, which is truthy:
  the new shell is then taken as the inner one.
- The wire loop uses the zone tolerance of the first wire for every wire.

**Ray fires and the caller's names.** The ray fires (here and in `topology.brep_p10`'s
final `FindAndSetInfiniteRegion`) used `BrepQuery::default()`, whose cache keys are the
arena identities. `MakeTopologyFromData` names geometry owners with the caller's
`NotifyEnv::key_of`, so the solve matched no face or edge and every ray "hit nothing",
returning the old infinite region. On `spine_doubled` with infinite-region testing the
C++ picks the new region instead. Both now build the query from the caller's `key_of`
(`brep::p2::caller_cache_key`, shared with p2's cache rebuild), and the brep_p10 runner's
closure op names objects as its build does (a `NotifyEnv` over the build's records with
the runner's `key_of`, not a `RegionNotifyState` keyed by `notify_ref`). Those cases now
reach `SmTol::GetZoneTol3d(const SmObject*)` (geom.tol) and defer. Other
`BrepQuery::default()` callers (hcr, stitch p2, heal_data p1, merge p4/p5) are unchanged;
they are right only when the B-rep's geometry owners carry arena identities.

**Harness gap (not fixed).** The brep_p10 C++ export walks regions in `GetRegions` (list)
order, the Rust walk in arena order; they diverge once a region-list head rotates
(`ResetInfiniteRegion` sets `m_pList = pNewInfRegion`).

Moves vs main (after #536): of the 110 cases that stopped at the OMITTED stop, 12 pass
(brep_p6 8, brep_p10 4); 79 stop at `SmSurface::DropCurve` (the closed primitives:
`FindInfiniteRegion` -> `SmEdge::IsSeam` -> `NormalizedEvaluate` builds a UV trim curve),
16 at geom.tol `GetZoneTol3d(const SmObject*)` and 3 at `topology.face_p2`. No case
fails and no former pass changed.

**Coverage follow-up (port/brep-p6-closure-coverage).** brep_p6 op 0 now exports the
whole post-split graph on both sides: regions with their infinite and void flags and
shells, shells, face uses and the rest, via the op-3 `post::export_graph` and
`export_graph`. Previously only face-use shells and counts were compared. All op-0
cases still pass. Eight `MakeFaceTopology` network fixtures target the remainder's
ray-fire branches: `two_cubes_at_a_vertex` (face uses joined only by a vertex),
`two_disjoint_cubes` and `cube_in_cube` (shell nesting), `cube_with_wire` and
`cube_with_inner_wire` (wires), `cube_with_inner_vertex` and `cube_with_outer_vertex`
(vertex shells), and `cube_faces_6_reversed`. All eight defer at geom.tol
`GetZoneTol3d(const SmObject*)` in the topology solver (`object_zone_tol_3d` on an
opaque handle). Wiring its null-object branch to the system tolerance only moves the
stop to `SmFace::PointClassify` (`query.face_p2`, `face_point_classify`), so those
branches stay uncompared until both opaque-handle stubs get the owning B-rep.
