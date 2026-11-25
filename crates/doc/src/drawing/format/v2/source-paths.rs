//! A drawing's source can name another design file (#3890): `sources: [{id, path}]`, `path` relative
//! to the folder of the design the drawing is in, with `/` (whole when the two share no root). A
//! source without `path` is the design the drawing is in, at most one of them; at most 64 sources,
//! listed by id. This replaces `in-design.rs`'s "at most one source, which has no `path`". No step:
//! an older drawing's sources have no path and read as they are. The last release doesn't know
//! drawings: it opens the design with a warning about its newer format and keeps the drawings, paths
//! and all, as it keeps any part it can't read (read in its code, not tried). A build with drawings
//! from before this change opens a drawing with such a source, keeps it and writes it back, unless
//! its views, sheets or tables name it: then it refuses the design as saved by a newer SolidCraft,
//! rather than drop the drawing.
