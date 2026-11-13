//! A sketch's control spline may save `nurbs`: its own `knots` (clamped, the point count plus
//! the degree plus one) and `weights` (one per point; none: all 1). A projected free-form edge
//! comes in as that one exact curve, where it came in as one spline per Bézier span. An older
//! design has none: its splines are the clamped uniform B-splines they were, and its projected
//! edges are read again as they rebuild. The last release refuses a design with this change
//! (the newer format number).
