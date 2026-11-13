//! An Align can name planes (`from_plane`, `to_plane`: an origin, construction or offset plane, or
//! an explicit one) instead of a point or a face's point and normal, so the bodies follow the plane
//! when it moves. Older designs need no transformation: an Align without them keeps the points and
//! normals it was saved with. The last release refuses a design saved with this (the newer format
//! number).
