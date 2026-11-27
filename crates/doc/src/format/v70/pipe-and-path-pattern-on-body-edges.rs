//! A Pipe and a Pattern on Path save the path they follow when the path is a chain of a body's
//! edges: `path_body` (the body) and `path_edges` (a point on each edge of the chain), beside the
//! curves of the path sketch they saved before this change. An older design has neither and keeps
//! its path of sketch curves, as it was built. A build before this change opens such a design
//! with a newer-format warning and keeps the fields it cannot read; it cannot build such a
//! feature, whose sketch path is empty, and shows the feature's error instead.
