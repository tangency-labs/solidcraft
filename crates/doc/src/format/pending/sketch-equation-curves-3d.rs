//! A sketch's equation curves (#8352) may now take a third formula, `z(t)` (#12662): the curve
//! keeps `z` beside `x` and `y` in its `equations` entry and, instead of placing a fit point
//! spline of the sketch's plane, places a drawn 3D curve (a wire in the sketch's `wires`, whose
//! id `curve` names). An older design has no `z`, and its equation curves mean what they meant:
//! plane curves. The last release opens a design that has one with a newer-format warning and
//! preserves what it cannot read (`crate::newer_file`).
