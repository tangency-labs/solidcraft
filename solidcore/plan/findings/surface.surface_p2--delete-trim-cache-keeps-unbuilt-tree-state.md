# surface.surface_p2: `delete_trim_cache` keeps an `Unbuilt` tree state; should it reset to `NoCache`? (2026-10-28)

Unit: `surface.surface_p2` (`Surface::delete_trim_cache`, `CacheTreeState`); found
through `query.face_p1` once its Rust side builds its own B-reps (#1374) on a main that
keeps the read's cache slots through the renumbering (#1381, `Surface::copy_cache_slot_of`).

**Open question.** Should `Surface::delete_trim_cache` reset `CacheTreeState::Unbuilt` to
`CacheTreeState::NoCache`, given that upstream's tree state lives in the deleted
`SM_OC_TRIMSRF` object? Today it leaves the state as it is ("the surface cache of
`SM_OC_SURFACE` keeps its own").

**Evidence.** `query.face_p1` cases gen0342, gen0625 and gen0890 (generated, seeds
4634842130399439390, ...673, ...938) all classify a point on `Endcap.smb` read with
`rebuild = true`. `values.pc[4]` (the classification's `SrcZoneTol3d`) was
cpp = the caller's `src_tol` (1e-8, 1e-6, 1e-10), rust = 1e-5 (the face tolerance).
- The C++ shim (`harness/cpp/shim/query_face_p1.cpp`, `drop_trim_cache`, called before
  `Point3DClassify`) runs `SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF, srf)`. The next
  `smsurf_GetSurfaceCache` makes a fresh trimmed cache whose tree is built (no Boolean in
  progress), so `SmFace::PointClassify` decides the point from a cache leaf node and
  leaves `SrcZoneTol3d` as the caller's.
- After the Rust `.smb` read, the face surface's slot is `Unbuilt`. Deleting only the
  trimmed cache keeps that state, so `trim_srf_cache_tree` skips the build for the new
  cache (`have_t_surface_cache = false`, face containment `NotDone`). `PointClassify`
  then takes the loop path, which sets `SrcZoneTol3d` to the face tolerance.
- Resetting the state to `NoCache` next to the delete makes all 2266 `query.face_p1`
  cases pass. The difftest runners of `query.face_p1` (Point3DClassify),
  `query.face_p3` (Internal) and `topology.face_p2` (OP_INTERNAL) now do this where
  their shims drop the cache (#1374). The library is unchanged pending this question.

**Second observation: kept read cache, lease tree not built during the global solve.**
Without the drop, `point_3d_classify` checks the face's kept (read) trimmed cache out of
its slot, then runs the global point solve. Upstream's `GlobalPointSolve` takes its tree
from the same object (`smsurf_GetSurfaceCache(this)->GetTree()` in
`GlobalPointSolve_0`), which builds it there; the Rust solve finds the slot checked out
and builds a separate, unkept cache, so the leased cache reaches `PointClassify` still
without a tree. Building the lease's tree (`face_cache_tree`) before the global solve
fixed gen0342 and gen0625 but not gen0890 (a UV guess; the local solve takes no tree).
Not changed; relevant wherever a kept trimmed cache is leased across a surface solve.
