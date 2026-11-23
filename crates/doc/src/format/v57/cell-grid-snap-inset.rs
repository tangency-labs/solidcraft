//! A `cell_grid` feature can carry a `snap_inset` (a whole-number expression, absent when 0): with
//! `clip: "snap"`, how many lattice lines further in each straight field edge moves after it snaps.
//! Older designs have no `snap_inset`, so nothing in them is rewritten, and a build that does not
//! know the field ignores it (the feature then snaps its edges onto the nearest lattice lines).
