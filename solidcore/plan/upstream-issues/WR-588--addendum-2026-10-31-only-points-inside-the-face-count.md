Updates: WR-588

#### WR-588 (update, 2026-10-31). The trim search counts only points inside the face (fork 95bf434)

- **Location:** the fork's `sm_CellDirectedMax` (`source/SMLib/src/SmFace.cpp`,
  tangency-labs/smlib@9d44c1b), not in the pinned SMLib.
- **What happens:** a cell's greatest point was taken as the face's when it classified as
  anything but `SM_PC_UNKNOWN`, so also on an edge or a vertex: within the face's zone
  tolerance of its boundary, inside or out. Where the surface runs on past the boundary the
  box reaches up to that tolerance outside the face.
- **Trigger:** a box of 10 with the three edges at a corner rounded r 1: the corner's sphere
  octant lies in [0, 1]^3 but boxed to 1.0000243 in x and y.
- **Fix:** tangency-labs/smlib@95bf434 counts only `SM_PC_FACE`; the boundary's extremes come
  from the edges' tight boxes the search starts from. Ported under the same option,
  `Kernel::fixes.face_tight_box_searches_trim` (`cell_directed_max`, `query/face/p3.rs`; the
  function runs only with the option). Regression test:
  `crates/solidcore/tests/fork_95bf434_rounded_corner_tight_box.rs` ([0, 1]^3 to 1e-9).
