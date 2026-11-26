//! A sketch point marked as construction (`points[].construction`, #18448) saves that flag
//! when it is true, so a Hole leaves the point out. Older designs need no transformation: a
//! point without it is a normal one, as before. The last release reads a design that has it
//! without the field, so the point is a normal hole site there.
