//! A Surface Revolve may save `"faced": true`: its faces face the side the curves give them, as
//! Surface Extrude's do (the curve's direction crossed with the sketch normal, so out of a
//! profile), and the faces of a profile agree across the edges they share. New surface revolves
//! save it. A design saved before it has no `faced`, and its surface revolves build each face
//! along its surface's own normal, as they always did (a plane's, a cylinder's outward one,
//! whichever way the curve runs), so a Thicken, an Offset or a Reverse Normal after them keeps
//! its result. The last release opens a design that has it with a warning about the newer format
//! number, keeps the field it doesn't read, and builds those faces the old way.
