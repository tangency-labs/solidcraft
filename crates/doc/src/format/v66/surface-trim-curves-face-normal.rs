//! A `SurfaceTrim` that cuts with sketch curves can project them along the surface's own normal
//! (`curves.face_normal`: true; each point of a curve goes to its foot on the surface) instead of
//! sweeping them along one direction. Older designs need no transformation: a trim without the
//! field saves as before and sweeps along `curves.direction` or the sketch's normal. The last
//! release warns about the newer format number and does not read the field: it would sweep such
//! a trim's curves along the sketch's normal.
