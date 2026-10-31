//! A sketch constraint can hold a point at a share of a curve's length (`"type": "point_along"`,
//! with the point `p`, the curve `c` and the share `at` from 0 to 1), as Divide Curve places its
//! points (#8969). Older designs need no transformation: they have no such constraint. The last
//! release refuses a design that has one and asks for an update, instead of failing to read its
//! sketch.
