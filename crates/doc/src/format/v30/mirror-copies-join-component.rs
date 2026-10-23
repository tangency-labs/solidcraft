//! A Mirror of bodies with `placements` puts each copy in the component of the body it mirrors
//! (or, when that component is placed more than once, the innermost component around it placed
//! once) instead of the Mirror's own component; the copy is mirrored at the same placement as
//! before. Nothing new is saved and older designs need no transformation: a Mirror without
//! `placements` mirrors the bodies' own geometry into its own component as before, and one saved
//! with `placements` by 0.15 puts its copies beside their bodies when it is opened. The last
//! release asks for an update instead of opening a design in this format.
