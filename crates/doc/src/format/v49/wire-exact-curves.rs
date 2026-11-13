//! A linked 3D sketch curve (an included edge or an included sketch curve that leaves the sketch
//! plane) saves its exact geometry with its points: `wires[].exact`, the record curves (schema 4
//! curve records with their intervals) end to end along the curve, so a sweep along it follows
//! the curve itself rather than its points. An older design has none and gets them when its
//! links are next resolved; until then such a curve is followed as its points. The last release
//! refuses a design with this change (the newer format number).
