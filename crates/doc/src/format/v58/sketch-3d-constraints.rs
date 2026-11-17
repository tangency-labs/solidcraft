//! A sketch can store constraints among the fit points of its drawn 3D curves
//! (`wire_constraints`: coincident, point on a line, fix, along an axis), and whether the drawn
//! 3D curves that met have been joined once (`wires_joined`). Older designs have neither, so
//! their 3D curves stay where they were saved and are joined when first constrained. One that
//! names a fit point its curve no longer has is dropped as the design is opened. The last
//! release asks for an update rather than opening a design with 3D constraints.
