# blend.fillet_executive_p1, RemoveTopoEdges and SetBevelCorner (PR #819, 2026-10-24; one disagreement, not located)

- **Corpus.** `RemoveTopoEdges` runs on fixture B-reps (upstream test files, generated
  faces, wires, the wire network) with 1-3 listed edges, compared through the
  `topology.brep_p10` snapshot after the call. The shim rejects lists where two edges
  share a vertex: `DeleteTopologicalVertex` at a vertex of one listed edge can delete
  another listed edge, which upstream then reads freed. It also rejects a face that does
  not own its surface (upstream compares surface pointers; the port's faces own theirs).
  Generated cases list one edge; the fixtures list several.
  `SetBevelCorner` runs in the executive script on real `SmFilletConvexNxNCorner`
  objects mapped to vertices. 600 seeds: no mismatch apart from the case below;
  `--perturb` detects every perturbed case.
- **Disagreement: two topo edges in sequence.** `Mouse.smb`, topo edges 1 and 2 (four
  different faces, no common vertex)
  (fixture `remove_topo_edges_mouse_two_struts`, listed in
  `known_mismatches.toml`). Each edge alone matches (fixture
  `remove_topo_edges_mouse_edge_2`, and every generated single-edge case). With both, the
  faces (surface type, natural and face domains, centre points, tolerances) and the one
  remaining edge (interval, curve type, mid point) agree, and both sides keep the same
  face's surface at each step. What differs is the UV curve of one edge use on the plane
  face (`dbls` 95-102: the interior samples run along v of about 15 in C++ and around the
  face in Rust; the end samples agree). Edge 1's deletion first gives face 0 a
  `CopyAndAddAnalytics` copy of face 2's plane and merges the two, so the plane face
  takes over edge uses made on face 0's old surface. Edge 2's deletion then merges its
  own two faces and the vertex merge joins edges on the plane, so the divergence is in
  `DeleteEdge` / `DeleteTopologicalVertex` / `SetSurface` (edge-use UV curves after a
  face's surface was replaced) rather than in the loop itself, but it is not located
  yet.
