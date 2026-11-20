//! A drawing's views can be pictorial (`kind: {"pictorial": {"parent", "diagonal"}}`, #13044): an
//! isometric view of its parent from one of its four diagonals (`above_right`, `above_left`,
//! `below_right`, `below_left`), at its parent's scale and settings, placed at a point, never lined
//! up. An older drawing has none. The last release warns about the newer format number, opens what
//! it can read and keeps the rest.
