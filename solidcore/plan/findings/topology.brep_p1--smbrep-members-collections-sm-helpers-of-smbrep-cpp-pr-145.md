# topology.brep_p1 (`SmBrep` members, collections, `sm_*` helpers of `SmBrep.cpp`, PR #145, 2026-10-16; partial, no disagreements)

Difftest (`fixtures/topology.brep_p1/`: 2335 cases on the 15 upstream `.smb` files read by
`SmBrep::ReadFromFile`, 238 on constructed B-reps (wire edges, plane and B-spline faces, the
`prog_test` wire network, 10 `sm_FindSideOfCurve` cases among them), 16 generated): 2589 cases,
2110 pass, of which 2065 compare values and 45 are guard agreements, not coverage (the C++ is not run: the shim stops before an upstream null dereference; 30 are the port's matching refusal, 15 a runner-side mirror with no port call), 15 rejected (no entity of the op's kind; nothing runs), 0 failed, 464 deferred (`harness:analytic-geometry-export` 217,
`topology.face_p2` 113, `query.brep` 71, `topology.face_p3` 21, `topology.face_p4` 16,
`intersect.surface_drop_lib_p3` 10, `query.face_p2` 9, `curve.crv_on_surf` 4,
`surface.surface_cache_p1` 3);
`--perturb` detects 2110/2110; `--fuzz 2000` FUZZ-PASS; a 1000-seed generated sweep (3573 cases)
passes 2849 with 0 failed, 709 deferred and 15 rejected (2773 compare values; the other 76 are guard
agreements of the same kinds: 53 port refusals, i.e. 43 UB-162 redistribute, 7 `CreateEdgeEdgeBlend` with a blend
edge use without face, 3 swaps without an old infinite region, and 23 runner-side `AddAttributes` mirrors); green runs 206 units with 0 failed (main merged
through 89dc103a).
`sm_SwapFaceUV` calls `topology.face_p1`'s `SmFace::SwapUV` (#148, wired after the merge; status ignored as upstream's release `SE`, `Unported` still propagates): on an unmarked face the shim now runs the C++ swap and both sides compare the swapped B-rep digest (the heal digest: surfaces, trim-curve fingerprints, loop-use links), but every such case defers at `SmFace::GetBrep` (`topology.face_p4`), the first call of face_p1's `swap_uv`, so none compares yet (the 16 former `topology.face_p1` deferrals are now `topology.face_p4` 16; no deferral flipped to pass).
Every compared value is bit-identical; no absolute floors. Of the 2110 passes, 2065 compare
values (5 of them, `CreateCornerBlend` on three edge uses, compare only the C++ failure status
and an empty surface list). The other 45 are guard agreements, not coverage: both sides report the null-dereference
guard (`both_errored` in the report) and no C++ runs, because the shim stops before an upstream null
dereference. 15 are `AddAttributes` on a null indexed target, where the runner itself writes the
Rust side's `-2` before any port call (a runner-side mirror, no port code exercised). 27 are
`sm_RedistributeSeamUVTrimCurves` with a trim curve on files with fewer than four seam edge uses
(UB-162) and 3 are `SetInfiniteRegion` swaps without an old infinite region: those 30 exercise only
the port's matching refusal. The 15 ops naming an entity kind the wire B-reps lack
(`ReflectShell`, `sm_MarkLoopsToDelete`, `sm_SwapFaceUV`, `FindSectorOfFace` local and global) are
rejected: nothing runs on either side.

