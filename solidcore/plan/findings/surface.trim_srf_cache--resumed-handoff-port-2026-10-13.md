# surface.trim_srf_cache: resumed handoff port (2026-10-13)

Resumed the interrupted `152e005` WIP under the draft-PR coordination workflow.
The constructor and type/state queries, PointTest bypass, and edge-use insertion
were compared with the pinned C++ reference: 620 cases (20 fixtures and 600 seeded
cases), 458 bit-identical, 0 failed, 162 deferred. The perturbation check detected
all 458 perturbed outputs. The touched surface.surface_cache_p1 runner also passes
its non-deferred cases (872 total, 709 identical, 163 deferred).

Deferred paths: surface.surface_cache_p3 (BuildTree, FindUVNodes, SubdivideNode;
103 cases), surface.surface_p1 (GetFace; 33), surface.surface_p3 (GetSingularities;
26). ImplantVertexuse and ImplantPolyEdge have translated bodies but cannot yet
execute past Evaluate1stDerivatives / MapTo2d (surface.surface_p1); Rust regressions
check that these explicit deferrals preserve state. They are not claimed as
C++-verified. The base MarkFaceContainment dispatch remains unported before it
could reach the trim-specific PointClassify; it must use that override when wired.
The face-build path preserves the temporary brep-editing guard through explicit
unported topology accessors, restoring the flag on both success and failure. These dependencies prevent end-to-end trimmed-face use.

Fixtures exercise prepend order, shared boundaries, degenerate and curved trim
curves, existing face extents and the deferred subdivision/pole/face paths. File
fixtures use Surface.sms and offset_nurb.sms from the allowed upstream corpus.
The C++ shim deliberately excludes classifying a surface without a face: upstream
would dereference null. FFI regression tests reject this path, truncated/trailing
records, invalid counts and non-finite coordinates before entering C++.

### surface.trim_srf_cache: review follow-ups (2026-10-13)

- **Test gap closed.** The generator's negative-area face extents for
  `ImplantEdgeuse` were rejected by the FFI validator (`rd.domain()` requires ordered
  bounds) and silently redrawn, so C++ never saw them. The validator now only checks
  finiteness; difftest is 620 cases, 462 bit-identical, 0 failed, 158 deferred
  (surface.surface_cache_p3 100, surface.surface_p1 32, surface.surface_p3 26);
  `--perturb` detects 462/462.
- **MarkFaceContainment dispatch gap (no UNPORTED marker possible).**
  `TrimSrfCache::build_tree` calls `SurfaceCache::mark_face_containment`, which uses
  the base `SurfaceCache::point_classify` (always `SM_ERR`). Upstream
  (`SmTrimSrfCache.cpp:343`) reaches the virtual override `SmTrimSrfCache::PointClassify`
  (`:1048`). That override belongs to surface.trim_srf_cache itself
  (`harness/callgraph/units.json`) and is already ported as
  `TrimSrfCache::point_classify`, so an `UNPORTED(<unit>)` marker would point at the
  wrong porter. The call site carries a `DISPATCH GAP` comment instead. It is
  unreachable today (the face path returns `Unported` earlier, at
  `CreateUVTrimCurves`), but whoever wires topology.face_p3 / topology.face_p4 here
  must make `mark_face_containment` dispatch to the trimmed override (e.g. take a
  classifier argument) before any face-backed case can pass.
