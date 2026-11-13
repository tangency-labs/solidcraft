# blend.fillet_solver_p1: pseudo-B-rep mutations

`SplitFilletGeomAtSideEdge`, `sm_SplitRail` and `sm_ComputeSupportEdge` (PR #192) move
vertices, replace edge curves and relink vertex uses of the fillet pseudo-B-rep
(`SmFilletBrep`, units `blend.fillet_geom_*`): `SmVertex::SetPoint` (`topology.vertex`),
`SmEdge::SetCurve` (`topology.edge`) and `SmOwningTopology::Remove` / `PostInsert` of a
vertex use (`topology.proto_topology_p1`). The Rust counterparts need the B-rep's
notification records (`vertex::set_point` and `edge::set_curve` take the record map,
attribute store and context of the B-rep being notified; `OwningTopology::remove` /
`post_insert` work on a proto-topology record map, not on a `topology::Brep`). This crate
has no pseudo-B-rep model yet (PR #147 adds `FilletBrep`), so these calls are
`UNPORTED(...)` stubs marked `justified:` with this section. Every one of them comes after
an unported `SmFilletGeom` / `SmFilletVertex` call in the same function, so no difftest
case reaches them; wire them to the pseudo-B-rep's records when `blend.fillet_geom_p1`
lands. No disagreement.
