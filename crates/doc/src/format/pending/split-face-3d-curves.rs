//! A Split Face feature (`split_face`) with a sketch tool can save `"wires": ["w1", …]`, the ids
//! of the sketch's 3D curves that split the faces (#22414), and `"direction": [x, y, z]`, the
//! direction its curves are swept along. Without `wires` the feature splits with the sketch's
//! planar curves, as every older design does; a sketch with no planar curves but with 3D curves
//! splits with all of them (older builds refused such a sketch, so no older design has one).
//! Without `direction` a planar curve is swept along the sketch's normal, as before, and a 3D
//! curve along the mean normal of the faces at it. Nothing in older designs is rewritten. The
//! last release does not know the keys: it opens a design that has them with a warning about the
//! newer format and drops them, so it splits with the sketch's planar curves along the sketch's
//! normal, or fails to rebuild the feature ("sketch has no curves to split with") when the sketch
//! has only 3D curves.
