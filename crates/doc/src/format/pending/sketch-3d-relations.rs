//! A sketch's 3D constraints can be parallel, perpendicular, collinear, equal and tangent
//! relations between drawn 3D curves, and relations and dimensions to the origin planes (a line
//! parallel to one, a point on one, a point's distance from one, a line's angle to one). An Equal
//! between arcs says so (`arcs`); a Tangent names the ends that meet (`line_end`, `arc_end`); a
//! point's distance from a plane and a line's angle to one keep the side they were measured on
//! (`reversed`). Older designs have none of these. The last release asks for an update rather
//! than opening a design with them.
