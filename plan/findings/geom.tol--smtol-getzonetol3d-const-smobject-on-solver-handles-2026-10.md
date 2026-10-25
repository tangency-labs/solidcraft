# geom.tol: `SmTol::GetZoneTol3d(const SmObject*)` on solver handles (2026-10-22)

- Solution handles stay opaque; the caller that owns the B-rep resolves them.
  `GlobalSolve::object_zone_tol_3d` (default: the `geom.tol` deferral) is what
  `AddSortedSolution` reads for `SM_SO_RAYFIRE` (`SmSolution::GetObjectTolerance` is
  `Solution::object_tolerance_with`). `TopologySolver` forwards it to
  `TopologyObjects::object_zone_tol_3d`; `BrepObjects` answers per `SM_USE_OLDTOL`:
  vertex/edge/face `GetTolerance`, a curve's `GetEdge()` tolerance (else the system
  zone tolerance), a surface's `GetFace()` tolerance (else system), a B-rep's
  tolerance; poly entities and foreign owners stay deferred. Curve point solves get
  the resolver through `GlobalPointSolveOptions::zone_tol_3d` (`ObjectZoneTol`).
- The topology solver's local solvers called `ts.base.add_sorted_solution`, i.e. the
  plain `GlobalSolver` impl rather than `SmTopologySolver`'s (upstream calls the
  virtual on the topology solver). Equivalent while no hook was overridden; they now
  sort through the `TopologySolver` so the tolerance hook applies.
- `face::p3::is_degenerate`'s `EdgeGeometry` now resolves its `EntityRef` handles
  (`topology_entity`), so `SmEdge::GetTolerance` in the point classification of its
  coincidence solutions reads the edge.
- With these, the B-rep ray fire of `split_closed` (`find_infinite_region` ->
  `shell::ray_fire`) gets past the tolerances and stops at
  `topology_solver::p1::face_point_classify` (`SmFace::PointClassify` on an opaque
  face handle, `query.face_p2`). Upstream classifies against the face surface's
  trimmed surface cache when one exists (`SmCacheMgr::GetObjectCache`), which the
  arena B-rep does not keep between operations; wiring it faithfully needs that
  cache state (and `&mut Brep` for missing UV trims), so it is left deferred.
