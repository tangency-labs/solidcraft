//! A sketch link can save an offset curve along a face as its source (#22414):
//! `{"type": "face_offset", "body", "at", "from", "distance", "tolerance"?}`, where `at` is a point
//! on the face (it also picks the side), `from` is the edge (`{"type": "edge", body, at}`) or the
//! sketch curve (`{"type": "sketch_curve", sketch, curve}`) the curve is offset from, `distance`
//! is in mm measured along the face, and `tolerance` (mm, left out for the default 0.01) is how far
//! a fitted curve may be from the true offset. `surface.offset_curve` makes it; its 3D curves are
//! the link's wires. An older design has no such link and opens as before. The last release opens
//! a design that has one with a warning about the newer format, keeps the sketch it can't read in
//! its place on the timeline, and saves it back as it came (`crates/doc/src/newer_file.rs`).
