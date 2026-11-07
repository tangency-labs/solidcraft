//! A drawing's dimension can be a baseline set (`baseline`, #13052) or an ordinate set (`ordinate`):
//! an origin and its targets (2 to 64 for a baseline set, 1 to 64 for an ordinate set), each saved by
//! name as a chain's points are, with the direction to measure along. An older drawing has none.
//! The last release refuses a drawing saved with one, as a newer format.
