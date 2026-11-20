# boolean.merge_p2: a ball's pole vertex lands beside the seam when the join copies its face (2026-11-20)

Unit: `boolean.merge_p2` (`make_faceuse`), with `boolean.merge_p1` (`merge_vertex`) and the
imprint's `merge_vertex_neighborhood` (`intersect/point_class.rs`).

Input: a round tube (a circle of radius 0.75 at distance 9.525 from z, in the yz plane, turned
a quarter about -z) joined with a ball of radius 0.75 centred `d` off the tube's centre
circle at the tube's start (the test
`a_ball_joined_onto_a_tubes_end_within_tolerance_of_its_centre_circle_is_valid` in
`crates/solidcore-ops/tests/boolean.rs`). The ball's poles lie on the tube's start circle.

Before: for `d` from 1e-8 to 1e-6 the result failed face test 36 ("Face Near missing Seam").
The intersection splits the tube's start circle at each pole. The imprint places the new
vertex where the neighbourhoods of the circle's point E and of the pole P overlap least, at
M = (E + P) / 2. When the merge copies the ball's face into the result, `merge_vertex` reuses
that mate for the pole and keeps M. The face's edge from the tube's outer side then runs
over the pole and ends d/2 past it, beside the end of the ball's seam:

![Seen from above the ball's pole: before, the vertex M sits between E and the pole P and the
edge from the right runs over P to M; after, the vertex sits on P and its tolerance holds
E](boolean.merge_p2--ball-pole-vertex-beside-seam.svg)

The mate cannot be judged by the seam edge it ends: by the time the face is copied the ball's
other half, which carries the seam edge, is gone, and the pole ends no seam edge
(`vertex::ends_seam_edge` false, measured). It still marks the pole of the face's sphere
(`vertex::is_on_pole`).

After (`seam_vertex_stays_on_seam`): `make_faceuse` moves the mate of a copied face's vertex
that holds its seam (ends a seam edge or marks a pole) to that vertex's point when the mate
holds none, and grows the mate's tolerance by the move, so its old neighbourhood stays inside
the new one. Every offset in the test is one valid solid of 3 faces, 5 edges and 4 vertices
with the closed-form volume.
