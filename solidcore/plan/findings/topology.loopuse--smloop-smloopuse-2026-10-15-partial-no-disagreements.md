# topology.loopuse (`SmLoop` / `SmLoopuse`, 2026-10-15; partial, no disagreements)

- **Harness.** The runner reuses unit `topology.edgeuse_p1`'s B-reps (15 upstream `.smb`
  files with and without `RebuildUVTrimCurves`, the `my_test_topology` wire network, wire
  edges, plane and B-spline faces incl. the tiny/huge/pinched/sub-domain faces of
  `topology.edgeuse_p2`'s fixtures) and its export/import. The shared export
  (`sc_edgeuse_graph.h`, and the copies in the `topology.vertexuse`, `topology.face_p2`
  and `query.edge` shims) now carries each loop use's `m_eOrientation` (new field
  `topology::Loopuse::orientation`). The edgeuse export still lacks each face's `m_pFU`;
  the loopuse shim has a `FaceFu` op and the runner patches it into the import.
- **Mutating ops** (`FlipLoopOrientation`, `CollectEdgeuses`, `SetEdgeuse`, `SetLoop`)
  leave graphs `~SmBrep` cannot free (a loop use pointing at another loop is freed
  twice): the shim snapshots the loop/edge/vertex use links and restores them before
  deleting the B-rep, after recording the state it compares.
- **Deferred.** `FlipLoopOrientation` of any loop on a face (`SmFace::Notify`,
  `topology.face_p3`; every loop of the test B-reps is on a face, so the flip itself is
  only checked by unit tests), `GetUVCornerBoundaries` of an edge loop
  (`GetOrCreateUVTrimCurve`, `topology.edgeuse_p3`), `IsMissingSeamAtVertices` on closed
  surfaces (sector computation) and the inexact-geometry cases (as `topology.edgeuse_p2`).
- **Upstream bug.** `IsClosed3d` never reports a loop open (WR-93); ported as is.
- **Review fixes (PR #102).** No case had reached `IsClosed3d`'s over-tolerance branch
  (`SmLoopuse.cpp:851-866`): over 3,600 closed3d cases the largest gap was 0.039 of its
  `XSectTol3d`. The `Closed3d` op now takes an optional `edge_tol` that sets every
  `SmEdge::m_sZoneTol3d` (stored as `fabs`) / `Edge::tolerance` first;
  `fixtures/topology.loopuse/tolerance_limit.json` (1,984 cases: 0, 1e-15, -1e-12, 1e-9,
  `bCheckAllGaps` both ways, loop and loop use) reaches the branch 176 times (88 with the
  early stop), all bit-identical. The shim's `closed3d_safe` now predicts that early stop
  from `SmTol::GetXSectTol3d(GetZoneTol3d(a), GetZoneTol3d(b))`, as upstream, not the raw
  edge use tolerances. `fixtures/topology.loopuse/review_adversarial.json` keeps a
  deterministic 4,712-case subset of the reviewer's 78,120-case sweep (every op on loop
  indices 0..23 of each B-rep; the full sweep passed with 0 failures).
- **Guard wider than the C++.** In `FlipLoopOrientation`'s vertex branch, both the shim
  and the Rust reject a mate vertex use that is not owned by a loop use. Upstream only
  reassigns `m_pSorLUorEU` there (not undefined); `topology::VertexuseOwner` cannot hold
  that state and the input is malformed, so the case stays rejected on both sides.
- **For `surface.surface_p5`.** `GetUVCornerBoundaries` uses the UV point `DropPoint`
  writes whether or not `bSuccess` is set (statuses ignored by `SE` in release); the
  `drop_point` stub must return `Some` for any written point, `None` only when the
  out-param is left unwritten. Unreachable today
  (`GetOrCreateUVTrimCurve` is `UNPORTED(topology.edgeuse_p3)`).
- **Duplicate merged.** `query::r#loop::contains_uv_point` now walks the arena topology
  (PR #117) and calls `topology::loopuse::uv_corner_boundaries`; the co-ported copy is gone.
- **Not wired**, still `UNPORTED(topology.loopuse)`: `intersect::point_class`
  (`SmLoop::IsConnectedTo`, `SmLoopuse::IsConnectedTo`) and `surface::trim_srf_cache`
  (`GetEdgeuses`, `GetVertexuses`). Those callers hold opaque object handles without a
  `Brep` (as for `topology.edgeuse_p1` / `topology.vertexuse`). Wired: the co-ports of
  `topology.edgeuse_p1` (`loopuse_edgeuses`, `loopuse_shell`), `topology.face_p2`
  (`loopuse_vertexuses`), `query.loop` (`calc_quad_num`) and the
  `SmLoopuse::GetOrientation` stub of `topology.vertexuse` now call this unit.
- **After merging main (topology.vertex, topology.edge, heal.gap).** `IsConnectedTo`
  with vertex/edge targets now calls `topology::vertex::is_connected_to` /
  `topology::edge::is_connected_to`; the shim runs it, with a null-dereference status
  when the loop has no loop use (or, for a vertex, its loop use's vertices cannot be
  walked), so the 99 formerly deferred cases pass (8854 cases, 7950 pass, 904
  deferred). The loop use orientation is added to the export copies in the
  `topology.vertex`, `topology.edge` and `heal.gap` shims and their importers.
  `topology::vertex::loopuse_vertices` and the private `topology::edge::loopuse_edges`
  (co-ports) duplicate this unit's `GetVertices` / `GetEdges` walk; merge them later.
- **`surface.surface_p5` merged, `DropPoint` stub kept.** `GetUVCornerBoundaries`'s
  `UNPORTED(surface.surface_p5)` `drop_point` stays a stub: it is reached only after
  `GetOrCreateUVTrimCurve` (`UNPORTED(topology.edgeuse_p3)`), so no case can compare it,
  and it needs "was the out-param written" (upstream writes `rResultUV` with a NULL guess
  only on success paths), which `Surface::drop_point` does not report (it returns
  `Vec2::UNDEF`). Wire it with that distinction when `topology.edgeuse_p3` lands.
