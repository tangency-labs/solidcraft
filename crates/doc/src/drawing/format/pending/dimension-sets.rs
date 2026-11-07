//! A drawing's dimension can be a baseline set (`baseline`, #13052): an origin and 2 to 64 targets,
//! each saved by name as a chain's points are, with the direction to measure along. An older drawing
//! has none. The last release refuses a drawing saved with one, as a newer format.
