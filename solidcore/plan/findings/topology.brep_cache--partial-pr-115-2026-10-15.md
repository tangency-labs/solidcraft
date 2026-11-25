# topology.brep_cache (partial; PR #115, 2026-10-15)

- `SmBrepCache` polygon tree construction, type/accessor queries and exact
  vertex/line-edge resizing compare in 663 cases: 662 bit-identical passes,
  zero failures, one explicit `query.brep` deferral. No absolute floors.
  The source `my_test_extent3d` points are re-expressed as cache geometry;
  fixed regressions and 600 seeded cases cover polygon degeneracies, shared
  boundaries, tree split thresholds, tolerance extremes and scale changes.
- Exact `BuildTrees` stops at `SmBrep::CalculateBoundingBox` (`query.brep`,
  currently PR #101); its exact geometry insertion paths are implemented but
  not certified. The subsequent `SmBrep::GetVertices/GetEdges/GetFaces` calls
  explicitly defer to `topology.brep_p1`; arena allocation order is not the
  B-rep linked-list order and includes unlinked records. Face resizing needs
  `SmFace::CalculateBoundingBox`
  (`query.face_p1`, PR #105) and `SmFace::GetEdges` (`topology.face_p4`). Those
  calls retain correctly owned `UNPORTED` markers. `BuildPolyTrees`, `GetBBox`,
  drawing and memory accounting are not functions assigned to this unit.
- The checked FFI retains native negative-tolerance (`fabs`) and
  `SM_USE_DEFAULT` handling. A separate native differential regression compares
  full Rust/C++ cache outputs for both; exact fixture tolerances go through
  `zone_tol3d` just like their native typed arguments.
- Geometry owner keys need not equal arena indices. Build/resize and the solver
  adapter take an explicit identity mapping; resize cases use keys offset by
  1,000,000,000. Cache invalidation and source identity remain with the caller,
  as for existing curve/surface cache factories. `geom::tools::find_face` now
  takes the caller's kernel, B-rep id, owner keys and intermediate-owner resolver;
  it never creates a default context or assumes arena-index keys. The solver
  adapter uses that B-rep id for shape membership. `Curve::edge` follows the
  supplied generated-curve/edgeuse owner chain in both resizing and solving;
  a native regression compares chained and direct ownership. Reverse edge/face
  lookup deliberately scans arena ids using the caller's key mapping: it resolves
  identity, not B-rep list membership, and does not affect traversal order.
  Native surface/curve containment uses zero tolerance, vertices use `EFF_ZERO`.
  `CacheSource` and typed object handles avoid references from the cache back into mutable geometry.
- The shim builds exact resize edges with `MakeShellVertex` plus
  `MakeWireEdgeVertex`, preserving the two distinct endpoints supplied to the
  Rust fixture. `CreateWireEdgeFromCurve` would intentionally `GlueVertices`
  below the model tolerance, changing the graph and the final vertex tolerance
  when the two endpoint setters touch the same vertex. This fixture correction
  cleared 53 small-scale mismatches; no comparison tolerance was changed.
- All incoming `UNPORTED(topology.brep_cache)` sites are wired. The object-only
  SAG validator now requests source geometry for new caches, matching the
  existing curve/surface factory convention. Its runner calls the real cache
  factory; 131 cache deferrals now correctly name `query.brep` (total remains
  168, including the 37 `topology.shell` cases). Generic topology adapters can
  expose their cache through `TopologyObjects::brep_cache`; existing explicit
  tree adapters continue to work. Exact end-to-end B-rep solves still await the
  dependencies above.
