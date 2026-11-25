//! A surface extrude can end at a face, vertex or point (To Object): its `extent.to` (a point on
//! the face, vertex or point) and `extent.to_offset` (how far past it) are now saved and built,
//! as a solid extrude's are. Older designs carry neither, so they rebuild as before. The last
//! release refused a surface extrude with `to`: it warns that the design is newer, opens it, keeps
//! the feature as saved, and shows that feature's error instead of its surface.
