#### WR-543. `CreateUVTrimCurve` stretches one piece of a split drop onto the whole edge interval: the trim runs off its edge
- **Severity:** Wrong result (UV trims up to 5 mm off their edges)
- **Area:** Topology (edge uses)
- **Location:** `source/SMLib/src/SmEdgeuse.cpp`, `SmEdgeuse::CreateUVTrimCurve` (def. `:1230`), the "more than one UVTrimCurve due to splitting" block: a piece whose parameter range is within 10% of the edge's is reparameterized onto the whole edge interval (`EditParameterization`) and the other pieces are deleted.
- **What happens:** `DropAndTrimCurve` splits a drop where the edge leaves the face's domain or crosses a seam. Each piece is the drop of its own interval of the edge, so reparameterizing a piece onto the whole edge interval runs the trim at the wrong pace. An edge along a spline face's boundary row came out 5 mm off itself at its end (Poppy's Cut-Extrude10), and others 0.08 to 2.6 mm.
- **Trigger:** a creased rational bicubic patch and an edge along its boundary row, on the edge's own non-uniform parameter, starting 0.05 mm past the patch. Its trim was 0.85 mm off the edge.
- **How found:** tangency-labs/solidcore#1383 (SolidCraft). The fork's fix is tangency-labs/smlib@e36feb1 ("a drop that comes back in pieces is joined where they meet, never stretched onto the edge"), which is not merged upstream.
- **Confirmed:** no C++ run was made here. The Rust port follows the pinned C++ (`3fd5b95`).
- **Rust port:** behaves as upstream by default. With `Kernel::fixes.uv_trim_joins_drop_pieces`, `create_uv_trim_curve` keeps the longest run of pieces that meet end to end (`longest_drop_run`, the fork's `sm_LongestDropRun`):
  - Two pieces meet when they share the edge parameter (`SM_ARE_SAME`) and the `(u, v)` to within the drop's UV tolerances.
  - A run of several pieces is joined with `CreateByJoining`; no piece is reparameterized.
  - The run then goes through the checks any trim shorter than its edge goes through.

  Test: `longest_drop_run_joins_the_pieces_that_meet` in `topology/edgeuse/p3.rs`.