- **Coverage, honestly.** `SmLockBrepBox` has no passing case (it defers at `query.brep`).
  Since topology.shell (#154) is wired, `GetShells` (21 cases), `ResetInfiniteRegion` (all three
  void settings) and `SetRegionIsVoidFlagsForNestedSolids` compare the region list, the infinite
  region and every region's raw void flag.
  `HealBrep` runs the C++ healer and the Rust `HealData` (`heal.heal_data_p3`, #164).
  `SmBrep::HealBrep` always returns `SM_SUCCESS`, so a status-only comparison compares nothing: both sides now emit the same digest of the healed B-rep after the heal (the shim's and the runner's `put_healed`: the `Export` extras, then every record's links, orientations, geometry ids and types, domains, tolerances and gap-init flags, in C++ list order and Rust arena order, which agree because the steps that complete reorder no list; a byte-identical re-export was not possible because the Rust import discards the region and shell lists). The sequence reaches `SmEdgeuse::NormalizedEvaluate` from `SM_HO_CACHE_EDGEPROPS` on, so only `SM_HO_NONE` and `SM_HO_FIX_BACKPOINTERS` complete: the 5 `*_heal_backpointers` fixtures on the B-spline-only files (Endcap, disk, ellips_ts2, plane_circ, plane_square) compare the healed B-rep (2785 values), the other 10 defer on the analytic export, the 15 `SM_HO_ALL` fixtures defer at `topology.edgeuse_p3`, and in the 1000-seed sweep 3 of the 33 generated heal cases complete and compare (718 values; the generator draws `heal` for 1% of cases). No heal case passes on status alone. The digest found one disagreement: after the heal the C++ region list head was the infinite region and the Rust's was `None`, because the runner's import checked the exported head flag but never set `Brep::region_list_head`; the import now sets it (no library change: `regions()` already started at list index 0, which is the infinite region).
  A harness defect surfaced earlier when #164's sequence replaced the `HealTgts` stub: the 15
  `HealBrep` fixtures passed healer op 39, outside `SmHealerOpType` (`SM_HO_ALL` is 26); the
  C++ ran every step for it while the Rust runner mapped the unknown code to `SM_HO_NONE` and
  ran nothing, so the 15 cases briefly "passed" on an always-equal status. The fixtures now
  pass `SM_HO_ALL`, the FFI rejects codes above 26, the generator draws 0..=26 and the runner
  refuses an unknown code instead of defaulting. The passes of `CreateCornerBlend` (36, fewer than two edge uses),
  `sm_SwapFaceUV` (66, an already marked face), `sm_RedistributeSeamUVTrimCurves` (57, no trim
  curve or not a seam) and `IsDegenerate` (39, the vertex quick check) are early returns; 28 of
  the 51 `sm_MakeVertexSplitUVTrimCurve` passes have no trim curve; `sm_MarkLoopsToDelete`
  never descends successfully on the fixtures (level 0 returns the loop, level 1 nothing).
  `GetSurfaces` / `GetCurves` compare the owners of the listed geometry, not the geometry. The
  shim decides the deferral itself (`-3`, the C++ is not run) for `BrepIntersect` with
  faces: that deferral is asserted from the stubbed Rust path, not observed from a C++ run
  (`CreateCornerBlend` with two or more edge uses and `CreateEdgeEdgeBlend` blends run the C++
  since the edge-use wiring below). A `-2` guard sentinel (the shim stopping before a known
  null dereference) is reported as the error `null_dereference_guard` on both sides, so those
  cases count as `both_errored`, not as value comparisons. `GetFaces` / `GetEdges` /
  `GetVertices` compare the native list order the runner *imports* (the extras of this unit's
  export), so they test that the Rust keeps the imported order; no case lists after a mutation
  or on a B-rep Rust code built itself.
- **Harness deferral label.** Cases whose geometry the shared export of `topology.edgeuse_p1`
  flattens to B-spline parts (cone, sphere, torus, extrusion and revolution faces; ellipse
  edges) are deferred as `harness:analytic-geometry-export` with the native type in the symbol:
  a harness limitation, not an unported unit (the earlier labels named green or partial surface
  units and `topology.brep_p7`). `site/build.py` renders every `deferred_by_unit` key as a unit
  link, so the `harness:` keys show as dead links with a `?` badge until it special-cases the
  prefix (follow-up). The ops concerned: `GetTopologicalEdges`,
  `GetTopologicalVertices`, `sm_CheckUVTrimCurve`, `sm_FindSectorOfFace`,
  `sm_MakeVertexSplitUVTrimCurve`, `CreateParallelProjectionCurves`. `CreateEdgeEdgeBlend`
  ruled surfaces are compared on every file: upstream also uses only the `SmBSplineCurve` part
  of the rails.
- **Upstream defect UB-98** (already in `plan/upstream-issues.md`, met again here):
  `SmCopyBrepMap::SetAt` with a null copy and `GetAt` of a key never stored both dereference
  null through the release `SM_ASSERT` (`if(a) {}` still evaluates `pOrigObj->GetType()`);
  `sm_SetObjectMap` documents a null copy as allowed. Reproduced with gdb (SIGSEGV in `GetAt`,
  then in `SetAt`). The Rust copy maps are `BTreeMap`s (no entry for a null copy, `None` on a
  miss); the shim only exercises the defined inputs.
- **Upstream defect UB-162** (new): `sm_RedistributeSeamUVTrimCurves` reads `sNewEdgeuses[3]`
  of a 16-slot stack array after a release no-op `SM_ASSERT(GetSize() == 4)`. Not reproduced;
  the shim refuses seam edges with fewer than four edge uses (`-2`), as the Rust port does
  (`InvalidInput`); with four or more it runs the C++ and the Rust defers at
  `GetOrCreateUVTrimCurve` (`topology.edgeuse_p3`).
- **`ResetInfiniteRegion(region, SM_VS_INFINITEREGION_SWAP)` with no old infinite region**
  dereferences null upstream (`SmBrep.cpp:8630`; found by the pre-review's fuzzing: SIGSEGV on
  every file with `SetInfiniteRegion(NULL)` first). The shim stops before the call and the
  Rust returns `InvalidInput` after setting the new region and list head (both `-2`); six
  fixture cases (`*_set_inf_swap_{null,keep}_guard`) cover it. It is not an FFI rejection, so
  the input stays comparable; the guard sentinel counts as `both_errored` (above).
- **List order.** `GetFaces` / `GetEdges` / `GetVertices` return the B-rep's `m_p*ListHead`
  list order (appended on `PostInsert`, every insert in SMLib), which differs from the traversal
  order the shared graph export numbers entities in (Mouse.smb: the first listed face is
  traversal index 2). `Brep` keeps the three lists explicitly (`face_list`, `edge_list`,
  `vertex_list`; `add_*_record` appends, `set_*_list` reorders; a future record deletion must
  unlink) and the runner imports the native order from the shim's extras. Likewise the region
  list head (`Brep::region_list_head`, moved by `ResetInfiniteRegion`).
- **Geometry owners.** `GetSurfaces` / `GetCurves` include only geometry whose `GetOwner()` is
  its face / edge and filter on the geometry's own attributes (`pSurface->FindAttribute`); the
  port compares the owner with the entity's `brep_cache::arena_identity` and takes a
  `GeometryAttributeFilter` keyed on the geometry. The runner therefore sets the owners at
  import (as `query.api_queries` does). Side effect, truthful: the surface units' stubs for
  `SmFace::GetTolerance` (`surface/surface/p1.rs`, `UNPORTED(topology.face_p2)`) are now
  reached for face-owned surfaces, so 44 cases of `GetTopologicalEdges` / `GetTopologicalVertices`,
  `CreateParallelProjectionCurves` and `sm_FindSectorOfFace` defer there; before, the Rust
  took the no-owner branch (system tolerance) and matched only because the files' face
  tolerances equal it. Those stubs are main's and labelled `topology.face_p2` there; the real
  blocker is the surface's opaque owner handle (the same gap as the self-tagged
  `GetZoneTol3d` sites), so they will not clear when `face_p2` goes green.
- **Point test switch.** Without a UV guess `sm_FindSectorOfFace` turns the surface cache's
  `m_bPointTestEnabled` off around `GlobalPointSolve`; the Rust cache has no such switch, so a
  face-owned surface defers on `SmFace::PointClassify` (`query.face_p2`, 9 cases) where upstream
  keeps every solution. Noted at the stub in `surface/surface/p4.rs` for the face_p2 porter.
- **Wire networks.** The shared export walk (`sc_edgeuse_graph.h`) registers only the head of a
  wire shell's list and never walks vertex -> other vertex uses, so the `prog_test` wire network
  exported partially (2 of 6 edges). This unit's shim adds a fixpoint pass (`walk_more`) that
  registers the remaining shell members and vertex uses before exporting; earlier indices are
  unchanged, so the shared units are unaffected.
- **Upward face use.** The shared import leaves `Face::faceuse` (`m_pFU`) unset; this runner
  sets it to the face's `SAME`-oriented face use (needed by `GetLoops`, `sm_MarkLoopsToDelete`,
  `sm_FindSectorOfFace`).
