# boolean.topology_intersector_p2

PR #203 (on main's #146). Part 2 of `SmTopologyIntersector.cpp`: `RegisterExistingSSI`,
`CheckForSqueezeVertices`, `DeleteFaces`, `FixGaps`, `RemoveRelationship`,
`IntersectInsertRelate`, `IntersectTopology`, `SetRelationships`, `FixupClassMates`,
`FixupSplitVertexPair`, `EdgeEdgeIntersect`, `EdgeFaceCoincidence` in
`boolean::topology_intersector::p2`.

- **Harness.** Part 2 runs on part 1's intersector and two-B-rep script: the p2 shim
  includes p1's (`SC_TI_EXTRA_OPS` hook, ops 41-51), the p2 FFI binding extends p1's
  validation (`ExtraOps`), and part 1's runner interprets part 2's ops (dispatch to
  `run_part2_op`). Part 1's `RegisterExistingSSI` op (16) now runs the port on the Rust
  side instead of filling the table directly; part 1's difftest is unchanged (827 cases,
  653 pass, 0 fail, 174 deferred). `SmTsectCurveType` inputs are bounded to the enum
  (0-6) in the FFI and an unknown code is a harness error (it mapped to `Unknown` before).
- **Coverage (what the passes complete).** Per op occurrence in non-deferred cases:
  `RemoveRelationship` 29 success / 35 `SM_ERR`; `CheckForSqueezeVertices` 75 success
  without a conflicting mate (empty lists) / 26 `SM_ERR` (existing relationship better),
  every conflict that combines defers inside `CombineCoincidentVertices` (wired to
  `topology.brep_p5` through `IntersectorBrepP2::brep_combine_coincident_vertices`);
  `DeleteFaces` 68 success, all with an empty face list (a face reaches `SmFace::GetEdges`,
  `topology.face_p4`); `FixGaps` 119 success without a gap pair / 14
  `SM_ERR_NULL_POINTER` (gap pair with an unmated vertex), a pair that combines defers
  as in `CheckForSqueezeVertices`; `SetRelationships` 39 success on
  managers without definitions; `FixupClassMates` 56 success / 17 `SM_ERR` (sizes differ);
  `FixupSplitVertexPair` 57 success, 6 removing the interval; `EdgeEdgeIntersect` 136
  success, none with a non-empty first split list (the 9 that reached
  `MakeVertexSplitEdge` compare since main's `topology.brep_p9`);
  `EdgeFaceCoincidence` 128 success without an imprint (`Point3DClassify`,
  `query.face_p1`); `IntersectTopology` 1 success (the smTopoTest pair
  `RegisterExistingSSI` marks as not intersecting) and `IntersectInsertRelate` none: with
  `SmBrep::GetFaces` wired to `topology.brep_p1` (`IntersectorBrepP2::brep_faces`), the
  former 124 `GetFaces` deferrals stop at `SmFace::GetTolerance` (70, `topology.face_p2`),
  `SmBrep::CalculateBoundingBox` (41, `query.brep`) and `SmFace::FaceIntersect` (12,
  `topology.face_p3`); 1 passes. With `CombineCoincidentVertices` wired to
  `topology.brep_p5` (follow-up to #203, after main@36cfad06), its 15 deferrals stop
  inside it: `SmBrep::SqueezeEdge` 8 (`topology.brep_p3`), `SmBrep::Notify` 4
  (`topology.brep_p2`), `SmBrep::m_bEditingEnabled` 3 (`topology.brep_p1`); none
  passes yet, so the shim's `squeeze_breps_ok` assumption (a first block that combines
  reaches the second) is still unexercised: re-check it when those land. Main's
  `topology.face_p3` (#139) moved the 3 `CreateUVTrimCurves` deferrals on to
  `SmFace::GetUpwardLoopuses` (`topology.face_p4`). The census is the ignored test
  `units::boolean_topology_intersector_p2::tests::census`. Main's topology.shell fixture
  changes (the shell-state export and import, `sc_topology_fixture::append_shell_state` /
  `topology_vertexuse::read_shell_state`, and the GetEdges-first walk) changed 10
  generated cases at the merge of main@86f2d3bd: the walk renumbers the handles, so the
  same seeds select other entities and the C++ reference output itself changes, not only
  the Rust side. Net -2 passes: 4 pass -> defer (2 at `MakeVertexSplitEdge`, 2 at
  `CombineCoincidentVertices`) and 2 defer -> pass; the other 4 still pass with a
  different value count (more in most, gen0767 172 -> 169). 0 fail before and after.
- **Guards.** The shim refuses (`-2`) what upstream dereferences: vertex members of a mate
  that is not a vertex, edges of a vertex without curve (`FixGaps`,
  `EdgeFaceCoincidence`), faces without surface, an interval index past a
  classification, a vertex classification without a vertex; a null B-rep only where a
  combination would run (the shim replays the scan up to the first candidate). The
  `FixGaps` / `EdgeFaceCoincidence` edge-curve checks are per vertex, slightly stricter
  than upstream (only common edges are measured); no fixture B-rep has such edges.
  `CheckForSqueezeVertices` checks the other vertex's B-rep mate only where the second
  block dereferences it (the first block's `SM_ERR` returns before). `SetRelationships`
  without the maps (constructor 1) is refused only for a manager with a definition whose
  two final topologies are set, the only ones that reach `Relate`'s map insert.
  `FixupSplitVertexPair` checks the index against both classifications only: upstream
  bounds its `index + 1` writes by the B-rep classification's size, so with a shorter
  other one the write lands in `SmTArray`'s spare slots (at least 16; `MAX_INTERVALS` is
  16) and the `RemoveAt` hides it. The port skips that write (case `split_oob`).
  `IntersectInsertRelate` (op 47) needs a nonzero approximation tolerance (HANG-25: `0`
  and `-0` gave no C++ result within 180 s; negative and tiny positive ones return) and
  B-rep blocks below 1e154 (a control point at 1e300 gave no result within 20 s, HANG-16's
  class). The port defers op 47 at `GetFaces` in every case, so the guard removes no
  compared case.
- **Memory (valgrind).** The 4a6bcc86 claim "valgrind 75 cases clean" was wrong: that
  sample had no op-47 program that builds proto edges. The pre-review ran 10 programs
  per op family, C++ and Rust. Three C++ op-47 runs (one a lone `IntersectInsertRelate`)
  read freed memory in `~SmProtoTopologyManager`. The cause is upstream's own teardown
  order (UB-175 [PSIRT]), not the shim's. The shim now runs a verbatim copy of
  `IntersectInsertRelate` that unlinks the stale proto-edge entries before the delete
  and answers `-2`. The port mirrors that: `intersect_insert_relate` returns
  `InvalidInput` after the teardown. The shim's own manager teardown at the end of a
  script unlinks them too (that `delete` is ours, lesson 23), so the manager ops of both
  parts stay defined. Re-run after the fix: see the PR body.
- **`CheckForSqueezeVertices`** appends to the caller's lists
  (`check_for_squeeze_vertices_into`, from oursland's #211): the first conflict block
  appends, the second can still return `SM_ERR`, and part 3's `MergeCurveClasses` keeps
  going after a failure.
- **Part 1 fuzz note (not changed here).** `difftest --unit boolean.topology_intersector_p1
  --fuzz 2000` reports FUZZ-PASS with 3 `differs` notes, all `sm_SetEdgeClass` with a
  mutated tolerance of -1e300 (C++ classifies an end to `SM_PC_VERTEX`, the port to
  `SM_PC_EDGE`): part 1's code, untouched by this PR.
- **CloseIntersectionLoop co-port.** `SmTopologyIntersector::CloseIntersectionLoop`
  (`SmTopologyIntersector.cpp:6108-6354`, called from `SmMerge.cpp:2958`) and its only
  callee `IIREdgeBrep` (`:2836-2971`) are listed by no unit in `units.json`; per the
  overseer they are co-ported into this unit (`close_intersection_loop`,
  `iir_edge_brep`). `IIREdgeBrep`'s `MergeCurveClasses` stays
  `UNPORTED(boolean.topology_intersector_p3)` (draft #211 ports it). Script op 52 runs it;
  it is not drawn by the generator (16 edge cases on a planar face and its copy plus 48
  seeded scripts, `close_loop_*`, so the 841 earlier cases are unchanged); on success it
  reports the other mate of each edge of the vertex (`GetMappedBrepObjects` cannot
  follow an op that may change topology). 58 compare: 11
  `SM_ERR_NULL_POINTER` early returns, 28 successes without new topology (every edge
  mated, or the far vertices unmated), 19 that relate the edge pair whose midpoint drops
  onto the mated vertices' only common edge; 6 reach the coincidence test and defer in
  `BrepCurveSolve` (`SmBrep::CalculateBoundingBox`, `query.brep`). **Uncovered:** no
  passing case reaches `BrepCurveSolve`, so the coincidence branch, `IIREdgeBrep` and the
  second loop past its skips have zero compared cases, and no pass makes new topology;
  every pass ends before the first `BrepCurveSolve`. The 19 drop-path passes compare a
  real `Relate` (removing it fails exactly those 19). The drop's tolerance
  (`2 * dMaxDist`) is not observable here: under `SM_SO_MINIMIZE` it does not gate
  success (the intersection tolerance is `SM_BIG_DOUBLE`) and no snap vector is passed,
  so it reaches only the global point solve's snapping, and `bSuccess` (the drop's only
  use) does not change; a 0.5x mutant survives for that reason, and a near-tolerance
  case would not tell them apart. The shim refuses a
  vertex whose edges lack a curve or vertices (or, for the B-rep vertex, a primary edge
  use) and the FFI requires both B-reps.
- **Time-budget guard on `IntersectInsertRelate` (op 47): |approx| <= 0.1.** A defined
  input, not undefined behaviour and not a hang: with a large approximation tolerance
  the full upstream intersection finishes, but slowly (fuzz drew `approx = -1e300` on
  `Mouse.smb` x `vase_rev.smb`: 39 s, over its 20 s limit). C++-side wall time of the
  op-47 fixtures (the port defers op 47), both signs:

  | fixture (generated case) | 1 | 10 | 1e3 | 1e6 |
  |---|---|---|---|---|
  | `Mouse.smb` x `vase_rev.smb` (gen0314) | 0.5 s | 5.1 / 9.8 s | 28.8 / 27.0 s | 20.1 / 20.5 s |
  | `torus.smb` x `ellips_ts2.smb` (gen0111) | 0.4 s | 1.2 / 1.3 s | 1.8 / 1.9 s | 1.8 s |
  | `box_with_large_fillet.smb` x `torus.smb` (iir) | 0.2 s | 0.4 s | 0.2 / 0.9 s | 0.2 / 0.9 s |
  | `surf_extrude.smb` x `box_with_large_fillet.smb` (gen0101) | 0.1 s | 0.2 / 0.1 s | 0.1 s | 0.1 s |
  | gen0070, gen0105, gen0143 | < 0.1 s | < 0.1 s | < 0.1 s | < 0.1 s |

  |1| is the largest measured magnitude under 2 s on all of them; the guard sits 10x
  below it. Generated tolerances are at most 1e-2, so no earlier case changes. No HANG
  id: every run terminated.
- Upstream issues: UB-184 (`RegisterExistingSSI` failure leaves the table's lists out of
  step), UB-175 [PSIRT] (`~SmProtoTopologyManager` use-after-free), HANG-25 (zero
  tolerance), WR-170 (copies of registered curves and a failed manager leak).
