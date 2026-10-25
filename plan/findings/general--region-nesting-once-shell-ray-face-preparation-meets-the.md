# Region nesting once shell ray face preparation meets the forwarded face hooks (#613 merged with main, 2026-10-24)

- **What.** #613 prepares face geometry (lazy UV trims, trimmed surface cache) for shell
  ray queries on the original B-rep. #639 made `WithMadeTrimCurves` forward the topology
  hooks to the caller's geometry. Each change alone leaves the 9 cases below deferred:
  with #613 but before #639 they stop at `SmEdge::GetCurve` (topology.edge, an
  unresolved face handle), and on main with #639 they stop at
  `SmEdgeuse::GetOrCreateUVTrimCurve (unprepared read-only topology)` (topology.edgeuse_p3)
  or `TrimSrfCache::build_tree_with_face for a face-owned surface` (surface.trim_srf_cache).
  With both, they run to completion and disagree with C++. Bisected locally by merging
  #613's head `c28753855` with main just before #639 (`b3f5aa691`: deferred, 0 failed)
  and with main at #639 (`73e557331`: the primitive_creation_p2 and brep_p6 failures below).
- **Cases (C++ vs Rust):**
  - primitive.primitive_creation_p2 `skin_concentric_loops`
    (`fixtures/primitive.primitive_creation_p2/cases.json#6`, TestSmPrimitives.cpp:642):
    record[7..9] cpp 1/4/1, rust 2/6/2.
  - sweep.api_surfaces `skin_primitive_annular` (`upstream.json#14`, TestSmPrimitives.cpp:596):
    faces[1..3] cpp 1/4/1, rust 2/6/2. The annular cap between the two concentric circles
    comes out as two faces in Rust, where C++ keeps one face with a hole, so the nesting of
    the inner loop in the outer one is decided differently.
  - topology.brep_p6 `make_face.json#89..95` (`two_cubes_at_a_vertex`,
    `two_disjoint_cubes`, `cube_in_cube`, `cube_with_wire`, `cube_with_inner_wire`,
    `cube_with_inner_vertex`, `cube_with_outer_vertex`, all through
    `SmBrep::TestClosureSplitRegion` via `MakeFaceTopology`): Rust exports fewer records,
    e.g. cube_in_cube geom.len cpp 1368, rust 1164 and state.len cpp 6147, rust 5775;
    two_cubes_at_a_vertex 1347/1228 and 6102/5885; cube_with_wire 700/632 and 3181/3057.
- **Ruled out by local probes (reverted):** routing the `query::face::p2`
  curve-on-classify through the read-only `classify_face_interval` (#614's copy model)
  instead of `classify_face_interval_mut` leaves the same primitive_creation_p2 and
  brep_p6 failures, and so does turning off #613's prepared trim-cache fast path in
  `point_classify_with_topology`. So neither #639's mutable face queries nor #613's
  cache-first point classification is the cause on its own.
- **Suspected cause.** A gap further down the region split's ray containment that
  neither branch reached before. The most likely candidates are the ones already
  recorded under the #639 entry above: `IntersectTSBound` walking every loop use instead
  of the face's trimmed-cache leaf nodes, or the face-owned trimmed cache history. Not
  yet narrowed to a call.
- **Status.** All 9 cases are listed in `crates/difftest/known_mismatches.toml`. They
  were deferred on both parents, so no case that previously passed now fails.
