# topology.vertex (`SmVertex`, 2026-10-15; partial, no disagreements)

- **Vertex use lists.** `topology::Vertex` gained `vertexuses` (`m_pList`, upstream list
  order; `GetPrimaryVertexuse` is its head) and `poly_vertex` (`m_pPolyVertex3D`). Every
  B-rep builder must keep `vertexuses` in step with the vertex use records: the
  `topology.vertexuse` / `topology.edgeuse_p1` runners fill it in arena order (their
  shims do not export the list), the `topology.vertex` shim exports the real list. The
  co-ported scans in `topology::vertexuse` now call `topology::vertex`.
- **`m_bExact` does not exist in the reference build.** `SM_TOLERANT_WATCH` is defined
  only by `NL_DataStruct.h`, which is not seen before `SmVertex.h`; the shim cannot even
  name `m_bExact`. `IsExact` is therefore always `TRUE` and `SetExact` does nothing; the
  port does the same (no field).
- **Uses that no owner chain reaches.** In prog_test `my_test_topology`'s wire network a
  vertex's use list holds vertex uses that the region/shell/edge walk never visits (how
  they got there was not investigated). The vertex shim registers the vertices' list
  members too; the earlier topology shims' exports omit them.
- **Chains of wire edges.** The shim adds a B-rep kind built as prog_test
  `my_test_edge_geometry_repairs` builds its wire (`MakeShellVertex`,
  `MakeWireEdgeVertex`, `MakeWireEdge`) from real `SmLine`s (optionally
  re-parameterised), degree-1 `SmBSplineCurve` lines and B-spline curves, so
  `IsTopologicalVertex` / `AreEdgesTangent` are compared on lines (the `IsLine` branch),
  on curves meeting at equal and unequal parameters, around the 0.5 degree limit, on
  cusps (upstream counts them, WR-92) and on reversed edges.
- **Curve identity.** Upstream compares edge curves by pointer in
  `IsTopologicalVertex`; each Rust edge owns its curve, so a B-rep whose edges share a
  curve object is rejected by the runner (none of the fixtures do).
- **Upstream bugs.** `GetVertexuseOfFace` ignores its face (WR-91),
  `GetMaxUpDimGap3d` adds to a `NULL` gap array by default (UB-105), the tangent tests
  read an uninitialised tangent after a failed evaluation (UB-106). Ported as written
  where defined; the undefined cases are `InvalidInput` and skipped by the shim.
- **Review fixtures (PR #96 review).** `fixtures/topology.vertex/review.json` (1408 cases,
  all pass, none skipped by the shim) adds what the generated chains never reach:
  rational edges at a vertex with two edges (`SmBSplineCurve::IsLine` handing a rational
  curve to the sampling `SmCurve::IsLine`), rational arcs meeting lines and arcs at equal
  and unequal parameters, turns at 0.49/0.51/179.49/179.51/180/90 degrees, 1e-12..1e12
  scales and tangent / gap / `SetTolerance` limits. Still not compared: `SmCircle` /
  `SmEllipse` edges at a vertex (the chain shim builds only `SmLine` and
  `SmBSplineCurve`); the shim reports `-2` (skipped) for an `IsTopologicalVertex` where
  upstream's SER would return before reading the uninitialised tangent, and the FFI
  rejects NaN tolerances that C++ handles (coverage only, no wrong result).
  `Brep::add_vertexuse_record` does not append to `Vertex::vertexuses`; a linking helper
  or a debug consistency check would stop silent drift once library builders create
  vertex uses.
- **Harness fixes (PR #117, after `GetOrCreateUVTrimCurve` was wired).** The gap, topological
  and pole cases of this unit used to defer on `UNPORTED(topology.edgeuse_p3)` (448 cases), so
  the importer's gaps went unnoticed; once they ran, 343 cases failed because (1) the shim
  exported no face UV domain or tolerance (the Rust faces had an empty domain and tolerance 0,
  so `ComputeUVPoint`'s global point solve found nothing): both are exported and imported now;
  (2) an `SmPlane` was exported as its B-spline part (now the whole plane, as
  `sc_edgeuse_graph.h`); (3) a cone, sphere, torus, revolution or extrusion held as its B-spline
  part is a stand-in whose solves differ: those cases defer (`surface.cone_p1`,
  `surface.sphere`, `surface.surf_of_extrusion_p2`, `topology.brep_p7`).
- **Not wired**, still `UNPORTED(topology.vertex)`: the generic-handle callers in
  `intersect::point_class` (`GetBrep`, `IsConnectedTo`, `GetPoint`, `SetPoint`,
  `GetTolerance`, `SetTolerance`, `SetExact`, `GetScaledZero`), `geom::solution_array`
  (`GetPoint`), `geom::object` (`GetPoint`), `geom::tol` (comments),
  `surface::trim_srf_cache` (`GetTolerance`, behind `UNPORTED(topology.vertexuse)`),
  `topology::proto_topology::p1::proto` (`GetVertexuses`, `GetPoint`) and
  `query::poly` (`Get/SetPolyVertex` of an original vertex key). They hold opaque object
  handles (or `ObjectKey`s) that cannot reach a `Brep` vertex yet (as for
  `topology.edgeuse_p1` and `topology.vertexuse`); whoever gives those handles a B-rep
  should call `topology::vertex`. `geom::tools`' `GetTolerance` is wired (its 60
  deferred cases now pass).
- **After merging main (topology.face_p2, heal.gap).** `SmFace::GetUpwardFaceuse` is
  wired to `topology::face::p2::upward_faceuse`; the shim now exports each face's
  upward face use (`m_pFU`) and the runner sets `Face::faceuse` from it. Run: 5508
  cases, 4627 pass, 0 fail, 881 deferred (topology.edgeuse_p3 399, surface.surface_p3
  382, topology.face_p3 76, topology.edge 24).
