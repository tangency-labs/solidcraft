//! A `cell_grid` feature can save a sketched cell (#21722): `cell_sketch` (the sketch holding the
//! cell's one closed profile) with its layout `pitch_x`, `pitch_y`, `stagger`, `cell_angle`,
//! `flip` and `cell_origin_x`/`cell_origin_y`, and then has no `size`. `solid.cell_grid` with
//! `shape: "sketch"` makes one. Older designs have no sketched cell: their cell grids keep the
//! built-in `shape` and `size` they always had and nothing in them is rewritten. The last release
//! opens a design that has this with a warning about the newer format, keeps the feature's place
//! on the timeline with an error saying a newer SolidCraft is needed to build it, and keeps the
//! feature as it came when saving.
