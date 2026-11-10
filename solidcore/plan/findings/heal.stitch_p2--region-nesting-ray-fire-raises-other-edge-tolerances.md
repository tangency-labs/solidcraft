# heal.stitch_p2: DoStitching's region nesting raises different edge tolerances (2026-10-27)

- **Units:** `heal.stitch_p2` (`SmStitch::DoStitching`), `heal.stitch_p1`
  (`SmApiStitch`, `SmApiStitchAdvanced`, which reach the same call). The divergence is
  in the region-nesting step after the stitch, not in the stitch itself: `SmBrep::
  FindAndSetInfiniteRegion` -> `FindInfiniteRegion` -> `SmBrep::RayFire` and the face
  classification it runs (units `topology.brep_p9`, `topology.brep_p2`, `query.brep`).
- **Inputs:** six unstitched planar quads of a unit cube
  (`fixtures/heal.stitch_p2/coverage.json`, `cov_cube*`), each corner moved by exactly the gap in a
  random direction; it shows when the gap (1e-5, 3e-5) is at or above the default edge
  tolerance 1e-5. Smallest case: `cov_cube_gap_at_edge_tol_nest` (`DoStitching`, tol
  1e-3, no edge splitting). Same B-rep with `nesting: false`
  (`cov_cube_gap_at_edge_tol_no_nest`) and the vertex-pair stitch alone
  (`cov_cube_gap_at_edge_tol_vertex_pairs`) agree bit for bit, so vertex and edge gluing
  agree and the difference arises while the infinite region is found.
- **Values** (`cov_cube_gap_at_edge_tol_nest`, list order of `GetEdges`/`GetVertices`):
  topology, counts, vertex points and the stitch outputs agree. C++ raises 8 of the 12
  edge tolerances during the ray fire, Rust 6: edge (0,0,1)-(0,1,1) C++
  `2.864833679468755e-5`, Rust `1e-5`; edge (0,1,0)-(0,1,1) C++ `3.270363227231925e-5`,
  Rust `1e-5`; and through the edge-to-vertex cascade vertex (0,0,1) C++
  `2.864833679468755e-5` vs Rust `2.0000000000014252e-5`, vertex (0,1,0) C++
  `3.89043892474386e-5` vs Rust `2.9452194623719303e-5`, vertex (0,1,1) `3.270363227231925e-5`
  vs `2.95675194961357e-5`. The other six raises (all four edges of the x=1 face, and the
  y=0/z=1 and y=1/z=1 edges) agree exactly.
- **Other cases:** in `cov_cube2/4/5_*` (gap 1e-5 at sizes 0.1 and 10, one face split
  in two so its neighbours' edges are split at the T-vertex) the same tolerances differ;
  in `cov_cube4_*` one split edge's interval start also differs (C++
  `0.5000004048922745`, Rust `0.49999939290479195`): `GlueEdgesGeneral` with nesting
  fires the same ray inside the stitch loop, so later splits read the diverged
  tolerances. Each cube's `do_sf_no_nest` case (the `StitchFaces` flags, nesting off)
  agrees bit for bit, cube4 included.
- **What the Rust side does:** one ray, from beyond the box corner (1.5,1.5,1.5) toward
  the closest sampled edge point (round 0, every edge below `1.01 *` B-rep tolerance),
  succeeds on the first fire; it enters near the x=1/z=1 edge and leaves through y=0.
  The edge tolerances it raises are those `SetUVTrimCurve` raises for the UV trim
  curves the classification makes. C++ also raises the two x=0 edges of the z=1 and
  y=1 faces, i.e. it makes UV trim curves for every edge use of those faces where Rust
  makes them for some (`query::brep::ray` prepares trim curves per request).
- **Suspected cause:** the set of edge uses whose UV trim curves the ray-fire face
  classification creates differs (lazy per-request preparation on the Rust side vs
  upstream's face classification). Not confirmed on the C++ side (the library path is
  not instrumented by the stitch shims); it needs a `topology.brep_p9` /
  `query.brep` case that compares the B-rep after `FindInfiniteRegion`, not only the
  region found.
- **Status:** the 22 affected cases are known mismatches in
  `crates/difftest/known_mismatches/heal.stitch_p1.toml` and `heal.stitch_p2.toml`.
- **Also found:** `Solids/fillet_ts.smb` (the `my_stitching_demo` input) is not used:
  reading it heals through UB-255
  (`plan/upstream-issues/UB-255--smhealdata-fix-coinvertices-rewrites-sprobarray-with-a-freed.md`),
  so its C++ export changes between reads in one process and even in a fresh child.
