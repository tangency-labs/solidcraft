# curve.curve_cache: edge tolerance at the remaining edge-curve cache builds (2026-10-27)

Updates: plan/findings/curve.curve_cache--edge-owned-approx-tol.md

The sites that hold the edge now record its tolerance on their evaluation copy of the
edge's curve (`Curve::set_owner_zone_tol_3d`, kept by `clone_for_evaluation`, dropped by
`Clone` and by an owner change), and `make_curve_cache` builds with it
(`SmTol::GetApproxTol3d(&m_crCurve)` = half `pEdge->GetTolerance()`):

- `create_uv_trim_curve`: the drops of the edge curve (`DropCurve`, `DropAndTrimCurve`;
  the reversed copy is a new curve without owner, as upstream's copy);
- `query::loop` `drop_on_edge` (`SmLoop.cpp:759`);
- `TopologySolver::solve_point_edge` (`SmTLSPointEdge`, the edge's tolerance through
  `TopologyObjects::edge_tolerance`);
- `delete_topological_vertex`'s `GlobalCoincidenceChecker` of the two edge curves
  (reached from the fillet's `RemoveTopoEdges`).

`SmEllipse::MakeNurb` [B630] builds the arc with the edge's tolerance where the caller
resolved it (`Ellipse::make_nurb_owned_with_edge_tol`): the tessellation of an edge
curve's cache, and curves carrying the recorded tolerance (validation's NURB fix-up,
primitive construction). Elsewhere an edge-owned ellipse without NURB still returns
`Unported`.

Since the v1.0.1 pin, `SmCurveCache::Tessellate` skips the distance test for spans of a
curve derived from `SmBSplineCurve` (`bExactSpan`, `SmCurveCache.cpp:1070`), so the
tolerance changes the tree only for the other kinds (`SmHermiteCurve`, `SmCrvOnSurf`,
offset, projected, composite, ...). B-spline, line and ellipse edges, which every
existing B-rep fixture uses, build the same tree at any tolerance; a probe over
`primitive.api_primitives`, `blend.fillet_geom_p1`, `blend.fillet_executive_p1`,
`topology.edgeuse_p3`, `query.loop` and `intersect.topology_solver_p1` found no
owned-curve build whose tree depends on it.

Evidence (new fixtures, Hermite edges; trees from a temporary span-count probe):
- `query.loop` `edge_owned_tol.json` (80 cases, scales 1 to 1e10, edge tolerances
  1e-10 to 1): trees of 11 against 865 spans (tolerance 1e-2 and 1 at 1e10) and 8192
  or 4708 against 11 (1e-10, 1e-8 at 1e6/1e7). All pass before and after; the largest
  relative difference goes from 3.1e-15 to 0.
- `intersect.topology_solver_p1` `edge_owned_tol.json` (60 cases, scales 1 to 1e7,
  edge tolerances default 0.0, 1e-10 to 1; the FFI bounds coordinates at 1e8): trees
  of 8185/8192 against 12 spans (1e-10) and 4542 against 12 (1e-8 at 1e7). All pass
  before and after (the polished solutions agree).

Not compared:
- `create_uv_trim_curve` and `delete_topological_vertex`: no harness B-rep has an edge
  curve of a kind whose tree depends on the tolerance and that is dropped. The
  `topology.edgeuse_p3` `SmCrvOnSurf` faces take the copy path, and a `SmCrvOnSurf`
  over a moved copy of the face's surface is dropped without building the edge curve's
  cache (checked with a probe); the edge graph export carries no `SmHermiteCurve`.
- [B630]: ellipses are built with their NURB, so no case reaches `MakeNurb` of an
  edge-owned ellipse.

Still on the system tolerance (no edge in reach; follow-ups): the `TrimSrfCache`
edge-use caches (#1307); owned edge-curve builds the probe found in `heal.stitch`
(`edit_test_and_split_edge`), the face `IsDegenerate` coincidence check
(`topology::face::p3`) and the curve classification's `intersect_edges`;
`Curve::length_base` over an owned curve (`Unported`); the `edit_parameterization` /
`reverse_parameterization` / `join_with` paths of an edge-owned ellipse without NURB
(`Unported`). Upstream also keeps a curve's cache across later edge-tolerance
changes; the port builds at the tolerance current at the call.
