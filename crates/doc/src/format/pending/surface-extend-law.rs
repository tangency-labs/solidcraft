//! An extended surface can be a law extension (#22415): its `mode` can now be `law`, a ruled
//! strip built from the picked edges themselves, and it saves `angle`, an angle expression (the
//! strip's angle to the surface at the edge; missing means 0, tangent). Older designs carry
//! neither, so they rebuild as before and nothing in them is rewritten. The last release does not
//! know the mode: it warns that the design is newer, opens it and keeps the feature as saved,
//! without building it.
