//! An extrude with a symmetric offset (`symmetric_offset: true`) saves the option in its
//! `extent`: both directions are offset symmetrically away from the symmetry plane, leaving a
//! clearance gap between the two sides (#16932). Older builds without the option do not have the
//! field and keep their single translated symmetric prism. No step is needed: existing designs
//! omit the field and open as before. The last release opens a design that has the field with a
//! newer-format warning, ignores it, and reads the distance as before.
