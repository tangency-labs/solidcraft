//! Sketches may save biarc splines with ordered fit points, tangent guides, per-span ratios
//! and references to the resulting arcs or lines. Older designs have an empty spline list.
//! The last release warns about the newer format, opens the geometry it can read, and
//! preserves the additional data when saving. Preview builds' fixed splines gain guides
//! when opened, retaining their curve IDs and endpoint positions.
