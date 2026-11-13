//! A joint can be a relation that ties only some directions or points of its two parts:
//! `parallel`, `perpendicular`, `angle` (its `angle` is then the tilt between the two z axes),
//! `point_on_plane`, `point_on_line` and `line_on_plane`, each with the values its free motion
//! needs. Several relations between one pair of parts close loops the solver meets together.
//! Older designs need no transformation; the last release asks for an update instead of meeting
//! a joint type it can't read.
