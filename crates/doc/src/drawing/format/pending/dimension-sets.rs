//! A drawing's dimension can be a baseline set (`baseline`, #13052) or an ordinate set (`ordinate`):
//! an origin and its targets (2 to 64 for a baseline set, 1 to 64 for an ordinate set), each saved by
//! name as a chain's points are, with the direction to measure along. An older drawing has none.
//! The last release can't read a drawing that has one. From a design file it warns that the drawing
//! is from a newer format, keeps that drawing as it came and opens the rest of the design
//! (`crates/io/src/design/drawing.rs`); a plain JSON design with one is refused.