- **`sm_MakeVertexSplitUVTrimCurve` on a concrete face surface.** The point lift uses
  the face `Surface` itself, and the tessellation branch now constructs its local-solve
  `CrvOnSurf` over that same concrete kind. Exact snapshots retain shared UV/surface
  geometry rather than erasing a plane into a B-spline base. The separate
  `CreateCornerBlend` path still requires B-spline face parts and NURBS 3D curves;
  those caller-specific requirements are not a missing `CrvOnSurf` type.
- **Bounds.** `sm_MarkLoopsToDelete` is bounded in depth (`MAX_LOOP_NESTING` = 256) and in
  total calls (`MAX_LOOP_MARK_CALLS` = 2^20): the recursion branches at every other loop of a
  face, so a cyclic radial chain through faces with three or more loops would otherwise grow
  without bound (upstream hangs there).
- **Status conventions of the runner.** `-2` marks inputs upstream would dereference null on
  (the shim stops before the call; the Rust port returns `InvalidInput` from
  `"topology::brep::p1"`); an `InvalidInput` raised by a callee is reported as
  `SM_ERR_INVALID_INPUT` (1007), as upstream's callee status; `-3` marks a call the shim does
  not make because the Rust side defers (listed above). A null vote, a null `AddAttributes`
  target with no index and a vertex-split near the trim curve ends are valid upstream and are
  compared (not guarded).
