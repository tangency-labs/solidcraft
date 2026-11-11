# blend.fillet_solver_p2: fillet-geometry stubs need a FilletBrep

`CreateCliffRollOverSideFG`, `sm_FindRolloverSideEdge`, `RecordFilletError`, the torus rail-end checks and the `store` step of
the `Calc*FilletGeom` methods call the `SmFilletGeom` / `SmFilletEdge` / `SmFilletVertex` / `SmFilletEdgeuse` /
`SmFilletVertexuse` members (`blend.fillet_geom_p1`) and the `SmFilletVertex(SmFilletCorner*)` / `SmFilletEdge(..)` /
`SmFilletGeom(..)` constructors (co-ported in `fillet_geom::p1` as `FilletVertex::new` / `FilletBrep::new_fillet_edge` /
`FilletGeom::new`). The ported ones live inside a `FilletBrep` and are addressed by its ids. Since PR #249's review round they
take the pseudo B-rep as the `FilletBrep` the executive owns (`GetPseudoBrep()` is the `pseudo` argument), and every one of the
27 `blend.fillet_geom_p1` sites is wired. PR #466 (merge of main after #303) dropped PR #249's own `FilletGeomRef { index }`
handle and its index mapping: the solver's `m_vFilletGeoms` are #303's `FilletGeomId`s of that pseudo B-rep, so there is one
FilletBrep threading (#303's), not two. `FilletGeom::new` takes the `SmFilletSolver` base, and `CreateCliffRollOverSideFG` gets
`this`'s `FilletSolverId`. `SmVertex::SetPoint` and `SmEdge::SetCurve` on the pseudo B-rep apply the `SM_NO_CHANGE_GEOMETRY`
resets directly, as `blend.fillet_geom_p1`/`_p4` and `blend.fillet_corner_p3` do (`fillet_geom::p4::{set_vertex_point,
set_edge_curve}` are `pub(crate)` for it), and `SmVertex::PostInsert` of a vertex use is the same direct write as
`blend.fillet_corner_p3`'s. What stays an `UNPORTED` stub: `SmFilletEdge::MakeFilletEdge` and `SmFilletEdge::CalcEdgeOnEdge`,
`SmFilletExecutive::GetFilletCornerOfVertex` / `NoteFilletError` (`blend.fillet_executive_p1`), the `PreInsert`/`PostInsert`
of edge uses (`topology.proto_topology_p1`, `justified:` by "blend.fillet_solver_p1: pseudo-B-rep mutations"), and the new edge
uses: `FilletBrep::new_fillet_edgeuse` is a real call that still stops at `SmBrep::SmBrep` (`topology.brep_p3`: the pseudo
B-rep has no infinite region). So `CreateCliffRollOverSideFG` runs up to the creation of the side geometry's rail edge uses, and
`sm_FindRolloverSideEdge` runs in full (Rust unit tests in `fillet_solver/p2.rs`; no difftest op: its C++ is a static function of
`SmFilletSolver.cpp` that needs a classified rail, a pseudo B-rep and a target B-rep of a matching shape). `sm_SrfSrfIntersection`
(`blend.fillet_geom_p2`) is wired in `CreateCliffRollOverSideFG`, with the points the two vertices were just set to. The
`Calc*FilletGeom` entry points run in the difftest (`geom` op) against a `FilletBrep` mirroring the shim's `MakeRailEdge` rails
(a fresh `SmFilletGeom`, passed explicitly, so no constructor geometry is involved); the torus rail-end checks set
`m_bExtendAfter` and are compared with rail ends typed `SM_FV_RAIL_X_EDGEUSE`, processed.
