//! Control splines can save construction guides owned by the spline and sharing its control points.
//! Sketches can hide the guides without removing their dimensions or constraints.
//! Older designs keep their control splines without stored guides. The last release opens newer
//! designs with a format warning and preserves the data it cannot read.
