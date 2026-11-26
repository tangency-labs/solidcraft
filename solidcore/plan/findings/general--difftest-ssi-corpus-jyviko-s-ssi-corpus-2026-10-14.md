# difftest `--ssi-corpus` (jyviko's SSI corpus, 2026-10-14)

- **Correctness report, not a gate.** `--ssi-corpus` grades both engines against the exact
  ground truth of jyviko's corpus (`corpus/ssi/`, fetched by `scripts/fetch-ssi-corpus.sh`).
  At approximation tolerance 1e-6 the C++ reference scores PASS=81, INCOMPLETE=25,
  WRONG TOPOLOGY=11, ERROR=2, the same as the corpus' published SMLib figure (81/119). The
  38 misses are in `plan/upstream-issues.md` WR-74 to WR-80. A faithful port should score
  the same. The Rust scorer (`crates/difftest/src/ssi/`) gives the same verdict and the same
  flags as the corpus' `score.py` on all 119 C++ outputs. Distances agree to roundoff
  (1e-16 to 1e-12 absolute).
- **Plane NURB deviation found.** Coincident planes took a different branch in Rust and in
  C++. This is a port deviation of `surface.plane_p1`, recorded under that unit's
  heading above (newest entry). The corpus harness builds each plane's NURB
  (`Plane::make_nurb`) before intersecting, which mirrors the C++ runner's state after
  `GetNaturalUVDomain`; `c6_planes_identical` / `c6_planes_overlap` are then deferred on
  `intersect.surface_intersector_p5`.
- **Rust coverage at the time of writing:** 3 cases answered (plane/plane, identical to the
  C++ and PASS), 116 deferred. 62 are deferred on `SmSurfaceIntersector::DoIntersection`
  (`intersect.surface_intersector_p5`), 24 on spheres (`surface.sphere`), 21 on cones and
  cylinders (`surface.cone_p3`), 9 on tori (`SmTorus` is ported but has no `SurfaceKind`
  variant yet; deferred on `surface.surface_p2`, which owns the enum).
