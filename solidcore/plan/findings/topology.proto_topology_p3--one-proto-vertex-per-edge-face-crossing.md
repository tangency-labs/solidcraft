# topology.proto_topology_p3, intersect.curve_class_p7: an edge with a loose tolerance crossed by a face at a shallow angle left the intersection loop open (2026-11-16)

**Units:** topology.proto_topology_p3 (`find_or_create_proto_vertex_pcs`), intersect.curve_class_p7 (`classify_face_interval_on`). **Flag:** none; both changes are the default. Reference: [Booleans](../../docs/booleans.md) §5, "One point per crossing of an edge with a face" and "A curve end on the face's boundary".

**Case** (tangency-labs/solidcraft#17595). A prism (50 x 130, corners r 5, 25.575 high) joined onto an imported STEP part of 1197 faces (zone tolerance 4.06e-4), its long wall 3 µm off the part's wall and cutting two spline faces of a rib. The union returned the part with the prism's faces deleted (+1.8 mm³ in place of +66506, the prism's 17 faces flooded as inside across three gaps in the intersection loop). Two 3 mm cubes of the part are the test fixtures (`crates/solidcore-ops/tests/data/issue_17595_*`).

**Gaps 1 and 2: two points for one crossing.** The wall crosses two spline edges of tolerance 4.0e-3 and 3.8e-3 at 1.3° to 1.8° (the sine of the edge's tangent against the wall's normal, 0.023 to 0.032 at the points sampled along one edge). Each face's curve ends where its own surface meets the wall: one end 6e-12 from the edge's curve, the other 1.6e-3 (1.1e-3 on the second edge) from it, 0.0569 (0.0376) apart along the edge. They became two vertices with an unmated piece of the edge between them.

- Why the lookup did not merge them: the capture distance is kept below half the smallest distance between two vertices of either operand (`intersect_insert_relate`, the rim that upstream's 10 mm ball collapsed into one vertex), and this part has vertices a few µm apart. Upstream's capture distance (0.01, or 2000 approximation tolerances, here about 0.4) would have merged them. The cap stays: a ball of the size these two points need would take in vertices the body keeps distinct.
- Why the loop closure did not close it: `close_intersection_loop` asks `SimpleCoincidenceChecker` whether the piece lies in the wall, and that asks for tangency within `|dot| < 0.017` (about 1°); the piece is within 1.6e-3 of the wall against a tolerance of 9.1e-3 but at dots of 0.023 to 0.032.
- Tried and dropped: relating the piece as a common edge (imprinting it into the wall's face). The sector test at that 0.057 edge put both of the block's face sets inside (the faces leave the edge within a fraction of a degree of the wall), and the union came back with 13 faces and no solid.
- Change: the lookup holds a point in a proto vertex by its ball or by its crossing (`find_proto_vertex_holding`): a definition on the same edge and face, with the edge between the two parameters proved inside the band that puts a point of the face on the edge (`edge_piece_in_face_band`).

**Gap 3: a curve end taken for inside the face.** Farther along, the wall leaves a spline face through an edge whose curve lies 3.27e-3 outside the face's surface (the edge runs along the surface's natural boundary v = 1; its imported tolerance is 2.81e-3, its intersection tolerance with the curve 3.21e-3). The wall's curve on the face ends on the natural boundary; the curve/edge intersection finds no crossing, and `ClassifyFaceInterval` took the unclassified end for a point inside the face. It became a vertex 8.8e-3 short of the vertex where the next face's curve starts on the edge. `PointClassify` at the end's UV puts it on the edge (the trim runs there), and the end is now classified that way first.

**Values** (kernel, default flags; the CLI's figures differ by its volume measure):

| Case | Before | After |
| --- | --- | --- |
| part ∪ prism, 25.575 deep | Ok, +1.7834, check lines as the part's | +66506.4, 1 solid, the same five check lines as the part's |
| the block alone (part ∩ a larger prism, lifted 5) ∪ prism | Ok, +1.7834 | +66505.85, valid |
| the block with the shared top ∪ prism, 15.575 deep | `SM_ERR` ("no radial edge use classifies the face's edge use") | +1720.4, valid: the room under the block's bottom round |
| part ∩ a 6 mm cube at the first crossing | the whole part, 1198 faces | 9 faces, 107.7 |
| first fixture ∪ slab, wall at 0 and 0.05 out | the corner alone, 13.57 (slab's 792 missing) | corner + slab − shared within 9e-4 |
| second fixture ∪ slab | the loop open at the edge (the test fails with the change switched off) | corner + slab − shared within 9.8e-3 |

The band widens to the gap at which either point was accepted where that exceeds the edge's zone (prisms 0.05 and 0.1 larger: a point 4.44e-3 from the wall against a band of 3.97e-3), capped at 1000 zone tolerances.

**Crossing, not a shared stretch.** The golden case `boolean.topology_intersector_p2` `gen0433` (a generated script intersecting two fixture bodies) has an edge of zone tolerance 7.56e-3 running beside a face: two curve ends on it at parameters 6.61936 and 6.61588, 0.0189 apart, 4.568e-3 and 4.526e-3 from the face. The piece between them is inside the band (7.57e-3), and the first form of the rule merged them (8 proto vertices became 7, the golden's only change). They are the ends of a stretch on which the edge runs in the face, the case `SimpleCoincidenceChecker` decides by tangency (`|dot| < 0.017`), not one crossing. The rule now also asks that the edge cross the face at both points, its tangent's sine against the face's normal at least that 0.017 (`COINCIDENT_TANGENT_SINE`, named in `surface/surface/p6.rs`); the issue's crossings have sines of 0.023 to 0.032. `gen0433` is unchanged again (913 cases, 0 changed).

**Open.**
- With the slab as the first operand the first fixture's union at 0 and 0.05 out is closed, with the faces and edges of the other order (10, 21), but the outward-solid proof does not pass and ops answers `NotSolid`. The part joins in either order.
- A loop still left open is classified as before: no refusal names it.
- The STEP import gave the gap-3 edge a tolerance below its own gap to the face (2.81e-3 against 3.27e-3; the gaps are sampled, [Validity and tolerances](../../docs/validity-and-tolerances.md) §10).
- A prism started on the block's bottom (z −45.575, down) still comes back as a second body.
