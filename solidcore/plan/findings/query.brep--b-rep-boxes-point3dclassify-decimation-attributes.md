# query.brep (B-rep boxes, `Point3DClassify`, decimation attributes, `SmApiEvaluateSurface*`; PR #329, takeover of #101, 2026-10-21; partial, no disagreements)

- **Lazy B-rep cache.** Upstream builds the `SM_OC_BREP` cache inside the solves that need it
  (`CacheMakeOrValidate`). `query::brep::BrepQuery` does the same: a single-region
  `Point3DClassify` (and one without boundary intersections that never fires a ray) builds
  none, and a failed build is a failed `RayFire` (ignored, next direction), not an error of
  the classification.
- **`const` queries that edit.** `RayFire` / `LocalCurveSector` may create missing UV trim
  curves on the B-rep (upstream does it inside `const` methods). Callers holding the B-rep
  shared (`query.api_queries`' containment tests, `SmShell::RayFire`'s sector) classify on
  a private copy (`BrepClassifier`), as `query.face_p3` and `mass.integrator` already do;
  callers with the B-rep mutable (the merge, `FindInfiniteRegion`) work on it directly.
- **Identities.** The cache keys and geometry owners are the caller's. Every wired library
  caller uses `BrepIdentity::arena` (records' `arena_identity`), which is what this crate's
  builders and importers give geometry; a caller with other keys passes its own
  `BrepIdentity`.
- **Ignored statuses in `CalculateTightBoundingBox`.** The face loop keeps `rFaceBBox` from
  face to face and uses what a failed call leaves; `query.face_p3` gained
  `calculate_tight_bounding_box_into`, which writes the caller's box at the same two points
  as `SmFace::CalculateTightBoundingBox`. A failed loose edge box is taken as the freshly
  initialised (empty) box: every curve override (`SmCurve`, `SmBSplineCurve`, `SmLine`,
  `SmHermiteCurve`, `SmCrvOnSurf`, `SmCompositeCurve`) returns its error before writing
  the caller's box (checked against upstream; the composite ignores its segments' statuses
  and always succeeds). No fixture reaches these ignored statuses: a probe over every box
  fixture (files, solids, wires, overflowing segments) saw the edge and face boxes fail only
  with `Unported` (`SmFace::GetTolerance`, `topology.face_p2`), which propagates. The
  inputs that would fail them (a face without surface, a curve whose span search fails)
  crash or are rejected on the C++ side, so the shim guards them out.
- **`merge_vertex` records.** Wiring `Point3DClassify` made `SmMerge::ProcessWiresAndShellVertices`
  reach `merge_vertex`'s `Copy` notification for vertices of imported operands, which had
  no `Topology` record (records are made when this crate creates an entity). Upstream every
  `SmVertex` is an `SmTopology`; `merge_vertex` now materialises a missing record as the
  default one.
- **Harness gaps found on newly reached paths.** `topology.brep_p2`'s RayFire op passed an
  unbuilt cache (hidden by the `query.brep` stub); `geom.a_object`, `boolean.merge_p4`,
  `boolean.topology_intersector_p1` and `mass.integrator` needed the runner fixes of #332
  (bherbruck), applied here. `boolean.merge_p1` defers its unmated
  `FindBrepRegionOfOtherFace` on analytic face geometry
  (`harness:analytic-geometry-export`).
- **Coverage limits.** The shared B-rep fixture gained kind 7 (the closed box of
  `SmApiCreateBox`) so the multi-region ray-fire path is compared: rays that miss, the
  retry order and boundary hits on edges and vertices compare; rays that hit a face defer
  on `geom.tol`'s `SmTol::GetZoneTol3d(const SmObject*)`. Kind 8 (up to 600 vertices and
  150 wire segments at given tolerances) reaches the 500-vertex / 100-edge approximation
  shortcut and non-default tolerance expansion.
- **Follow-ups for other units (no open PR covers them).**
  - `topology.brep_p2`: `merge_curves_on_surface` takes `&mut Brep` and the classification
    world separately, so no world can read the B-rep's geometry during the call;
    `topology::brep::p10::merge_curves_world` stays a `DISPATCH GAP` until the world can
    own the B-rep (as `boolean::merge::p4::ArenaPair` does).
  - `geom.object` (done in the follow-up PR): `Shape::new` got B-rep keys but no store to
    resolve them, so the B-rep and face boxes were `DISPATCH GAP`s (and the edge and vertex
    lists stale `UNPORTED` stubs). It now takes a `BrepStore` and runs the whole
    constructor; the runner's C++ shape is built on the shared fixture B-rep, and the Rust
    import gets upstream's `GetFaces` / `GetVertices` orders (the walk numbers them
    differently; whole-B-rep shapes list their members in those orders). `geom.object` is
    green. `query.api_queries`' 31 `SmApiFaceClosestPoint` cases that stopped at the face
    box now build their shape and defer later (`topology.face_p2` 23, `geom.tol` 8).
- **Wider seeds (2000 generated cases per unit, local macOS run).** `query.brep`,
  `boolean.merge_p1`, `boolean.merge_p4` and `topology.brep_p9` show no failures. Two units
  fail on generated cases this PR does not change; both are left to their owners:
  - `boolean.merge_p5` (64 failures). (1) On a repeated call, a marked other wire without a
    mate makes `SmMerge::ProcessWiresAndShellVertices` append `NULL` to `keep_as_wires`
    upstream; the Rust list is a `Vec<EdgeId>` and has no null entry, so it reports a null
    pointer instead. (2) The entities `merge_vertex` / `MakeShellVertex` create are numbered
    in creation order on both sides (`SmOwningTopology::PostInsert` appends, as the arenas
    do), but the runner's walk visits them in a different order than the C++ walk, so the
    topology indices permute. Neither involves `Point3DClassify`'s result.
  - `topology.shell` (8 failures): `IsDegenerate` on the loose shell box path of
    `query.face_p1`, a path this PR does not reach.
- **Runners newly exposed after merging main's `boolean.merge_p2` (#229) (2026-10-21).**
  - `boolean.api_brep` passes empty stand-in B-reps (the box and sphere constructors are
    `primitive.primitive_creation_p1`). The `query.brep` stub used to stop every call that
    reached the merge engine; with it ported, those calls ran on the empty stand-ins and
    113 cases failed. The runner now defers a call that took a mark
    (`SmNewMarkAndLock`: the merge engine does, input validation never does) on
    `primitive.primitive_creation_p1`, whatever it returned. The compared set is
    unchanged (151 validation cases); the 114 former `query.brep` deferrals now name the
    stand-ins.
  - `boolean.merge_p5` imported its operands without geometry owners, so the B-rep cache
    of `Point3DClassify`'s boundary solve indexed no curves and a wire midpoint lying on an
    edge classified to the region (`partial_other_marks_classification`). The import now
    calls `own_arena_geometry`, as the `query.brep` and `topology.brep_p2` runners do.
  - Later merges of main exposed three more such places, handled the same way:
    - The `boolean.topology_intersector_p1/p2` runner keyed its B-rep caches by
      `query_key` but imported unowned geometry (4 failing `IntersectTopology`
      scripts). `Two::new` now sets those owners.
    - `offset.offset_executive_p2` (#201) fails closed when a Rust call returns a B-rep
      its scaffolded fixtures cannot certify. Six cases do so now that the box is
      ported; they defer on `harness:offset-result-adapter` until the runner has an
      output adapter.
    - `boolean::merge::p4::arena_pair` called the box under caller notification keys
      with no kernel; it is retagged as a `DISPATCH GAP` of `boolean.merge_p4` (the
      cache there also needs the caller's geometry identities).
