//! A design's `visibility` can keep `opacity`: how opaque bodies (`bodies`, by name) and
//! placements (`placements`, by occurrence path) are drawn in the modelling view, in percent
//! (`browser.opacity`). It is apart from the appearances, which it never changes.
//!
//! An older design is not rewritten: without `opacity` every body is drawn as opaque as its
//! appearance says, as before. The last release opens a design that has it with a warning about
//! the newer format, draws its bodies as their appearances say, and keeps what it cannot read.
