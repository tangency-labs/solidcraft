//! A sketch's 3D constraints can be dimensions (a distance, a distance along an axis, an angle,
//! an arc's radius) driven by a model parameter or driven (reference), and a fixed 3D point can
//! be placed by expressions over the design's parameters. A distance along an axis keeps the
//! side it was measured on (`reversed`); an angle between lines that meet is measured from the
//! end they share (`a_reversed`, `b_reversed`). A driven dimension's value is whatever it
//! measures, so any number opens; a driving one outside the range it may take is dropped as the
//! design opens. Older designs have none of these. The last release asks for an update rather
//! than opening a design with them.
