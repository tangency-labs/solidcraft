//! A constructor feature authored in a component from a profile sketch owned by another component
//! (`Extrude`, `Revolve`, `SurfaceExtrude`, `SurfaceRevolve`) stores `Feature::profile_frame`: the
//! rigid transform of the feature's component relative to the lowest common ancestor of both
//! components at the time the feature was created. Later moves or joints on the feature's
//! component then move the built body rigidly with the component instead of pulling it back to the
//! sketch's world placement, while moves of the sketch's own component still propagate.
//!
//! Older designs omit `profile_frame` (`None`) and keep the previous live cross-component sketch
//! placement when evaluated. The last release opens a design carrying this field with a newer-format
//! warning and keeps unknown fields untouched when saving.
