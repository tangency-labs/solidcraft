//! A design can save its analysis studies: `studies`, a name for each, the solver it asks
//! (`thermal`, `flow` or `stress`) and the setup that solver's command reads -- the material,
//! the lattice fineness, the boundary conditions, the loads. A study is the question, kept so
//! it can be asked again after the part changes; the answer is not saved, because an answer
//! worked out for a shape the design no longer has is worse than none.
//!
//! An older design has no studies, which reads back as a design nobody has analysed yet: the
//! field is absent, and absent is empty. Nothing is rewritten.
//!
//! The last release refuses a design that has them, since the format number it carries is
//! newer than the one that release reads. That is deliberate: an older release would drop the
//! studies silently on the next save, and losing the setup of a study is losing work.
