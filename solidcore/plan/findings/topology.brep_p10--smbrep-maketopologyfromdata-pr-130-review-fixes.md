# topology.brep_p10 (`SmBrep::MakeTopologyFromData`, PR #130): review fixes

- **UV trim curve cast.** Upstream stores an edge use's UV trim curve as
  `pNewEU1->m_pUVTrimCurve = SM_CAST_PTR(SmBSplineCurve, pNewUV)`, which is NULL for a
  copy that is not an `SmBSplineCurve` (composite, offset, projected, curve-on-surface,
  ...). The copy is still made, gets the attributes and gets its owner set, but is not
  stored. The first port always stored it. A version-38 file whose UV curve 0 is an
  `SmCompositeCurve` (`fixtures/topology.brep_p10/uv_composite_v38.smb`) showed the
  difference in the exported value stream; fixed.
- **Coverage.** The upstream `.smb` files contain no valid wire-edge shells, vertex
  shells, vertex loops (loop type 1 with an eu_type 1 record) or analytic edge-curve
  replacements. Crafted fixtures `wire_vertex{,_nm}.smb`, `wire_quad_{full,sub,rev_nm}.smb`
  (a collinear quadratic edge curve with full / sub-range / reversed interval, so both
  outcomes of the `CreateAnalyticCurve` end-point check run) and `uv_bspline.smb` cover
  them (`review_cases.json`, 128 cases). They use axis-aligned planes, so the analytic
  paths are compared instead of being deferred by `SmCone::IsNurbSurfaceCone`
  (surface.cone_p2).
- **Known harness limits.** The shim refuses the closure / remove-all / project ops
  after a `Set` patch: its guard proves only that `MakeTopologyFromData`'s record
  indexing is defined, not that the graph it builds is consistent enough for the later
  walks (an inconsistent loop or radial list can make them loop or read freed
  objects). The generator never emits `Op::Project`; add it once topology.brep_p1
  (`CreateParallelProjectionCurves`) lands. An `SmConic` UV curve cannot reach the
  cast: `SmConic` has no `Copy`, so both sides fail at the copy and agree.
- **Second review (2026-10-19, after topology.brep_p1 #145 merged).** Main's `Brep` now
  holds `m_bEditingEnabled`, `region_list_head` and the face/edge/vertex lists; this PR's
  duplicate `editing_enabled` was dropped and `Brep::new_with_infinite_region` sets
  `region_list_head` to the infinite region (`PreInsert`). The six
  `UNPORTED(topology.brep_p1)` sites now call `p1::add_attributes` (new
  `TopologyEnv::all_attributes` = `rAllAttributes`; the B-rep, loop uses and face uses
  receive their own lists), `p1::heal_brep(.., HealerOpType::All, None, None, None,
  bMakeUVTrimCurves)`, `p1::regions`, `p1::faces`, `p1::wire_edges`,
  `p1::create_parallel_projection_curves` (approx tol `&dTol`, NULL projection distance,
  all three outputs) and `p1::find_side_of_curve`. The HealBrep cases now defer at
  `SmFace::GetTolerance` (topology.face_p2, 63). topology.brep_p7's two
  `UNPORTED(topology.brep_p10)` sites are wired: `ReadFromFile` (now `&mut Kernel` plus a
  `TopologyEnv`, with an empty `sAttributes` as upstream) and `GlueEdgesGeneral`'s
  `TestClosureSplitRegion(TRUE, edge)`; its 30 brep_p10 deferrals now defer at
  surface.cone_p2 (24) and topology.face_p2 (6), 0 failed. Unit totals: 1404 cases, 839
  passed, 0 failed, 565 deferred, `--perturb` SENSITIVE 839/839.
- **Open (not fixed here).** (1) Geometry attributes are not copied when a shared data
  surface/curve or a borrowed geometry is copied (upstream's `SmAObject` copy constructor
  copies them; `a_object::copy_construct(env.attributes, from, to)` would match). The
  harness cannot see it (ReadFromFile reads no attributes, the shim exports none) and no
  unit calls `copy_construct` yet, so this needs a codebase-wide decision. (2) The shim
  export walks only from the regions: face uses on no shell and vertices with no uses are
  never compared; exporting `m_pFaceListHead` / `m_pEdgeListHead` / `m_pVertexListHead`
  in list order would close that. (3) The topology.vertex, topology.edgeuse_p1/p2 and
  topology.loopuse runners still label their inexact `.smb` imports
  `topology.brep_p10 SmBrep::MakeTopologyFromData` (harness deferrals, not code stubs);
  they could now read the file through `p7::read_from_file`. (4) topology.brep_p7's
  `cpp-guard:unvalidated-topology` read guard (UB-164) is still in place.
