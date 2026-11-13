//! A tangent construction plane can save a `reference` (a plane or planar face) and an `angle`
//! (#8822): the plane touches the face where it turns towards the reference, turned by the angle
//! about the face's axis. An older design has neither, and its tangent planes touch where they
//! were picked, as before. The last release refuses a design that has them (it asks for an
//! update) instead of opening those planes at the picked point, away from their reference.
