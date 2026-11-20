//! A sketch link projecting a model face can save `curve_edges`: for each of its curves, in the
//! order of the link's curves, the persistent name of the model edge that curve was projected
//! from (`faceA|faceB`, as an edge link's `edge_name`), empty for a curve no edge names exactly.
//! When a parameter moves the face so far that its edges come back in another order, the fresh
//! pieces are matched to the link's curves by these names, so the constraints and dimensions on
//! the projected curves stay (#18711). Older designs lack the field (empty): their links are
//! matched by geometry, exactly as before, and record the names when their sketch is edited or
//! the face is projected again. The last release warns about this newer design format and keeps
//! the field it cannot read.
