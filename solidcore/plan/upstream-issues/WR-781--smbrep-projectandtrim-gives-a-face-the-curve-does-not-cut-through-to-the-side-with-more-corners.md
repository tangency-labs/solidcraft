#### WR-781. `SmBrep::ProjectAndTrim` gives a face the curve does not cut through to the side with more corners
- **Severity:** Wrong result
- **Area:** Topology, trimming
- **Location:** `source/SMLib/src/SmBrep.cpp:25474` (the majority of the votes) and `:25477` (the tie test), in `SmBrep::ProjectAndTrim` (def. `:25278`); `:25390` and `:25483` for the list of edges.
- **What happens:** After the projected curve is imprinted, each face is given a side of the curve. Its vertices vote (`sm_FindSideOfCurve` for each; a vertex on the curve, or one whose projection fails, has no vote). With no vote the mid-points of its edges vote. The side with more votes wins. Only when the votes are equal is the face judged by a point: the point of the face nearest a ray through the reference point along the projection direction.

  A face that the curve cuts in two becomes two faces, each on one side. A face that the curve ends inside is not cut: the imprint leaves a dangling edge in it, and the face lies on both sides of the curve. For such a face the votes are only a count of the corners that happen to project onto the curve. A face with one corner left of the curve and three right of it is taken to be right of the curve, also when the reference point lies on the face left of the curve. `SM_TT_KEEP_POINT` then deletes the face that the reference point is on.

  The same code builds the shape for the ray test from `sEdges`, which is filled only for a face that had no vertex vote. For a face with tied vertex votes the list still holds the edges of an earlier face, or none.
- **Trigger:** `SM_TT_KEEP_POINT`, projection along Z, the segment from (0.5,-1,1) to (0.5,0.5,1), which ends inside each of these planar faces. Measured on the port before the change, which followed the C++ line for line here.
  - Corners (0,0), (1,0), (1.2,0.1), (1.2,0.3), (1,1), (0,1), reference point (0.25,0.25,0). The votes are one left ((0,0)) and three right; the corners at y = 1 are beyond the end of the segment and have no vote. The face is deleted and the B-rep is left empty.
  - Corners (0.6,0), (1,0), (1,1), (0,1), (0,0.6), reference point (0.25,0.4,0). Both votes are right. The face is deleted.
  - The square with corners (0,0), (1,0), (1,1), (0,1) has one vote on each side and is kept.
- **How found:** tangency-labs/solidcraft#13697, on the port. The second trigger is from a review of tangency-labs/solidcraft#15599.
- **Confirmed:** by reading the C++ (the lines above) and on the port. Solidcore takes a face as not cut when an edge that the projection imprinted is used twice by one loop of the face (a dangling edge), or when the face has votes on both sides. It judges such a face as it judges a tie, by its point nearest the reference point, and builds the shape for that test from the face's own edges. Test `a_face_the_curve_ends_inside_is_kept_with_the_reference_point` (`topology::brep::p10`). All 283 golden units give the same results, except seven cases of `primitive.api_trim_ops`. In each of the seven an arc or a segment ends inside one or more faces.

  | case | trim | faces left before | now | what changed |
  |---|---|---|---|---|
  | `gen0086` | keep, box | 2 | 4 | the top (the reference point is on it) and the bottom are kept |
  | `gen0156` | keep, box | 2 | 4 | the same |
  | `gen0070` | keep, box | 1 | 3 | the same |
  | `gen0026` | keep, box | 1 | 3 | the same; both ends of the arc are inside the top |
  | `gen0084` | keep, box | 3 | 4 | the bottom is kept; the segment starts 0.014 inside it, and the reference point projects onto it |
  | `gen0049` | delete, one face | 1 | 0 | the face under the reference point is deleted |
  | `gen0018` | delete, filleted box | 25 | 24 | a fillet face that both ends of the arc lie inside is now on the reference point's side, and is deleted |
- **Not covered:** what a trim should do when the curve ends inside a face is still a choice (tangency-labs/solidcraft#13697 lists three: keep the face whole, cut it along an extension of the curve, or refuse). This change keeps the first and makes it follow the reference point. A face that is not cut and is not under the reference point is still judged by the tie rule as upstream wrote it: when its point nearest the reference point is beyond the end of the curve, the face is taken to be on the other side (the left wall of `gen0084`, deleted before and now).
