# trimmed surface cache history: tolerance-growth deferral removed (2026-10-24)

Follows "classification face queries through `ClassGeometryMut`" (#639). The mutable
face queries of `intersect::curve_class::p7` (`FindVertexSector`,
`EdgeCurveOnClassify`, `PointClassify`, `Point3DClassify`) deferred on
`smsurf_GetSurfaceCache (trimmed surface cache history: tolerance growth)`
(`surface.surface_p2`) whenever the query raised a vertex or edge tolerance: 230 cases on
main a5dc2be24 (intersect.curve_class_p4 169, query.face_p3 36, heal.face_props 15,
query.face_p4 5, heal.heal_data_p1 4, topology.brep_p1 1).

- **The check is obsolete.** It was written when the port kept no object caches. The
  surface's cache slot now keeps the trimmed cache upstream keeps on the surface
  (`Surface::take_trim_cache` / `store_trim_cache`; a standalone cache made while
  `IntersectVertices` / `CreateUVTrimCurve(s)` clear the owner is kept,
  `keep_standalone_trim_cache`), so `query::face::p2::face_surface_cache` reuses a
  cache made earlier and builds one with the face (UV trim curves, tolerance growth)
  only when upstream does. Checked against the C++:
  - shim probe (`face->GetSurface()->GetCacheObj()`, edge tolerances after each op) on
    heal.face_props gen0010 / gen0646 / gen1312: upstream builds an `SmTrimSrfCache`
    with the face (`m_bHaveTSurfaceCache = 1`, `m_bFaceContainmentDone = 1`) in the
    first op and raises the edge tolerances (gen1312: 6.01084, 6.02664, 6.83397,
    7.76739); the port's classification snapshot holds exactly those tolerances and
    UV trim curves when the next face query runs;
  - gdb on the C++ reference (break on `SmTrimSrfCache::SmTrimSrfCache` and
    `SmFace::CreateUVTrimCurves`): the first cache of gen0646 / gen1312 is made in
    `SmEdgeuse::EvaluateBinormal` <- `SmFace::SectorClassifyCurveOn` <-
    `FindVertexSector` <- `ClassifyFaceInterval` <- `CurveOnClassify` <-
    `HasSeamProblem`, with one `BuildTree` -> `CreateUVTrimCurves`, the same order as
    the port's `face_surface_cache` calls.
- **Removed** the `Tolerances` before/after check in `p7` (`Mutable` face queries). Linux
  difftest, before -> after: query.face_p3 1163 pass / 36 deferred -> 1199 pass (green);
  intersect.curve_class_p4 3032 / 169 deferred -> 3195 pass, 6 known mismatches;
  heal.face_props 1513 / 15 deferred -> 1525 pass, 3 known mismatches; query.face_p4
  493 / 5 deferred -> 498 pass (2 earlier known mismatches); heal.heal_data_p1 456 / 11
  deferred -> 458 pass, 7 deferred (`primitive.api_primitives`), 2 new known
  mismatches; topology.brep_p1 2587 / 15 deferred -> 2587 pass, 14 deferred, 1 new known
  mismatch. No other unit changed (Mac `--green` compared with main's Linux reports;
  the differences are the documented macOS ones).
- **The 12 remaining disagreements are not cache history** (listed in
  `crates/difftest/known_mismatches.toml`):
  - heal.face_props gen0646, gen0790, gen1312 and intersect.curve_class_p4 gen1570,
    gen1622, gen1841, gen1876, gen1968, gen2061 (all `SmSurfOfRevolution` torus faces
    with face tolerance 1000 and line-edge loops far off the surface): the seam
    problem flags of `SetProps_Stage2` differ (`m_bBadMissingSeam` /
    `m_eBadMissingSeam`, or crossed / near miss). gen1312: both sides classify the V
    seam into the same 5 intervals with the same vertex ends, but C++ classifies
    intervals 2-5 `SM_PC_FACE` and the port `SM_PC_OUTSIDE`: every
    `FindVertexSector` of the V seam returns not inside in the port, run on a B-rep
    whose edge tolerances and UV trim curves equal the C++ ones (above). Suspected
    cause: `FindVertexSector` / `SectorClassifyCurveOn` (`topology.face_p4`,
    `query.face_p3`) for a vertex far (about 6 units) off the surface, within the
    raised edge tolerance; not investigated further.
  - heal.heal_data_p1 file_Endcap, file_Endcap_rebuild_reverse: `out[215]` C++
    -1.23456e-61 (the runner's unset sentinel), port 7.692503008378702e-4.
    topology.brep_p1 Endcap_heal: `out[234]` C++ 1 / port 0, `out[238]` C++ 0 / port 1
    (a healed topology record). gdb: the C++ makes all 8 Endcap trimmed caches in
    `SmHealData::HealTgts` -> `Cache_VertexProps` -> `SmVertexuse::ComputeUVPoint`
    (global fallback) without `CreateUVTrimCurves`, which the port's
    `global_point_solve` -> `face_surface_cache` also does. Cause not found.
- **Not done:** the seam check (`topology::face::p4::has_seam_problem`) reads the
  caller's objects (`BrepSeamObjects` over the caller's B-rep) for the V seam and for
  its post-classification analysis, while upstream reads the one B-rep the U
  classification has changed (raised tolerances, UV trim curves). Routing those reads
  through the classification snapshot (tried: a wrapper reading tolerances, trim
  curves, gaps and topology from a copy of the snapshot) changed no case in the six
  units above, so it is left out.
- `surface.surf_of_extrusion_p1` (HANG-23 guard follow-up, 2026-10-24): the
  in-flight centre-cap sketch (`|c| > 1e8`) was replaced after probing the C++
  directly (`sc_surface_surf_of_extrusion_p1_eval`, 8 s timeouts): the hang is a
  minor-radius razor, not a centre-distance one. An ellipse extrusion with
  `rx = 1` and `ry` below `1/DBL_MAX` (5.562684646268003e-309) hangs C++ on
  `AdjustSTEPUVDomain` for every centre except exactly `0` (`0/0.25/1e-16/0.001/
  1/2/4/1e10/1e300 pounds all hang at ry = 5e-324; ry is fine for any one
  centre >= 5.6e-309, including 1e300; `rx / ry` finite never hangs; a denormal
  MAJOR radius with a normal minor one never hangs; circles with denormal
  radii never hang). The hang is chaotic inside the subnormal centre edge:
  `c = 6e-308` and `6.5e-308` pass while `6.1/6.3/6.7e-308` hang, and
  `c = 1e-300` hangs while `1e-309` passes (with `ry = 5e-324`) — so no closed
  predicate separates hang from pass inside that sliver; the guard rejects the
  whole `rx / ry == inf && centre != origin` rectangle, which keeps every
  probe-passing conic with resolvable radii (the accepted sliver's 21-order
  noise regime included) documented here. Base counters unchanged (2071 cases,
  2000-draw fuzz clean: rejected 337, ran 903, 0 timeouts/0 differs).
