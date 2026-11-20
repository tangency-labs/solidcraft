//! A `bounding_solid` feature can carry `contents: true` (absent when false): it is its
//! component's envelope, a box around every body placed inside the feature's own component (its
//! subcomponents' at any depth and each of their placements), in that component's frame, instead
//! of around the bodies it lists, measured on each body's exact bounds. `solid.bounding_solid`
//! with `component` and `file.insert_step` with `envelope: "box"` make one. Without `contents`
//! (every older design) the feature is unchanged: a box around the listed bodies' display meshes,
//! so an existing Bounding Solid rebuilds at the size it had and nothing in older designs is
//! rewritten. The last release opens a design that has it with a newer-format warning, names
//! `contents` among what it didn't understand and keeps it when saving; it builds the feature as
//! a box around every body of the model, unplaced, as an empty `bodies` list means there.
