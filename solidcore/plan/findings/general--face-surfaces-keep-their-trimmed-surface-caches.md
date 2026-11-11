# Face surfaces keep their trimmed surface caches; IntersectTSBound's cached branch (PR #697, 2026-10-24)

Follows "classification face queries through `ClassGeometryMut`" (#639) and "Region
nesting once shell ray face preparation meets the forwarded face hooks".

- **Model.** A surface's cache slot (`Surface::cache_tree_state`'s `Arc`, shared with its
  evaluation copies) now also holds the face surface's `SmTrimSrfCache`
  (`TrimCacheSlot`: empty, stored, or checked out while in use).
  `query::face::p2::face_surface_cache` is `smsurf_GetSurfaceCache` through
  `SmCacheMgrTSrf::CacheMakeOrValidate`: a stored cache is reused whether it was built
  with the face or standalone, unless `m_bFaceWasModified` and editing is off (then it is
  made again); a new one is built with the face, or standalone while the owner is
  cleared, and stored. A lookup while the cache is checked out gets an unkept build (the
  old behaviour); upstream would find the same object. `PointClassify`'s
  `GetObjectCache` reads the stored cache, `SmFace::Notify(PRE_EDIT)` marks it modified.
  A face-unaware `smsurf_GetSurfaceCache` made while a face's owner is cleared keeps the
  standalone cache too. The 200-entry LRU of upstream's global cache queue is not
  modelled.
- **Consumers.** `IntersectTSBound` (cached leaf-node branch, its `SmNewMarkAndLock` and
  `GetOrCreateUVTrimCurve`), `GlobalLineIntersect` of a face-owned surface (ray path;
  the caller-held cache now only stands for a surface without face),
  `ComputeUVPoint`'s global solve (cache checked out around it, fast-drop trim test on
  the B-rep when the cache is trimmed with point testing on), `EvaluateBinormal`.
- **Marks.** The context marks and locks are atomics, so a query holding `&Kernel` claims
  a mark (`Context::claim_mark`) as upstream's `const` queries do; no `&mut Kernel`
  cascade through `CurveOnClassify`'s many callers. Brett to confirm this choice.
- **The history fixes found on the way.** `IntersectVertices` and `IntersectEdges` clear
  the surface's owner while they read the vertex/face and edge/face gaps
  (SmCurveClass.cpp:14700, 15315, `SmTemporaryChangeValue` on `m_pOwner`), so upstream's
  first cache of a heal.face_props face is standalone (C++ probe in the shim, not
  committed: after the first `SetProps`, `m_bHaveTSurfaceCache` 0, containment 0, point
  test 0, tree built, no UV trim curves). The port's gap reads now run in an
  owner-cleared scope. The classification snapshots, `HasSeamProblem`'s seam objects and
  `curve_class::p4`'s binormal and sector scratch copies share the B-rep's cache slots
  (`Brep::clone_sharing_surface_caches`), else each copy started without cache and built
  one with the face (the copy sites of #681 that this needed; the others are unchanged).
- **Harness.** The shared graph import's gap refill would now build face caches with
  UV trim curves the C++ B-rep lacks (topology.brep_p4: 61 failures, last edge use field =
  "has a UV trim curve"); the refill now runs as during a Boolean operation and drops the
  caches it kept. topology.brep_p4 also seeds the C++ read's cache states.
  intersect.curve_class_p9's ComputeUVPoint fixture attaches a point-test-off cache
  first, as its shim does.
- **Results.** heal.face_props 1431 -> 1439 pass, 97 -> 89 deferred (the 15
  surface.surface_p2 cache-history deferrals go); offset.offset_geometry_creation 832 ->
  835 pass; nothing else changes on main. Merged locally with #694: topology.brep_p6's 7
  region-nesting cases pass (Mark2 included), topology.topology_traverser stays 909/909,
  creation_p2 gen0201 and gen0214 pass. `skin_concentric_loops` still differs
  (record[7] cpp 1, rust 2: the annular cap comes
  out as two faces), so #694 keeps that known mismatch; `skin_primitive_annular` (sweep.api_surfaces) was not re-run after the last change.
