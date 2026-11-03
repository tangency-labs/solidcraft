//! A sketch's 3D constraints can hold drawn 3D geometry to fixed geometry: the ends, line or
//! circle of a model curve included in the sketch, a 2D point or curve of the sketch, or a plane
//! of the model. A plane is `"xy"`, `"yz"` or `"xz"` as before, or an object naming a
//! construction plane (`source: {type: "plane", name}`) or a planar body face (`source: {type:
//! "face", body, at}` with its `face_name`). A point on a curve is `point_on_curve`. Older designs
//! have none of these. The last release asks for an update rather than opening a design with them.
