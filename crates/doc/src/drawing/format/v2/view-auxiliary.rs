//! A drawing's views can be auxiliary (`kind: {"auxiliary": {"parent", "toward"}}`, #13044): its
//! parent folded toward the side `toward` (a unit along the parent's paper x and y), so it looks
//! square to an edge or a face the parent shows as a line, at its parent's scale and settings,
//! placed at a point, never lined up. An older drawing has none. The last release warns about the
//! newer format number, opens what it can read and keeps the rest.
