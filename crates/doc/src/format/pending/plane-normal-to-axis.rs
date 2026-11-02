//! A construction plane can be saved as `normal_to_axis` (#11134): square to a line (a straight
//! edge, sketch line or axis) and through a point, following both. An older design has no such
//! plane, so it opens as before. The last release refuses a design that has one (it asks for an
//! update) instead of failing to read the plane.
