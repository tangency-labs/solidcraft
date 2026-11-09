Updates: WR-751

#### WR-751 (update, 2026-11-05). Six difftest cases that passed before the nearest-point split now show the defect

The fix for WR-751 (tangency-labs/solidcore#2735, merged as f06f897ea: `split_parameter` splits
the edge where its curve passes nearest the classified point when that is nearer than the
classification's `TParam` and within the edge's tolerance) moved six difftest cases that had passed
against the C++ (CI run 41988264776 passed them all, run 41991964300 on #2735 fails them; both
reproduced locally against the pinned reference). In each the C++ answer is the defect this record
describes: it trusts `TParam` where the point is not the edge's point at `TParam`. The cases are
listed with `cpp_defect = "WR-751"` in `crates/difftest/known_mismatches/<unit>.toml` (parked).

- **Confirmed:** yes, by the six cases below, each compared against the pinned C++ build.

**`topology.brep_p9`** (`MergeIntoObject` on a wire of one line edge from (0,0,0) to (1,0,0),
interval [0,1], the point's and the object's zone tolerances 0; the fixture gives a `TParam` that is
not the point's parameter):

- `point_merge_6_same_point`: point (0,0,0), `TParam` 0.5. The C++ splits the edge at 0.5 (edges
  [0,0.5] and [0.5,1], `geometry.len` 26, record: 2 new edges, 1 new vertex, vertex id 1) and the
  merged vertex ends at (0.25000495,0,0) with tolerance 0.25000505, the midpoint between the split
  point (0.5,0,0) and the source point with a tolerance covering both: a vertex half the edge long.
  solidcore finds the edge passes the point at t = 0 (distance 0 against 0.5 at `TParam`), which is
  the start vertex: no split, the point classifies to vertex 0 (`geometry.len` 15, record: 0 new
  edges, 0 new vertices, vertex id 0).
- `point_merge_6_source_dominates`: point (1e-8,0,0), `TParam` 0.5. The same: C++ vertex at
  (0.250004955,0,0), tolerance 0.25000505; solidcore classifies the point to the start vertex.
- `review_op6_parameter_inf`: point (0.2,0,0), `TParam` +inf, editing enabled. `MakeVertexSplitEdge`
  takes an infinite parameter as past the end and returns the end vertex (1,0,0) without a split;
  `MergeIntoObject` returns success (record: vertex id 1, 0 new edges, 0 new vertices) with the
  point 0.8 from the vertex it is classified to, and no tolerance update (the near-end early return).
  solidcore: the edge passes the point at t = 0.2 (distance 0 against 0.8 at `TParam`), split there:
  edges [0,0.2] and [0.2,1], a new vertex (0.2,0,0) with tolerance 1e-5 (`geometry.len` 26, record:
  2 new edges, 1 new vertex).
- `review_op6_parameter_-inf`: as above with `TParam` -inf: the C++ returns the start vertex (0,0,0),
  0.2 from the point (record vertex id 0); solidcore splits at t = 0.2 (record vertex id 1).

**`topology.assert_valid`** (`SmApiShellBrepFull` of `Solids/off_extrusion.smb` by -0.5 with face 0
open, then the capi check of the result, `copy_offset_surface.json`):

- `copy_shell_off_extrusion_offset_surface_faces_capi0` (check level 0) and
  `copy_shell_off_extrusion_offset_surface_faces_capi2` (level 2). While merging the shell's
  Boolean, a point is classified to edge 0 (a circle, interval [0, pi], tolerance 1e-5) with
  `TParam` 2.356194490192345, whose curve point is 1.110543610826214e-5 from the classified point
  (1.2322411794226957, 5.2322254842987235, -4.86e-17); the edge passes the point at
  t = 2.356190281517396, 4.02781684609563e-7 away. The C++ splits at `TParam` and its own check then
  reports `SmBrep::Test #0 Bad Gap: Edge/Vertex gap across Edgeuse start larger than tol
  (offset=1.1105436e-05, tolerance= 1e-05) [owner SmEdgeuse]`: 4 failures at level 0 and 7 at
  level 2 (`failure_count`, `reports_count`; 571 and 946 line bytes). solidcore splits at the nearer
  parameter and the gap line is absent: 3 and 6 failures (434 and 809 line bytes), the remaining
  lines identical on both sides (`SmFace::Test #39`, `SmEdge::Test #9` twice; at level 2 also
  `SmFace::Test #38` twice and `SmFace::Test #32`). Those remaining failures are the body's and are
  not this defect.

One more case moved within tolerance and is recorded as a known mismatch, not a C++ defect:
`blend.fillet_executive_p3` `source-file-corner_box-edge9`
(`plan/findings/blend.fillet_executive_p3--corner-box-edge9-split-nearest-point.md`).
