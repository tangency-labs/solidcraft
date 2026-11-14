#### WR-785. `SmBrep::RayFire` reads a hit on the boundary shared by two faces from one of them
- **Severity:** Wrong result (a solid's outside taken for its inside)
- **Area:** Topology (regions, infinite region)
- **Location:** `SmBrep::RayFire` (the best-hit choice and `LocalCurveSector`); solidcore
  `topology/brep/p2.rs` `ray_fire_with_bbox_and_solver`. `SmBrep::FindInfiniteRegion` aims its rays
  at a sampled point of the nearest edge.
- **What happens:** a ray aimed at an edge can meet the two faces ending there, each in a point of its
  trim boundary, at one distance, and report no hit on the edge itself. The best hit then is a face
  (all candidates have the same number of variables, so the lowest dimensional element is a tie and
  the first by parameter wins) and `LocalCurveSector` finds the sector of that one face. Which side of
  one face the ray start is on says nothing about the region it lies in: it can be the far side of the
  wall and the near side of the cap. `FindInfiniteRegion` then makes the inside the infinite region,
  and the solid is built inside out (a negative volume, "no mass"; a union of two is refused by the
  check with "Bad Infinite Region").
- **Trigger:** a closed rational spline ellipse of semi-axes 5 and 67.5 (the four quarters of a
  sketch ellipse), counter-clockwise from the end of the major axis on +y, swept 4 against +z by the
  linear sweep (`solidcore/tests/boolean_api.rs`
  `a_long_thin_elliptic_prism_faces_out_and_cuts_a_block`). The same ellipse starting at the other
  end of the axis is hit on its edge, and comes out right.
- **How found:** solidcraft#16347 (a circular pattern of an intersect extrude whose tool was such a
  prism failed with "SMLib's result is not a closed solid").
- **Fix (always on, no flag):** when the best hit is a face and others at the same distance (within the
  scaled zero the choice already uses) are other faces, the hit is classified on the edge that `best`'s
  face shares with them nearest the hit point (or on its vertex, at an end), by the edge's radial
  sectors, as a hit on the edge would be.
- **Confirmed:** no C++ run; read from the port of the pinned source, and measured in solidcore (the
  test above fails without the fix).
