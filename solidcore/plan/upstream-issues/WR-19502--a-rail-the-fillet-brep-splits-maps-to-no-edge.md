#### WR-19502. A rail that the fillet B-rep splits maps to no edge, so a full round whose rails meet along part of one fails
- **Severity:** Wrong result (a valid fillet refused, status 1001)
- **Area:** Blending (fillet B-rep edge map)
- **Location:** `source/SMLib/src/SmFilletGeom.cpp`, `SmFilletEdge::FindFilletBrepEdge`, called by
  `SmFilletExecutive::SetFilletBrepEdgeMap` for every rail with `bFindMultipleEdges` off
  (solidcore `blend/fillet_geom/p1.rs` `find_fillet_brep_edge`, `blend/fillet_executive/p2.rs`
  `set_fillet_brep_edge_map`).
- **What happens:** two rounds whose bands meet exactly share one rail edge in the stitched
  fillet B-rep. Where one rail runs on past the other's end, the stitch splits the longer rail's
  edge at that end. `FindFilletBrepEdge` looks for one edge whose midpoint is the rail's midpoint,
  finds none (the pieces' midpoints are elsewhere), and the fillet fails with "no fillet B-rep
  edge matches the fillet edge". Upstream's multiple-edge mode (for set-back corners) maps a rail
  to two edges at most, and the trim of the original faces and the insertion still walk the
  rail as one piece, with no vertex where it splits.
- **Trigger:** a wall 1 thick joined flush to a plate, its end's two long edges rounded half its
  thickness (0.5). The top edge runs on over the plate and the lower one ends on it.
- **How found:** a user's design (SolidCraft #19502), reduced.
- **Confirmed:** yes, in the port, which keeps upstream's single-edge midpoint match unchanged:
  `solidcore-ops` `blend::a_full_round_whose_rails_meet_along_part_of_one_of_them` fails at
  r = 0.5 without the fix. The pinned C++ was not run.
- **solidcore:** `FilletBrep::fillet_brep_pieces_along` and `fillet_solver::split_rail_along`
  split such a rail in the pseudo B-rep where the fillet B-rep splits it, before the map. The
  pieces join the geometry's other rails.
