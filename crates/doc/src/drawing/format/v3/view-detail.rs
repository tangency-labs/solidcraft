//! Detail views: a view of kind `detail` saves its parent's id and its region of the parent, a
//! circle (`{"circle": {"centre": [x, y], "radius": r}}`) or a rectangle (`{"rectangle": {"min":
//! [x, y], "max": [x, y]}}`) on the parent's plane in model millimetres, with a scale of its own
//! and a place at a point. Older drawings have none and are unchanged. The last release meets a
//! view kind it doesn't know: it warns that the drawing is from a newer format, opens what it can
//! read and keeps the rest (`crates/doc/src/newer_file.rs`).
