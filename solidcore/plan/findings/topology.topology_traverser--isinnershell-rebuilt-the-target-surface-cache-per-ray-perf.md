# topology.topology_traverser: `IsInnerShell` rebuilt the target surface cache per ray (perf, 2026-10-21)

- **Symptom.** Case `adversarial_02_op7_00` (`fixtures/topology.topology_traverser/traversals.json`,
  op 7 `IsInnerShell` on one face of a 3x3 biquadratic B-spline whose weights are all
  `-f64::MAX`) took ~410 s on the Rust side against ~4 s for the C++ side. The outputs agree.
- **Cause.** Upstream `SmSurface::GlobalLineIntersect` takes the surface's cache from
  `smsurf_GetSurfaceCache` (`source/SMLib/src/SmSurface.cpp:3749`,
  `SmCacheMgr::GetOrCreateObjectCache`): the surface keeps one cache, so `IsInnerShell`
  (`SmTopologyTraverser.cpp:1239`) builds it on its first ray and reuses it for the
  rest. The port built a fresh cache for every ray
  (`Surface::base_global_line_intersect_in` -> `get_surface_cache`). That is 48 ray/face
  intersections here (2 face uses x 12 rays x 2 targets), and each one ran a trimmed
  surface cache build and then a plain cache build.
- **Why the build is so slow on this surface.** The tolerance test never passes on the
  degenerate patches, so `SubdivideToTolerances` splits until the
  `MinimumSideLengthRatioUV` stop. Upstream builds 1,411,883 patches (~3.5 s, 1.4 GB RSS,
  measured with a standalone program linking `bin/cpp/lib`) and reuses them. The port stops
  at `MAX_SUBDIVISION_NODES` (`2^20`, `surface/surface_cache/p3.rs`, a documented
  memory bound) with `InvalidInput`, so each build fails after ~4.3 s. This gives `SM_ERR_NULL_POINTER`
  from `GlobalLineIntersect`, and `IsInnerShell` skips the face. Upstream's build succeeds
  (a standalone surface `GlobalLineIntersect` from the first sample point returned one
  solution). The case still agrees on both sides: neither one counts a hit
  beyond the face tolerance.
- **Fix.** `IsInnerShell` keeps each target face surface's cache for the whole call
  (`HeldSolveCache`, `Surface::global_line_intersect_held`,
  `topology::face::p4::global_line_intersect_held`), as upstream keeps it on the surface.
  The case now takes ~9 s: one failing pair of builds per face.
- **Deviation.** A failed build is kept and its error returned again. Upstream retries
  the build on every call. The retry is deterministic, so it fails the same way.
- **Open.** The node cap remains a real difference from upstream on this input
  (upstream's tree is 1.35x the cap). Raising it would cost >1.7 GB per cache in Rust.
