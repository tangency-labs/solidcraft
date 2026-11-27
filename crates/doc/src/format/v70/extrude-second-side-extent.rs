//! A two-sided extrude's second side can run through all or to an object, as the first side can
//! (#18691): `extent.through_all2` (true), or `extent.to2` (a point on the face, vertex or point
//! it ends at) with `extent.to_offset2`. An extrude whose second side is one of these still
//! saves `distance2` (0 by default), so it stays a two-sided extrude. Older designs need no
//! transformation: without the fields the second side is the distance `distance2`, as before.
//! The last release asks for an update (the version stamp) instead of building the second
//! side as `distance2` long, not through all or to the object.
