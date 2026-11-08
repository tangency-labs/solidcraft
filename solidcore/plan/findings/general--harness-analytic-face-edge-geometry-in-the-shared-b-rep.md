# harness: analytic face/edge geometry in the shared B-rep export (cross-unit coverage gap)

Raised by oursland's review of #145. The B-rep graph export of `harness/cpp/shim/sc_edgeuse_graph.h` (shared by topology.edgeuse_p1/p2, topology.vertexuse, topology.brep_p1 and every runner that imports through them, e.g. heal.stitch_p1) carries `SmBSplineSurface`/`SmPlane` faces and `SmBSplineCurve`/`SmLine` edges exactly. `SmCone`, `SmCylinder`, `SmSphere`, `SmTorus`, `SmSurfOfExtrusion`, `SmSurfOfRevolution` faces and `SmEllipse`/`SmCircle`/other analytic edges arrive as their B-spline part, so the Rust side would evaluate different geometry. Runners therefore defer every op that evaluates such geometry under `harness:analytic-geometry-export` (topology.brep_p1: 217 of 2343 cases; heal.stitch_p1: 12). Those analytic branches are never compared (AGENTS.md §4 step 5 item 5). Closing it needs the export to carry the analytic kinds (type code + defining data) and the importers to rebuild them; it is a harness task, not a unit's, and every topology unit that exports faces inherits it until then.

**Update (exact `SmSurfOfRevolution` export, 2026-10-20).** Both shared graph exports
(`sc_edgeuse_graph.h`, `sc_topology_fixture.h`) now put an exact head before a face surface
that is an `SmSurfOfRevolution` itself (`harness/cpp/shim/sc_analytic_export.h`):
`2 type version surf_oob gen_oob nbytes bytes`, the surface's binary `WriteToDB` record
(generator, polar converter with its circle, placement, analytic domain, swap flag, NURB),
then the regular block. The shim sends it only when it is lossless for the C++ object (NURBs
present so the write mutates nothing; a C++ `ReadFromDB` of the bytes writes back the same
bytes; the polar curve and its out-of-bounds flag survive). The importers
(`topology_edgeuse_p1::read_face_surface`, `topology_vertexuse::read_face_surface`) rebuild
it with the ported `SurfOfRevolution::read_from_db`, set the two out-of-bounds flags the
record lacks, and check that its B-spline part is bit for bit the regular block. Every
newly compared case passes bit for bit (`--green`: 0 failed; e.g. `topology.brep_p1` 92
revolution deferrals -> 56 pass, 36 defer on real stubs; `topology.edgeuse_p1` +200,
`topology.edgeuse_p3` +332, `topology.loopuse` +87, `blend.fillet_standard_solver_p1` +6
passes). With the parallel owner-zone-tolerance `SmFace::GetTolerance` PR on top, the `surf_rev` / `vase_rev` /
`torus` / `toruswvy` `edge_edge_blend` cases (flag != 0) match at 0, including the two that
differed by up to 7.3e-4 through `DropPointFast`. Still flattened (no `SurfaceKind`, the
dispatch gap of `Surface::read_from_db`): `SmSurfOfExtrusion`, `SmCone`, `SmCylinder`,
`SmSphere`, `SmTorus`, and analytic edge curves. A non-local `FaceuseOffset` of a revolution
face is still not run by the `offset.offset_geometry_creation` shim (HANG-26). That runner
also reads its own appended `WriteToDB` records (main's #156 follow-up ed40f92c, which landed
the same HANG-26 keying independently), so its counts are unchanged by this export; the two
imports of a revolution face agree.
`topology.face_p4` (landed on main after this export was written) builds seam curves on face
surfaces: with revolution faces exact, 32 `seam` cases (torus / toruswvy) reach
`Surface::create_seam_curve` on an `SmSurfOfRevolution`, which returned `InvalidInput` (the
ported `CrvOnSurf` holds only a B-spline surface) while the C++ builds the curve. It now
returns `Unported(curve.crv_on_surf)` with the same DISPATCH GAP marker as
`surface::surface::p11`, so those cases defer (face_p4 3957 pass unchanged, 0 failed).
