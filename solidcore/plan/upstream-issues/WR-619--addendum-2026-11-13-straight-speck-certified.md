Updates: WR-619

#### WR-619 (update, 2026-11-13). A straight speck is decided by its ends

`speck_on_vertex` (`topology/proto_topology/p2.rs`, condition 3 of `collapsed_beside_sibling`) decides a definition
on a line by its two ends, each distance to the vertex tested with its rounding counted: the distance from a point
to the points of a segment is convex along it, so the ends bound it. Definitions on other curves keep the nine
samples. The recorded gap, and so the vertex tolerance `ImprintIntersections` grows from it, is unchanged (the
largest of the nine samples on a line is an end's).
