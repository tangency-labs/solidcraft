# topology.brep_p8 (`SmBrep` face builders, PR #72, 2026-10-20; partial, no disagreements)

- **Builds on PR #204** (`topology.brep_p3`, head `293ce7fc`, merged into the branch):
  `MakeFaceWithCurves` makes its wire edges with #204's `MakeWireEdge` /
  `MakeWireEdgeVertex`, and `CreateFacesWithCurves` deletes faces with its `DeleteFace`.
  #72 cannot merge before #204.
- **Every face construction stops in `MakeFaceTopology` (`topology.brep_p6`).** All seven
  functions are ported (`topology::brep::p8`; the "new algorithm" branch of
  `CreateFaceInRegionFromSurface` is dead code, `sbUseOldAlgorithm` is a `constexpr TRUE`).
  Every path that makes a face goes `MakeFaceWithCurves` (`brep_p7`) -> `MakeFace` ->
  `MakeFaceTopology` (`brep_p6`), whose port stops at its `SetTolerance` call: the stub
  there is labelled `UNPORTED(topology.face_p3): SmFace::SetTolerance`, but
  `face::p3::set_tolerance` is ported and the stub stands for the unported remainder of
  `MakeFaceTopology` (face use / loop / edge use construction, radial sectors, trims,
  `TestClosureSplitRegion`; `SmBrep.cpp:14141-14828`), which `topology.brep_p6` owns. So
  every case that would build a face defers on `topology.face_p3` (298 of 339). **Unblock:**
  finish `MakeFaceTopology` in `topology.brep_p6`; then the 298 deferred cases (faces on
  planes and B-spline patches, natural and sub-domains, creased and closed surfaces,
  planar loops with holes and several outer loops, the pyramid of
  `primitive.api_primitives`) compare the whole B-rep they build.
- **What is compared now** (41 cases, all with values): the status and the whole B-rep
  after the op (counts, shell digest, the full topology graph of the `topology.brep_p3`
  runner, and every vertex, edge, face and edge use's geometry): domains outside the
  natural one, null surfaces and curve arrays, open, non-planar, empty and
  degenerate-line planar curve sets, and three `MakeFacesWithCurves` split paths (a UV
  loop with a hole on surfaces creased in `u`, `v` and both, 479 values each: the crease
  iso curves are intersected with the loops, the loops split and the iso segments
  inserted, the sub-patches made; each sub-patch's `MakeFaceWithCurves` fails, which
  upstream ignores, leaving its wire edges in the B-rep). `--perturb` SENSITIVE 41/41;
  `--fuzz 2000` FUZZ-PASS (rejected 474, malformed 822, ran 704, 0 differs/crashes).
- **UB-119 behind the plane fit.** `CreatePlaneWith3DCurves` of loops far from the origin
  relative to their size (the pyramid base at `1e9` with side 5) tessellates them to zero
  area, and upstream `FindOuterLoops` reads past its arrays (UB-119); the C++ goes on with
  garbage counts (and builds the pyramid). The port refuses it (`InvalidInput` from
  `topology::trimming_tools`); the face builders pass that error through instead of
  `SM_ERR`, and the runners of this unit and `primitive.api_primitives` defer such cases
  as `cpp-guard:ub-119` (4 generated pyramids).
- **`MakeFacesWithCurves` group counts** (WR-210): a sub-patch's second outer-loop group
  is counted with the first group's loop counts; ported as is.
- **Wired back-edges:** `topology.trimming_tools` (`MakeFacesWithCurves`),
  `heal.heal_data_p3` (`CreateFaceInRegionFromSurface` under the edit tracker, its changes
  kept also on failure), `boolean.merge_p3` and `offset.offset_executive_p1`
  (`CreateFaceFromSurface`, arena-only notification stores; `build_side_wall_of_edgeuse`
  now takes `&mut Kernel`) and `primitive.api_primitives` (`SmApiCreatePyramid` builds
  its five faces and stitches them; it takes the kernel). Two remain:
  `offset_executive` `CreatePlanarFaceWith3DCurves` (the 2D-boolean continuation around it,
  `SmOffsetExecutive.cpp:3816-3928`, is unported and needs `Boolean2D`,
  `primitive.primitive_creation_p1`), and the runner-side deferrals of
  `mass.integrator` and `surface.trim_srf_cache` (their Rust sides build no face yet).
- **Wired stale stubs in `geom.tools`** (`FindCoincidentEdge`, `FindEdge`): the curve
  owner, evaluation, `DropPoint` and `CurveMaxDistanceBetween` of the green
  `curve.curve_p2` / `curve.curve_p6`; `MakeFaceWithCurves` reached them for every
  second curve of a loop. `CoincidentEdgeQuery` gets `curve_owner` (the caller resolves
  `pCurve->GetOwner()`), and `find_coincident_edge` / `find_edge` take the kernel. The
  `geom.tools` runner now compares the coincident-edge cases on the file B-rep.
- **Shim bug fixed in `primitive.api_primitives`:** `put_brep(o, SmApiCreateX(.., b), b)`
  read `b` before the call wrote it (unspecified argument order; GCC evaluates right to
  left), so every successful C++ B-rep reported `has = 0`; `put_brep` now takes `b` by
  reference. Unseen before, as every Rust B-rep entry point deferred.
- **Analytic surfaces** (`SmCylinder`, `SmSphere`, `SmTorus`, `SmCone`) are not in the
  shim: the Rust `Surface` has no such kinds (issue #76), so a face on one could not be
  compared with its C++ overrides.
- **`topology.brep_p5` runner after merging #204 into main:** `CreateWireEdgeFromCurve`
  now runs through (main wired `GetInfiniteRegion`, #204 `MakeShellVertex` /
  `MakeWireEdgeVertex`), and `MakeEdgeVertexTopology` unlinks the start vertex's shell
  vertex use, which upstream deletes. The arena keeps it, and that runner's `graph` lists
  unreachable records too, so `isolated_op13_editing_*` showed one vertex use more than
  the C++. The runner now defers a result holding unlinked uses or shells on
  `topology.brep_p4` (`SmBrep::~SmBrep`, the record erasure its notes already name): 744
  pass, 0 fail, 53 deferred, as on main.
