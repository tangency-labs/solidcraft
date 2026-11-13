//! A sheet's perforation field can run its grid from the flat's right edge (`from_right`): the
//! field of a mirrored sheet copy, whose holes are the mirror image of its source's. Older designs
//! need no transformation: a field without `from_right` runs from the left as before. The last
//! release refuses a design saved with this (the newer format number).
