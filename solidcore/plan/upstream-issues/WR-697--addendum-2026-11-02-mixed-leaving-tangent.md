Updates: WR-697

#### WR-697 (update, 2026-11-02). The Mixed corner's leaving tangent is the edge's own, one-sided, not turned towards a sample point
- **Severity:** Wrong result
- **Area:** Fillets (corners)
- **Location:** the fork's `source/SMLib/src/SmBlendCornerMixed.cpp`, `sm_MixedLeaving` (solidcore: `blend::blend_corner::mixed`, `leaving`).
- **What happens:** `sm_MixedLeaving` took the edge's end tangent with `SmEdge::GetEndTangent`, which evaluates both ends of the edge's interval from above, and turned it towards the edge's point 1e-3 of its interval in from the vertex. At the edge's end the tangent from above reads past the edge: where the edge's curve continues beyond its interval with a turn (a kink at the end vertex), the tangent is the next segment's, and the 1e-3 sample does not orient it reliably. On the chamfered peg's front edge it pointed back along the edge.
- **Source:** tangency-labs/smlib b785227 ("Fillet: the mixed corner's leaving tangent is oriented exactly, by the edge's own sense, not by a sample point"; smlib#82). A port of that fork commit.
- **Fix:** an edge's start vertex is its curve's start, so the curve's derivative taken inside the edge, from above at the start (`from_left` true) and from below at the end (`from_left` false), points into the edge at its start and out of it at its end, where it is negated. No sample point. An edge that does not meet the vertex is `SM_ERR`. `SmEdge::GetEndTangent` (`topology::edge::end_tangent`) is shared code and is unchanged.
- **Rust port:** under the same opt-in `Kernel::fixes.fillet_blend_corner` (the Mixed corner is reached only under it); no new flag.
- **Triggers:** an edge whose curve turns at the edge's end vertex: the end tangent from above disagrees with the edge's own sense.
- **Confirmed:** in solidcore by a unit test, `blend::blend_corner::mixed::tests::leaving_end_vertex_takes_the_edge_sense_from_inside`: a box edge whose curve is a degree 1 polyline turning a right angle at the edge's end vertex. The leaving tangent at the end vertex is now exactly the edge's reversed direction (before: the next segment's direction, square to the edge). The Mixed corner tests (`issue_2360_mixed_corner.rs`, `issue_2256_peg_mixed_corner.rs`, `issue_1475_mixed_corner_concave_side_edge.rs`) still pass.
