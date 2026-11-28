//! A `dome` feature (#19532): a planar face of a body raised into a cap. It holds `face` (a point
//! on the face, which follows the face as the design's parameters change), `height` (an
//! expression), `shape` (`elliptical`, absent by default, or `spherical`) and `reverse` (absent
//! when false; cutting the dome into the body is not built yet and is refused), and `body` when
//! the design names it. Every older design has no dome, so nothing in them is rewritten. The last
//! release opens a design that has one with a newer-format warning, keeps the feature as it came
//! and does not rebuild it, as it does for any feature it does not know.
