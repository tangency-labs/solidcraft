//! A sweep saves the path it follows when the path is a chain of a body's edges: `path_body`
//! (the body) and `path_edges` (a point on each edge of the chain), beside the curves of the
//! path sketch a sweep saved before this change has. An older design has none and keeps its
//! path of sketch curves, as it was built. The last release refuses a design that has it (it
//! asks for an update) instead of opening it without the path.