- **Null votes.** `sm_VoteForRegion` is called with `pRegionFound` straight from
  `sm_FindSectorOfFace` (`SmBrep.cpp:11864`), which can be `NULL`; upstream counts the null
  region as a region of its own and `sm_FindHighestVote` can return it. The port takes
  `Option<RegionId>` votes (a null winner and no regions both read `None`, as upstream's
  `NULL`); the pre-review's fuzzing found the earlier runner rejecting null votes (39 cases),
  now fixtures (`*_vote_null_*`) and generated votes include them.
- **Back-edges wired:** `SmBrep::GetInfiniteRegion` in `topology.edge` (5 cases continue to
  `topology.brep_p2`), `GetVertices/GetEdges/GetFaces` in `topology.brep_cache` (unreachable
  until `query.brep` lands: the tree build takes the bounding box first; its report is
  unchanged), `GetTolerance` / `GetThisModelSizeEstimate` / `SetThisModelSizeEstimate` in
  `heal.heal_data_p2` (`fix_tol_sizes` takes `&mut Kernel` now), `GetEdges` in `geom.tools`,
  `GetFaces/GetEdges/GetVertices` in `mass.integrator` and `query.api_queries`, and (merged
  from main meanwhile, #109) `GetVertices` / `GetEdges` / `GetFaces`, the region walk from the
  infinite region and `SmBrep::HealBrep` in `heal.heal_data_p1` (with #164 merged, of its 12
  former `topology.brep_p1` deferrals 7 stop at `topology.brep_p2` (`SmBrep::SetTolerance` in
  `Fix_TolSizes`), 3 at `topology.shell`, 1 at `topology.face_p2` and 1 passes: 266 pass, 227
  deferred; the heal harness of #128/#109 never exported the
  infinite region, so `Fix_InfiniteRegion` on the three boxes compared a B-rep without one
  against one with it: the lists export now carries the infinite-region index), and (#163)
  `GetEdges` / `GetShells` in `boolean.merge_p1` (`GetShells` still stops at
  `SmRegion::GetShells`, `topology.shell`, so its 33 cases continue to the next stub: 26 there, 4
  `boolean.topology_intersector_p3`, 3 `topology.face_p2`);
  in the other direction
  `SmBrep::HealBrep` creates a `HealData` (on a loop handle, `HealLoopHandle`) and calls
  `HealData::heal_brep`, ignoring every status but `Unported`, as upstream ignores the
  healer's. Wiring the last three of the first group exposed runner limitations, fixed or labelled truthfully: the `geom.tools` runner ran
  `FindEdge` / `FindFace` on an empty `Brep` (now it imports the file's B-rep through this
  unit's export; the 64 cases defer on `SmCurve::DropPoint` and the B-rep cache bounding box),
  the `query.api_queries` runner lacked the native list order (its shim export and import now
  carry it; 14 cases that would have failed pass), and the `mass.integrator` runner builds no
  Rust face for its aggregate queries 2-6 (the Rust still runs on the empty B-rep so a genuine
  unit blocker keeps its label; only a face-dependent outcome is deferred as
  `harness:mass.integrator-face-construction`). No `UNPORTED(topology.brep_p1)` remains.
- **Stubs retagged** (they named `topology.brep_p1` but their missing callee is elsewhere):
  the eight `SmTol::GetZoneTol3d(const SmObject*)` sites (`curve_cache`, `ellipse/p1`,
  `topology_solver/p1`, `trim_srf_cache`, `surface_cache/p1`, `surface_cache/p2`,
  `solution_array`, the `geom::tol` header) dispatch to `topology::{vertex, edge, face::p2,
  edgeuse::p1}::tolerance` and `Brep::tolerance`, all ported, but through an opaque object
  handle (the owning edge / face / vertex is not reachable there); they are self-tagged with
  that reason (as `query.api_queries` was) pending the maintainer's call (question in the PR).
  `geom::object` -> `query.brep` (`SmBrep::CalculateBoundingBox`, the first missing call after
  the ported `GetInfiniteRegion`); `proto_topology/p1` -> `topology.brep_p2`
  (`SmBrep::GetMinEdgeBBoxSize`); `trim_srf_cache` editing flag -> `topology.face_p4`
  (`SmFace::GetBrep`). Their stored reports were re-run (`deferred_by_unit` only changed).
- **Deferred, by design until their units land:** `GetShells` / `ResetInfiniteRegion` void flags /
  `SetRegionIsVoidFlagsForNestedSolids` (`topology.shell`), `NormalizedEvaluate` /
  `GetOrCreateUVTrimCurve` / `EvaluateBinormal` (`topology.edgeuse_p3`), `SmFace::GetBrep` /
  `GetUpwardLoopuses` inside face_p1's `SwapUV` (`topology.face_p4`), `SmFace::FaceIntersect` (`topology.face_p3`), `CalculateBoundingBox` /
  `LockBoundingBox` (`query.brep`). The error
  paths before each stub are compared (`ResetInfiniteRegion(NULL)`, the "infinite region not
  first" check, `CreateCornerBlend` with fewer than two edge uses, `CreateEdgeEdgeBlend`
  without edges or NURBS rails). `SmSurface::CreateBlendSurface` (`surface.surface_p8`,
  partial) is called; every blend still defers earlier at `EvaluateBinormal`.

- **Edge-use wiring (main merged through 89dc103a: #117 `topology.edgeuse_p3`, #175, #186, #230).**
  The 3 `UNPORTED(topology.edgeuse_p3)` sites call `topology::edgeuse::p3` with upstream's
  arguments: `NormalizedEvaluate` (header default `bOKToMakeUVTrimCurve = TRUE`;
  `sm_RedistributeSeamUVTrimCurves` requests the first derivative it never reads, as upstream),
  `GetOrCreateUVTrimCurve` (header defaults `NULL`, `NULL`, `TRUE`; the curve upstream returns
  beside the status is read back with `SmEdgeuse::GetUVTrimCurve`) and `EvaluateBinormal`
  (`bUse3DOnly = TRUE` at both sites). `CreateEdgeEdgeBlend` ignores both `EvaluateBinormal`
  statuses as upstream (SmBrep.cpp:2926-2927) and goes on with the outputs upstream sees: they
  start as `SM_UNDEF_DOUBLE`, the not-a-loop-edge-use status returns before
  `EvaluateBinormal`'s "Init outputs" step (all undefined), every later status after it (point,
  tangent and normal zero, binormal undefined: it is written only on success). That is defined
  arithmetic in the flag tests that follow, not uninitialised data (review of e97b63fd, S1; an
  earlier round propagated the failure on that wrong reasoning). Only `Unported` and
  edgeuse_p3's own null-dereference `InvalidInput` propagate. No compared case reaches it yet
  (all 45 blend-flag fixtures defer later). The B-rep is taken mutably. `offset_executive_p1`'s `SmBrep::GetEdges` stub now calls
  `topology::brep::p1::edges` (list order); its `CreateEdgeEdgeBlend` stub keeps the
  `topology.brep_p1` label, since upstream pairs an original-B-rep and an offset-B-rep edge use and
  this port takes both from one B-rep.
  - **Harness.** Until now the shim ran no C++ for `CreateCornerBlend` with two or more edge uses
    or for `CreateEdgeEdgeBlend` blends: it wrote `-3` ("the Rust side defers"). With the stubs
    gone it runs both. Running the edge-edge blend corrupted the heap in
    `CreateSkinnedSurface` (UB-157, also reachable from here; valgrind: 8-byte read/write just
    past a 128-double `sIntParams` copy). The shim now replays `CreateEdgeEdgeBlend` up to its
    `CreateBlendSurface` call on its own rails and applies `surface.surface_p8`'s probe (`SM_ERR`
    for a failing sample, `1007` when knot synchronisation moves rail 2), the port's statuses
    for the same inputs. The corner blend replays upstream's ordering and UV phase on a copy of
    the B-rep (surface types kept) and reports the null-dereference guard only where upstream
    would copy a NULL UV trim curve; no fixture or sweep case reaches it (`cases_both_errored`
    stays 45). The corner blend now also emits the blend surfaces after the count. Both
    probes' refusals are statuses the shim decides (the port's codes for those inputs): once
    the Rust side stops deferring, such cases are guard agreements, not compared C++ runs. The
    edge-edge probe's `GetSampleValues` may build the face surfaces' caches before the real
    call, which builds the same caches at that point (results unchanged).
  - **Flips against the previous report.** 240 new UNSURE cases pass (the raw `2` flag through
    `ResetInfiniteRegion` and its copy in the swap; the nested-solids `!IsVoid()` of UNSURE is
    unreachable upstream, not just uncovered: `SetRegionIsVoidFlagsForNestedSolids` sets the
    infinite region first and every queued region to 0 or 1 before reading it). 11 deferrals pass: 5
    `CreateCornerBlend` on three edge uses, which compare the C++ failure status and an empty
    surface list, and 6 `sm_RedistributeSeamUVTrimCurves`, which compare the status and the swap
    flag. 82 deferrals moved to later units (69 `topology.face_p2`, 10
    `intersect.surface_drop_lib_p3`, 3 `topology.face_p3`). No pass changed. All 45 blend-flag
    `CreateEdgeEdgeBlend` fixtures still defer on the Rust side, so the blend path has no compared
    case.
  - **valgrind** (`--error-exitcode=99 --suppressions=harness/valgrind/ub11.supp`) on the
    corner-blend, redistribute, UNSURE, infinite-region and edge-edge blend samples: 0 errors.
    The suppression covers only UB-11 (`Memcheck:Cond` in `N_BasisInsertKnots`), which the
    edge-edge blends reach under `CreateBlendSurface`; UB-11's entry records why that read
    cannot change a result. Without it, the edge-edge sample reports exactly that one error.

- **Follow-up to #145's maintainer review (bherbruck, at 5b1b28ae; #145 merged with #196).**
  - **Corner blend on non-B-spline edge curves.** The ordered 3D curves stay `Curve` through the
    point-curve padding (upstream copies, trims and evaluates any `SmCurve`). At the
    `SmBSplineSurface::CreateCornerBlend` call, the first curve that is not B-spline-derived gives
    `SM_ERR_NULL_POINTER` (1006, `sm_CreateNonRationalCurve`'s `NER(pNurb)`), after upstream's NULL
    UV curve / surface check, which returns the same status. When a rational curve comes before it,
    the case defers to `nurbs.b_spline_surface_n_lib_p2`, because that conversion is not run here.
    It used to report the null-dereference guard (-2). 30 `crv_on_surf_face` corner-blend fixtures
    (10 B-reps x eus `[0,1,2,3]`, `[3,2,1,0]` and all-non-tangent): 25 pass and 5 defer at
    `nurbs.b_spline_surface_n_lib_p2`.
  - **`CreateEdgeEdgeBlend` takes each edge use's B-rep** (`b_2: None` when both are in `b_1`).
    `offset_executive_p1`'s stub is wired: `build_side_wall_of_edgeuse` takes the original B-rep and
    now defers at `SmBrep::CreateFaceFromSurface` (`topology.brep_p8`). A new op, `edge_edge_blend2`,
    compares the ruled blend across two B-reps: distinguishable rails, unequal intervals, all
    three direction flags and the mate edge use, with the second B-rep imported into its own
    Rust arena (5 cases).
  - **Known back-edges left** (`UNPORTED(topology.brep_p1)` sites outside this unit):
    - `offset_executive_p1.rs:827` (`GetInfiniteRegion` ahead of #175's unported
      `MakeFaceWithCurves` tail): its own follow-up, in COORD.md.
    - `intersect/curve_class/p2.rs:267` (from #85): `IsLiveTopologyMember` is ported, but the site
      holds a generic point-class handle and needs a `ClassObject` → `EntityRef` accessor in
      `intersect.point_class`.
    - `topology/brep/p7.rs` `GetInfiniteRegion` / `SetInfiniteRegion`: wired by the brep_p7 owner
      in #246, together with the region-list head removal and the p7 runner's native infinite region.
  - **Shared `SmCrvOnSurf` import:** owner flag 1, as the C++ graph builds it. Compared case by case
    over all 208 green/partial units; it changes no case.
  - **`SmTol::GetZoneTol3d(const SmObject*)` stubs** are tagged `geom.tol` (the owner in
    `units.json`; the maintainer's answer), at the 7 call sites and the `geom/tol.rs` header. That
    moves 26 `curve.curve_p1`, 48 `intersect.topology_solver_p1` and 6 `intersect.topology_solver_p2`
    deferrals to the `geom.tol` label; nothing else changes.
  - **`ParallelProj`:** a non-positive angle tolerance, or an approximation tolerance below the
    smallest normal double (HANG-32: 5e-324 spins upstream), is rejected on both sides instead of
    failing.
  - **Fuzz findings on the corner-blend path** (the shim has run the real C++ since the edge-use
    wiring; the new fixtures give the mutator new shapes):
    - UB-195 [PSIRT]: `sm_CreateNonRationalCurve` reads `sUVCurves[0]` after a rational curve's
      `DropCurve` succeeds with no curve. Located with gdb on a `-g` build at
      SmBSplineSurfaceNLib.cpp:2949. The shim's corner-blend replay now runs on to that drop, and
      both sides report the guard.
    - HANG-39: tangency tolerances below about 1e-24 over-refine the cross tangents, and the Coons
      knot merge then runs for tens of seconds. A tolerance below 1e-20 is rejected only where
      upstream reaches the cross-tangent approximation: the shim's replay has passed every early
      return and the curve conversion (four sides with a neighbour surface, or more sides). Calls
      that stop earlier keep their statuses, which are compared: one edge use gives `SM_ERR`, four
      non-tangent edge uses give 1006. The later `MakeCurvesCompatible` / `EditParameterization`
      statuses are not replayed, so a call stopping there can be rejected too.
    - A face block with a 1e300 control point recurses in `GlobalCoincidenceChecker` while its UV
      trim curves are built. That is HANG-29 (#147); the reserved HANG-33 was retired as a
      duplicate. Every block value is bounded at 1e150.
  - **Open question for the `curve.line` / `topology.edge` / `topology.face_p2` owners.** A Rust
    `Line` whose parameterisation was edited carries no NURB, and the slicing copies
    (`topology::edge::bspline_slice` in `CreateTrimmedNURBSCurve`, `topology::face::p2::bspline_slice`,
    this unit's `bspline_part`) hand on that NURB-less base: a ruled `CreateEdgeEdgeBlend` over such
    edges fails with `make_nurb` `SM_ERR`. Upstream's `new SmBSplineCurve(*SM_CAST_PTR(..))` copies
    the base too, and whether its `MakeNurb` then succeeds depends on `SmLine::EditParameterization`.
    Not changed here; the difftest never builds `Line` edges, and the offset unit test uses B-spline
    edges.
