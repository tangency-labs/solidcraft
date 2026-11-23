//! A Plane at Angle turned about an `{origin, dir}` axis can hold length expressions (strings) in
//! its `axis_origin` as well as numbers, so a plane that turns about a parameter-driven point
//! follows the parameter. An origin of plain numbers is saved as before, and older designs need
//! no transformation. The last release opens a design with such an origin with a newer-format
//! warning; it cannot read that plane, so it keeps the feature as one it cannot evaluate and
//! saves it back as it came. Features built on that plane, such as a sketch on it, fail there too.
