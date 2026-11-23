//! A `cell_grid` feature can save `"mode": "raised"` (#14044): its cells are added as material,
//! standing `depth` from the sketch plane in `direction` and joined to the targets (a new body
//! when there is none), instead of being cut out of them. `solid.cell_grid` with `mode: "raised"`
//! makes one. Without it, and with `"mode": "cut"` (every older design), the feature cuts as it
//! always did, so nothing in older designs is rewritten. The last release does not know the value:
//! it opens a design that has it with a warning about the newer format, keeps the feature's place
//! on the timeline with an error saying a newer SolidCraft is needed to build it, builds the rest
//! and keeps the feature as it came when saving.
