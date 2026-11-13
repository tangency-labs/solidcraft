# ClassifyFaceInterval's PointClassify wired; classification reads snapshot tolerances (2026-10-24)

`SmFace::PointClassify` from `SmCurveClassification::ClassifyFaceInterval`
(`intersect::curve_class::p7`) now calls `query::face::p2::point_classify`. On the mutable
path it runs on the B-rep the face handle resolves to. On the read-only path it runs
`point_classify_prepared` over the unchanged topology (no copy), and a needed UV trim still
defers. Most of the deferrals reported at `SmFace::PointClassify`, and many reported at
`SmEdge::GetCurve` / `SmEdge::GetTolerance` (`topology.edge`), were not unported callees:
they were dispatch gaps. Two adapters could not resolve their face handle:
`topology::face::p3::IntersectObjects` (FaceIntersect, also used by
`MergeSelfIntersectionCurves`) had no `topology_entity`, and `CalculateAnInternalPoint`
built its `UvLineObjects` with `resolve_topology: false`. Both now resolve.

- **Tolerance read after the trimmed cache build.** `topology.face_p2` `extra00702`
  (`CalculateAnInternalPoint` on a 0.003 x 0.004 B-spline face) then disagreed: cpp
  `(8.50e-5, 1.96e-4, -2.53e-4)`, rust `(1.62e-3, 8.40e-4, 4.22e-5)`. C++ (shim
  instrumented, not committed) classifies the UV line at `v = 1/21` with its crossing at
  `u = 0.0482` snapped to the corner vertex `(0, 0, -3.19e-4)`. That vertex's tolerance
  has been raised from 1e-5 to 8.05e-4 by the face's trimmed cache build
  (`smsurf_GetSurfaceCache` in `IntersectTSBound`). The port builds that cache on the
  classification snapshot, but `sm_InsertSolutions`' edge-end vertex test read the
  tolerance from the caller's unchanged B-rep (1e-5), so the crossing stayed an edge
  hit. `WithMadeTrimCurves::topology_zone_tol_3d` now reads the face's B-rep entities
  from the snapshot, as upstream reads the B-rep it just edited. The case passes, and
  `topology.face_p2` is green.
- **topology.brep_p1 BrepIntersect.** The shim refused `BrepIntersect` on a B-rep with
  faces (`-3`, "the Rust side defers"). Once the Rust FaceIntersect ran, the 10
  `*_brep_intersect` cases failed (cpp status only, rust curves). The shim now runs
  upstream's `BrepIntersect`, and the Rust side intersects the B-rep with itself (as the
  shim does), not with a clone. 20 of 21 pass; 1 defers (`topology.face_p2`).
- **Still deferred (dispatch gaps, not unported):**
  - `intersect.topology_solver_p2`: 4 cases at `SmFace::PointClassify`. The solver's
    `TopologyObjects` cannot resolve the face handle (`intersect/topology_solver/p1.rs`
    `face_point_classify`).
  - `boolean.merge_p4`: 66 cases (and `boolean.merge_p3` 4) moved from
    `SmEdge::GetTolerance` to `SmFace::PointClassify`. The merge arena-pair adapter
    (`boolean/merge/p4/arena_pair.rs`) now reaches the face queries but cannot resolve
    the face handle or a classified entity for `PointClassify` (`unresolved_uv` in
    `intersect/curve_class/p7.rs`). That file is in an open PR, so it is left for a
    follow-up.
