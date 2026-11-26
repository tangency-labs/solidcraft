Updates: WR-614

#### WR-614 (update, 2026-11-13). The crossing is decided before the faces are intersected

`drop_crossings_in_coincident_strips` (a removal of the crossing's edge definition after `IntersectTopology`) is
replaced by `crossing_inside_coincident_pair` (`boolean/topology_intersector/coincident_strip.rs`), called in
`IntersectTopology` before a pair of planar faces is intersected. When on both sides of their crossing a straight
boundary edge of either face spans the crossing's extent, faces back toward it and lies within the intersection
tolerance of the other face's plane (both end distances, with their rounding: the distance is affine along a line),
the pair makes no curve, no edge definition and no vertex definitions. The removal had three gaps, each measured on
the strip of `tests/wr_614_coincident_strip_crossing.rs`:

- it left the crossing's end vertex definitions, so a dangling edge to the crossing stayed in each end face (union
  10/26/18 instead of 10/24/16 at d = 5e-6 after the radius change of this branch);
- at d <= 2e-6 the classification had already read the crossing as running along the bound edges, so it no longer
  looked like a face/face crossing and was not removed (union refused or non-manifold);
- it required the bounds to run parallel to the crossing and farther from it than the tolerance; the certificate is
  the bound's two end distances to the other plane, and a bound within the tolerance of the crossing coincides with
  it.

The ends are handled with two decisions made before intersecting, for line edges (`boolean/topology_intersector/p2.rs`):
a vertex of one operand within the intersection tolerance of a line edge of the other, off its ends, is an event
(the edge is split at its drop; the two are one proto vertex), and two line edges whose ends lie within their
intersection tolerance of each other, end for end, are one coincident interval, where a crossing of the two gets no
vertex definition (`ProtoTopologyManager::set_coincident_edges`).

Results (`tests/wr_614_coincident_strip_crossing.rs`): narrow strip union 10/24/16, one valid manifold solid, for
d = 1e-5 down to 1.2e-6, intersection empty, difference box A; wider box (A = [-2, 10]) union one valid manifold
solid down to 2.5e-6 (13 faces: A's and B's coplanar end faces stay two faces sharing the coincident piece), its
intersection empty; below the offsets where the strip's events are pairwise within the tolerance (wider
d <= 1.5e-6, narrow d <= 8e-7 tested) the union is the two boxes touching along the line through B's corner. Open:
the offsets where an event's distance to another equals the tolerance to rounding (narrow d = 1e-6, refused by the
merged vertex's reach check; wider d = 2e-6, an invalid three-region union, as on main before this change).
