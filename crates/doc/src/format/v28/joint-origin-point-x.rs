//! A point joint origin (`Snap::Point`) can save `x`, the direction its frame's x takes (laid
//! square to z). Older designs need no transformation: a point origin without `x` takes its x
//! from z alone, as before; the last release asks for an update instead of dropping the `x` and
//! turning the part.
