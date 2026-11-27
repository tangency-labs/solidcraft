//! A Surface Loft may save `"faced": true`: its walls through planar sketch sections face the
//! side Surface Extrude's would at the first section, so out of the region closed sections bound
//! (as a solid loft's faces do) and along an open section's direction crossed with its sketch
//! normal. New surface lofts save it. A design saved before it has no `faced`, and its surface
//! lofts build each wall along the normal of the surface it is skinned with, as they always did,
//! so a Thicken, an Offset or a Reverse Normal after them keeps its result. The last release
//! opens a design that has it with a warning about the newer format number, keeps the field it
//! doesn't read, and builds those walls the old way.
