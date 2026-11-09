# boolean.merge_p2 (`SmMerge` constructor, `MergeEdge`, `MakeFaceuse`, `MergeAddedFace`, `MergeAddedTopology`; PR #229, 2026-10-20; partial, no disagreements)

- **Two data models.** `MergeEdge` and `MakeFaceuse` copy other-operand topology into
  the primary B-rep, like `merge_p1`'s `MergeVertex`, so they work on concrete arena
  `Brep`s with the shared `Relations` mates (which gain face mates and the edge/face
  branches of `Relate`). `MergeAddedTopology` drives the intersector's phases, which
  are generic over handles `O`, so it is generic too: the B-rep and `SmStitch` calls
  go through the `MergeBrep` trait (on top of `IntersectorBrepP2`), one method per
  upstream function. Callees ported on arena B-reps are required methods (the runner
  routes them on the intersector runner's `Two` adapter: the three `SmStitch` calls,
  `TestClosureSplitRegion`, `FindAndSetInfiniteRegion`, `SmBrep::Notify` and
  `IsManifoldSolid`; `GetFaces` comes from `IntersectorBrepP2`); the rest default to
  `Unported` (the `SweepAccess` style of #222). The runner's `Notify` deletes the
  adapter's B-rep cache on `PRE_EDIT`, as upstream, and notifies on an empty attribute
  store over the regions (as `heal.stitch_p1`'s).
- **`SmStitch`.** The merge's one `SmStitch` is `StitchSettings`; the runner builds a
  `heal::stitch::p1::Stitch` from it for each call. Equivalent: the default
  `SmStitchCallback` is stateless and the stitch carries only its settings.
- **Partial outputs of a failed callee (known difference).** The ported stitch and
  intersector calls return no out-lists or gaps with an error, so after an ignored
  failure the merge continues with the entry values (empty, `0`, `SM_BIG_DOUBLE`).
  Upstream keeps what was appended before the failure: `VertexFaceIntersect`
  (`SmTopologyIntersector.cpp:7029`), `SplitEdgesWithVertices` (`SmStitch.cpp:3777`)
  and the edge/face intersectors `SER` mid-loop when `bIgnoreProblems` is `FALSE`,
  after appending to `rProcessedVerts` and raising the gaps, and `MergeAddedTopology`
  appends whatever is there (`SmMerge.cpp:6045`-`6092`). `SmFeatureExecutive.cpp:2127`
  passes its `m_bIgnoreProblems`, so this is reachable. Fixing it needs those callees
  (units `heal.stitch_p2`, `boolean.topology_intersector_p1` / `_p2`) to hand back
  their partial outputs with the error; until then, a `MergeAddedTopology` case with
  `ignore_problems = false` and a failing phase may differ. Every current case defers
  before the phases (`SmFace::GetEdges`, `topology.face_p4`) or adds nothing.
- **Null processed vertices.** `sProcessedVerts` keeps the null entries the edge
  intersectors return (`EdgeFaceIntersect`'s `MakeVertexSplitEdge` failures);
  `FaceFaceIntersect`'s non-null list drops them, which is equivalent because
  `FindCommonElements` never matches a null against a face's vertices.
- **Curve and surface mates.** An arena edge owns its curve and a face its surface,
  so `GetBrepMate(pCurve)` / `GetBrepMate(pOldSurface)` (only reached for an unmated
  edge or face) are always null; the curve is always copied and the surface always
  `CopyAndAddAnalytics`-copied. The fixtures build every edge and face with its own
  curve and surface, as arena B-reps must.
- **Merge flags.** `m_bManifoldBoolean` / `m_bIntersectionLoopClosed` are
  `MergeState::manifold_boolean` / `intersection_loop_closed`: `Some(true)` from the
  B-rep constructor (compared), `None` from the context-only one, which leaves them
  uninitialised. `ManifoldBoolean` (`boolean.merge_p4`) reads the first.
- **Coverage.** The constructor, `MergeEdge`'s mated and null paths and `MakeFaceuse`'s
  mated-face fast path and `SM_ERR` paths compare values and both operand snapshots.
  `MakeFace` is wired (`topology.brep_p7`) and now stops at `SmFace::SetTolerance`
  (`topology.face_p3`, 114 of the 1000-seed deferrals); new wire edges stop at
  `MakeWireEdge` (`topology.brep_p3`). With `topology.brep_p2` (#149) wired, computed
  tolerances (`GetMinEdgeBBoxSize`), new shell vertices (`MakeShellVertexTopology`,
  through `topology.brep_p9`) and a `MergeAddedTopology` that adds nothing compare
  (45 cases: 30 pass, 15 deferred; `--seeds 1000`: 631 pass, 398 deferred). The
  other `MergeAddedTopology` / `MergeAddedFace` cases defer at `SmFace::GetEdges`
  (`topology.face_p4`) or later, while the C++ still runs them; the stitch and
  intersector phases are wired but not yet compared. When those land, rerun with a
  high `--seeds`.
- **Snapshot numbering.** The shim snapshots in `sc_merge_fixture.h`'s `native_graph`
  order (edge list, vertex list, then the walk from the regions), so a new vertex
  shell's vertex use is numbered first (shells are walked before face uses), while the
  Rust arena appends it. The shared `boolean.merge_p5` snapshot now numbers every
  record by the same walk (`Canon::of_native` in the `boolean.merge_p1` runner), which
  is the identity on imported fixtures (`merge_p5` unchanged, 140/108/32).
- **Fixture records.** A copy notification propagates `SmTopology` flags from source
  to copy, so the runner registers a fresh `Topology` record for every vertex, edge
  and face of both operands and every primary vertex use (the flags are not compared).
- **Guards.** Two inputs crash the reference build and are refused by the FFI
  wrapper: a computed tolerance with a null other B-rep (UB-167, which makes
  `SmApiStitch(pBrep, pFace)` crash on every call) and a vertex listed twice in a
  `MergeAddedTopology` list once stitching glues it (UB-176, now confirmed on the
  reference build through this route). The wrapper also refuses, as a deliberate
  over-refusal, any repeated face, edge or vertex and any `MergeAddedTopology` with
  an other B-rep. Non-finite tolerances and angles were probed on every fixture kind
  and run, so they are not refused.
- **Upstream defaults.** `MergeAddedTopology`'s header and definition disagree on the
  position of `bDoGluing` (WR-197); `MergeAddedOptions::default` is the effective
  `(TRUE, FALSE, TRUE, TRUE)`.
- **Back-edges.** Wired: `boolean.merge_p5`'s `MergeEdge` (both drivers; the shared
  `collect` stage merges each kept wire edge inside its classification loop, as
  upstream, through a `StageKernels` adapter: the primary kernel copies the curve and
  the source context is notified, so one shared kernel goes through `MergeContext`);
  `boolean.api_brep`'s two-B-rep booleans (they construct the merge through
  `merge_p3`'s `DependencyMerge`, call `IsManifoldSolid` (`topology.brep_p2`) and stop
  at `merge_p3`'s `ManifoldBoolean` stub, `boolean.merge_p4`, directly or through
  `NonManifoldBoolean`; the entry points take the kernel mutably);
  `heal.stitch_p1`'s `SmApiStitch(pBrep, pFace)` (the constructor fails on the null
  other B-rep where upstream crashes, UB-167); `topology.brep_p7`'s `CopyFaces` with the
  merge (`MakeFaceuse` per face with `DoingBoolean` set; `copy_faces` takes the kernel
  mutably; its notifications use `env`'s one identity space; unreached behind
  `SmFace::GetBrep`, `topology.face_p4`); `boolean.merge_p3`'s `DependencyMerge::construct`
  (builds #287's `MergeEngine` through `MergeEngine::from_merge`, which `ManifoldBoolean`
  runs `boolean.merge_p4` on) and its runner's `Pool` (the same engine; family 3
  constructs first, as the shim's `MergeAccess` does)
  (`construct`, and `process_wires` with empty mates and marks, which is the pool's
  actual state: its intersector names only B-reps and surfaces); `boolean.merge_p4`'s
  `ManifoldIntersector::make_faceuse` (now required; the kernel adapter calls
  `make_faceuse_with` with its mates and records). `SmFace::CalculateBoundingBox` calls
  `query.face_p1` with `bTight = FALSE`. Difftest re-runs after merging main
  `7b57d8de` (with #287's engine): `boolean.merge_p3` 387/222/165 (of its 87
  `boolean.merge_p2` deferrals 7 pass and 80 stop at `query.brep` inside
  `ManifoldBoolean`), `boolean.api_brep` 287/151/136 (its 114 now at `query.brep`),
  `topology.brep_p7` 316/237/79 (its 3 now at `topology.face_p4`); `boolean.merge_p4`
  311/30/278, `heal.stitch_p1` 300/232/68, `boolean.merge_p5` 140/108/32,
  `boolean.merge_p1` 248/206/42, `boolean.topology_intersector_p1` 830/713/117 and
  `topology.brep_p9` 684/638/46 unchanged from main.
- **Integration gap (PR #204, topology.brep_p3): `SmBrep::MakeWireEdge` stays a stub.**
  `merge_edge_with`'s `make_wire_edge` keeps `UNPORTED(topology.brep_p3)` although
  `topology::brep::p3::make_wire_edge` is ported. The callee needs `&mut Kernel`
  (`topology::brep::p2::make_edge_topology` transfers shells), and #229's
  `MergeContext::Split` carries only `&Kernel` plus an optional `Context`. It also
  needs a `NotifyEnv` naming the new edge, edge uses and vertex uses, which has to be
  built from `TopologyCopy`'s destination keys. Wiring it means changing
  `MergeContext::Split` to `&mut Kernel` for every `merge_edge` caller, so it is a
  small follow-up after #204 lands. `OrientTrimmedSurfaces` is wired in #204: it is a
  required `MergeBrep` method, implemented by the runner's `Two` with
  `topology::brep::p3::orient_trimmed_surfaces` under the kernel's identities.
