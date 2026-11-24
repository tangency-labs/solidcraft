Updates: WR-305

#### WR-305 (update, 2026-11-13). Straight intervals along straight edges are classified by default

`CurveClassification::classify_edge_interval` (`intersect/curve_class/p3.rs`) now runs on every interval in
`CurveOnClassify` (`query/face/p2.rs`). When the interval and the face edge both lie on lines, the interval is
classified on the edge exactly when both its ends lie within the intersection tolerance of the edge's line, with
their rounding counted: the distance from a segment to a line is affine along it, so the ends bound it (a
certificate, not the fork's midpoint sample). Curved pieces keep the fork's midpoint test behind
`Kernel::fixes.classify_edge_interval`, off by default.
