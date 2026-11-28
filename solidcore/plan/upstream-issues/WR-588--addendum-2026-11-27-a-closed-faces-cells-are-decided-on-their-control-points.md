Updates: WR-588

#### WR-588 (update, 2026-11-27). A closed face's cells are decided on their control points too (#23004)

- **Location:** the fork's `sm_FaceDirectedMax` (`source/SMLib/src/SmFace.cpp`,
  tangency-labs/smlib@9d44c1b), not in the pinned SMLib; the port's `face_directed_max_with`
  (`crates/solidcore/src/query/face/p3.rs`).
- **What happens:** the search decided a cell on the cell's exact subpatch only for a B-spline
  surface open in u and in v; a face closed in a way, a loft's side face or a torus's, kept the
  fork's rules and was decided on its solve alone. A solve's greatest is a value the surface
  takes, a lower bound on the cell's greatest, so a cell was set aside on a solve that found the
  box so far. A closed loop's net is one subpatch over its period and one patch over each of its
  cells, so the bound the open case uses was there to take.
- **Trigger:** the loft of #23004, four elliptical sections on XY at z = 0, 15, 55 and 85 mm
  with semi-axes 36 x 20.25, 40 x 22.5, 34.4 x 20.7 and 38 x 21.375, the major axis along x. Its
  side face bulges to x = 40.1244 at the end of the middle sections' major axis, 2.1 mm past its
  greatest edge (the z = 85 ellipse at 38). In +x the edges give best = 38, the solve over the
  whole face returns a solution on the z = 85 edge at x = 38, and the whole face is set aside:
  the face and the body are boxed to x max 38. The same box decides the boolean's cells, so a cut
  with a box starting at x = 32 removes nothing.
- **Fix:** a B-spline surface is decided on its cells' subpatches whether it is open or closed in
  a way. The rule is the one the open case uses (WR-588, 2026-11-14: branch and bound on a bound
  of the cell, Moore-Skelboe, and the convex hull property of a B-spline with positive weights,
  Piegl and Tiller section 4.4): a cell whose hull reaches no higher than the box so far is set
  aside, a value at a point inside the face only raises the box, and a cell at the limits counts
  with its own bound. Measured on the loft: the face's box in x goes from 38 to
  40.12443624068248, against a greatest x of 40.124074652217 sampled on a 201 x 201 grid of the
  face's domain, so the box holds the face and stays 3.6e-4 mm above it; the tight box of the
  body takes 0.30 s.
  Regression test: `a_closed_faces_ridge_is_boxed_although_the_solve_misses_it`
  (`crates/solidcore/src/query/face/p3.rs`), a tube closed in v whose ridge at x = 1.0520 is
  inside the face: with the fork's rule the box is 1, the ends' value, and the test fails.
