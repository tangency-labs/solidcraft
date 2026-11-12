# topology.edgeuse_p1 (`SmEdgeuse` / `SmFaceuse` part 1, 2026-10-14; partial, no disagreements)

- **B-rep import.** No Rust B-rep reader or builder exists yet (units `topology.brep_*`,
  `topology.proto_topology_*`). The shim builds real `SmBrep`s (15 upstream `.smb` files
  through `SmBrep::ReadFromFile`, optionally with `RebuildUVTrimCurves`; wire edges and
  faces from B-spline curves/surfaces and `SmPlane`s; the wire network of prog_test
  `my_test_topology`) and exports the graph in traversal order from raw members
  (pointer-to-member access for the protected ones), never through the functions under
  test. The runner imports it into `topology::Brep` and compares the ported functions'
  results as traversal indices. UV trim curves are imported exactly: they are often
  `SmLine`s (upstream converts linear UV curves), whose evaluation differs from a plain
  B-spline's by an ulp (`Mouse.smb` gave `0` vs `2.2e-16` before lines were imported as
  `Line`s, via `Line::from_parts`). Edge 3D curves of other kinds and analytic
  `SmBSplineSurface` subclasses (cone, sphere, torus, revolution, extrusion) are imported
  as their B-spline part, so any case that evaluates them is deferred (see the second
  review below).
- **Not wired**, still `UNPORTED(topology.edgeuse_p1)`: `surface::trim_srf_cache`
  (`SmEdgeuse::GetUVTrimCurve` of the cache's edge uses), `intersect::point_class`
  (`SmEdgeuse::GetBrep`, `GetEdge`) and the `SmTol` object dispatch in `geom::tol`. Those
  callers hold opaque object handles and no `Brep`, so the edge use record cannot be
  reached; wiring needs them to carry the B-rep (or a resolver) first.
- **Co-ports.** The use records (`Region`, `Faceuse`, `Loopuse`, `Edgeuse`, `Vertexuse`),
  the new skeleton fields and the small accessors of `topology.edge`, `topology.loopuse`,
  `topology.vertexuse`, `topology.shell` and `topology.proto_topology_p1` are minimal; an
  owner's intrusive circular list is a `Vec` in list order from its head, and an edge use's
  `m_pNext` / `m_pLast` are its neighbours there. Their porters extend these (see the
  CO-PORTED notes in `plan/modules.toml`).
- **Review fixes (PR #55).** FFI guards widened to what the C++ handles: values up to
  `1e300` (was `1e6`) and `uv` within 100 domain lengths of a B-spline surface's domain
  (plane `uv` up to `1e6`). Every previously rejected huge-scale or far-out-of-domain case
  tried (points to `1e200`, nets scaled `1e6`/`1e8`, `uv` 50 domains out) agrees exactly.
  The Seam case generator copied curve 0's knots onto curve 1 of a different degree
  (invalid curve; `--seeds 6000` case `gen5286` was a harness error); it now requires
  equal degrees.
- **Review fixes (2026-10-16, follow-up to PR #55).** Since `heal.gap` and
  `surface.surface_p5` were wired, `CalcMaxEdgeFaceGap` / `CalcEdgeCCWEdgeGap` really
  evaluate the edge curve and face surface, but the runner compared only statuses and
  `IsInit` flags. Comparing the values (`gap_3d`, `edge_t`, `face_uv`; `edge_t0`,
  `edge_t1`, only for gaps the op recomputes) gave 16 failures on the upstream `.smb`
  fixtures (Mouse, cone, disk, surf_extrude, surf_rev, torus, toruswvy, vase_rev
  `_gaps0`/`_gaps2`, e.g. `disk_gaps0` face_uv C++ `(-0.5,-1.7535)` vs Rust
  `(-1.7535,0.5)`; `torus_gaps0` edge_t C++ 60 vs Rust 85). Two harness causes, no
  library defect: (1) an `SmPlane` was exported as origin + normal and rebuilt with
  `Plane::from_normal`, whose UV frame is rotated 90 degrees from the C++ one; the shared
  `sc_edgeuse_graph.h` now exports origin, X/Y axes, UV scale and STEP domain and the
  import builds the exact `Plane` (as `sc_topology_fixture.h` does); (2) cases evaluating
  geometry imported as its B-spline part were not deferred; `inexact_geometry` now defers
  them (gap ops on loop edge uses, the 3D-drop branch of `GetStartPointUVLoopGap`) to
  `surface.cone_p1`, `surface.sphere`, `surface.surf_of_extrusion_p2` or
  `topology.brep_p7` (relabelled to `topology.brep_p10` `MakeTopologyFromData` by PR #155),
  with the mapping of `topology.edgeuse_p2`. With the exact import,
  every computed gap agrees bit for bit. `fixtures/topology.edgeuse_p1/review_gap_values.json`
  (1680 cases) keeps them compared. The units sharing the import (`topology.edgeuse_p2`,
  `topology.loopuse`, `query.api_queries`, `query.edge`, `query.face_p2`) give unchanged
  results. Open: `calc_max_edge_face_gap` uses `context_xsect_tol_3d(k, None)` (upstream
  takes the edge use's, edge's or face's context); revisit once B-rep records carry one.
