//! A `cell_grid` feature can carry a `margin` (a length expression, absent when 0): with
//! `clip: "whole"`, the least clearance from a cell to every edge of the field and its keep-outs.
//! Older designs have no `margin`, so nothing in them is rewritten, and a build that does not know
//! the field ignores it (the feature then keeps cells that touch an edge).
