# audit.recursion: final re-audit through main `ed6d93d8` (PR #273, 2026-10-23)

#541 wires `SmFace::PointClassify` and the vertex/edge zone tolerances into the topology
solver's face solves (`topology_solver::p1::face_point_classify` calls
`query::face::p2::point_classify_prepared`). No code in `query::face`,
`intersect::point_class` or `intersect::curve_class` reaches the topology solver, so this
adds no cycle. The new `TopologyObjects` methods (`edge_tolerance`, `face_entity`) are
lookups. The merge was clean and changed no kernel code of this PR's. Locally, fmt,
clippy, the wasm check, 45 difftest tests, the plan check (261 units) and 2041 of 2042
solidcore tests pass. The exception is the Windows-only
`solids_run_primitive_creation_until_the_next_unported_call` (see the previous entry).
Every unit is now green or partial. The open PRs #563, #178 and #270 add no units.
