Updates: WR-588

#### WR-588 (update, 2026-11-14). The trim search passes over cells wholly outside the face (#16394)

- **Location:** the fork's `sm_FaceDirectedMax` (`source/SMLib/src/SmFace.cpp`,
  tangency-labs/smlib@9d44c1b), not in the pinned SMLib; the port's `face_directed_max_with`
  (`crates/solidcore/src/query/face/p3.rs`).
- **What happens:** a cell outside the trim whose surface rises past the box so far, and whose
  least value is below it, is neither inside the face (rule 2) nor wholly above the box
  (rule 3). Unless `S . dir` is monotone over it (rule 4), it is divided. Along the level of
  the box so far, that division goes on to the depth limit cell after cell, until the 4096-cell
  limit. Every undecided cell left on the stack then counts with its greatest value, although
  no point of it lies in the face.
- **Trigger (#16394):** a radius 1 round, `tangent_chain: false`, on the edge where a ring
  segment's torus corner meets its flat end. The blend's surface runs on past its face along
  the tangent edges, and the face's search covers that run-on. `inspect.measure` gave a body
  box from y = −26.45 where the body starts at 17.32, in about 20 s. Arc wall rows of the same
  report: max z 6.0013 for 6, x ±10.25 for ±10.
- **Fix (not the fork's):** rule 5. A cell that no box of the face's UV trim curves meets
  (`TrimCover`: per knot span of a B-spline trim curve the box of its control points, per
  32nd of a trim line the box of that piece, each grown by 1e-6 of its size and coordinates)
  contains no point of the face's boundary. It therefore lies wholly inside or wholly outside
  the face, and its middle's point classification tells which. One that classifies outside
  (`SM_PC_UNKNOWN` from a classification that succeeded) is passed over before it is solved
  (`cell_outside_face`). A face whose trim curves cannot be bounded so (no edge uses, a kind
  that is neither a line nor a B-spline, a weight that is not positive, a coordinate that is
  not finite) takes no cell as outside, as before. Under the same option,
  `Kernel::fixes.face_tight_box_searches_trim`.
- **Tests:** `trim_cover_of_a_square_tells_cells_outside_the_face` and
  `search_passes_over_cells_outside_the_trim_and_keeps_the_peak` (`query/face/p3.rs`). The
  crowned face, trimmed to `[-10, 10]^2` in `[-20, 20]^2` and rising to 50 at the outer corner,
  keeps its peak 4. With rule 5 no cell wholly outside the trim is solved, and fewer cells are
  solved in all. SolidCraft: `a_round_on_a_ring_segments_torus_edge_keeps_the_bodys_box` and
  `a_round_on_an_arc_walls_edges_keeps_the_bodys_box` (`crates/engine/src/measure_surface_tests.rs`).
