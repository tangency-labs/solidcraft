# blend.fillet_geom_p5: what the difftest does and does not cover (PR #264, review round 3)

- **Real-value comparisons**: `TrimRailCurves` / `TrimCliffRails` (rail curves, intervals,
  UV curves, vertex-use UV points of both rails and of the other rails of a cliff
  rollover; cliff-rail pieces taking a side surface from a second geometry; closed rails
  `pStartV == pEndV`; unprocessed `SM_FV_CLIFF_RAIL_X_EDGE` vertices computed by
  `CalcCliffRailIntEdge`), `CalcCrossSection`, `CalcFilletIntSideFace`
  (`blend.fillet_geom_p4` / `_p2` wired), and `UpdateSplitFace` (original faces and split
  faces per edge, the shim setting the rails' original faces first).
- **Status-only or status + early-exit comparisons**: `CalcFilletIntFillet` and
  `CalcEdgeOnExtendedEdge` can only reach the first `NER` of their corner
  (`NER(m_cpCorner)` cpp:2491, `NER(pOptCorner)` cpp:7638), because the shim refuses a
  real corner: the bodies of `calc_fillet_int_fillet` and `calc_edge_on_extended_edge` are
  compared by no case until `blend.fillet_corner_p1` lands. The builders
  (`MakeFilletEdge`, `MakeRailEdge`, `MakeRailEdgeuse`, `CreateEndCurve`) compare the
  status and the edge count today, and all of them past their early exits defer at
  `topology.brep_p3` (the infinite region of `SmBrep()`). After a success both sides now
  also append `blend.fillet_geom_p1`'s whole pseudo-B-rep export (the new edge's type,
  both uses' orientations and list order, each vertex use's vertex, for `EndCurve` the 3D
  curve, interval, primary UV curve and the geometry's side uses, for `RailEdge` the
  geometry's rail and vertices): its Rust side is written but unreachable until
  `topology.brep_p3` lands, so it has never been compared with the C++ record; the porter of
  `topology.brep_p3` should expect to debug it. One known difference to account for
  then: a rail made by `MakeRailEdgeuse` has no vertex uses at the far end, while the
  Rust arena record `Edge.vertices` names the fillet vertex at both ends (the export reads
  that field; every topology accessor derives the vertices from the uses). The
  `RailEdgeuse` op compares its two checks before the allocations (`NER(pFilletSolver)`,
  `NER(pFilletVertex)`); the base solvers of the harness carry no edge uses, so past them
  upstream ends in `NER(pBaseEU)`, whose `SmObjDelete` clean-up deletes a vertex use that
  is still linked in the fillet vertex (a double free in `~SmFilletVertex`, UB-238, source
  inspection): the shim refuses that path, so one generated case runs the op today.
  The C++ side of the success export did run once: `MakeRailEdge` hands its two new
  vertices to the geometry (`m_vVertices`), so the shim takes them back out of the
  model's own vertex list after the export (the model deleted them a second time and
  crashed `~SmFilletGeom` otherwise).
  `copy_fillet_geom` has no shim op: its first dereference is
  `SmFilletExecutive::GetPseudoBrep` (`blend.fillet_executive_p1`), which the harness
  cannot build.
- **Unreachable**: the `SM_OT_OPPOSITE` primary-use `SER(SM_ERR)` in `TrimRailCurves`
  (cpp:10625-10628): the model builds every primary edge use `SM_OT_SAME` (`EuIn` has no
  orientation).
- **`SM_BT_BLEND` end curves**: the harness model pairs `SM_PC_EDGE` with an `SmEdge`
  (`pc_kind` 4 of `blend.fillet_geom_p1`'s model; upstream always does:
  `SmFilletSolver.cpp:7605-7619`, read back as `SmEdge*` by `FindEdgeTangentAtPoint` and at
  `SmFilletSolver.cpp:7665`; `CalcCliffRailIntEdge` reads an `SmEdgeuse` only for
  `SM_FV_CLIFF_RAIL_X_EDGE` vertices, which are classified `SM_PC_EDGEUSE`). An earlier
  note that `FindEdgeTangentAtPoint` casts an edge-use-classified vertex to an `SmEdge` was
  a harness artefact (the model could build `SM_PC_EDGE` with an edge use) and is
  retracted; the shim now refuses only an `SM_PC_EDGE` object that is not an `SmEdge`.
  BLEND cases defer at `FindEdgeTangentAtPoint` (`blend.fillet_geom_p6`).
