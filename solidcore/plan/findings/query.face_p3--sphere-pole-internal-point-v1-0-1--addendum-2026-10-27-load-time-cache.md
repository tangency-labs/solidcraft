# query.face_p3: sphere pole internal point resolved: a load-time healer cache in the C++ harness (2026-10-27)

Updates: plan/findings/query.face_p3--sphere-pole-internal-point-v1-0-1.md

- **Units:** `query.face_p3` (`internal_sphere_f0`) and `topology.face_p2` (`extra00744`, `extra00806`, `extra01154`).
- **Resolution:** this was a harness gap. C++ 1.0.1 and the port are both correct, and the C++ result is not a defect.
- **Cause:**
  - In the C++ shims, reading `sphere.smb` (`SmBrep::ReadFromFile` -> `MakeTopologyFromData` -> `HealBrep`) runs the healer.
  - The healer's `HealTgts` -> `Fix_InfiniteRegion` -> `FindAndSetInfiniteRegion` -> `RayFire` -> `BrepPointSolve` -> `SmTLSPointFace::SolveIt` -> `SmSurface::GlobalLineIntersect` -> `SmSurfaceCache::GetTree` builds the `SmTrimSrfCache` of each face surface the ray fire reaches while `m_bEditingEnabled` is TRUE (three `BuildTree` calls on `sphere.smb`, face 0's among them).
  - So `SmTrimSrfCache::BuildTree` takes its editing branch. It returns `SM_SUCCESS` with a subdivision tree but `m_bHaveTSurfaceCache = FALSE` and `m_bFaceContainmentDone = FALSE`: no UV trim curves and no containment.
  - The cache stays on the surface.
  - The Rust runners import the B-rep from the C++ export, which carries no caches.
- **How the two runs diverged:**
  - On `CalculateAnInternalPoint`'s first UV line (V = Vmin, the line through the pole (0, 0, 5)), C++ reuses the stale untrimmed cache, so `SmCurveClassification::IntersectTSBound` takes its no-trimmed-cache branch.
  - That branch walks every vertex use of the face. It reaches the far-pole vertex use first: vertex (0, 0, -5), `ComputeUVPoint` (0, 15.3073).
  - The 3D drop of that vertex onto the lifted line misses (distance 10). The global solve then returns `SM_ERR_NULL_POINTER` (1006), which `SER` propagates. `CurveOnClassify` fails (`SM_ERR`, 1001) with nothing inserted, and `CalculateAnInternalPoint` (status ignored) moves on to line 2, whose first FACE interval gives (-0.659, -0.203, 4.952).
  - The port builds a fresh trimmed cache with editing off and takes the cached branch. That branch only meets the near-pole vertex use, Vertex(1) at (0, 0, 5), UV (30.6147, 0). Its drop succeeds at t = 0, the line's ends become Vertex(1) with a FACE mid, and the result is the pole.
  - Both vertex UVs are correct for their vertices.
- **Evidence:** gdb traces gsph8 to gsph11 on the pin101 opt-0 build against cpp-101 (gsph10: fixer-2's run showed `FindUVNodes` never fires in C++ on lines 1 and 2; fixer-3's showed the cache flags):
  - the C++ `BuildTree` backtraces with brep editing = 1;
  - the cache flags at `IntersectTSBound` entry;
  - `FindUVNodes` never called in C++;
  - the visited vertex uses and drop results on both sides.
- **Correcting the original record:**
  - The `IntersectUVDiscontinuties` solutions and "rejected" inserts traced earlier belong to `SmCrvOnSurf::GetKnots`' private classification, not to the face classification.
  - The 1.0.1 hunks on this path are all ported: the `IntersectUVDiscontinuties` slot index, the `SmVertexuse::ComputeUVPoint` seam and missing-seam changes, the `ComputeUVSector` orientation count and the `GetVertexuseOfFace` face filter.
  - Why the C++ value moved between the old pin c9c5979 and v1.0.1 was not re-traced (unverified). The likely cause is that the 1.0.1 healer changes (`SmHealData.cpp`) now reach `Fix_InfiniteRegion`'s ray fire, and with it the cache build, on this file.
- **Fix:**
  - `harness/cpp/shim/query_face_p3.cpp` (`drop_trim_cache`) and `harness/cpp/shim/topology_face_p2.cpp` (op `internal`) delete the face surface's `SM_OC_TRIMSRF` cache before `CalculateAnInternalPoint`, as `query_face_p1.cpp` and `query_face_p2.cpp` already do with load-time incidental caches. Both sides then start without a cache.
  - No kernel change.
  - The four cases leave `crates/difftest/known_mismatches/`.
- **Scope:**
  - The drop applies to op `internal` only. Op `tight` (`CalculateTightBoundingBox`) still runs on the load-time cache in C++; if tight cases on healed files ever diverge, check this first.
  - With the drop, these cases no longer compare `IntersectTSBound`'s loop-use branch as reached through a stale cache. That branch is still reached wherever no trimmed cache exists.
- **Not needed:** a `p9_vertex` drop-error change.
  - `CurveClassification::p9_vertex` already propagates the drop's error with `?`, as upstream's `SER`.
  - The port's global point solve already maps a missing curve cache or tree to `null_pointer`, as upstream's `NER(pCC)` / `NER(pCurveTree)`.
- **Tolerance:** unchanged.
- **PR:** `fix/sphere-pole-load-time-cache`.
