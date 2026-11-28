//! A curve drawn in a 3D sketch can be a control point spline (#3855). It is saved as a drawn 3D
//! curve with one more field, `control`, which holds its `degree`; the curve's `fit` points are
//! then its control points, which it passes through at its two ends only, and its `pts` are its
//! samples. Older designs have no such field: their drawn curves stay lines, arcs and splines
//! through their fit points, and read as they did. No step is needed. The last release opens a
//! design that has one with a newer-format warning and keeps the field when it saves; it reads
//! the curve as a spline through the same points, so a path along the curve has another shape
//! there.
