//! A `bounding_solid` feature can carry `margins`: six length expressions, one for each side of the
//! box (-X, +X, -Y, +Y, -Z, +Z), used instead of its single `margin`. Older designs have no
//! `margins`, so nothing in them is rewritten and their boxes keep one margin on every side. The
//! last release opens a design that has the field with a warning about the newer format, builds the
//! box with the single `margin`, and keeps the field it cannot read when the design is saved.
